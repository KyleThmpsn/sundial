//! Follow every emitted counter contribution and its independently decoded event filter.
use super::*;

pub(super) fn check(
    reader: &Source,
    stock: &mut Reader,
    source: &Payload,
    s: usize,
    native: &Payload,
    n: usize,
) -> CheckResult<()> {
    assert_eq!(native.u32(n - 4)?, 0x80803E30);
    assert_eq!(source.bytes::<8>(s + 0x20)?, native.bytes::<8>(n + 0x20)?);
    assert_eq!(native.u32(n + 0x28)?, f32::MIN.to_bits());
    assert_eq!(source.u32(s + 0x28)?, native.u32(n + 0x2C)?);
    assert_eq!(source.u32(s + 0x2C)?, native.u32(n + 0x30)?);
    assert!(native.u32(n + 0x30)? <= 1);
    let rows = source.array(s + 8, 32, Some(0x8080316C))?;
    let emitted = native.array(n + 8, 32, Some(0x80803E32))?;
    assert!(!rows.is_empty());
    assert_eq!(rows.len(), emitted.len());
    let mut mask = 0u64;
    let mut held = 0;
    let mut ordinals = BTreeSet::from([native.u8(n + 7)?]);
    for (s, n) in rows.into_iter().zip(emitted) {
        assert_eq!(source.bytes::<8>(s + 8)?, native.bytes::<8>(n + 8)?);
        assert_eq!(source.bytes::<12>(s + 20)?, native.bytes::<12>(n + 20)?);
        let seconds = source.f32(s + 16)?;
        if seconds > 0.0 {
            held += 1;
            // D1's setter rounds to the nearest tick, ties to even, then enforces
            // one tick. Decode the staged duration back into that clock domain.
            let scaled = seconds * 60.0;
            let floor = scaled.floor();
            let ticks = if scaled - floor > 0.5
                || (scaled - floor == 0.5 && !(floor as u32).is_multiple_of(2))
            {
                floor + 1.0
            } else {
                floor
            }
            .max(1.0);
            assert!((1.0..=32767.0).contains(&ticks));
            assert_eq!(native.f32(n + 16)?.to_bits(), (ticks / 60.0).to_bits());
        } else {
            assert_eq!(source.u32(s + 16)?, native.u32(n + 16)?);
        }
        let s = source.pointer(s)?;
        let n = native.pointer(n)?;
        let kind = source.u8(s + 5)?;
        let translated_kind = match kind {
            14..=18 => kind - 1,
            33 => 32,
            _ => kind,
        };
        assert_eq!(native.u8(n + 5)?, translated_kind);
        assert_eq!(source.bytes::<5>(s)?, native.bytes::<5>(n)?);
        assert!(ordinals.insert(native.u8(n + 7)?));
        assert_ne!(native.u8(n + 7)?, 255);
        mask |= 1u64 << translated_kind;
        super::check(reader, stock, source, s, native, n)?;
    }
    assert_eq!(native.u64(n + 0x18)?, mask);
    assert!(
        held <= 1,
        "Multiple source holds need distinct timer allocation"
    );
    // The root action must route both the counter and its contributing event kinds.
    for (descriptor, compiled) in [(0x20, 0x88), (0x48, 0x90), (0x58, 0x98)] {
        if native
            .array(descriptor, 8, Some(0x808040BA))?
            .into_iter()
            .any(|row| native.pointer(row).ok() == Some(n))
        {
            assert_eq!(native.u64(compiled)? & (mask | (1 << 26)), mask | (1 << 26));
        }
    }
    Ok(())
}

pub(super) fn evidence(native: &Payload) -> CheckResult<Value> {
    let mut counters = Vec::new();
    for (phase, descriptor) in [("activation", 0x20), ("removal", 0x48), ("rearm", 0x58)] {
        for row in native.array(descriptor, 8, Some(0x808040BA))? {
            let at = native.pointer(row)?;
            if native.u8(at + 5)? != 26 {
                continue;
            }
            assert_eq!(native.u32(at - 4)?, 0x80803E30);
            let mut contributions = Vec::new();
            for row in native.array(at + 8, 32, Some(0x80803E32))? {
                let child = native.pointer(row)?;
                contributions.push(json!({"kind":native.u8(child + 5)?,
                    "ordinal":native.u8(child + 7)?,
                    "success_operation":native.u8(row + 8)?,
                    "success_value":native.f32(row + 12)?,
                    "hold_seconds":native.f32(row + 16)?,
                    "failure_operation":native.u8(row + 20)?,
                    "failure_value":native.f32(row + 24)?}));
            }
            counters.push(json!({"phase":phase,"offset":at,
                "threshold":native.f32(at + 0x20)?,"reset":native.f32(at + 0x24)?,
                "minimum":native.f32(at + 0x28)?,"maximum":native.f32(at + 0x2C)?,
                "randomize_from_magazine":native.u8(at + 0x30)? != 0,
                "event_mask":native.u64(at + 0x18)?,"contributions":contributions}));
        }
    }
    Ok(json!(counters))
}

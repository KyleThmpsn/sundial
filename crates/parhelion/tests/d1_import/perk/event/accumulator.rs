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
    let rows = source.array(s + 8, 32, Some(0x8080316C))?;
    let emitted = native.array(n + 8, 32, Some(0x80803E32))?;
    assert!(!rows.is_empty());
    assert_eq!(rows.len(), emitted.len());
    let mut mask = 0u64;
    let mut held = 0;
    let mut ordinals = BTreeSet::from([native.u8(n + 7)?]);
    for (s, n) in rows.into_iter().zip(emitted) {
        assert_eq!(source.bytes::<24>(s + 8)?, native.bytes::<24>(n + 8)?);
        let seconds = source.f32(s + 16)?;
        if seconds > 0.0 {
            held += 1;
            // The accepted source durations are whole 60 Hz ticks. Native holds
            // retain seconds and have one timer per counter, keyed by its ordinal.
            assert_eq!((seconds * 60.0).fract(), 0.0);
            assert!((1.0..=32767.0).contains(&(seconds * 60.0)));
        }
        let s = source.pointer(s)?;
        let n = native.pointer(n)?;
        let kind = source.u8(s + 5)?;
        let translated_kind = if matches!(kind, 14..=18) {
            kind - 1
        } else {
            kind
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

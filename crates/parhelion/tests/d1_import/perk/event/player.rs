//! Read original and emitted player predicates at their consumer-owned offsets.
use super::*;

pub(super) fn check(
    reader: &Source,
    stock: &mut Reader,
    source: &Payload,
    s: usize,
    native: &Payload,
    n: usize,
) -> CheckResult<()> {
    assert_eq!(source.u32(s - 4)?, 0x80803101);
    assert_eq!(native.u32(n - 4)?, 0x80803DCE);
    assert_eq!(native.u8(n + 5)?, 20);
    fields(reader, stock, source, s, native, n)
}

pub(super) fn fields(
    reader: &Source,
    stock: &mut Reader,
    source: &Payload,
    s: usize,
    native: &Payload,
    n: usize,
) -> CheckResult<()> {
    // The action compiler assigns condition ordinals independently of the source.
    assert_eq!(source.bytes::<5>(s)?, native.bytes::<5>(n)?);
    assert_eq!(source.u8(s + 6)?, native.u8(n + 6)?);
    for at in (8..0x38).step_by(8) {
        assert_eq!(source.bytes::<8>(s + at)?, native.bytes::<8>(n + at)?);
    }
    assert_eq!(source.u8(s + 0x38)?, native.u8(n + 0x38)?);
    assert_eq!(source.u8(s + 0x60)?, native.u8(n + 0x80)?);
    let mask = source.u8(s + 0x61)?;
    assert_eq!(native.u8(n + 0x81)? & 1, mask & 1);
    assert_eq!((native.u8(n + 0x81)? >> 2) & 1, (mask >> 1) & 1);
    assert_eq!(native.u8(n + 0x81)? & !5, 0);
    for index in 0..7 {
        assert_eq!(
            source.bytes::<8>(s + 0x64 + index * 8)?,
            native.bytes::<8>(n + 0x84 + index * 8)?
        );
    }
    assert_eq!(source.u32(s + 0x9C)?, 0x811C9DC5);
    assert_eq!(native.u32(n + 0xD4)?, 0x811C9DC5);
    assert_eq!(source.bytes::<8>(s + 0xA0)?, native.bytes::<8>(n + 0xD8)?);
    assert_eq!(source.bytes::<8>(s + 0xA8)?, [0; 8]);
    assert_eq!(source.u32(s + 0xBC)?, 0);
    assert_eq!(native.u32(n + 0xCC)?, 0);
    assert_eq!(source.bytes::<3>(s + 0xC1)?, [0; 3]);
    assert_eq!(native.bytes::<3>(n + 0xD1)?, [0; 3]);
    let pattern = source.u32(s + 0xB8)?;
    if pattern == u32::MAX {
        assert_eq!(source.u64(s + 0xB0)?, 0);
        assert_eq!(source.u8(s + 0xC0)?, 0);
        assert_eq!(native.u32(n + 0xC8)?, u32::MAX);
        assert_eq!(native.u8(n + 0xD0)?, 0);
    } else {
        assert_eq!(source.u8(s + 0xC0)?, 3);
        assert_eq!(native.u8(n + 0xD0)?, 3);
        if source.u64(s + 0xB0)? != 0 {
            let path = source.pointer(s + 0xB0)?;
            assert!(source.0[path..].contains(&0));
        }
        ability_event::pattern(reader, stock, pattern, native.u32(n + 0xC8)?)?;
    }
    assert_eq!(source.u32(s + 0xC4)?, native.u32(n + 0xE0)?);
    assert_eq!(source.u8(s + 0xC8)?, native.u8(n + 0xF8)?);
    ability_ranges(source, s + 0x40, native, n + 0x60)?;
    named_ranges(source, s + 0x50, native, n + 0x70)?;
    assert_eq!(native.u64(n + 0xC0)?, 0);
    for at in [0x48, 0xE8] {
        assert_eq!(native.bytes::<16>(n + at)?, [0; 16]);
    }
    Ok(())
}

fn named_ranges(source: &Payload, s: usize, native: &Payload, n: usize) -> CheckResult<()> {
    let original = source.array(s, 8, Some(0x8080316F))?;
    let emitted = native.array(n, 8, Some(0x808040C2))?;
    assert_eq!(original.len(), emitted.len());
    if original.is_empty() {
        assert_eq!(native.bytes::<16>(n)?, [0; 16]);
    }
    for (original, emitted) in original.into_iter().zip(emitted) {
        let original = source.pointer(original)?;
        let emitted = native.pointer(emitted)?;
        assert_eq!(source.u32(original - 4)?, 0x808031C6);
        assert_eq!(native.u32(emitted - 4)?, 0x808040C7);
        assert!(matches!(
            source.u32(original)?,
            0xAD7C4580 | 0xBB7679EB | 0xCF6062E0
        ));
        assert_eq!(source.bytes::<12>(original)?, native.bytes::<12>(emitted)?);
    }
    Ok(())
}

pub(super) fn ability_ranges(
    source: &Payload,
    s: usize,
    native: &Payload,
    n: usize,
) -> CheckResult<()> {
    let original = source.array(s, 12, Some(0x808031E8))?;
    let emitted = native.array(n, 12, Some(0x808040CA))?;
    assert_eq!(original.len(), emitted.len());
    if original.is_empty() {
        assert_eq!(native.bytes::<16>(n)?, [0; 16]);
    }
    for (original, emitted) in original.into_iter().zip(emitted) {
        assert!(source.u32(original)? <= 2);
        assert_eq!(source.bytes::<12>(original)?, native.bytes::<12>(emitted)?);
    }
    Ok(())
}

//! Read original and emitted player predicates at their consumer-owned offsets.
use super::*;

pub(super) fn check(source: &Payload, s: usize, native: &Payload, n: usize) -> CheckResult<()> {
    assert_eq!(source.u32(s - 4)?, 0x80803101);
    assert_eq!(native.u32(n - 4)?, 0x80803DCE);
    assert_eq!(native.u8(n + 5)?, 20);
    fields(source, s, native, n)
}

pub(super) fn fields(source: &Payload, s: usize, native: &Payload, n: usize) -> CheckResult<()> {
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
    assert_eq!(source.u32(s + 0xB8)?, u32::MAX);
    assert_eq!(native.u32(n + 0xC8)?, u32::MAX);
    assert_eq!(source.u32(s + 0xC4)?, native.u32(n + 0xE0)?);
    assert_eq!(source.u8(s + 0xC8)?, native.u8(n + 0xF8)?);
    for at in [0x40, 0x50] {
        assert_eq!(source.bytes::<16>(s + at)?, [0; 16]);
    }
    assert_eq!(native.u64(n + 0xC0)?, 0);
    for at in [0x48, 0x60, 0x70, 0xE8] {
        assert_eq!(native.bytes::<16>(n + at)?, [0; 16]);
    }
    Ok(())
}

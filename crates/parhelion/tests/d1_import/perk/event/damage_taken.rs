//! Read the incoming event's threshold and filters through emitted package references.
use super::*;

pub(super) fn check(
    reader: &Source,
    stock: &mut Reader,
    source: &Payload,
    s: usize,
    native: &Payload,
    n: usize,
) -> CheckResult<()> {
    assert_eq!(source.u32(s - 4)?, 0x8080313D);
    assert_eq!(native.u32(n - 4)?, 0x80803DDC);
    assert_eq!(source.bytes::<7>(s)?, native.bytes::<7>(n)?);
    assert_eq!(source.u32(s + 0x16C)?, native.u32(n + 0x98)?);
    assert_eq!(source.u32(s + 0x170)?, native.u32(n + 0xA4)?);
    assert_eq!(native.bytes::<4>(n + 0x9C)?, [0; 4]);
    assert_eq!(native.u32(n + 0xA0)?, 0x811C9DC5);
    player::ability_ranges(source, s + 0x178, native, n + 0xA8)?;
    assert_eq!(native.bytes::<16>(n + 0xB8)?, [0; 16]);
    assert_eq!(source.bytes::<64>(s + 0x98)?, [0; 64]);
    assert_eq!(source.bytes::<128>(s + 0xE8)?, [0; 128]);
    assert_eq!(source.bytes::<4>(s + 0x168)?, [1; 4]);
    filter::damage(reader, stock, source, s + 8, native, n + 8)
}

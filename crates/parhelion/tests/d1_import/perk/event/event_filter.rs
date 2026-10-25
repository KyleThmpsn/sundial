//! Follow additional emitted event predicates against their original source operands.
use super::*;

pub(super) fn check(
    reader: &Source,
    stock: &mut Reader,
    source: &Payload,
    s: usize,
    native: &Payload,
    n: usize,
) -> CheckResult<()> {
    let kind = source.u8(s + 5)?;
    assert_eq!(source.bytes::<7>(s)?, native.bytes::<7>(n)?);
    match kind {
        3 => {
            assert_eq!(native.u32(n - 4)?, 0x80803DE5);
            labels(reader, stock, source, s + 8, native, n + 8)?;
            labels(reader, stock, source, s + 0x128, native, n + 0xB0)?;
            assert_eq!(source.bytes::<8>(s + 0x1FC)?, native.bytes::<8>(n + 0x110)?);
            assert_eq!(source.bytes::<2>(s + 0x204)?, native.bytes::<2>(n + 0x118)?);
            assert_eq!(native.bytes::<0x40>(n + 0x68)?, [0; 0x40]);
            assert_eq!(native.u64(n + 0xA8)?, 0);
            assert_eq!(native.u32(n + 0x11C)?, 0x811C9DC5);
        }
        9 => {
            assert_eq!(native.u32(n - 4)?, 0x80803E00);
            assert_eq!(source.bytes::<4>(s + 8)?, native.bytes::<4>(n + 8)?);
        }
        23 => {
            assert_eq!(native.u32(n - 4)?, 0x80803DDA);
            assert_eq!(source.bytes::<4>(s + 8)?, native.bytes::<4>(n + 8)?);
        }
        _ => return Err("Unexpected additional event condition".into()),
    }
    Ok(())
}

//! Read the staged reserve-transfer node from its original D1 operands.
use super::*;

pub(super) fn check(
    readback: &mut Reader,
    source: &Payload,
    s: usize,
    native: &Payload,
    n: usize,
) -> CheckResult<()> {
    assert_eq!(source.u32(s - 4)?, 0x8080321E);
    assert_eq!(native.u32(n - 4)?, 0x808029EC);
    assert_eq!(native.bytes::<2>(n)?, [16, 0]);
    assert_eq!(source.u8(s + 2)?, native.u8(n + 0x68)?);
    assert_eq!(source.u8(s + 3)?, native.u8(n + 0x69)?);
    assert_eq!(source.u8(s + 5)?, native.u8(n + 0x6A)?);
    assert_eq!(source.u8(s + 4)?, 0);
    assert_eq!(native.bytes::<2>(n + 0x6B)?, [255, 0]);
    assert_eq!(native.bytes::<0x40>(n + 8)?, [0; 0x40]);
    let path = b"content/common/native/sandbox/label_globals.label_globals.tft\0";
    let text = native.pointer(n + 0x48)?;
    assert_eq!(&native.0[text..text + path.len()], path);
    let dictionary = readback.tag(native.u32(n + 0x50)?, None)?;
    assert!(!dictionary.array(8, 4, Some(0x80800070))?.is_empty());
    assert_eq!(native.u32(n + 0x54)?, 0);
    assert_eq!(native.u8(n + 0x58)?, 255);
    assert_eq!(native.bytes::<15>(n + 0x59)?, [0; 15]);
    // The native equipped lane is independent of owning weapon and inventory slots.
    for (program, original) in [
        (0x78, None),
        (0xB0, Some(8)),
        (0xE8, Some(12)),
        (0x120, Some(16)),
        (0x158, Some(20)),
    ] {
        let code = native.array(n + program, 1, Some(0x80800009))?;
        assert_eq!(code.len(), 4);
        assert_eq!(native.bytes::<4>(code[0])?, [0x34, 0, 0x3E, 0]);
        let values = native.array(n + program + 16, 16, Some(0x80800090))?;
        assert_eq!(values.len(), 1);
        let expected = original
            .map(|offset| source.u32(s + offset))
            .transpose()?
            .unwrap_or(0);
        for lane in 0..4 {
            assert_eq!(native.u32(values[0] + lane * 4)?, expected);
        }
        assert_eq!(native.u64(n + program + 32)?, 1);
        assert_eq!(native.u64(n + program + 40)?, 1);
    }
    Ok(())
}

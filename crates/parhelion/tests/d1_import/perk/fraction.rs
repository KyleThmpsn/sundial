//! Follow proportional ammunition operands through one persisted native effect.
use super::*;

pub(super) fn check(
    readback: &mut Reader,
    source: &Payload,
    s: usize,
    native: &Payload,
    n: usize,
) -> CheckResult<()> {
    assert_eq!(source.u32(s - 4)?, 0x8080221F);
    assert_eq!(native.u32(n - 4)?, 0x80803E3E);
    assert_eq!(source.bytes::<2>(s)?, [14, 0]);
    assert_eq!(native.bytes::<2>(n)?, [15, 0]);
    assert_eq!(source.bytes::<4>(s + 2)?, native.bytes::<4>(n + 0x68)?);
    assert_eq!(native.bytes::<6>(n + 2)?, [0; 6]);
    assert_eq!(native.bytes::<0x40>(n + 8)?, [0; 0x40]);
    let path = b"content/common/native/sandbox/label_globals.label_globals.tft\0";
    let text = native.pointer(n + 0x48)?;
    assert_eq!(&native.0[text..text + path.len()], path);
    let dictionary = readback.tag(native.u32(n + 0x50)?, None)?;
    assert!(!dictionary.array(8, 4, Some(0x80800070))?.is_empty());
    assert_eq!(native.u32(n + 0x54)?, 0);
    assert_eq!(native.u8(n + 0x58)?, 255);
    assert_eq!(native.bytes::<15>(n + 0x59)?, [0; 15]);
    // Owning and explicit-slot amounts add before rounding. They must remain
    // in one effect, while native ammunition-category amounts remain absent.
    assert_eq!(source.bytes::<16>(s + 8)?, native.bytes::<16>(n + 0x6C)?);
    assert_eq!(native.bytes::<12>(n + 0x7C)?, [0; 12]);
    Ok(())
}

pub(super) fn evidence(action: &Payload) -> CheckResult<Vec<Value>> {
    let mut rows = Vec::new();
    for row in action.array(0x38, 8, Some(0x808040AC))? {
        let at = action.pointer(row)?;
        if action.u8(at)? != 15 {
            continue;
        }
        let mut amounts = Vec::new();
        for offset in (0x6C..0x88).step_by(4) {
            amounts.push(action.u32(at + offset)?);
        }
        rows.push(json!({"offset":at,"class":action.u32(at - 4)?,
            "controls":action.bytes::<4>(at + 0x68)?,"amount_bits":amounts}));
    }
    Ok(rows)
}

//! Follow a wrapper's private controller and independently compare every child event and effect.
use super::*;

pub(super) fn check(
    reader: &Source,
    stock: &mut Reader,
    emitted: &mut Reader,
    source: &Payload,
    tag: u32,
) -> CheckResult<()> {
    assert!(
        stock.manager.get_entry(tiger_pkg::TagHash(tag)).is_none(),
        "Nested graph must be private"
    );
    let native = emitted.tag(tag, Some(0x80809C0F))?;
    let owners = source.array(32, 12, Some(0x80800715))?;
    let targets = native.array(16, 12, Some(0x80809C04))?;
    assert_eq!((owners.len(), targets.len()), (1, 1));
    assert_eq!(source.bytes::<32>(48)?, [0; 32]);
    assert_eq!(native.bytes::<32>(32)?, [0; 32]);
    let original = reader.read(source.u32(owners[0])?)?;
    let owner_tag = native.u32(targets[0])?;
    assert!(
        stock
            .manager
            .get_entry(tiger_pkg::TagHash(owner_tag))
            .is_none(),
        "Wrapper owner must be private"
    );
    let owner = emitted.tag(owner_tag, Some(0x80809C36))?;
    let (s, n) = (original.pointer(24)?, owner.pointer(24)?);
    assert_eq!(original.u32(s - 4)?, 0x80802A94);
    assert_eq!(owner.u32(n - 4)?, 0x80803E52);
    let child_tag = owner.u32(n + 0x50)?;
    assert!(
        stock
            .manager
            .get_entry(tiger_pkg::TagHash(child_tag))
            .is_none(),
        "Nested controller must be private"
    );
    let child = emitted.tag(child_tag, Some(0x808040B5))?;
    let original = reader.read(original.u32(s + 0x68)?)?;
    super::super::check_action(reader, stock, emitted, &original, &child)
}

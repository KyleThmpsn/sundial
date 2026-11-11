//! A wildcard grant whose rows set one script parameter in every source Super lowers to the Named
//! Property write Shadowkeep's own port of the behavior uses, when a stock action makes exactly
//! that write.
//!
//! Rise of Iron key `9D51973E` (Keeper of the Pack) sets `AA4290AB` (`extra_orb`) from 0 to 1 in
//! all nine Super banks. Shadowkeep keeps `9D51973E` rows in eight Super banks, but no stock action
//! or subclass entry applies them, and the Fist of Havoc bank `80BC3CB0` lists no such parameter.
//! Crest of Alpha Lupi's Survival Well action `80BBCBD6` instead writes `AA4290AB` = 1 through a
//! Named Property node (kind 10, class `808029ED`) with ability mask 2, the Super slot, and writes 0
//! on removal. Setter `00576010` writes every component of the resolved Super that declares the
//! name and creates none, and every stock Super's script owner declares `AA4290AB`, also where its
//! bank lists no such parameter. The node sets rather than adds, which equals the source's add and
//! replace rows while the parameter has one writer.
use super::*;
use sundial::package_authoring::sandbox_perk::action::{FactValue, NAMED_PROPERTY_LABELS, decode};

/// A stock action's Named Property write of `name` on ability `mask`, constant `value`, replacing,
/// with removal policy one and removal value `restore`, as the program action and the action tag.
pub(super) fn witness(
    native: &mut Reader,
    name: u32,
    mask: u32,
    value: u32,
    restore: u32,
) -> Result<Option<(Action, u32)>> {
    let needle = name.to_le_bytes();
    let labels = &NAMED_PROPERTY_LABELS;
    for tag in native.classes(0x808040B5) {
        let stock = native.tag(tag, Some(0x808040B5))?;
        if !stock.0.windows(4).any(|word| word == needle) {
            continue;
        }
        let Ok(decoded) = decode(&stock.0) else {
            continue;
        };
        for effect in decoded.effects().filter(|effect| effect.kind == 10) {
            let fact = |label: &str| {
                effect
                    .facts
                    .iter()
                    .find(|fact| fact.label == label)
                    .map(|fact| &fact.value)
            };
            let selector = |label: &str| match fact(label) {
                Some(FactValue::Selector(byte)) => Some(*byte),
                _ => None,
            };
            let number = |label: &str| match fact(label) {
                Some(FactValue::Number(number)) => Some(number.to_bits()),
                _ => None,
            };
            if fact(labels.key) == Some(&FactValue::Key(name))
                && fact(labels.ability_mask) == Some(&FactValue::Mask(u64::from(mask)))
                && number(labels.value) == Some(value)
                && number(labels.restore).unwrap_or(0) == restore
                && selector(labels.target) == Some(0)
                && selector(labels.operation) == Some(0)
                && selector(labels.removal) == Some(1)
                && selector(labels.input) == Some(0)
                && selector(labels.flag) == Some(1)
            {
                return Ok(Some((
                    Action::Property {
                        key: name,
                        target: 0,
                        operation_byte: 0,
                        removal: 1,
                        value_bits: value,
                        restore_bits: restore,
                        ability_mask: mask,
                        input: 0,
                        flag: 1,
                    },
                    tag,
                )));
            }
        }
    }
    Ok(None)
}

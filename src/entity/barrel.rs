//! The Barrel definition's projectile slots. A weapon whose content block names no firing graph
//! fires the graph named here: all 259 such stock weapons name one, and the second slot repeats
//! the first in every stock Barrel. A content block's graph takes its place where one is named.
use super::WeaponComponentBinding;
use super::spread::{SPREAD, Spread};
use crate::package_payload::u32_at;

const PROJECTILE_SLOTS: [usize; 2] = [0xB80, 0xB90];

/// Each projectile slot of the Barrel definition, as an offset into the owner, with the graph it
/// names if it names one.
pub fn projectile_slots(
    owner: &[u8],
    binding: WeaponComponentBinding,
) -> Result<Vec<(usize, Option<u32>)>, String> {
    let definition = Spread::read(owner, binding)?.slot - SPREAD;
    PROJECTILE_SLOTS
        .iter()
        .map(|offset| {
            let at = definition + offset;
            let tag = u32_at(owner, at)?;
            Ok((at, (tag != 0 && tag != u32::MAX).then_some(tag)))
        })
        .collect()
}

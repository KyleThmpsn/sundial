//! Marathon item tiers. An item definition names a tier resource at `+0xD8` that stores its
//! grade, the tier table row plus four, as a `u16` at `+4`. Weapon definitions name none.
use super::*;

const RESOURCE: u32 = 0x8080_92AD;

/// Each grade's Marathon tier and the Destiny rarity it maps to.
const TIERS: [(u16, &str, &str); 6] = [
    (4, "Standard", "common"),
    (5, "Enhanced", "uncommon"),
    (6, "Deluxe", "rare"),
    (7, "Superior", "legendary"),
    (8, "Prestige", "exotic"),
    (9, "Contraband", "exotic"),
];

/// The grade of `definition`'s tier, when it names one.
pub(super) fn grade(definition: &Payload) -> Option<u16> {
    if definition.u64(0xD8).ok()? == 0 {
        return None;
    }
    let at = definition.pointer(0xD8).ok()?;
    if at < 4 || definition.u32(at - 4).ok()? != RESOURCE {
        return None;
    }
    definition.u16(at + 4).ok()
}

/// The Marathon tier and the Destiny rarity of `grade`.
pub(super) fn tier(grade: u16) -> Option<(&'static str, &'static str)> {
    TIERS
        .iter()
        .find(|(known, ..)| *known == grade)
        .map(|&(_, tier, rarity)| (tier, rarity))
}

//! Cross-family appearances: move the appearance's rig and animations onto the gameplay
//! runtime, so the model keeps its own bones and its own reload.
//!
//! A weapon family's runtime entity keeps its presentation in four component owners: the
//! skeleton the gear parts are weighted to, the animation lookup and animation set that drive
//! the gun's own moving parts, and the first-person attachment that reaches the hands. Every
//! gameplay component the player feels lives in a different owner, so promoting those four
//! leaves trigger, barrel, magazine, reload timing and stats exactly as the gameplay donor
//! wrote them.
//!
//! Families do not agree about all of this, so it is attempted rather than assumed. Auto
//! rifles, hand cannons and pulse rifles interchange; a shotgun places its skeleton's event
//! receiver at a different offset and is refused. A refusal is not a failed build: the
//! appearance's parts are pinned to the gameplay rig's root bone instead, which draws the
//! model without its moving parts. See `weapon::emission::reskin`.
use sundial::package_authoring::weapon_entity::{
    graft_weapon_component_bindings_with, owner::EventPolicy,
};

const SKELETON: u32 = 0x1C80_DD4A;
const ANIMATION_LOOKUP: u32 = 0x681C_2C0D;
const ANIMATION_SET: u32 = 0x8983_4B2B;
const FIRST_PERSON_ATTACHMENT: u32 = 0xD3A5_500E;

/// Promoted together. A rig without its clips, or clips without the hands that hold them,
/// would leave the weapon half-converted and is worse than not trying.
pub(crate) const PRESENTATION_BINDINGS: [u32; 4] = [
    SKELETON,
    ANIMATION_LOOKUP,
    ANIMATION_SET,
    FIRST_PERSON_ATTACHMENT,
];

/// Replace the gameplay runtime's presentation owners with the appearance runtime's.
///
/// This uses the retargeting event policy: the two entities wire the same component through
/// different neighbours, because every other component of the weapon differs, so their event
/// sets cannot be paired. Each connection into a replaced owner keeps its class and interior
/// offset and changes only which owner it names, and only where the appearance's own event
/// graph already addresses that same class at that same offset.
pub(crate) fn graft_presentation(target: &mut Vec<u8>, donor: &[u8]) -> Result<(), String> {
    let grafts = PRESENTATION_BINDINGS
        .iter()
        .map(|binding| (*binding, donor))
        .collect::<Vec<_>>();
    graft_weapon_component_bindings_with(target, &grafts, EventPolicy::Retarget)
}

/// Whether this appearance's presentation can be promoted onto this runtime, decided by doing
/// it on a copy. Predicting it would mean restating every rule the graft already enforces.
pub(crate) fn presentation_graft_applies(target: &[u8], donor: &[u8]) -> bool {
    graft_presentation(&mut target.to_vec(), donor).is_ok()
}

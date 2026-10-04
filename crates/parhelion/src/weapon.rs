//! Weapon-only rules that the item build applies: ammo, Exotic behaviors, type markers and
//! first-person animations borrowed from other weapons, reload-hold element switching,
//! cross-family rigs, and the projection into Sunrise's replicated weapon perk bank. The build
//! itself, which every item kind goes through, is `crate::item`.
pub(crate) mod ammo;
pub(crate) mod animations;
pub mod behavior;
pub(crate) mod crosshair;
pub(crate) mod glow;
pub(crate) mod lenders;
pub(crate) mod perk_bank;
pub(crate) mod rig;
pub(crate) mod variable_damage;

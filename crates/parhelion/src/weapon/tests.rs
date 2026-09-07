use super::*;
mod independent_damage;
mod staged_generation;
use crate::progression::COLLECTIBLE_DISPLAY_CONDITION_OFFSET;
use sundial::package_authoring::investment_schema::{
    LEGACY_ARC_DAMAGE_PERK_INDEX, LEGACY_SOLAR_DAMAGE_PERK_INDEX, MODERN_ARC_DAMAGE_PERK_INDEX,
    MODERN_SOLAR_DAMAGE_PERK_INDEX, MODERN_VOID_DAMAGE_PERK_INDEX,
};
use sundial::package_authoring::weapon_runtime::{
    WeaponRuntimeFieldLocator, WeaponRuntimePathElement, WeaponRuntimeRootKind, WeaponRuntimeValue,
};

mod presentation;

mod validation;

mod stats;

mod sockets;

mod staged_builds;

mod damage;

mod fixtures;
use fixtures::*;

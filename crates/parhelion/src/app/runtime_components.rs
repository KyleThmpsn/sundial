//! The weapon components the runtime editor exposes, in the order its tabs show them.
use super::*;

#[derive(Clone, Copy)]
pub(super) struct RuntimeComponentControl {
    pub(super) binding_hash: u32,
    pub(super) label: &'static str,
    pub(super) tooltip: &'static str,
}

pub(super) const PRIMARY_RUNTIME_COMPONENTS: [RuntimeComponentControl; 4] = [
    RuntimeComponentControl {
        binding_hash: WEAPON_TRIGGER_COMPONENT_KEY,
        label: "Firing Behavior",
        tooltip: "Changes firing cadence and trigger behavior. Stats and appearance are unchanged.",
    },
    RuntimeComponentControl {
        binding_hash: WEAPON_BARREL_COMPONENT_KEY,
        label: "Barrel Runtime",
        tooltip: "One part of projectile emission. Projectile speed can come from elsewhere.",
    },
    RuntimeComponentControl {
        binding_hash: WEAPON_MAGAZINE_COMPONENT_KEY,
        label: "Magazine Behavior",
        tooltip: "Magazine and reserve behavior. Ammo Type is on the Weapon tab.",
    },
    RuntimeComponentControl {
        binding_hash: WEAPON_RELOAD_COMPONENT_KEY,
        label: "Reload Behavior",
        tooltip: "The whole reload component, not only the animation. Depends on the other components.",
    },
];

pub(super) const ADDITIONAL_RUNTIME_COMPONENTS: [RuntimeComponentControl; 4] = [
    RuntimeComponentControl {
        binding_hash: WEAPON_STAT_TRANSLATOR_COMPONENT_KEY,
        label: "Weapon Stat Translator",
        tooltip: "Specific to the weapon family, not a projectile speed control. Swapping across families can freeze the game.",
    },
    RuntimeComponentControl {
        binding_hash: WEAPON_CONTROLLER_COMPONENT_KEY,
        label: "Weapon Controller",
        tooltip: "Broader than trigger or barrel. Affects several behaviors at once.",
    },
    RuntimeComponentControl {
        binding_hash: WEAPON_INPUT_COMPONENT_KEY,
        label: "Input",
        tooltip: "The weapon's input handling.",
    },
    RuntimeComponentControl {
        binding_hash: WEAPON_TRIGGER_CHARGE_COMPONENT_KEY,
        label: "Trigger Charge",
        tooltip: "Optional. The gameplay donor and component donor must both have it.",
    },
];

pub(super) fn runtime_component_control(binding_hash: u32) -> Option<RuntimeComponentControl> {
    PRIMARY_RUNTIME_COMPONENTS
        .into_iter()
        .chain(ADDITIONAL_RUNTIME_COMPONENTS)
        .find(|control| control.binding_hash == binding_hash)
}

//! The common choices Suggested lists first, and the behaviors whose uses it pools.
use super::*;

/// A choice Suggested puts ahead of the rest.
#[derive(Clone, Copy)]
pub(super) enum Lead {
    /// A kill trigger preset, whose kill filter is its own group.
    Kill(Trigger),
    /// Every group of a node kind.
    Kind(u8),
    /// A recipe, by its title.
    Recipe(&'static str),
}

/// The triggers most weapon perks start from, in the order Suggested lists them.
const TRIGGER_LEAD: [Lead; 13] = [
    Lead::Kill(Trigger::WeaponKill),
    Lead::Kill(Trigger::PrecisionKill),
    // While Equipped, While Drawn and Always Active.
    Lead::Kind(14),
    Lead::Kind(16),
    Lead::Kind(0),
    // On Dealing Damage, On Reloading and On Aiming Down Sights.
    Lead::Kind(4),
    Lead::Kind(19),
    Lead::Kind(23),
    Lead::Kill(Trigger::AnyKill),
    // On Taking Damage.
    Lead::Kind(5),
    Lead::Kill(Trigger::MeleeKill),
    Lead::Kill(Trigger::GrenadeKill),
    // On Picking Up Ammo.
    Lead::Kind(6),
];

/// The actions most weapon perks are built from, in the order Suggested lists them.
const ACTION_LEAD: [Lead; 14] = [
    // Change a Weapon or Ability Stat, Change Fired Projectile, Change Outgoing Damage and
    // Reload from Reserves.
    Lead::Kind(10),
    Lead::Kind(26),
    Lead::Kind(40),
    Lead::Kind(16),
    // Adjust Ammo by Capacity, Adjust Ammo and Change Ability Energy for every ability.
    Lead::Kind(15),
    Lead::Kind(14),
    Lead::Kind(8),
    // Spawn an Object or Effect, Attach an Effect and Generate Orbs of Light.
    Lead::Kind(3),
    Lead::Kind(1),
    Lead::Kind(5),
    Lead::Recipe(recipes::FULL_AUTO),
    // Change Damage Type, Change Incoming Damage and Extend Timers.
    Lead::Kind(6),
    Lead::Kind(33),
    Lead::Kind(32),
];

impl Lead {
    fn matches(self, family: &Family) -> bool {
        match (self, family) {
            (Self::Kill(trigger), Family::Condition(condition)) => {
                trigger_family(trigger).as_ref() == Some(condition)
            }
            (Self::Recipe(title), Family::Effect(_, label)) => label.as_deref() == Some(title),
            (Self::Kind(kind), _) => family.kind() == Some(kind),
            _ => false,
        }
    }
}

/// The conditions endings and requirements are most often built from, in the order Suggested
/// lists them. State checks come last because each state is a group of its own.
const CONDITION_LEAD: [Lead; 13] = [
    // Always, After a Delay, On Holster, On Unequip and On Reloading.
    Lead::Kind(0),
    Lead::Kind(1),
    Lead::Kind(17),
    Lead::Kind(15),
    Lead::Kind(19),
    Lead::Kill(Trigger::WeaponKill),
    Lead::Kill(Trigger::AnyKill),
    // On Taking Damage, On Dealing Damage, On Firing This Weapon, On Aiming Down Sights and
    // On Sprinting.
    Lead::Kind(5),
    Lead::Kind(4),
    Lead::Kind(27),
    Lead::Kind(23),
    Lead::Kind(25),
    // State Check.
    Lead::Kind(20),
];

impl Purpose {
    /// The common choices Suggested puts first in this picker. Choose a Condition serves
    /// endings and requirements, so it leads with what those use rather than the everyday
    /// triggers.
    pub(super) fn leads(self) -> &'static [Lead] {
        match self {
            Self::Trigger => &TRIGGER_LEAD,
            Self::Action => &ACTION_LEAD,
            Self::Condition => &CONDITION_LEAD,
        }
    }
}

/// Where Suggested places a group: its place among the picker's common choices, or after them
/// all.
pub(super) fn lead_rank(leads: &[Lead], family: &Family) -> usize {
    leads
        .iter()
        .position(|lead| lead.matches(family))
        .unwrap_or(usize::MAX)
}

/// The behavior a group is a variant of, whose uses Suggested pools: a kill with any filter,
/// or ability energy for any ability. Every other group counts its own uses.
pub(super) fn pool(family: &Family) -> Option<u8> {
    match family {
        Family::Condition(ConditionFamily::Kill { .. } | ConditionFamily::Kind(2)) => Some(2),
        Family::Effect(8, _) => Some(8),
        _ => None,
    }
}

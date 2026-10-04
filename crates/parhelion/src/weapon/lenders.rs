//! What each stock weapon can lend another through a part row: its type markers, which only a
//! weapon in the same content owner can take, and its first-person animations, which only a
//! weapon in the same attachment owner can take.
use crate::recipe::AnimationAction;
use crate::weapon::animations::actions::Machine;
use crate::{AuthoringResult, error::invalid};
use std::collections::BTreeMap;
use sundial::package_authoring::PackageManager;
use sundial::package_authoring::entity::weapon_component_bindings;
use sundial::package_authoring::runtime::load_weapon_runtime_entities_at_pattern_indices_with_manager;

const CONTENT: u32 = 0x5F0D_D954;
const ATTACHMENT: u32 = 0xD3A5_500E;
const BARREL: u32 = 0xEC71_1FA3;

/// The weapon type names the runtime's type markers name, by their FNV-1 hash. Each matches the
/// weapons of exactly that type in a survey of all 824 stock weapons. Fusion rifles, linear
/// fusion rifles, machine guns and several exotics carry keys whose names are not recovered.
const TYPE_NAMES: [&str; 13] = [
    "auto_rifle",
    "bow",
    "grenade_launcher",
    "hand_cannon",
    "pulse_rifle",
    "rocket_launcher",
    "scout_rifle",
    "shotgun",
    "sidearm",
    "sniper_rifle",
    "submachine_gun",
    "sword",
    "trace_rifle",
];

/// The native name a type marker's key stands for, when it is recovered.
pub(crate) fn type_name(key: u32) -> Option<&'static str> {
    TYPE_NAMES
        .iter()
        .copied()
        .find(|name| sundial::package_authoring::fnv1_name_hash(name) == key)
}

/// A type marker's name, or its key when the name is not recovered.
pub(crate) fn type_label(key: u32) -> String {
    type_name(key).map_or_else(|| format!("0x{key:08X}"), str::to_owned)
}

/// One stock weapon's owners and its own type marker.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Lender {
    pub(crate) content_owner: Option<u32>,
    /// The type name key its own content block carries.
    pub(crate) type_key: Option<u32>,
    /// The frame key beside it, which hand cannons set per frame.
    pub(crate) frame_key: Option<u32>,
    /// The two keys its first-person attachment row plays, its animation profile.
    pub(crate) animation_keys: Option<[u32; 2]>,
    pub(crate) attachment_owner: Option<u32>,
    /// The owner holding its trigger, barrel and magazine, which only a weapon with one can lend.
    pub(crate) gameplay_owner: Option<u32>,
    /// What its profile plays for each action in `AnimationAction::ALL` order, as a fingerprint
    /// of the branches the arms rig's states take: equal values play the same clips. Zero where
    /// the rig has no such action or its states do not test the profile.
    pub(crate) actions: [u32; AnimationAction::ALL.len()],
}

/// Reads every weapon in `weapons`, each an item hash and its pattern row. A weapon that cannot
/// be read is left out rather than failing the others.
pub(crate) fn index(
    manager: &PackageManager,
    weapons: &[(u32, u16)],
) -> AuthoringResult<BTreeMap<u32, Lender>> {
    let patterns = weapons
        .iter()
        .map(|(_, pattern)| *pattern)
        .collect::<Vec<_>>();
    let sources = load_weapon_runtime_entities_at_pattern_indices_with_manager(manager, &patterns)
        .map_err(invalid)?;
    let owner =
        |entity: &[u8], binding| match weapon_component_bindings(entity, binding).ok().as_deref() {
            Some([binding]) => Some(binding.owner_tag),
            _ => None,
        };
    let mut contents = BTreeMap::new();
    // One read of each arms rig's action states, shared by every weapon of its owner.
    let mut machines = BTreeMap::<u32, Option<Machine>>::new();
    let mut lenders = BTreeMap::new();
    for (&(item_hash, _), source) in weapons.iter().zip(sources) {
        let Ok(source) = source else {
            continue;
        };
        let content_owner = owner(&source.payload, CONTENT);
        let group = source.weapon_content_group_hash;
        let content = content_owner.and_then(|tag| {
            contents
                .entry(tag)
                .or_insert_with(|| crate::weapon::behavior::content(manager, &source.payload).ok())
                .as_ref()
        });
        let type_key =
            content.and_then(|content| crate::weapon::behavior::type_name_key(content, group));
        let frame_key =
            content.and_then(|content| crate::weapon::behavior::frame_key(content, group));
        let animation_keys =
            crate::weapon::animations::profile(manager, &source.payload, Some(group))
                .ok()
                .map(|profile| profile.keys);
        let attachment_owner = owner(&source.payload, ATTACHMENT);
        let machine = attachment_owner.and_then(|tag| {
            machines
                .entry(tag)
                .or_insert_with(|| {
                    let rig =
                        crate::weapon::animations::arms_rig(manager, &source.payload, Some(group))
                            .ok()?;
                    Machine::read(rig.states, &rig.parameters).ok()
                })
                .as_ref()
        });
        let actions = AnimationAction::ALL.map(|action| {
            machine
                .zip(animation_keys)
                .and_then(|(machine, keys)| machine.outputs(action, keys[1]).ok().flatten())
                .map_or(0, |outputs| fingerprint(&outputs))
        });
        lenders.insert(
            item_hash,
            Lender {
                content_owner,
                type_key,
                frame_key,
                animation_keys,
                attachment_owner,
                gameplay_owner: owner(&source.payload, BARREL),
                actions,
            },
        );
    }
    Ok(lenders)
}

/// A nonzero FNV-1a fingerprint of an action's branch outputs.
fn fingerprint(outputs: &[i16]) -> u32 {
    let mut hash = 0x811C_9DC5_u32;
    for byte in outputs.iter().flat_map(|output| output.to_le_bytes()) {
        hash = (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193);
    }
    hash.max(1)
}

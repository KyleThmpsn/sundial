//! Projectile sources reached through typed perk-action graph references.
//! Replacing a projectile preserves the source action's trigger and host requirements.
use std::collections::BTreeSet;

use crate::package_runtime::reader::PackageManager;
use serde::{Deserialize, Serialize};
use tiger_pkg::TagHash;

use super::{SandboxPerkRuntimeAction, SandboxPerkRuntimeGraphSource};
use crate::weapon_entity::{WEAPON_ENTITY_CLASS, validate_weapon_entity};

pub mod catalog;
pub mod parameters;
pub mod residency;

/// Shadowkeep entity object type, also used by Sunrise's projectile spawner.
const OBJECT_TYPE_OFFSET: usize = 0x96;
const PROJECTILE_OBJECT_TYPE: u8 = 18;
const EMITTER_OBJECT_TYPE: u8 = 17;

/// Object types that stock Create Entity nodes attach, measured over the 1,010 references in
/// the surveyed actions: type 23 (452), 28 (380), 24 (30), 17 (17), 25 (4), 26 (2), 22 (1)
/// and 14 (1). An entity of one of these types is what the asset picker offers to attach,
/// whether or not a stock perk references it.
pub const ATTACHED_OBJECT_TYPES: [u8; 8] = [14, 17, 22, 23, 24, 25, 26, 28];

/// The client's object-type names from its placed-content table.
#[must_use]
pub const fn object_type_name(object_type: u8) -> Option<&'static str> {
    match object_type {
        0 => Some("inherited"),
        1 => Some("static_mesh"),
        2 => Some("prop_simple_deprecated"),
        3 => Some("prop_expensive_deprecated"),
        4 => Some("prop_cosmetic_static"),
        5 => Some("prop_cosmetic_movable"),
        6 => Some("prop_cosmetic_movable_garbage"),
        7 => Some("prop_networked_static"),
        8 => Some("prop_networked_movable"),
        9 => Some("prop_cinematic"),
        10 => Some("speedtree"),
        11 => Some("interactive"),
        12 => Some("biped"),
        13 => Some("creature"),
        14 => Some("weapon"),
        15 => Some("vehicle"),
        16 => Some("turret"),
        17 => Some("emitter"),
        18 => Some("projectile"),
        19 => Some("item"),
        20 => Some("item_ammo"),
        21 => Some("item_loot"),
        22 => Some("gear"),
        23 => Some("hop_on"),
        24 => Some("hop_on_gear_biped"),
        25 => Some("hop_on_gear_weapon"),
        26 => Some("hop_on_gear_ship"),
        27 => Some("hop_on_gear_sparrow"),
        28 => Some("system"),
        _ => None,
    }
}

/// A readable label for an object type: the client's name when recorded, else the number.
#[must_use]
pub fn object_type_label(object_type: u8) -> String {
    object_type_name(object_type).map_or_else(
        || format!("type {object_type}"),
        std::borrow::ToOwned::to_owned,
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Kind {
    Projectile,
    Emitter,
    /// Native item, ammunition or loot object. Uses generic object creation.
    Pickup,
    /// Physical props and interactable world objects, including native health orbs.
    Object,
    /// Any other entity graph of a type stock perks attach. Not a spawn or pattern candidate.
    Entity,
}

impl Kind {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Projectile => "Projectile",
            Self::Emitter => "Emitter",
            Self::Pickup => "Pickup",
            Self::Object => "World Object",
            Self::Entity => "Entity",
        }
    }

    /// Whether the compiler accepts this kind for a spawn action.
    #[must_use]
    pub const fn spawnable(self) -> bool {
        !matches!(self, Self::Entity)
    }
}

/// Replace one directly referenced projectile without replacing the perk's action.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub source_graph: u32,
    pub donor_graph: u32,
}

/// Uses the entity's native object type, including ground waves and bolt variants
/// that do not have the moving-projectile component.
pub fn is_projectile(payload: &[u8]) -> Result<bool, String> {
    Ok(kind(payload)? == Some(Kind::Projectile))
}

pub fn kind(payload: &[u8]) -> Result<Option<Kind>, String> {
    validate_weapon_entity(payload)?;
    Ok(match payload.get(OBJECT_TYPE_OFFSET) {
        Some(&PROJECTILE_OBJECT_TYPE) => Some(Kind::Projectile),
        Some(&EMITTER_OBJECT_TYPE) => Some(Kind::Emitter),
        _ => None,
    })
}

/// Kinds accepted by the generic create-object route. Projectile replacement still uses
/// `kind` so a world object cannot become a weapon's fired pattern by accident.
pub fn spawn_kind(payload: &[u8]) -> Result<Option<Kind>, String> {
    if let Some(kind) = kind(payload)? {
        return Ok(Some(kind));
    }
    Ok(match payload.get(OBJECT_TYPE_OFFSET) {
        Some(19..=21) => Some(Kind::Pickup),
        Some(1..=8 | 11) => Some(Kind::Object),
        _ => None,
    })
}

/// Resolve replacements before applying scalar edits. Offsets still address the source action.
pub fn resolve(
    manager: &PackageManager,
    action: &SandboxPerkRuntimeAction,
    selections: &[Selection],
) -> Result<Vec<SandboxPerkRuntimeGraphSource>, String> {
    let mut graphs = action.graphs.clone();
    if selections.is_empty() {
        return Ok(graphs);
    }
    let mut seen = BTreeSet::new();
    for selection in selections {
        if !seen.insert(selection.source_graph) {
            return Err("Two projectile selections target the same source graph".into());
        }
        let source = action
            .graphs
            .iter()
            .position(|graph| graph.tag.0 == selection.source_graph)
            .ok_or_else(|| {
                format!(
                    "Projectile source 0x{:08X} is not directly referenced by this perk",
                    selection.source_graph
                )
            })?;
        if kind(&action.graphs[source].payload)?.is_none() {
            return Err("The selected source graph is not a projectile or emitter".into());
        }
        graphs[source] = SandboxPerkRuntimeGraphSource {
            tag: TagHash(selection.donor_graph),
            payload: load(manager, selection.donor_graph)?,
            action_offsets: action.graphs[source].action_offsets.clone(),
        };
    }
    Ok(graphs)
}

/// A donor may come from a weapon, ability, enemy, or private authored resource.
pub fn load(manager: &PackageManager, tag: u32) -> Result<Vec<u8>, String> {
    let entry = manager
        .get_entry(TagHash(tag))
        .ok_or_else(|| format!("Projectile 0x{tag:08X} is not installed"))?;
    if entry.file_type != 8 || entry.reference != WEAPON_ENTITY_CLASS {
        return Err(format!("Projectile 0x{tag:08X} is not an entity graph"));
    }
    let payload = manager
        .read_tag(TagHash(tag))
        .map_err(|error| error.to_string())?;
    if payload.len() != entry.file_size as usize || kind(&payload)?.is_none() {
        return Err(format!(
            "Entity 0x{tag:08X} is not a valid projectile or emitter"
        ));
    }
    Ok(payload)
}

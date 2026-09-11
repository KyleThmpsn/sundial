//! Projectile sources reached through typed perk-action graph references.
//! Replacing a projectile preserves the source action's trigger and host requirements.
use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use tiger_pkg::{PackageManager, TagHash};

use super::{SandboxPerkRuntimeAction, SandboxPerkRuntimeGraphSource};
use crate::weapon_entity::{WEAPON_ENTITY_CLASS, validate_weapon_entity};

pub mod catalog;
pub mod parameters;

/// Shadowkeep entity object type, also used by Sunrise's projectile spawner.
const OBJECT_TYPE_OFFSET: usize = 0x96;
const PROJECTILE_OBJECT_TYPE: u8 = 18;
const EMITTER_OBJECT_TYPE: u8 = 17;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Kind {
    Projectile,
    Emitter,
}

impl Kind {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Projectile => "Projectile",
            Self::Emitter => "Emitter",
        }
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

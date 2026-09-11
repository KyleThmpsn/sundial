//! Complete structural inventory of pattern hosts and perk-supplied graphs.
//! Stock associations and resolved graphs are evidence, never compatibility gates.
use std::{collections::BTreeMap, path::Path, sync::Arc};

pub mod content;

use serde::{Deserialize, Serialize};
use tiger_pkg::{PackageManager, TagHash};

use super::{
    FINISHED_SANDBOX_PERK_CATALOG_CLASS, SANDBOX_PERK_RUNTIME_MAP_TAG, finished_sandbox_perk_at,
    finished_sandbox_perk_count, load_sandbox_perk_runtime_action, sandbox_perk_runtime_assignment,
    validate_sandbox_perk_runtime_map,
};
use crate::{
    investment_schema::{
        GLOBALS_FINISHED_SANDBOX_PERK_TABLE_SLOT, GLOBALS_SANDBOX_PATTERN_TABLE_SLOT,
        investment_globals_table_tag,
    },
    package_payload::{native_array_at, u32_at},
    package_runtime::resolve_live_named_tag,
    weapon_entity::{
        WEAPON_ENTITY_CLASS, sandbox_pattern_identity_at, validate_weapon_entity,
        weapon_component_binding_hashes, weapon_component_bindings, weapon_entity_assignment,
    },
};

/// A component selected by a validated entity binding. This is not a host requirement.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Component {
    pub binding: u32,
    pub owner: u32,
    pub class: u32,
    pub offset: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entity {
    pub tag: u32,
    pub components: Vec<Component>,
}

/// Every pattern row is retained, including inactive and unresolved rows.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pattern {
    pub index: usize,
    pub item_hash: u32,
    pub runtime_key: u32,
    pub translation_group: u32,
    pub entity: Option<Entity>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Perk {
    pub index: usize,
    pub hash: u32,
    pub runtime_key: u32,
    pub action: Option<u32>,
    pub graphs: Vec<Entity>,
    pub error: Option<String>,
}

impl Perk {
    /// An unassigned marker can still participate in behavior supplied elsewhere.
    #[must_use]
    pub fn status(&self) -> &'static str {
        if self.error.is_some() {
            "Inspection Failed"
        } else if self.action.is_none() {
            "No Standalone Action"
        } else if self.graphs.is_empty() {
            "Action With No Direct Entity Graph"
        } else {
            "Action With Entity Graphs"
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Index {
    pub patterns: Vec<Pattern>,
    pub perks: Vec<Perk>,
    /// Exact stock configuration, not a claim that a stock pattern is the only valid host.
    pub caster: Option<CasterConfiguration>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CasterConfiguration {
    pub perk_index: usize,
    pub pattern_index: usize,
    pub binding: u32,
    pub owner: u32,
    pub projectile_graphs: Vec<Entity>,
}

static CACHE: crate::package_runtime::index_cache::Cache<Index> =
    crate::package_runtime::index_cache::Cache::new();

/// Reuse structural evidence across editor opens and application restarts.
pub fn cached(
    packages: &Path,
    manager: &PackageManager,
    progress: impl FnMut(usize, usize),
) -> Result<Arc<Index>, String> {
    crate::package_runtime::index_cache::cached(
        packages,
        "native-indexes",
        "dependencies-v1",
        &CACHE,
        || inspect(manager, progress),
        |_| true,
    )
}

fn caster_configuration(manager: &PackageManager, index: &Index) -> Option<CasterConfiguration> {
    // Authenticate the known Shadowkeep configuration before describing the marker's role.
    // Alternate/private configurations remain unmapped rather than inheriting this claim.
    let perk = index.perks.get(2002)?;
    let pattern = index.patterns.get(395)?;
    if perk.action.is_some()
        || perk.error.is_some()
        || perk.runtime_key != 0x811C_9DC5
        || pattern.item_hash != 0x0222_2CBF
        || pattern.runtime_key != 0xF46D_6805
    {
        return None;
    }
    let host = pattern.entity.as_ref()?;
    if host.tag != 0x81A6_ABFC
        || !host.components.iter().any(|component| {
            component.binding == 0x2D8A_944C
                && component.class == 0x8080_43D2
                && component.owner == 0x81A6_ABFB
        })
    {
        return None;
    }
    let owner = read(manager, 0x81A6_ABFB).ok()?;
    let mut projectiles = Vec::new();
    for (offset, expected) in [(0x54A8, 0x81A6_AB70), (0x56A8, 0x81A6_ABA5)] {
        if u32_at(&owner, offset).ok()? != expected {
            return None;
        }
        projectiles.push(entity(manager, expected).ok()?);
    }
    Some(CasterConfiguration {
        perk_index: 2002,
        pattern_index: 395,
        binding: 0x2D8A_944C,
        owner: 0x81A6_ABFB,
        projectile_graphs: projectiles,
    })
}

fn read(manager: &PackageManager, tag: u32) -> Result<Vec<u8>, String> {
    manager
        .read_tag(TagHash(tag))
        .map_err(|error| format!("Could not read dependency tag 0x{tag:08X}: {error}"))
}

fn entity(manager: &PackageManager, tag: u32) -> Result<Entity, String> {
    let entry = manager
        .get_entry(TagHash(tag))
        .ok_or_else(|| format!("Entity 0x{tag:08X} is not live"))?;
    if entry.file_type != 8 || entry.reference != WEAPON_ENTITY_CLASS {
        return Err(format!(
            "Entity 0x{tag:08X} is not a structured entity resource"
        ));
    }
    let payload = read(manager, tag)?;
    if usize::try_from(entry.file_size).ok() != Some(payload.len()) {
        return Err(format!(
            "Entity 0x{tag:08X} package size disagrees with its payload"
        ));
    }
    validate_weapon_entity(&payload)?;
    let mut components = Vec::new();
    for binding in weapon_component_binding_hashes(&payload)? {
        for resource in weapon_component_bindings(&payload, binding)? {
            components.push(Component {
                binding,
                owner: resource.owner_tag,
                class: resource.concrete_class,
                offset: resource.resource_offset,
            });
        }
    }
    Ok(Entity { tag, components })
}

/// Inventories every native pattern and finished-perk row from the same package snapshot.
/// Direct graph discovery does not follow selectors or prove event/host compatibility.
pub fn inspect(
    manager: &PackageManager,
    mut progress: impl FnMut(usize, usize),
) -> Result<Index, String> {
    let globals_tag = resolve_live_named_tag(manager, "investment_globals", None)?;
    let globals = read(manager, globals_tag.0)?;
    let patterns = read(
        manager,
        investment_globals_table_tag(&globals, GLOBALS_SANDBOX_PATTERN_TABLE_SLOT)?,
    )?;
    let perks_tag =
        investment_globals_table_tag(&globals, GLOBALS_FINISHED_SANDBOX_PERK_TABLE_SLOT)?;
    if manager
        .get_entry(TagHash(perks_tag))
        .is_none_or(|entry| entry.reference != FINISHED_SANDBOX_PERK_CATALOG_CLASS)
    {
        return Err("Finished-perk dependency source has the wrong package class".into());
    }
    let perks = read(manager, perks_tag)?;
    let assignments = read(manager, SANDBOX_PERK_RUNTIME_MAP_TAG)?;
    validate_sandbox_perk_runtime_map(&assignments)?;
    let pattern_count = native_array_at(&patterns, 8)?.0;
    let perk_count = finished_sandbox_perk_count(&perks)?;
    let total = pattern_count
        .checked_add(perk_count)
        .ok_or("Dependency inventory count overflowed")?;
    let mut result = Index::default();
    let mut entities = BTreeMap::<u32, Result<Entity, String>>::new();
    for index in 0..pattern_count {
        let row = sandbox_pattern_identity_at(&patterns, index)?
            .ok_or("Pattern row disappeared during inspection")?;
        let resolved = if matches!(row.item_hash, 0 | 0x811C_9DC5)
            || matches!(row.pattern_global_id_hash, 0 | 0x811C_9DC5)
        {
            Err("Inactive pattern row".to_owned())
        } else {
            weapon_entity_assignment(&assignments, row.pattern_global_id_hash)?
                .ok_or_else(|| "Pattern runtime key is not assigned".to_owned())
                .and_then(|tag| {
                    entities
                        .entry(tag)
                        .or_insert_with(|| entity(manager, tag))
                        .clone()
                })
        };
        let (entity, error) = match resolved {
            Ok(entity) => (Some(entity), None),
            Err(error) => (None, Some(error)),
        };
        result.patterns.push(Pattern {
            index,
            item_hash: row.item_hash,
            runtime_key: row.pattern_global_id_hash,
            translation_group: row.weapon_translation_group_hash,
            entity,
            error,
        });
        progress(index + 1, total);
    }
    let mut actions = BTreeMap::<u32, Result<Vec<Entity>, String>>::new();
    for index in 0..perk_count {
        let row = finished_sandbox_perk_at(&perks, index)?;
        let assignment = sandbox_perk_runtime_assignment(&assignments, row.runtime_key)?;
        let action_tag = assignment.map(|assignment| assignment.runtime_tag);
        let mut perk = Perk {
            index,
            hash: row.perk_hash,
            runtime_key: row.runtime_key,
            action: action_tag,
            graphs: Vec::new(),
            error: None,
        };
        if let Some(tag) = action_tag {
            let inspected = actions.entry(tag).or_insert_with(|| {
                let action = load_sandbox_perk_runtime_action(manager, &globals, index)?;
                action
                    .graphs
                    .iter()
                    .map(|graph| {
                        entities
                            .entry(graph.tag.0)
                            .or_insert_with(|| entity(manager, graph.tag.0))
                            .clone()
                    })
                    .collect()
            });
            match inspected {
                Ok(graphs) => perk.graphs.clone_from(graphs),
                Err(error) => perk.error = Some(error.clone()),
            }
        }
        result.perks.push(perk);
        progress(pattern_count + index + 1, total);
    }
    result.caster = caster_configuration(manager, &result);
    Ok(result)
}

#[cfg(test)]
mod tests;

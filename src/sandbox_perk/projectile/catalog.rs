//! Discover installed projectiles independently of whether a perk references them.
use std::{collections::BTreeMap, path::Path, sync::Arc};

use serde::{Deserialize, Serialize};
use tiger_pkg::PackageManager;

use super::*;
use crate::{
    package_runtime::{index_cache, tft},
    sandbox_perk::dependencies,
    weapon_entity::{weapon_component_binding_hashes, weapon_component_bindings},
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub graph: u32,
    pub kind: Kind,
    pub owners: Vec<u32>,
    pub package: String,
    pub native_name: Option<String>,
    pub native_paths: Vec<String>,
    pub contexts: Vec<Context>,
    pub perk_indices: Vec<u16>,
}

impl Entry {
    /// A direct native identity, followed by explicitly qualified usage context.
    /// A parent's filename must never be presented as this projectile's own name.
    pub fn label(&self) -> String {
        self.label_with_perks(|_| None)
    }

    pub fn label_with_perks(&self, mut perk_name: impl FnMut(u16) -> Option<String>) -> String {
        if let Some(path) = self
            .native_paths
            .iter()
            .find(|path| !path.trim().is_empty())
        {
            return tft::asset_label(path);
        }
        if let Some(name) = self
            .native_name
            .as_ref()
            .filter(|name| !name.trim().is_empty())
        {
            return name.clone();
        }
        let contexts = self
            .contexts
            .iter()
            .filter(|context| !context.path.trim().is_empty())
            .map(|context| tft::asset_label(&context.path))
            .collect::<BTreeSet<_>>();
        if let Some(first) = contexts.first() {
            let more = if contexts.len() > 1 {
                format!(" (+{})", contexts.len() - 1)
            } else {
                String::new()
            };
            return format!("{} · Referenced by {first}{more}", self.kind.label());
        }
        let perks = self
            .perk_indices
            .iter()
            .map(|&index| {
                perk_name(index)
                    .filter(|name| !name.trim().is_empty())
                    .unwrap_or_else(|| format!("Effect {index}"))
            })
            .collect::<BTreeSet<_>>();
        if let Some(first) = perks.first() {
            let more = if perks.len() > 1 {
                format!(" (+{})", perks.len() - 1)
            } else {
                String::new()
            };
            return format!("{} · Used by {first}{more}", self.kind.label());
        }
        format!("Unidentified {}", self.kind.label())
    }

    pub fn label_rank(&self) -> u8 {
        if self.native_paths.iter().any(|path| !path.trim().is_empty())
            || self
                .native_name
                .as_ref()
                .is_some_and(|name| !name.trim().is_empty())
        {
            0
        } else if self
            .contexts
            .iter()
            .any(|context| !context.path.trim().is_empty())
        {
            1
        } else if !self.perk_indices.is_empty() {
            2
        } else {
            3
        }
    }
}

/// A named parent resource contains this projectile tag. It describes usage,
/// not the projectile's identity or a guarantee about a native execution path.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Context {
    pub graph: u32,
    pub owner: u32,
    pub offset: usize,
    pub path: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Catalog {
    pub entries: Vec<Entry>,
    pub errors: Vec<String>,
}

fn owners(payload: &[u8]) -> Result<Vec<u32>, String> {
    let mut owners = BTreeSet::new();
    for binding in weapon_component_binding_hashes(payload)? {
        for resource in weapon_component_bindings(payload, binding)? {
            owners.insert(resource.owner_tag);
        }
    }
    Ok(owners.into_iter().collect())
}

pub fn inspect(
    manager: &PackageManager,
    index: &dependencies::Index,
    names: &tft::Index,
) -> Result<Catalog, String> {
    let mut catalog = Catalog::default();
    let native_names = names.names();
    catalog.errors.extend(names.errors.iter().cloned());
    let mut references = BTreeMap::<u32, Vec<u16>>::new();
    for perk in &index.perks {
        if let Ok(perk_index) = u16::try_from(perk.index) {
            for graph in &perk.graphs {
                references.entry(graph.tag).or_default().push(perk_index);
            }
        }
    }
    let mut entries = manager.get_all_by_reference(WEAPON_ENTITY_CLASS);
    entries.sort_by_key(|(tag, _)| tag.0);
    for (tag, entry) in entries {
        if entry.file_type != 8 {
            continue;
        }
        let payload = match manager.read_tag(tag) {
            Ok(payload) => payload,
            Err(error) => {
                catalog.errors.push(format!("{tag}: {error}"));
                continue;
            }
        };
        if !matches!(
            payload.get(OBJECT_TYPE_OFFSET),
            Some(&PROJECTILE_OBJECT_TYPE | &EMITTER_OBJECT_TYPE)
        ) {
            continue;
        }
        let kind = match kind(&payload) {
            Ok(Some(kind)) => kind,
            Ok(None) => continue,
            Err(error) => {
                catalog.errors.push(format!("{tag}: {error}"));
                continue;
            }
        };
        let owners = match owners(&payload) {
            Ok(owners) => owners,
            Err(error) => {
                catalog.errors.push(format!("{tag}: {error}"));
                continue;
            }
        };
        catalog.entries.push(Entry {
            graph: tag.0,
            kind,
            owners,
            package: manager
                .package_paths
                .get(&tag.pkg_id())
                .map(|package| package.name.clone())
                .unwrap_or_default(),
            native_name: manager.get_tag_name(tag),
            native_paths: native_names.get(&tag.0).cloned().unwrap_or_default(),
            contexts: Vec::new(),
            perk_indices: references.remove(&tag.0).unwrap_or_default(),
        });
    }
    attach_context(manager, &mut catalog, &native_names);
    Ok(catalog)
}

fn attach_context(
    manager: &PackageManager,
    catalog: &mut Catalog,
    names: &BTreeMap<u32, Vec<String>>,
) {
    let positions = catalog
        .entries
        .iter()
        .enumerate()
        .map(|(index, entry)| (entry.graph, index))
        .collect::<BTreeMap<_, _>>();
    for (&graph, paths) in names {
        if manager
            .get_entry(TagHash(graph))
            .is_none_or(|entry| entry.reference != WEAPON_ENTITY_CLASS)
        {
            continue;
        }
        let Ok(payload) = manager.read_tag(TagHash(graph)) else {
            continue;
        };
        let Ok(owners) = owners(&payload) else {
            continue;
        };
        for owner in owners {
            let Ok(payload) = manager.read_tag(TagHash(owner)) else {
                continue;
            };
            for (offset, word) in payload.chunks_exact(4).enumerate() {
                let target = u32::from_le_bytes(word.try_into().expect("four-byte chunk"));
                let Some(&index) = positions.get(&target) else {
                    continue;
                };
                for path in paths {
                    catalog.entries[index].contexts.push(Context {
                        graph,
                        owner,
                        offset: offset * 4,
                        path: path.clone(),
                    });
                }
            }
        }
    }
}

static CACHE: index_cache::Cache<Catalog> = index_cache::Cache::new();

/// Keep repeat parameter opens fast and invalidate after any package changes.
pub fn cached(packages: &Path, manager: &PackageManager) -> Result<Arc<Catalog>, String> {
    index_cache::cached(
        packages,
        "native-indexes",
        "projectiles-v1",
        &CACHE,
        || {
            let dependencies = dependencies::cached(packages, manager, |_, _| {})?;
            let names = tft::cached(packages, manager, |_, _| {})?;
            inspect(manager, &dependencies, &names)
        },
        // Preserve incomplete-discovery warnings along with usable results.
        // Compiling a selected effect still reads and validates its live tags.
        |_| true,
    )
}

#[cfg(test)]
mod tests;

//! Structural loading prerequisites. Passing this check is not a firing compatibility claim.
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use tiger_pkg::{PackageManager, TagHash};

use crate::{
    package_payload::{native_array_at, u32_at, u64_at},
    package_runtime::{loading, references},
    weapon_entity::{WEAPON_ENTITY_CLASS, validate_weapon_entity},
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Requirement {
    pub tag: u32,
    pub referenced_by: u32,
    pub role: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Report {
    pub graph: u32,
    pub graph_loaded: bool,
    pub missing: Vec<Requirement>,
}

impl Report {
    /// Existing native records that must be enrolled with an authored reference. These are
    /// typed component prerequisites, not an activity-wide dependency index or guessed tags.
    pub fn additions(&self) -> BTreeSet<u32> {
        let mut additions = self
            .missing
            .iter()
            .map(|entry| entry.tag)
            .collect::<BTreeSet<_>>();
        if !self.graph_loaded {
            additions.insert(self.graph);
        }
        additions
    }

    pub fn check(&self, role: &str, graph_is_cloned: bool) -> Result<(), String> {
        if self.missing.is_empty() && (graph_is_cloned || self.graph_loaded) {
            return Ok(());
        }
        let mut missing = self
            .missing
            .iter()
            .map(|entry| {
                format!(
                    "{} 0x{:08X} referenced by 0x{:08X}",
                    entry.role, entry.tag, entry.referenced_by
                )
            })
            .collect::<Vec<_>>();
        if !graph_is_cloned && !self.graph_loaded {
            missing.insert(0, format!("entity graph 0x{:08X}", self.graph));
        }
        let total = missing.len();
        missing.truncate(4);
        Err(format!(
            "{role} 0x{:08X} needs {total} resources that are absent from the investment loading index: {}. Rebuild the authored package to include these dependencies before loading this asset.",
            self.graph,
            missing.join(", ")
        ))
    }
}

/// Checks the graph's entire native component list, including components with no named binding.
/// Returns missing typed prerequisites for enrollment with an authored reference. It does not
/// infer a complete transitive import from aligned words, paths or an activity loading index.
pub fn inspect(manager: &PackageManager, graph: u32) -> Result<Report, String> {
    let loaded = loading::investment(manager)?;
    let payload = loading::read(manager, graph, 8, WEAPON_ENTITY_CLASS)?;
    validate_weapon_entity(&payload)?;
    let mut required = BTreeMap::new();
    for owner in component_owners(&payload)? {
        required.insert(
            owner,
            Requirement {
                tag: owner,
                referenced_by: graph,
                role: "component resource".into(),
            },
        );
        let data = loading::read(manager, owner, 8, 0x8080_9C36)?;
        for requirement in owner_requirements(manager, owner, &data)? {
            required.entry(requirement.tag).or_insert(requirement);
        }
    }
    for reference in references::closure(manager, [graph])? {
        required
            .entry(reference.tag)
            .or_insert_with(|| Requirement {
                tag: reference.tag,
                referenced_by: reference.parent,
                role: if reference.offset == usize::MAX {
                    "resource backing data".into()
                } else {
                    format!("native reference at +0x{:X}", reference.offset)
                },
            });
    }
    Ok(report(graph, &loaded, required.into_values()))
}

fn report(
    graph: u32,
    loaded: &BTreeSet<u32>,
    required: impl IntoIterator<Item = Requirement>,
) -> Report {
    Report {
        graph,
        graph_loaded: loaded.contains(&graph),
        missing: required
            .into_iter()
            .filter(|entry| !loaded.contains(&entry.tag))
            .collect(),
    }
}

fn component_owners(payload: &[u8]) -> Result<BTreeSet<u32>, String> {
    let (count, _, rows, class) = native_array_at(payload, 0x10)?;
    if class != 0x8080_9C04
        || rows
            .checked_add(count.saturating_mul(12))
            .is_none_or(|end| end > payload.len())
    {
        return Err("Entity component list has unsupported bounds or class".into());
    }
    (0..count).map(|i| u32_at(payload, rows + i * 12)).collect()
}

fn owner_requirements(
    manager: &PackageManager,
    owner: u32,
    payload: &[u8],
) -> Result<Vec<Requirement>, String> {
    let mut result = Vec::new();
    let template = u32_at(payload, 0x44)?;
    if !matches!(template, 0 | u32::MAX | 0x811C_9DC5) {
        loading::read(manager, template, 8, 0x8080_9BBB)?;
        result.push(Requirement {
            tag: template,
            referenced_by: owner,
            role: "component layout".into(),
        });
    }
    for pointer in [0x10, 0x18] {
        let relative = u64_at(payload, pointer)?;
        if relative == 0 {
            continue;
        }
        let start = pointer
            .checked_add(
                usize::try_from(relative)
                    .map_err(|_| "Component root offset exceeds this platform")?,
            )
            .ok_or("Component root offset overflow")?;
        if start < 4 || start > payload.len() {
            return Err("Component root lies outside its owner".into());
        }
        let schema = u32_at(payload, start - 4)?;
        if schema & 0xFFF0_0000 == 0x8080_0000 {
            continue;
        }
        if manager
            .get_entry(TagHash(schema))
            .is_none_or(|entry| entry.reference != 0x8080_0000 || entry.file_type != 8)
        {
            return Err(format!(
                "Component 0x{owner:08X} has an unavailable generated schema 0x{schema:08X}"
            ));
        }
        result.push(Requirement {
            tag: schema,
            referenced_by: owner,
            role: "generated component schema".into(),
        });
    }
    Ok(result)
}

#[cfg(test)]
mod tests;

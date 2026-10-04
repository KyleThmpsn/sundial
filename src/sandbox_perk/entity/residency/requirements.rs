//! Structural loading prerequisites. Meeting them is not a firing compatibility claim.
use std::collections::{BTreeMap, BTreeSet};

use crate::package_runtime::reader::PackageManager;
use serde::{Deserialize, Serialize};
use tiger_pkg::TagHash;

use crate::{
    entity::{WEAPON_ENTITY_CLASS, validate_weapon_entity},
    package_payload::{native_array_at, u32_at, u64_at},
    package_runtime::{loading, references},
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
}

/// Checks the graph's entire native component list, including components with no named binding.
/// Returns missing typed prerequisites for enrollment with an authored reference. It does not
/// infer a complete transitive import from aligned words, paths or an activity loading index.
pub fn inspect(manager: &PackageManager, graph: u32) -> Result<Report, String> {
    let loaded = loading::investment(manager)?;
    Ok(report(
        graph,
        &loaded,
        requirements(manager, &BTreeSet::from([graph]))?.into_values(),
    ))
}

/// The enrollment additions of several graphs, as the union of each graph's
/// [`Report::additions`]. Donor graphs share most of their reference closure, so one walk
/// and one read of the loading index replace one of each per graph.
pub fn additions(
    manager: &PackageManager,
    graphs: impl IntoIterator<Item = u32>,
) -> Result<BTreeSet<u32>, String> {
    let graphs = graphs.into_iter().collect::<BTreeSet<_>>();
    if graphs.is_empty() {
        return Ok(BTreeSet::new());
    }
    let loaded = loading::investment(manager)?;
    let required = requirements(manager, &graphs)?;
    Ok(graphs
        .into_iter()
        .chain(required.into_keys())
        .filter(|tag| !loaded.contains(tag))
        .collect())
}

fn requirements(
    manager: &PackageManager,
    graphs: &BTreeSet<u32>,
) -> Result<BTreeMap<u32, Requirement>, String> {
    let mut owners = BTreeMap::new();
    for &graph in graphs {
        let payload = loading::read(manager, graph, 8, WEAPON_ENTITY_CLASS)?;
        validate_weapon_entity(&payload)?;
        for owner in component_owners(&payload)? {
            owners.entry(owner).or_insert(graph);
        }
    }
    let mut required = BTreeMap::new();
    for (owner, graph) in owners {
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
    for reference in references::closure(manager, graphs.iter().copied())? {
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
    Ok(required)
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

/// Read the native layout prerequisites of a component payload, including private copies.
pub fn owner_requirements(
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
    // Optic providers also expose event inputs. Their dispatch helpers live in
    // the component's typed resource views, independently of the entity's
    // interface registrations. These fields are declared tag references in
    // native class 8080393B, not a scan for tag-looking words.
    let relative = crate::package_payload::i64_at(payload, 0x18)?;
    if relative == 0 {
        return Ok(result);
    }
    let data = crate::package_payload::relative_offset(0x18, 0, relative)?;
    if data >= 4 && u32_at(payload, data - 4)? == 0x8080_393B {
        crate::package_payload::bytes_at::<0x290>(payload, data)?;
        for field in [0x50, 0x1B0, 0x1C8, 0x240, 0x258, 0x270] {
            let helper = u32_at(payload, data + field)?;
            loading::read(manager, helper, 8, 0x8080_9C54)?;
            result.push(Requirement {
                tag: helper,
                referenced_by: owner,
                role: format!("optic dispatch at +0x{field:X}"),
            });
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests;

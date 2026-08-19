use std::{collections::HashMap, mem::size_of};

use serde::{Deserialize, Serialize};
use tiger_pkg::{PackageManager, TagHash};

use super::{
    package::{array_at, bool_at, i32_at, i64_at, relative_offset, u16_at, u32_at, u64_at},
    resolve_string,
};

const OBJECTIVE_DEFINITION_TABLE_SLOT: usize = 58;
const OBJECTIVE_STRING_TABLE_SLOT: usize = 38;
const OBJECTIVE_DEFINITION_ROW_SIZE: usize = 160;
const OBJECTIVE_STRING_ROW_SIZE: usize = 64;
const OBJECTIVE_COMPLETION_VALUE_OFFSET: usize = 0x30;
const OBJECTIVE_ALLOW_OVERCOMPLETION_OFFSET: usize = 0x29;
const OBJECTIVE_ALLOW_NEGATIVE_VALUE_OFFSET: usize = 0x2A;
const OBJECTIVE_ALLOW_VALUE_CHANGE_WHEN_COMPLETED_OFFSET: usize = 0x2B;
const OBJECTIVE_IS_COUNTING_DOWNWARD_OFFSET: usize = 0x2C;
const OBJECTIVE_NAME_OFFSET: usize = 0x08;
const OBJECTIVE_DISPLAY_DESCRIPTION_OFFSET: usize = 0x10;
const OBJECTIVE_PROGRESS_DESCRIPTION_OFFSET: usize = 0x18;
const ITEM_OBJECTIVE_INDEX_ROW_CLASS: u32 = 0x8080_87B1;
const COLLECTIBLE_DEFINITION_TABLE_SLOT: usize = 19;
const COLLECTIBLE_DEFINITION_ROW_CLASS: u32 = 0x8080_3475;
const COLLECTIBLE_DEFINITION_ROW_SIZE: usize = 0xB8;
const COLLECTIBLE_PRESENTATION_NODE_PARENTS_OFFSET: usize = 0x18;
const COLLECTIBLE_INVENTORY_ITEM_INDEX_OFFSET: usize = 0x2C;
const METRIC_DEFINITION_TABLE_SLOT: usize = 55;
const METRIC_STRING_TABLE_SLOT: usize = 37;
const METRIC_DEFINITION_ROW_SIZE: usize = 0x48;
const METRIC_STRING_ROW_SIZE: usize = 0x18;
const METRIC_HASH_OFFSET: usize = 0x28;
const METRIC_OBJECTIVE_INDEX_OFFSET: usize = 0x2C;
const RECORD_DEFINITION_TABLE_SLOT: usize = 72;
const RECORD_STRING_TABLE_SLOT: usize = 49;
const RECORD_DEFINITION_ROW_SIZE: usize = 0xD8;
const RECORD_STRING_ROW_SIZE: usize = 0x80;
const RECORD_HASH_OFFSET: usize = 0x28;
const RECORD_OBJECTIVE_LIST_OFFSET: usize = 0x30;
const RECORD_OBJECTIVE_INDEX_ROW_CLASS: u32 = 0x8080_7455;
const PRESENTATION_NODE_DEFINITION_TABLE_SLOT: usize = 63;
const PRESENTATION_NODE_STRING_TABLE_SLOT: usize = 41;
const PRESENTATION_NODE_DEFINITION_ROW_SIZE: usize = 0xA8;
const PRESENTATION_NODE_STRING_ROW_SIZE: usize = 0x2C;
const PRESENTATION_NODE_HASH_OFFSET: usize = 0x28;
const PRESENTATION_NODE_PARENTS_OFFSET: usize = 0x18;
const PRESENTATION_NODE_OBJECTIVE_INDEX_OFFSET: usize = 0x50;
const PRESENTATION_NODE_INDEX_ROW_CLASS: u32 = 0x8080_3962;
const UNLOCK_FLAG_DEFINITION_TABLE_SLOT: usize = 112;
const UNLOCK_FLAG_DEFINITION_ROW_CLASS: u32 = 0x8080_7D4F;
const UNLOCK_FLAG_DEFINITION_COUNT: usize = 21_613;
const UNLOCK_FLAG_DISPLAY_TABLE_SLOT: usize = 78;
const UNLOCK_FLAG_DISPLAY_ROW_CLASS: u32 = 0x8080_5EAF;
const UNLOCK_FLAG_DISPLAY_ROW_SIZE: usize = 16;
const UNLOCK_FLAG_DISPLAY_POINTER_OFFSET: usize = 8;
const UNLOCK_FLAG_DISPLAY_BLOCK_SIZE: usize = 16;
const UNLOCK_FLAG_DISPLAY_NAME_OFFSET: usize = 0;
const UNLOCK_FLAG_DISPLAY_DESCRIPTION_OFFSET: usize = 8;
const UNLOCK_VALUE_DEFINITION_TABLE_SLOT: usize = 114;
const UNLOCK_VALUE_DEFINITION_ROW_CLASS: u32 = 0x8080_7C96;
const UNLOCK_DEFINITION_ROW_SIZE: usize = 8;
const UNLOCK_DEFINITION_CODE_OFFSET: usize = 4;
const UNLOCK_DEFINITION_SLOT_OFFSET: usize = 6;
const UNLOCK_DEFINITION_UNBANKED_SLOT: u16 = u16::MAX;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectiveDef {
    pub hash: u64,
    pub description: String,
    pub completion_value: i32,
    pub allow_overcompletion: bool,
    pub allow_negative_value: bool,
    pub allow_value_change_when_completed: bool,
    pub is_counting_downward: bool,
    pub owners: Vec<ObjectiveOwnerDef>,
    #[serde(default)]
    pub related_unlock_value_definition_index: Option<u16>,
}

impl ObjectiveDef {
    pub const fn maximum_value(&self) -> Option<i32> {
        if self.allow_overcompletion || self.is_counting_downward {
            None
        } else {
            Some(self.completion_value)
        }
    }

    pub const fn minimum_value(&self) -> Option<i32> {
        if self.allow_overcompletion || !self.is_counting_downward {
            None
        } else {
            Some(self.completion_value)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ObjectiveOwnerKind {
    InventoryItem,
    Metric,
    Record,
    PresentationNode,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectiveOwnerDef {
    pub hash: u64,
    pub kind: ObjectiveOwnerKind,
    pub name: String,
    pub type_name: String,
    pub paths: Vec<Vec<String>>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnlockDefinition {
    pub hash: u64,
    pub code: u16,
    pub compact_slot: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl UnlockDefinition {
    pub const fn bank(&self) -> u8 {
        self.code.to_le_bytes()[0]
    }
}

#[derive(Clone, Debug)]
pub(super) struct PresentationNodeDef {
    hash: u64,
    name: String,
    parents: Vec<usize>,
    objective_index: Option<usize>,
}

pub(super) fn unlock_state_indices(definitions: &[UnlockDefinition]) -> HashMap<(u8, u16), usize> {
    let mut indices = HashMap::new();
    for (index, definition) in definitions.iter().enumerate() {
        if let Some(slot) = definition.compact_slot {
            indices.entry((definition.bank(), slot)).or_insert(index);
        }
    }
    indices
}

pub(super) fn add_objective_owner(
    objectives: &mut [ObjectiveDef],
    objective_index: usize,
    owner: ObjectiveOwnerDef,
) {
    let Some(objective) = objectives.get_mut(objective_index) else {
        return;
    };
    if objective
        .owners
        .iter()
        .any(|existing| existing.kind == owner.kind && existing.hash == owner.hash)
    {
        return;
    }
    objective.owners.push(owner);
}

pub(super) fn scan_unlock_flag_definitions(
    manager: &PackageManager,
    root: &[u8],
) -> Result<Vec<UnlockDefinition>, String> {
    let definitions = scan_unlock_definitions(
        manager,
        root,
        UNLOCK_FLAG_DEFINITION_TABLE_SLOT,
        UNLOCK_FLAG_DEFINITION_ROW_CLASS,
        "flag",
    )?;
    if definitions.len() != UNLOCK_FLAG_DEFINITION_COUNT {
        return Err(format!(
            "The installed unlock flag definition table has {} rows; expected {UNLOCK_FLAG_DEFINITION_COUNT}",
            definitions.len()
        ));
    }
    Ok(definitions)
}

pub(super) fn scan_unlock_value_definitions(
    manager: &PackageManager,
    root: &[u8],
) -> Result<Vec<UnlockDefinition>, String> {
    scan_unlock_definitions(
        manager,
        root,
        UNLOCK_VALUE_DEFINITION_TABLE_SLOT,
        UNLOCK_VALUE_DEFINITION_ROW_CLASS,
        "value",
    )
}

fn scan_unlock_definitions(
    manager: &PackageManager,
    root: &[u8],
    table_slot: usize,
    expected_row_class: u32,
    kind: &str,
) -> Result<Vec<UnlockDefinition>, String> {
    let pointer = 8_usize
        .checked_add(
            table_slot
                .checked_mul(16)
                .ok_or_else(|| format!("Unlock {kind} table offset overflowed"))?,
        )
        .ok_or_else(|| format!("Unlock {kind} table offset overflowed"))?;
    let table = manager
        .read_tag(TagHash(u32_at(root, pointer)?))
        .map_err(|error| format!("Could not read unlock {kind} definitions: {error}"))?;
    let (count, rows, row_class) = array_at(&table, 8)?;
    if row_class != expected_row_class {
        return Err(format!(
            "The installed unlock {kind} table has unexpected row class 0x{row_class:08X}"
        ));
    }
    (0..count)
        .map(|index| {
            let row = rows
                .checked_add(
                    index
                        .checked_mul(UNLOCK_DEFINITION_ROW_SIZE)
                        .ok_or_else(|| format!("Unlock {kind} row offset overflowed"))?,
                )
                .ok_or_else(|| format!("Unlock {kind} row offset overflowed"))?;
            let slot = u16_at(&table, row + UNLOCK_DEFINITION_SLOT_OFFSET)?;
            Ok(UnlockDefinition {
                hash: u64::from(u32_at(&table, row)?),
                code: u16_at(&table, row + UNLOCK_DEFINITION_CODE_OFFSET)?,
                compact_slot: (slot != UNLOCK_DEFINITION_UNBANKED_SLOT).then_some(slot),
                name: None,
                description: None,
            })
        })
        .collect()
}

pub(super) fn scan_unlock_flag_displays(
    manager: &PackageManager,
    globals: &[u8],
    localized_tags: &[TagHash],
    localized_cache: &mut HashMap<u32, HashMap<u32, String>>,
    definitions: &mut [UnlockDefinition],
) -> Result<(), String> {
    let pointer = 16_usize
        .checked_add(
            UNLOCK_FLAG_DISPLAY_TABLE_SLOT
                .checked_mul(16)
                .ok_or("Unlock flag display table offset overflowed")?,
        )
        .ok_or("Unlock flag display table offset overflowed")?;
    let table = manager
        .read_tag(TagHash(u32_at(globals, pointer)?))
        .map_err(|error| format!("Could not read unlock flag displays: {error}"))?;
    let display_blocks = unlock_flag_display_blocks(&table, definitions)?;

    for (definition, display) in definitions.iter_mut().zip(display_blocks) {
        definition.name = nonblank_localized_string(resolve_string(
            manager,
            localized_tags,
            localized_cache,
            &table,
            display + UNLOCK_FLAG_DISPLAY_NAME_OFFSET,
        ));
        definition.description = nonblank_localized_string(resolve_string(
            manager,
            localized_tags,
            localized_cache,
            &table,
            display + UNLOCK_FLAG_DISPLAY_DESCRIPTION_OFFSET,
        ));
    }
    Ok(())
}

fn unlock_flag_display_blocks(
    table: &[u8],
    definitions: &[UnlockDefinition],
) -> Result<Vec<usize>, String> {
    let (count, rows, row_class) = array_at(table, 8)?;
    if row_class != UNLOCK_FLAG_DISPLAY_ROW_CLASS {
        return Err(format!(
            "The installed unlock flag display table has unexpected row class 0x{row_class:08X}"
        ));
    }
    if count != definitions.len() {
        return Err(format!(
            "The installed unlock flag definition and display tables do not match ({} definitions, {count} displays)",
            definitions.len()
        ));
    }

    let mut blocks = Vec::with_capacity(count);
    for (index, definition) in definitions.iter().enumerate() {
        let row = rows
            .checked_add(
                index
                    .checked_mul(UNLOCK_FLAG_DISPLAY_ROW_SIZE)
                    .ok_or("Unlock flag display row offset overflowed")?,
            )
            .ok_or("Unlock flag display row offset overflowed")?;
        let display_hash = u32_at(table, row)?;
        if u64::from(display_hash) != definition.hash {
            return Err(format!(
                "Unlock flag definition and display row {index} do not match"
            ));
        }
        // Read the reserved field as part of validating the complete fixed-size row.
        let _ = u32_at(table, row + 4)?;
        let pointer = row
            .checked_add(UNLOCK_FLAG_DISPLAY_POINTER_OFFSET)
            .ok_or("Unlock flag display pointer offset overflowed")?;
        let display = relative_offset(pointer, 0, i64_at(table, pointer)?)?;
        let end = display
            .checked_add(UNLOCK_FLAG_DISPLAY_BLOCK_SIZE)
            .ok_or("Unlock flag display block offset overflowed")?;
        if end > table.len() {
            return Err(format!(
                "Unlock flag display row {index} points outside the table"
            ));
        }
        blocks.push(display);
    }
    Ok(blocks)
}

fn nonblank_localized_string(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.trim().is_empty())
}

pub(super) fn scan_objectives(
    manager: &PackageManager,
    root: &[u8],
    globals: &[u8],
    localized_tags: &[TagHash],
    localized_cache: &mut HashMap<u32, HashMap<u32, String>>,
    unlock_value_definitions: &[UnlockDefinition],
) -> Result<Vec<ObjectiveDef>, String> {
    // Shadowkeep does not expose an explicit objective-to-unlock-value field here.
    // Keep this conservative association to definitions that share the same hash.
    let related_unlock_value_indices = unlock_value_definitions
        .iter()
        .enumerate()
        .map(|(index, definition)| (definition.hash, index))
        .collect::<HashMap<_, _>>();
    let definition_pointer = 8_usize
        .checked_add(
            OBJECTIVE_DEFINITION_TABLE_SLOT
                .checked_mul(16)
                .ok_or("Objective definition table offset overflowed")?,
        )
        .ok_or("Objective definition table offset overflowed")?;
    let string_pointer = 16_usize
        .checked_add(
            OBJECTIVE_STRING_TABLE_SLOT
                .checked_mul(16)
                .ok_or("Objective string table offset overflowed")?,
        )
        .ok_or("Objective string table offset overflowed")?;
    let definitions = manager
        .read_tag(TagHash(u32_at(root, definition_pointer)?))
        .map_err(|error| format!("Could not read objective definitions: {error}"))?;
    let strings = manager
        .read_tag(TagHash(u32_at(globals, string_pointer)?))
        .map_err(|error| format!("Could not read objective strings: {error}"))?;
    let (definition_count, definition_rows, _) = array_at(&definitions, 8)?;
    let (string_count, string_rows, _) = array_at(&strings, 8)?;
    if definition_count != string_count {
        return Err("The installed objective definition and string tables do not match".into());
    }

    let mut objectives = Vec::with_capacity(definition_count);
    for index in 0..definition_count {
        let definition = definition_rows
            .checked_add(
                index
                    .checked_mul(OBJECTIVE_DEFINITION_ROW_SIZE)
                    .ok_or("Objective definition row offset overflowed")?,
            )
            .ok_or("Objective definition row offset overflowed")?;
        let string = string_rows
            .checked_add(
                index
                    .checked_mul(OBJECTIVE_STRING_ROW_SIZE)
                    .ok_or("Objective string row offset overflowed")?,
            )
            .ok_or("Objective string row offset overflowed")?;
        let definition_hash = u32_at(&definitions, definition)?;
        let string_hash = u32_at(&strings, string)?;
        if definition_hash != string_hash {
            return Err(format!(
                "Objective definition and string row {index} do not match"
            ));
        }
        let description = [
            OBJECTIVE_PROGRESS_DESCRIPTION_OFFSET,
            OBJECTIVE_NAME_OFFSET,
            OBJECTIVE_DISPLAY_DESCRIPTION_OFFSET,
        ]
        .into_iter()
        .filter_map(|offset| {
            resolve_string(
                manager,
                localized_tags,
                localized_cache,
                &strings,
                string + offset,
            )
        })
        .find(|value| !value.trim().is_empty())
        .unwrap_or_default();
        objectives.push(ObjectiveDef {
            hash: u64::from(definition_hash),
            description,
            completion_value: i32_at(&definitions, definition + OBJECTIVE_COMPLETION_VALUE_OFFSET)?,
            allow_overcompletion: bool_at(
                &definitions,
                definition + OBJECTIVE_ALLOW_OVERCOMPLETION_OFFSET,
            )?,
            allow_negative_value: bool_at(
                &definitions,
                definition + OBJECTIVE_ALLOW_NEGATIVE_VALUE_OFFSET,
            )?,
            allow_value_change_when_completed: bool_at(
                &definitions,
                definition + OBJECTIVE_ALLOW_VALUE_CHANGE_WHEN_COMPLETED_OFFSET,
            )?,
            is_counting_downward: bool_at(
                &definitions,
                definition + OBJECTIVE_IS_COUNTING_DOWNWARD_OFFSET,
            )?,
            owners: Vec::new(),
            related_unlock_value_definition_index: related_unlock_value_indices
                .get(&u64::from(definition_hash))
                .copied()
                .and_then(|index| u16::try_from(index).ok()),
        });
    }
    Ok(objectives)
}

pub(super) fn scan_presentation_nodes(
    manager: &PackageManager,
    root: &[u8],
    globals: &[u8],
    localized_tags: &[TagHash],
    localized_cache: &mut HashMap<u32, HashMap<u32, String>>,
    objective_count: usize,
) -> Result<Vec<PresentationNodeDef>, String> {
    let definitions = manager
        .read_tag(TagHash(u32_at(
            root,
            8 + PRESENTATION_NODE_DEFINITION_TABLE_SLOT * 16,
        )?))
        .map_err(|error| format!("Could not read presentation-node definitions: {error}"))?;
    let strings = manager
        .read_tag(TagHash(u32_at(
            globals,
            16 + PRESENTATION_NODE_STRING_TABLE_SLOT * 16,
        )?))
        .map_err(|error| format!("Could not read presentation-node strings: {error}"))?;
    let (definition_count, definition_rows, _) = array_at(&definitions, 8)?;
    let (string_count, string_rows, _) = array_at(&strings, 8)?;
    if definition_count != string_count {
        return Err(
            "The installed presentation-node definition and string tables do not match".into(),
        );
    }

    let mut nodes = Vec::with_capacity(definition_count);
    for index in 0..definition_count {
        let definition = definition_rows + index * PRESENTATION_NODE_DEFINITION_ROW_SIZE;
        let string = string_rows + index * PRESENTATION_NODE_STRING_ROW_SIZE;
        let hash = u32_at(&definitions, definition + PRESENTATION_NODE_HASH_OFFSET)?;
        if u32_at(&strings, string)? != hash {
            return Err(format!(
                "Presentation-node definition and string row {index} do not match"
            ));
        }
        let name = [0x08, 0x10]
            .into_iter()
            .filter_map(|offset| {
                resolve_string(
                    manager,
                    localized_tags,
                    localized_cache,
                    &strings,
                    string + offset,
                )
            })
            .find(|value| !value.trim().is_empty())
            .unwrap_or_default();
        let parents = definition_index_list(
            &definitions,
            definition + PRESENTATION_NODE_PARENTS_OFFSET,
            PRESENTATION_NODE_INDEX_ROW_CLASS,
            definition_count,
            "presentation-node parent",
        )?;
        let objective_index = usize::from(u16_at(
            &definitions,
            definition + PRESENTATION_NODE_OBJECTIVE_INDEX_OFFSET,
        )?);
        let objective_index = (objective_index != usize::from(u16::MAX)
            && objective_index < objective_count)
            .then_some(objective_index);
        nodes.push(PresentationNodeDef {
            hash: u64::from(hash),
            name,
            parents,
            objective_index,
        });
    }
    Ok(nodes)
}

pub(super) fn attach_presentation_node_objective_owners(
    objectives: &mut [ObjectiveDef],
    nodes: &[PresentationNodeDef],
) {
    for node in nodes {
        let Some(objective_index) = node.objective_index else {
            continue;
        };
        add_objective_owner(
            objectives,
            objective_index,
            ObjectiveOwnerDef {
                hash: node.hash,
                kind: ObjectiveOwnerKind::PresentationNode,
                name: node.name.clone(),
                type_name: "Presentation node".into(),
                paths: presentation_paths(nodes, &node.parents),
            },
        );
    }
}

fn presentation_paths(nodes: &[PresentationNodeDef], parents: &[usize]) -> Vec<Vec<String>> {
    let mut paths = Vec::new();
    for &parent in parents {
        collect_presentation_paths(nodes, parent, &mut Vec::new(), &mut Vec::new(), &mut paths);
    }
    paths.retain(|path| !path.is_empty());
    let mut deduplicated = Vec::new();
    for path in paths {
        if !deduplicated.contains(&path) {
            deduplicated.push(path);
        }
    }
    deduplicated
}

fn collect_presentation_paths(
    nodes: &[PresentationNodeDef],
    index: usize,
    visited: &mut Vec<usize>,
    names: &mut Vec<String>,
    paths: &mut Vec<Vec<String>>,
) {
    if visited.contains(&index) {
        if !names.is_empty() {
            paths.push(names.clone());
        }
        return;
    }
    let Some(node) = nodes.get(index) else {
        if !names.is_empty() {
            paths.push(names.clone());
        }
        return;
    };
    visited.push(index);
    let added_name = !node.name.trim().is_empty();
    if added_name {
        names.push(node.name.clone());
    }
    if node.parents.is_empty() {
        if !names.is_empty() {
            paths.push(names.clone());
        }
    } else {
        for &parent in &node.parents {
            collect_presentation_paths(nodes, parent, visited, names, paths);
        }
    }
    if added_name {
        names.pop();
    }
    visited.pop();
}

pub(super) fn scan_collectible_item_paths(
    manager: &PackageManager,
    root: &[u8],
    presentation_nodes: &[PresentationNodeDef],
) -> Result<HashMap<usize, Vec<Vec<String>>>, String> {
    let definitions = manager
        .read_tag(TagHash(u32_at(
            root,
            8 + COLLECTIBLE_DEFINITION_TABLE_SLOT * 16,
        )?))
        .map_err(|error| format!("Could not read collectible definitions: {error}"))?;
    collectible_item_paths_from_definitions(&definitions, presentation_nodes)
}

fn collectible_item_paths_from_definitions(
    definitions: &[u8],
    presentation_nodes: &[PresentationNodeDef],
) -> Result<HashMap<usize, Vec<Vec<String>>>, String> {
    let (definition_count, definition_rows, row_class) = array_at(definitions, 8)?;
    if row_class != COLLECTIBLE_DEFINITION_ROW_CLASS {
        return Err(format!(
            "The installed collectible table has unexpected row class 0x{row_class:08X}"
        ));
    }

    let mut paths_by_item_index = HashMap::<usize, Vec<Vec<String>>>::new();
    for index in 0..definition_count {
        let definition = definition_rows
            .checked_add(
                index
                    .checked_mul(COLLECTIBLE_DEFINITION_ROW_SIZE)
                    .ok_or("Collectible definition row offset overflowed")?,
            )
            .ok_or("Collectible definition row offset overflowed")?;
        let item_index = u16_at(
            definitions,
            definition + COLLECTIBLE_INVENTORY_ITEM_INDEX_OFFSET,
        )?;
        if item_index == u16::MAX {
            continue;
        }
        let parents = definition_index_list(
            definitions,
            definition + COLLECTIBLE_PRESENTATION_NODE_PARENTS_OFFSET,
            PRESENTATION_NODE_INDEX_ROW_CLASS,
            presentation_nodes.len(),
            "collectible presentation-node parent",
        )?;
        let paths = presentation_paths(presentation_nodes, &parents);
        if paths.is_empty() {
            continue;
        }
        let item_paths = paths_by_item_index
            .entry(usize::from(item_index))
            .or_default();
        for path in paths {
            if !item_paths.contains(&path) {
                item_paths.push(path);
            }
        }
    }
    Ok(paths_by_item_index)
}

fn definition_index_list(
    data: &[u8],
    descriptor: usize,
    expected_class: u32,
    definition_count: usize,
    label: &str,
) -> Result<Vec<usize>, String> {
    if u64_at(data, descriptor)? == 0 {
        return Ok(Vec::new());
    }
    let (count, rows, class) = array_at(data, descriptor)?;
    if class != expected_class {
        return Err(format!("Unexpected {label} row class 0x{class:08X}"));
    }
    if count > definition_count {
        return Err(format!("{label} list is larger than its definition table"));
    }
    let byte_count = count
        .checked_mul(size_of::<u16>())
        .ok_or_else(|| format!("{label} list size overflowed"))?;
    let end = rows
        .checked_add(byte_count)
        .ok_or_else(|| format!("{label} list offset overflowed"))?;
    if end > data.len() {
        return Err(format!("{label} list extends beyond its package data"));
    }
    (0..count)
        .map(|index| {
            let definition_index = usize::from(u16_at(data, rows + index * size_of::<u16>())?);
            if definition_index >= definition_count {
                Err(format!("{label} index {definition_index} is out of range"))
            } else {
                Ok(definition_index)
            }
        })
        .collect()
}

pub(super) fn scan_metric_objective_owners(
    manager: &PackageManager,
    root: &[u8],
    globals: &[u8],
    localized_tags: &[TagHash],
    localized_cache: &mut HashMap<u32, HashMap<u32, String>>,
    presentation_nodes: &[PresentationNodeDef],
    objectives: &mut [ObjectiveDef],
) -> Result<(), String> {
    let definitions = manager
        .read_tag(TagHash(u32_at(
            root,
            8 + METRIC_DEFINITION_TABLE_SLOT * 16,
        )?))
        .map_err(|error| format!("Could not read metric definitions: {error}"))?;
    let strings = manager
        .read_tag(TagHash(u32_at(
            globals,
            16 + METRIC_STRING_TABLE_SLOT * 16,
        )?))
        .map_err(|error| format!("Could not read metric strings: {error}"))?;
    let (definition_count, definition_rows, _) = array_at(&definitions, 8)?;
    let (string_count, string_rows, _) = array_at(&strings, 8)?;
    if definition_count != string_count {
        return Err("The installed metric definition and string tables do not match".into());
    }

    for index in 0..definition_count {
        let definition = definition_rows + index * METRIC_DEFINITION_ROW_SIZE;
        let string = string_rows + index * METRIC_STRING_ROW_SIZE;
        let hash = u32_at(&definitions, definition + METRIC_HASH_OFFSET)?;
        if u32_at(&strings, string)? != hash {
            return Err(format!(
                "Metric definition and string row {index} do not match"
            ));
        }
        let objective_index = usize::from(u16_at(
            &definitions,
            definition + METRIC_OBJECTIVE_INDEX_OFFSET,
        )?);
        if objective_index == usize::from(u16::MAX) || objective_index >= objectives.len() {
            continue;
        }
        let parents = if presentation_nodes.is_empty() {
            Vec::new()
        } else {
            definition_index_list(
                &definitions,
                definition + PRESENTATION_NODE_PARENTS_OFFSET,
                PRESENTATION_NODE_INDEX_ROW_CLASS,
                presentation_nodes.len(),
                "metric parent",
            )?
        };
        let name = [0x08, 0x10]
            .into_iter()
            .filter_map(|offset| {
                resolve_string(
                    manager,
                    localized_tags,
                    localized_cache,
                    &strings,
                    string + offset,
                )
            })
            .find(|value| !value.trim().is_empty())
            .unwrap_or_default();
        add_objective_owner(
            objectives,
            objective_index,
            ObjectiveOwnerDef {
                hash: u64::from(hash),
                kind: ObjectiveOwnerKind::Metric,
                name,
                type_name: "Metric".into(),
                paths: presentation_paths(presentation_nodes, &parents),
            },
        );
    }
    Ok(())
}

pub(super) fn scan_record_objective_owners(
    manager: &PackageManager,
    root: &[u8],
    globals: &[u8],
    localized_tags: &[TagHash],
    localized_cache: &mut HashMap<u32, HashMap<u32, String>>,
    presentation_nodes: &[PresentationNodeDef],
    objectives: &mut [ObjectiveDef],
) -> Result<(), String> {
    let definitions = manager
        .read_tag(TagHash(u32_at(
            root,
            8 + RECORD_DEFINITION_TABLE_SLOT * 16,
        )?))
        .map_err(|error| format!("Could not read record definitions: {error}"))?;
    let strings = manager
        .read_tag(TagHash(u32_at(
            globals,
            16 + RECORD_STRING_TABLE_SLOT * 16,
        )?))
        .map_err(|error| format!("Could not read record strings: {error}"))?;
    let (definition_count, definition_rows, _) = array_at(&definitions, 8)?;
    let (string_count, string_rows, _) = array_at(&strings, 8)?;
    if definition_count != string_count {
        return Err("The installed record definition and string tables do not match".into());
    }

    for index in 0..definition_count {
        let definition = definition_rows + index * RECORD_DEFINITION_ROW_SIZE;
        let string = string_rows + index * RECORD_STRING_ROW_SIZE;
        let hash = u32_at(&definitions, definition + RECORD_HASH_OFFSET)?;
        if u32_at(&strings, string)? != hash {
            return Err(format!(
                "Record definition and string row {index} do not match"
            ));
        }
        let Ok((objective_count, objective_rows, class)) =
            array_at(&definitions, definition + RECORD_OBJECTIVE_LIST_OFFSET)
        else {
            continue;
        };
        if objective_count == 0 {
            continue;
        }
        if class != RECORD_OBJECTIVE_INDEX_ROW_CLASS {
            return Err(format!("Record {index} has an unexpected objective list"));
        }
        let parents = if presentation_nodes.is_empty() {
            Vec::new()
        } else {
            definition_index_list(
                &definitions,
                definition + PRESENTATION_NODE_PARENTS_OFFSET,
                PRESENTATION_NODE_INDEX_ROW_CLASS,
                presentation_nodes.len(),
                "record parent",
            )?
        };
        let name = [0x08, 0x10]
            .into_iter()
            .filter_map(|offset| {
                resolve_string(
                    manager,
                    localized_tags,
                    localized_cache,
                    &strings,
                    string + offset,
                )
            })
            .find(|value| !value.trim().is_empty())
            .unwrap_or_default();
        for objective in 0..objective_count {
            let objective_index = usize::from(u16_at(
                &definitions,
                objective_rows + objective * size_of::<u16>(),
            )?);
            if objective_index >= objectives.len() {
                continue;
            }
            add_objective_owner(
                objectives,
                objective_index,
                ObjectiveOwnerDef {
                    hash: u64::from(hash),
                    kind: ObjectiveOwnerKind::Record,
                    name: name.clone(),
                    type_name: "Triumph / record".into(),
                    paths: presentation_paths(presentation_nodes, &parents),
                },
            );
        }
    }
    Ok(())
}

pub(super) fn item_objective_indices(item: &[u8], objective_count: usize) -> Vec<usize> {
    let mut indices = Vec::new();
    let Some(last_descriptor) = item.len().checked_sub(16) else {
        return indices;
    };
    for descriptor in (0..=last_descriptor).step_by(8) {
        let Ok((count, rows, class)) = array_at(item, descriptor) else {
            continue;
        };
        if class != ITEM_OBJECTIVE_INDEX_ROW_CLASS || count > objective_count {
            continue;
        }
        let Some(byte_count) = count.checked_mul(size_of::<u16>()) else {
            continue;
        };
        if rows
            .checked_add(byte_count)
            .is_none_or(|end| end > item.len())
        {
            continue;
        }
        for index in 0..count {
            if let Ok(index) = u16_at(item, rows + index * size_of::<u16>()) {
                let index = usize::from(index);
                if index < objective_count {
                    indices.push(index);
                }
            }
        }
    }
    indices.sort_unstable();
    indices.dedup();
    indices
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unlock_flag_display_table(hash: u32) -> Vec<u8> {
        const HEADER: usize = 32;
        const ROW: usize = HEADER + 16;
        const DISPLAY: usize = 80;

        let mut table = vec![0_u8; DISPLAY + UNLOCK_FLAG_DISPLAY_BLOCK_SIZE];
        table[8..16].copy_from_slice(&1_u64.to_le_bytes());
        table[16..24].copy_from_slice(&((HEADER - 16) as i64).to_le_bytes());
        table[HEADER..HEADER + 8].copy_from_slice(&1_u64.to_le_bytes());
        table[HEADER + 8..HEADER + 12]
            .copy_from_slice(&UNLOCK_FLAG_DISPLAY_ROW_CLASS.to_le_bytes());
        table[ROW..ROW + 4].copy_from_slice(&hash.to_le_bytes());
        table[ROW + 4..ROW + 8].copy_from_slice(&0x1234_5678_u32.to_le_bytes());
        table[ROW + UNLOCK_FLAG_DISPLAY_POINTER_OFFSET
            ..ROW + UNLOCK_FLAG_DISPLAY_POINTER_OFFSET + 8]
            .copy_from_slice(
                &((DISPLAY - (ROW + UNLOCK_FLAG_DISPLAY_POINTER_OFFSET)) as i64).to_le_bytes(),
            );
        table
    }

    #[test]
    fn unlock_flag_displays_validate_aligned_hashes_and_relative_blocks() {
        let definition = UnlockDefinition {
            hash: 0x1234_5678,
            ..UnlockDefinition::default()
        };
        let table = unlock_flag_display_table(definition.hash as u32);

        assert_eq!(
            unlock_flag_display_blocks(&table, std::slice::from_ref(&definition)),
            Ok(vec![80])
        );

        let mut wrong_hash = table.clone();
        wrong_hash[48..52].copy_from_slice(&0x8765_4321_u32.to_le_bytes());
        assert!(unlock_flag_display_blocks(&wrong_hash, &[definition.clone()]).is_err());

        let mut wrong_class = table.clone();
        wrong_class[40..44].copy_from_slice(&0_u32.to_le_bytes());
        assert!(unlock_flag_display_blocks(&wrong_class, &[definition.clone()]).is_err());

        let mut outside = table;
        outside[56..64].copy_from_slice(&i64::MAX.to_le_bytes());
        assert!(unlock_flag_display_blocks(&outside, &[definition.clone()]).is_err());
        assert!(
            unlock_flag_display_blocks(&unlock_flag_display_table(definition.hash as u32), &[])
                .is_err()
        );
    }

    #[test]
    fn unlock_display_text_only_filters_blank_localized_strings() {
        assert_eq!(nonblank_localized_string(None), None);
        assert_eq!(nonblank_localized_string(Some(" \t ".into())), None);
        assert_eq!(
            nonblank_localized_string(Some("  Exact package text  ".into())),
            Some("  Exact package text  ".into())
        );
    }

    #[test]
    fn unlock_state_indices_use_the_compact_bank_and_keep_the_first_definition() {
        let definitions = vec![
            UnlockDefinition {
                hash: 0x1111_1111,
                code: 0x0201,
                compact_slot: Some(58),
                name: None,
                description: None,
            },
            UnlockDefinition {
                hash: 0x2222_2222,
                code: 0x0001,
                compact_slot: Some(58),
                name: None,
                description: None,
            },
            UnlockDefinition {
                hash: 0x3333_3333,
                code: 0x0002,
                compact_slot: Some(58),
                name: None,
                description: None,
            },
            UnlockDefinition {
                hash: 0x4444_4444,
                code: 0x0001,
                compact_slot: None,
                name: None,
                description: None,
            },
        ];

        let indices = unlock_state_indices(&definitions);

        assert_eq!(indices.get(&(1, 58)), Some(&0));
        assert_eq!(indices.get(&(2, 58)), Some(&2));
        assert!(!indices.contains_key(&(1, u16::MAX)));
    }

    #[test]
    fn objective_limits_respect_direction_and_overcompletion() {
        let mut objective = ObjectiveDef {
            completion_value: 70,
            ..ObjectiveDef::default()
        };
        assert_eq!(objective.maximum_value(), Some(70));
        assert_eq!(objective.minimum_value(), None);

        objective.allow_overcompletion = true;
        assert_eq!(objective.maximum_value(), None);
        assert_eq!(objective.minimum_value(), None);

        objective.allow_overcompletion = false;
        objective.is_counting_downward = true;
        assert_eq!(objective.maximum_value(), None);
        assert_eq!(objective.minimum_value(), Some(70));
    }

    #[test]
    fn presentation_paths_keep_immediate_parent_first_and_preserve_branches() {
        let node = |hash, name: &str, parents| PresentationNodeDef {
            hash,
            name: name.into(),
            parents,
            objective_index: None,
        };
        let nodes = vec![
            node(1, "Metrics", vec![]),
            node(2, "Account", vec![0]),
            node(3, "Crucible", vec![0]),
            node(4, "Account", vec![0]),
        ];

        assert_eq!(
            presentation_paths(&nodes, &[1]),
            vec![vec!["Account".to_owned(), "Metrics".to_owned()]]
        );
        assert_eq!(
            presentation_paths(&nodes, &[1, 2]),
            vec![
                vec!["Account".to_owned(), "Metrics".to_owned()],
                vec!["Crucible".to_owned(), "Metrics".to_owned()],
            ]
        );
        assert_eq!(presentation_paths(&nodes, &[1, 3]).len(), 1);
    }

    #[test]
    fn item_objective_lists_require_the_package_row_class_and_deduplicate() {
        let mut item = vec![0_u8; 40];
        item[0..8].copy_from_slice(&3_u64.to_le_bytes());
        item[8..16].copy_from_slice(&8_i64.to_le_bytes());
        item[16..24].copy_from_slice(&3_u64.to_le_bytes());
        item[24..28].copy_from_slice(&ITEM_OBJECTIVE_INDEX_ROW_CLASS.to_le_bytes());
        item[32..34].copy_from_slice(&5_u16.to_le_bytes());
        item[34..36].copy_from_slice(&2_u16.to_le_bytes());
        item[36..38].copy_from_slice(&5_u16.to_le_bytes());

        assert_eq!(item_objective_indices(&item, 10), vec![2, 5]);

        item[24..28].copy_from_slice(&0_u32.to_le_bytes());
        assert!(item_objective_indices(&item, 10).is_empty());
    }

    #[test]
    fn collectible_item_paths_use_the_authored_u16_item_index_and_presentation_parents() {
        let nodes = vec![
            PresentationNodeDef {
                hash: 1,
                name: "Items".into(),
                parents: Vec::new(),
                objective_index: None,
            },
            PresentationNodeDef {
                hash: 2,
                name: "Majestic Solstice Suit".into(),
                parents: vec![0],
                objective_index: None,
            },
        ];
        let mut definitions = vec![0_u8; 250];
        definitions[8..16].copy_from_slice(&1_u64.to_le_bytes());
        definitions[16..24].copy_from_slice(&16_i64.to_le_bytes());
        definitions[32..40].copy_from_slice(&1_u64.to_le_bytes());
        definitions[40..44].copy_from_slice(&COLLECTIBLE_DEFINITION_ROW_CLASS.to_le_bytes());
        let row = 48;
        definitions[row + 0x18..row + 0x20].copy_from_slice(&1_u64.to_le_bytes());
        definitions[row + 0x20..row + 0x28].copy_from_slice(&152_i64.to_le_bytes());
        definitions[row + 0x2C..row + 0x30].copy_from_slice(&0xBEEF_0007_u32.to_le_bytes());
        definitions[232..240].copy_from_slice(&1_u64.to_le_bytes());
        definitions[240..244].copy_from_slice(&PRESENTATION_NODE_INDEX_ROW_CLASS.to_le_bytes());
        definitions[248..250].copy_from_slice(&1_u16.to_le_bytes());

        let paths = collectible_item_paths_from_definitions(&definitions, &nodes).unwrap();

        assert_eq!(
            paths.get(&7),
            Some(&vec![vec![
                "Majestic Solstice Suit".to_owned(),
                "Items".to_owned(),
            ]])
        );
        assert!(!paths.contains_key(&0xBEEF_0007));
    }

    #[test]
    fn collectible_item_paths_reject_an_unexpected_table_layout() {
        let mut definitions = vec![0_u8; 48];
        definitions[8..16].copy_from_slice(&0_u64.to_le_bytes());
        definitions[16..24].copy_from_slice(&16_i64.to_le_bytes());
        definitions[32..40].copy_from_slice(&0_u64.to_le_bytes());
        definitions[40..44].copy_from_slice(&0xDEAD_BEEF_u32.to_le_bytes());

        let error = collectible_item_paths_from_definitions(&definitions, &[]).unwrap_err();

        assert!(error.contains("unexpected row class 0xDEADBEEF"));
    }
}

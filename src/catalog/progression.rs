use std::{
    collections::{HashMap, HashSet},
    mem::size_of,
};

use serde::{Deserialize, Serialize};
use tiger_pkg::{PackageManager, TagHash};

use super::{
    package::{array_at, bool_at, i32_at, i64_at, relative_offset, u16_at, u32_at, u64_at},
    resolve_string,
};

const OBJECTIVE_DEFINITION_TABLE_SLOT: usize = 58;
const OBJECTIVE_STRING_TABLE_SLOT: usize = 38;
const OBJECTIVE_DEFINITION_ROW_SIZE: usize = 160;
const OBJECTIVE_DEFINITION_ROW_CLASS: u32 = 0x8080_775F;
const OBJECTIVE_CONDITIONS_OFFSET: usize = 0x08;
const OBJECTIVE_STRING_ROW_SIZE: usize = 64;
const OBJECTIVE_COMPLETION_VALUE_OFFSET: usize = 0x30;
const OBJECTIVE_ALLOW_OVERCOMPLETION_OFFSET: usize = 0x29;
const OBJECTIVE_ALLOW_NEGATIVE_VALUE_OFFSET: usize = 0x2A;
const OBJECTIVE_ALLOW_VALUE_CHANGE_WHEN_COMPLETED_OFFSET: usize = 0x2B;
const OBJECTIVE_IS_COUNTING_DOWNWARD_OFFSET: usize = 0x2C;
const OBJECTIVE_NAME_OFFSET: usize = 0x08;
const OBJECTIVE_DISPLAY_DESCRIPTION_OFFSET: usize = 0x10;
const OBJECTIVE_PROGRESS_DESCRIPTION_OFFSET: usize = 0x18;
const ACTIVITY_DEFINITION_TABLE_SLOT: usize = 4;
const ACTIVITY_STRING_TABLE_SLOT: usize = 3;
const ACTIVITY_DEFINITION_ROW_CLASS: u32 = 0x8080_76FC;
const ACTIVITY_STRING_ROW_CLASS: u32 = 0x8080_5E15;
const ACTIVITY_INDEX_ROW_SIZE: usize = 0x10;
const ACTIVITY_GATE_LIST_OFFSET: usize = 0x08;
const ACTIVITY_GATE_ROW_CLASS: u32 = 0x8080_0070;
const ACTIVITY_AVAILABILITY_TABLE_SLOT: usize = 2;
const ACTIVITY_AVAILABILITY_LIST_OFFSET: usize = 0x30;
const ACTIVITY_AVAILABILITY_ROW_CLASS: u32 = 0x8080_7703;
const ACTIVITY_AVAILABILITY_ROW_SIZE: usize = 0x48;
const ACTIVITY_AVAILABILITY_GROUP_OFFSETS: [usize; 2] = [0, 0x10];
const ACTIVITY_AVAILABILITY_GROUP_ROW_CLASS: u32 = 0x8080_7D2F;
const ACTIVITY_AVAILABILITY_GROUP_ROW_SIZE: usize = 0x10;
const ACTIVITY_AVAILABILITY_GATE_HASH_OFFSET: usize = 0x40;
const ITEM_OBJECTIVE_RESOURCE_POINTER_OFFSET: usize = 0x30;
const ITEM_OBJECTIVE_RESOURCE_CLASS: u32 = 0x8080_77EB;
const ITEM_OBJECTIVE_INDEX_ROW_CLASS: u32 = 0x8080_87B1;
const COLLECTIBLE_DEFINITION_TABLE_SLOT: usize = 19;
const COLLECTIBLE_DEFINITION_ROW_CLASS: u32 = 0x8080_3475;
const COLLECTIBLE_DEFINITION_ROW_SIZE: usize = 0xB8;
const COLLECTIBLE_PRESENTATION_NODE_PARENTS_OFFSET: usize = 0x18;
const COLLECTIBLE_HASH_OFFSET: usize = 0x28;
const COLLECTIBLE_INVENTORY_ITEM_INDEX_OFFSET: usize = 0x2C;
const COLLECTIBLE_CONDITION_OFFSETS: [usize; 4] = [0x30, 0x40, 0x60, 0x70];
const METRIC_DEFINITION_TABLE_SLOT: usize = 55;
const METRIC_STRING_TABLE_SLOT: usize = 37;
const METRIC_DEFINITION_ROW_SIZE: usize = 0x48;
const METRIC_STRING_ROW_SIZE: usize = 0x18;
const METRIC_TRAIT_LIST_OFFSET: usize = 0x08;
const METRIC_TRAIT_INDEX_ROW_CLASS: u32 = 0x8080_2C50;
const METRIC_HASH_OFFSET: usize = 0x28;
const METRIC_OBJECTIVE_INDEX_OFFSET: usize = 0x2C;
const TRAIT_DEFINITION_TABLE_SLOT: usize = 98;
const TRAIT_STRING_TABLE_SLOT: usize = 62;
const TRAIT_DEFINITION_ROW_CLASS: u32 = 0x8080_2C45;
const TRAIT_STRING_ROW_CLASS: u32 = 0x8080_28BF;
const TRAIT_DEFINITION_ROW_SIZE: usize = 0x08;
const TRAIT_STRING_ROW_SIZE: usize = 0x18;
const LOCATION_DEFINITION_TABLE_SLOT: usize = 29;
const LOCATION_STRING_TABLE_SLOT: usize = 24;
const LOCATION_DEFINITION_ROW_CLASS: u32 = 0x8080_7A35;
const LOCATION_STRING_ROW_CLASS: u32 = 0x8080_5B03;
const LOCATION_DEFINITION_ROW_SIZE: usize = 0x48;
const LOCATION_STRING_ROW_SIZE: usize = 0x18;
const LOCATION_RELEASE_LIST_OFFSET: usize = 0x38;
const LOCATION_RELEASE_ROW_CLASS: u32 = 0x8080_7A3A;
const LOCATION_RELEASE_ROW_SIZE: usize = 0x50;
const LOCATION_RELEASE_ACTIVITY_INDEX_OFFSET: usize = 0x12;
const LOCATION_DISPLAY_LIST_OFFSET: usize = 0x08;
const LOCATION_DISPLAY_ROW_CLASS: u32 = 0x8080_5B05;
const LOCATION_DISPLAY_ROW_SIZE: usize = 0x20;
const LOCATION_RELEASE_CONDITION_TABLE_SLOT: usize = 28;
const LOCATION_RELEASE_CONDITION_ROW_CLASS: u32 = 0x8080_7459;
const LOCATION_RELEASE_CONDITION_ROW_SIZE: usize = 0x28;
const LOCATION_RELEASE_LOCATION_INDEX_OFFSET: usize = 0;
const LOCATION_RELEASE_CONDITIONS_OFFSET: usize = 0x08;
const LOCATION_RELEASE_CONDITION_ACTIVITY_INDEX_OFFSET: usize = 0x20;
const RECORD_DEFINITION_TABLE_SLOT: usize = 72;
const RECORD_STRING_TABLE_SLOT: usize = 49;
const RECORD_DEFINITION_ROW_SIZE: usize = 0xD8;
const RECORD_STRING_ROW_SIZE: usize = 0x80;
const RECORD_HASH_OFFSET: usize = 0x28;
const RECORD_OBJECTIVE_LIST_OFFSET: usize = 0x30;
const RECORD_OBJECTIVE_INDEX_ROW_CLASS: u32 = 0x8080_7455;
const RECORD_INTERVAL_OBJECTIVE_LIST_OFFSET: usize = 0x40;
const RECORD_INTERVAL_OBJECTIVE_ROW_CLASS: u32 = 0x8080_2C0F;
const RECORD_INTERVAL_OBJECTIVE_ROW_SIZE: usize = 0x0C;
const RECORD_INTERVAL_OBJECTIVE_INDEX_OFFSET: usize = 0;
const RECORD_CONDITION_OFFSETS: [usize; 6] = [0x68, 0x78, 0x88, 0x98, 0xA8, 0xC0];
const PRESENTATION_NODE_DEFINITION_TABLE_SLOT: usize = 63;
const PRESENTATION_NODE_STRING_TABLE_SLOT: usize = 41;
const PRESENTATION_NODE_DEFINITION_ROW_SIZE: usize = 0xA8;
const PRESENTATION_NODE_STRING_ROW_SIZE: usize = 0x2C;
const PRESENTATION_NODE_HASH_OFFSET: usize = 0x28;
const PRESENTATION_NODE_PARENTS_OFFSET: usize = 0x18;
const PRESENTATION_NODE_OBJECTIVE_INDEX_OFFSET: usize = 0x50;
pub(super) const PRESENTATION_NODE_INDEX_ROW_CLASS: u32 = 0x8080_3962;
const PRESENTATION_NODE_CONDITION_OFFSETS: [usize; 2] = [0x30, 0x40];
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
const CONDITION_EXPRESSION_ROW_CLASS: u32 = 0x8080_7D31;
const CONDITION_EXPRESSION_ROW_SIZE: usize = 8;
const CONDITION_FLAG_KIND: u32 = 1;
const CONDITION_VALUE_KIND: u32 = 10;

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
pub struct ObjectiveOwnerTraitDef {
    pub hash: u64,
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectiveOwnerDef {
    pub hash: u64,
    pub kind: ObjectiveOwnerKind,
    pub name: String,
    pub type_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub traits: Vec<ObjectiveOwnerTraitDef>,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tested_by: Vec<ProgressionContextDef>,
}

impl UnlockDefinition {
    pub const fn bank(&self) -> u8 {
        self.code.to_le_bytes()[0]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProgressionContextKind {
    InventoryItem,
    Collectible,
    Record,
    Objective,
    PresentationNode,
    Activity,
    ActivityAvailability,
    Location,
    LocationRelease,
    ExpressionMapping,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgressionContextDef {
    pub hash: u64,
    pub kind: ProgressionContextKind,
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub type_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<Vec<String>>,
}

#[derive(Clone, Debug)]
pub(super) struct PresentationNodeDef {
    hash: u64,
    name: String,
    parents: Vec<usize>,
    objective_index: Option<usize>,
    condition_references: ConditionReferences,
}

#[derive(Clone, Debug)]
pub(super) struct PendingProgressionContext {
    hash: u64,
    kind: ProgressionContextKind,
    references: ConditionReferences,
    paths: Vec<Vec<String>>,
}

pub(super) struct ProgressionPackageData<'a> {
    manager: &'a PackageManager,
    root: &'a [u8],
    globals: &'a [u8],
    localized_tags: &'a [TagHash],
    localized_cache: &'a mut HashMap<u32, HashMap<u32, String>>,
}

impl<'a> ProgressionPackageData<'a> {
    pub(super) fn new(
        manager: &'a PackageManager,
        root: &'a [u8],
        globals: &'a [u8],
        localized_tags: &'a [TagHash],
        localized_cache: &'a mut HashMap<u32, HashMap<u32, String>>,
    ) -> Self {
        Self {
            manager,
            root,
            globals,
            localized_tags,
            localized_cache,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct ConditionReferences {
    flags: Vec<usize>,
    values: Vec<usize>,
}

fn add_progression_context(
    definitions: &mut [UnlockDefinition],
    definition_index: usize,
    context: &ProgressionContextDef,
) {
    let Some(definition) = definitions.get_mut(definition_index) else {
        return;
    };
    if let Some(existing) = definition
        .tested_by
        .iter_mut()
        .find(|existing| existing.kind == context.kind && existing.hash == context.hash)
    {
        if existing.name.trim().is_empty() && !context.name.trim().is_empty() {
            existing.name.clone_from(&context.name);
        }
        if existing.type_name.trim().is_empty() && !context.type_name.trim().is_empty() {
            existing.type_name.clone_from(&context.type_name);
        }
        if existing.description.trim().is_empty() && !context.description.trim().is_empty() {
            existing.description.clone_from(&context.description);
        }
        for path in &context.paths {
            if !path.is_empty() && !existing.paths.contains(path) {
                existing.paths.push(path.clone());
            }
        }
        return;
    }
    definition.tested_by.push(context.clone());
}

pub(super) fn sort_progression_contexts(definitions: &mut [UnlockDefinition]) {
    for definition in definitions {
        for context in &mut definition.tested_by {
            context.paths.sort();
            context.paths.dedup();
        }
        definition.tested_by.sort_by(|left, right| {
            progression_context_priority(left.kind)
                .cmp(&progression_context_priority(right.kind))
                .then_with(|| {
                    left.name
                        .trim()
                        .is_empty()
                        .cmp(&right.name.trim().is_empty())
                })
                .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
                .then_with(|| left.hash.cmp(&right.hash))
        });
    }
}

const fn progression_context_priority(kind: ProgressionContextKind) -> u8 {
    match kind {
        ProgressionContextKind::InventoryItem => 0,
        ProgressionContextKind::Collectible => 1,
        ProgressionContextKind::Record => 2,
        ProgressionContextKind::Objective => 3,
        ProgressionContextKind::PresentationNode => 4,
        ProgressionContextKind::Activity => 5,
        ProgressionContextKind::Location => 6,
        ProgressionContextKind::LocationRelease => 7,
        ProgressionContextKind::ActivityAvailability => 8,
        ProgressionContextKind::ExpressionMapping => 9,
    }
}

fn attach_condition_context(
    flag_definitions: &mut [UnlockDefinition],
    value_definitions: &mut [UnlockDefinition],
    references: &ConditionReferences,
    context: &ProgressionContextDef,
) {
    for &index in &references.flags {
        add_progression_context(flag_definitions, index, context);
    }
    for &index in &references.values {
        add_progression_context(value_definitions, index, context);
    }
}

fn condition_references_at(data: &[u8], descriptor: usize) -> Result<ConditionReferences, String> {
    if u64_at(data, descriptor)? == 0 {
        return Ok(ConditionReferences::default());
    }
    let (count, rows, row_class) = array_at(data, descriptor)?;
    if row_class != CONDITION_EXPRESSION_ROW_CLASS {
        return Err(format!(
            "Unexpected condition-expression row class 0x{row_class:08X}"
        ));
    }
    condition_references_from_rows(data, rows, count)
}

fn objective_condition_references_at(
    definitions: &[u8],
    definition: usize,
) -> Result<ConditionReferences, String> {
    let descriptor = definition
        .checked_add(OBJECTIVE_CONDITIONS_OFFSET)
        .ok_or("Objective condition descriptor offset overflowed")?;
    condition_references_at(definitions, descriptor)
}

fn condition_references_from_rows(
    data: &[u8],
    rows: usize,
    count: usize,
) -> Result<ConditionReferences, String> {
    let byte_count = count
        .checked_mul(CONDITION_EXPRESSION_ROW_SIZE)
        .ok_or("Condition-expression size overflowed")?;
    let end = rows
        .checked_add(byte_count)
        .ok_or("Condition-expression offset overflowed")?;
    if end > data.len() {
        return Err("Condition expression extends beyond its package data".into());
    }

    let mut references = ConditionReferences::default();
    for index in 0..count {
        let row = rows + index * CONDITION_EXPRESSION_ROW_SIZE;
        let operand = usize::try_from(u32_at(data, row + 4)?)
            .map_err(|_| "Condition-expression definition index is too large")?;
        match u32_at(data, row)? {
            CONDITION_FLAG_KIND => references.flags.push(operand),
            CONDITION_VALUE_KIND => references.values.push(operand),
            _ => {}
        }
    }
    references.flags.sort_unstable();
    references.flags.dedup();
    references.values.sort_unstable();
    references.values.dedup();
    Ok(references)
}

fn merge_condition_references(target: &mut ConditionReferences, source: ConditionReferences) {
    target.flags.extend(source.flags);
    target.values.extend(source.values);
    target.flags.sort_unstable();
    target.flags.dedup();
    target.values.sort_unstable();
    target.values.dedup();
}

fn scan_condition_expressions(data: &[u8]) -> ConditionReferences {
    let mut references = ConditionReferences::default();
    let Some(last_descriptor) = data.len().checked_sub(16) else {
        return references;
    };
    for descriptor in (0..=last_descriptor).step_by(8) {
        let Ok((count, rows, row_class)) = array_at(data, descriptor) else {
            continue;
        };
        if row_class != CONDITION_EXPRESSION_ROW_CLASS {
            continue;
        }
        let Ok(found) = condition_references_from_rows(data, rows, count) else {
            continue;
        };
        merge_condition_references(&mut references, found);
    }
    references
}

fn scan_condition_expressions_in(
    data: &[u8],
    owner_start: usize,
    owner_end: usize,
) -> Result<ConditionReferences, String> {
    if owner_start > owner_end || owner_end > data.len() {
        return Err("Condition-expression owner range is outside its package data".into());
    }
    let Some(last_descriptor) = owner_end.checked_sub(16) else {
        return Ok(ConditionReferences::default());
    };
    if last_descriptor < owner_start {
        return Ok(ConditionReferences::default());
    }

    let mut references = ConditionReferences::default();
    let mut arrays = HashSet::new();
    for descriptor in (owner_start..=last_descriptor).step_by(8) {
        let Ok((count, rows, row_class)) = array_at(data, descriptor) else {
            continue;
        };
        if row_class != CONDITION_EXPRESSION_ROW_CLASS || !arrays.insert((rows, count)) {
            continue;
        }
        let pointer = descriptor
            .checked_add(8)
            .ok_or("Condition-expression pointer offset overflowed")?;
        let header = relative_offset(descriptor, 8, i64_at(data, pointer)?)?;
        let rows_end = rows
            .checked_add(
                count
                    .checked_mul(CONDITION_EXPRESSION_ROW_SIZE)
                    .ok_or("Condition-expression size overflowed")?,
            )
            .ok_or("Condition-expression row offset overflowed")?;
        if header < owner_start || rows_end > owner_end {
            continue;
        }
        merge_condition_references(
            &mut references,
            condition_references_from_rows(data, rows, count)?,
        );
    }
    Ok(references)
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
                tested_by: Vec::new(),
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
    unlock_flag_definitions: &mut [UnlockDefinition],
    unlock_value_definitions: &mut [UnlockDefinition],
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
    let (definition_count, definition_rows, definition_class) = array_at(&definitions, 8)?;
    let (string_count, string_rows, _) = array_at(&strings, 8)?;
    if definition_class != OBJECTIVE_DEFINITION_ROW_CLASS {
        return Err(format!(
            "The installed objective table has unexpected row class 0x{definition_class:08X}"
        ));
    }
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
        let condition_references = objective_condition_references_at(&definitions, definition)?;
        attach_condition_context(
            unlock_flag_definitions,
            unlock_value_definitions,
            &condition_references,
            &ProgressionContextDef {
                hash: u64::from(definition_hash),
                kind: ProgressionContextKind::Objective,
                name: description.clone(),
                type_name: String::new(),
                description: String::new(),
                paths: Vec::new(),
            },
        );
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
    package: &mut ProgressionPackageData<'_>,
    objective_count: usize,
    flag_definitions: &mut [UnlockDefinition],
    value_definitions: &mut [UnlockDefinition],
) -> Result<Vec<PresentationNodeDef>, String> {
    let ProgressionPackageData {
        manager,
        root,
        globals,
        localized_tags,
        localized_cache,
    } = package;
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
        let mut condition_references = ConditionReferences::default();
        for offset in PRESENTATION_NODE_CONDITION_OFFSETS {
            merge_condition_references(
                &mut condition_references,
                condition_references_at(&definitions, definition + offset)?,
            );
        }
        nodes.push(PresentationNodeDef {
            hash: u64::from(hash),
            name,
            parents,
            objective_index,
            condition_references,
        });
    }
    for node in &nodes {
        let context = ProgressionContextDef {
            hash: node.hash,
            kind: ProgressionContextKind::PresentationNode,
            name: node.name.clone(),
            type_name: String::new(),
            description: String::new(),
            paths: presentation_paths(&nodes, &node.parents),
        };
        attach_condition_context(
            flag_definitions,
            value_definitions,
            &node.condition_references,
            &context,
        );
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
                description: String::new(),
                traits: Vec::new(),
                paths: presentation_paths(nodes, &node.parents),
            },
        );
    }
}

pub(super) fn presentation_paths(
    nodes: &[PresentationNodeDef],
    parents: &[usize],
) -> Vec<Vec<String>> {
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

pub(super) fn scan_collectible_condition_contexts(
    manager: &PackageManager,
    root: &[u8],
    presentation_nodes: &[PresentationNodeDef],
) -> Result<HashMap<usize, Vec<PendingProgressionContext>>, String> {
    let definitions = manager
        .read_tag(TagHash(u32_at(
            root,
            8 + COLLECTIBLE_DEFINITION_TABLE_SLOT * 16,
        )?))
        .map_err(|error| format!("Could not read collectible definitions: {error}"))?;
    let (definition_count, definition_rows, row_class) = array_at(&definitions, 8)?;
    if row_class != COLLECTIBLE_DEFINITION_ROW_CLASS {
        return Err(format!(
            "The installed collectible table has unexpected row class 0x{row_class:08X}"
        ));
    }

    let mut contexts = vec![None::<(usize, PendingProgressionContext)>; definition_count];
    for (index, context) in contexts.iter_mut().enumerate() {
        let row = definition_rows
            .checked_add(
                index
                    .checked_mul(COLLECTIBLE_DEFINITION_ROW_SIZE)
                    .ok_or("Collectible definition row offset overflowed")?,
            )
            .ok_or("Collectible definition row offset overflowed")?;
        let item_index = usize::from(u16_at(
            &definitions,
            row + COLLECTIBLE_INVENTORY_ITEM_INDEX_OFFSET,
        )?);
        if item_index == usize::from(u16::MAX) {
            continue;
        }
        let parents = definition_index_list(
            &definitions,
            row + COLLECTIBLE_PRESENTATION_NODE_PARENTS_OFFSET,
            PRESENTATION_NODE_INDEX_ROW_CLASS,
            presentation_nodes.len(),
            "collectible presentation-node parent",
        )?;
        let mut references = ConditionReferences::default();
        for offset in COLLECTIBLE_CONDITION_OFFSETS {
            merge_condition_references(
                &mut references,
                condition_references_at(&definitions, row + offset)?,
            );
        }
        *context = Some((
            item_index,
            PendingProgressionContext {
                hash: u64::from(u32_at(&definitions, row + COLLECTIBLE_HASH_OFFSET)?),
                kind: ProgressionContextKind::Collectible,
                references,
                paths: presentation_paths(presentation_nodes, &parents),
            },
        ));
    }

    let mut by_item = HashMap::<usize, Vec<PendingProgressionContext>>::new();
    for (item_index, context) in contexts.into_iter().flatten() {
        if !context.references.flags.is_empty() || !context.references.values.is_empty() {
            by_item.entry(item_index).or_default().push(context);
        }
    }
    Ok(by_item)
}

pub(super) struct ItemProgressionContext<'a> {
    pub hash: u64,
    pub name: &'a str,
    pub type_name: &'a str,
    pub paths: &'a [Vec<String>],
}

pub(super) fn attach_item_condition_contexts(
    item: &[u8],
    item_context: ItemProgressionContext<'_>,
    collectible_contexts: &[PendingProgressionContext],
    flag_definitions: &mut [UnlockDefinition],
    value_definitions: &mut [UnlockDefinition],
) {
    let direct_references = scan_condition_expressions(item);
    let direct_context = ProgressionContextDef {
        hash: item_context.hash,
        kind: ProgressionContextKind::InventoryItem,
        name: item_context.name.to_owned(),
        type_name: item_context.type_name.to_owned(),
        description: String::new(),
        paths: item_context.paths.to_vec(),
    };
    attach_condition_context(
        flag_definitions,
        value_definitions,
        &direct_references,
        &direct_context,
    );

    for pending in collectible_contexts {
        let context = ProgressionContextDef {
            hash: pending.hash,
            kind: pending.kind,
            name: item_context.name.to_owned(),
            type_name: item_context.type_name.to_owned(),
            description: String::new(),
            paths: pending.paths.clone(),
        };
        attach_condition_context(
            flag_definitions,
            value_definitions,
            &pending.references,
            &context,
        );
    }
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

pub(super) fn definition_index_list(
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

fn metric_trait_indices(
    definitions: &[u8],
    metric_definition: usize,
    trait_count: usize,
) -> Result<Vec<usize>, String> {
    definition_index_list(
        definitions,
        metric_definition + METRIC_TRAIT_LIST_OFFSET,
        METRIC_TRAIT_INDEX_ROW_CLASS,
        trait_count,
        "metric trait",
    )
}

pub(super) fn scan_metric_objective_owners(
    package: &mut ProgressionPackageData<'_>,
    presentation_nodes: &[PresentationNodeDef],
    objectives: &mut [ObjectiveDef],
) -> Result<(), String> {
    let metric_traits = scan_metric_traits(package)?;
    let ProgressionPackageData {
        manager,
        root,
        globals,
        localized_tags,
        localized_cache,
    } = package;
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
        let traits = metric_trait_indices(&definitions, definition, metric_traits.len())?
            .into_iter()
            .map(|trait_index| metric_traits[trait_index].clone())
            .collect();
        let description = resolve_string(
            manager,
            localized_tags,
            localized_cache,
            &strings,
            string + 0x10,
        )
        .unwrap_or_default();
        let mut name = resolve_string(
            manager,
            localized_tags,
            localized_cache,
            &strings,
            string + 0x08,
        )
        .unwrap_or_default();
        if name.trim().is_empty() {
            name.clone_from(&description);
        }
        add_objective_owner(
            objectives,
            objective_index,
            ObjectiveOwnerDef {
                hash: u64::from(hash),
                kind: ObjectiveOwnerKind::Metric,
                name,
                type_name: "Metric".into(),
                description,
                traits,
                paths: presentation_paths(presentation_nodes, &parents),
            },
        );
    }
    Ok(())
}

fn scan_metric_traits(
    package: &mut ProgressionPackageData<'_>,
) -> Result<Vec<ObjectiveOwnerTraitDef>, String> {
    let ProgressionPackageData {
        manager,
        root,
        globals,
        localized_tags,
        localized_cache,
    } = package;
    let definitions = manager
        .read_tag(TagHash(u32_at(root, 8 + TRAIT_DEFINITION_TABLE_SLOT * 16)?))
        .map_err(|error| format!("Could not read trait definitions: {error}"))?;
    let strings = manager
        .read_tag(TagHash(u32_at(globals, 16 + TRAIT_STRING_TABLE_SLOT * 16)?))
        .map_err(|error| format!("Could not read trait strings: {error}"))?;
    let (definition_count, definition_rows, definition_class) = array_at(&definitions, 8)?;
    let (string_count, string_rows, string_class) = array_at(&strings, 8)?;
    if definition_class != TRAIT_DEFINITION_ROW_CLASS {
        return Err(format!(
            "The installed trait table has unexpected row class 0x{definition_class:08X}"
        ));
    }
    if string_class != TRAIT_STRING_ROW_CLASS {
        return Err(format!(
            "The installed trait-string table has unexpected row class 0x{string_class:08X}"
        ));
    }
    if definition_count != string_count {
        return Err("The installed trait definition and string tables do not match".into());
    }

    let mut traits = Vec::with_capacity(definition_count);
    for index in 0..definition_count {
        let definition = definition_rows
            .checked_add(
                index
                    .checked_mul(TRAIT_DEFINITION_ROW_SIZE)
                    .ok_or("Trait definition row offset overflowed")?,
            )
            .ok_or("Trait definition row offset overflowed")?;
        let string = string_rows
            .checked_add(
                index
                    .checked_mul(TRAIT_STRING_ROW_SIZE)
                    .ok_or("Trait string row offset overflowed")?,
            )
            .ok_or("Trait string row offset overflowed")?;
        let hash = u32_at(&definitions, definition)?;
        let _ = u32_at(&definitions, definition + 4)?;
        if u32_at(&strings, string)? != hash {
            return Err(format!(
                "Trait definition and string row {index} do not match"
            ));
        }
        let name = resolve_string(
            manager,
            localized_tags,
            localized_cache,
            &strings,
            string + 0x08,
        )
        .unwrap_or_default();
        let description = resolve_string(
            manager,
            localized_tags,
            localized_cache,
            &strings,
            string + 0x10,
        )
        .unwrap_or_default();
        traits.push(ObjectiveOwnerTraitDef {
            hash: u64::from(hash),
            name,
            description,
        });
    }
    Ok(traits)
}

#[derive(Clone, Debug)]
pub(super) struct LocationContext {
    hash: u64,
    name: String,
    description: String,
    releases: Vec<LocationDefinitionRelease>,
}

#[derive(Clone, Debug)]
struct LocationDefinitionRelease {
    activity_index: Option<usize>,
    references: ConditionReferences,
}

fn location_definition_release_at(
    definitions: &[u8],
    release: usize,
) -> Result<LocationDefinitionRelease, String> {
    let activity_offset = release
        .checked_add(LOCATION_RELEASE_ACTIVITY_INDEX_OFFSET)
        .ok_or("Location release activity offset overflowed")?;
    let activity_index = usize::from(u16_at(definitions, activity_offset)?);
    Ok(LocationDefinitionRelease {
        activity_index: (activity_index != usize::from(u16::MAX)).then_some(activity_index),
        references: condition_references_at(definitions, release)?,
    })
}

pub(super) fn scan_location_condition_contexts(
    package: &mut ProgressionPackageData<'_>,
) -> Result<Vec<LocationContext>, String> {
    let ProgressionPackageData {
        manager,
        root,
        globals,
        localized_tags,
        localized_cache,
    } = package;
    let definitions = manager
        .read_tag(TagHash(u32_at(
            root,
            8 + LOCATION_DEFINITION_TABLE_SLOT * 16,
        )?))
        .map_err(|error| format!("Could not read location definitions: {error}"))?;
    let strings = manager
        .read_tag(TagHash(u32_at(
            globals,
            16 + LOCATION_STRING_TABLE_SLOT * 16,
        )?))
        .map_err(|error| format!("Could not read location strings: {error}"))?;
    let (definition_count, definition_rows, definition_class) = array_at(&definitions, 8)?;
    let (string_count, string_rows, string_class) = array_at(&strings, 8)?;
    if definition_class != LOCATION_DEFINITION_ROW_CLASS {
        return Err(format!(
            "The installed location table has unexpected row class 0x{definition_class:08X}"
        ));
    }
    if string_class != LOCATION_STRING_ROW_CLASS {
        return Err(format!(
            "The installed location-string table has unexpected row class 0x{string_class:08X}"
        ));
    }
    if definition_count != string_count {
        return Err("The installed location definition and string tables do not match".into());
    }

    let mut locations = Vec::with_capacity(definition_count);
    for index in 0..definition_count {
        let definition = definition_rows
            .checked_add(
                index
                    .checked_mul(LOCATION_DEFINITION_ROW_SIZE)
                    .ok_or("Location definition row offset overflowed")?,
            )
            .ok_or("Location definition row offset overflowed")?;
        let string = string_rows
            .checked_add(
                index
                    .checked_mul(LOCATION_STRING_ROW_SIZE)
                    .ok_or("Location string row offset overflowed")?,
            )
            .ok_or("Location string row offset overflowed")?;
        let hash = u32_at(&definitions, definition)?;
        if u32_at(&strings, string)? != hash {
            return Err(format!(
                "Location definition and string row {index} do not match"
            ));
        }

        let mut name = String::new();
        let mut description = String::new();
        if u64_at(&strings, string + LOCATION_DISPLAY_LIST_OFFSET)? != 0 {
            let (display_count, display_rows, display_class) =
                array_at(&strings, string + LOCATION_DISPLAY_LIST_OFFSET)?;
            if display_class != LOCATION_DISPLAY_ROW_CLASS {
                return Err(format!("Location {index} has an unexpected display list"));
            }
            for display_index in 0..display_count {
                let display = display_rows
                    .checked_add(
                        display_index
                            .checked_mul(LOCATION_DISPLAY_ROW_SIZE)
                            .ok_or("Location display row offset overflowed")?,
                    )
                    .ok_or("Location display row offset overflowed")?;
                if name.trim().is_empty() {
                    name = resolve_string(
                        manager,
                        localized_tags,
                        localized_cache,
                        &strings,
                        display + 0x04,
                    )
                    .unwrap_or_default();
                }
                if description.trim().is_empty() {
                    description = resolve_string(
                        manager,
                        localized_tags,
                        localized_cache,
                        &strings,
                        display + 0x0C,
                    )
                    .unwrap_or_default();
                }
            }
        }

        let mut releases = Vec::new();
        if u64_at(&definitions, definition + LOCATION_RELEASE_LIST_OFFSET)? != 0 {
            let (release_count, release_rows, release_class) =
                array_at(&definitions, definition + LOCATION_RELEASE_LIST_OFFSET)?;
            if release_class != LOCATION_RELEASE_ROW_CLASS {
                return Err(format!("Location {index} has an unexpected release list"));
            }
            for release_index in 0..release_count {
                let release = release_rows
                    .checked_add(
                        release_index
                            .checked_mul(LOCATION_RELEASE_ROW_SIZE)
                            .ok_or("Location release row offset overflowed")?,
                    )
                    .ok_or("Location release row offset overflowed")?;
                releases.push(location_definition_release_at(&definitions, release)?);
            }
        }
        let context = LocationContext {
            hash: u64::from(hash),
            name,
            description,
            releases,
        };
        locations.push(context);
    }
    Ok(locations)
}

#[derive(Clone, Debug)]
struct ActivityContext {
    hash: u64,
    definition_start: usize,
    name: String,
    description: String,
    gate_hashes: Vec<u32>,
}

pub(super) fn scan_activity_condition_contexts(
    package: &mut ProgressionPackageData<'_>,
    locations: &[LocationContext],
    flag_definitions: &mut [UnlockDefinition],
    value_definitions: &mut [UnlockDefinition],
) -> Result<(), String> {
    let ProgressionPackageData {
        manager,
        root,
        globals,
        localized_tags,
        localized_cache,
    } = package;
    let definitions = manager
        .read_tag(TagHash(u32_at(
            root,
            8 + ACTIVITY_DEFINITION_TABLE_SLOT * 16,
        )?))
        .map_err(|error| format!("Could not read activity definitions: {error}"))?;
    let strings = manager
        .read_tag(TagHash(u32_at(
            globals,
            16 + ACTIVITY_STRING_TABLE_SLOT * 16,
        )?))
        .map_err(|error| format!("Could not read activity strings: {error}"))?;
    let (definition_count, definition_rows, definition_class) = array_at(&definitions, 8)?;
    let (string_count, string_rows, string_class) = array_at(&strings, 8)?;
    if definition_class != ACTIVITY_DEFINITION_ROW_CLASS {
        return Err(format!(
            "The installed activity table has unexpected row class 0x{definition_class:08X}"
        ));
    }
    if string_class != ACTIVITY_STRING_ROW_CLASS {
        return Err(format!(
            "The installed activity-string table has unexpected row class 0x{string_class:08X}"
        ));
    }
    if definition_count != string_count {
        return Err("The installed activity definition and string tables do not match".into());
    }

    let mut activities = Vec::with_capacity(definition_count);
    for index in 0..definition_count {
        let definition_row = definition_rows
            .checked_add(
                index
                    .checked_mul(ACTIVITY_INDEX_ROW_SIZE)
                    .ok_or("Activity definition row offset overflowed")?,
            )
            .ok_or("Activity definition row offset overflowed")?;
        let string_row = string_rows
            .checked_add(
                index
                    .checked_mul(ACTIVITY_INDEX_ROW_SIZE)
                    .ok_or("Activity string row offset overflowed")?,
            )
            .ok_or("Activity string row offset overflowed")?;
        let hash = u32_at(&definitions, definition_row)?;
        if u32_at(&strings, string_row)? != hash {
            return Err(format!(
                "Activity definition and string row {index} do not match"
            ));
        }
        let definition_pointer = definition_row
            .checked_add(8)
            .ok_or("Activity definition pointer offset overflowed")?;
        let definition_start =
            relative_offset(definition_row, 8, i64_at(&definitions, definition_pointer)?)?;
        if u32_at(&definitions, definition_start)? != hash {
            return Err(format!(
                "Activity definition row {index} points to another hash"
            ));
        }

        let string_pointer = string_row
            .checked_add(8)
            .ok_or("Activity string pointer offset overflowed")?;
        let string_structure = relative_offset(string_row, 8, i64_at(&strings, string_pointer)?)?;
        let display = relative_offset(string_structure, 0, i64_at(&strings, string_structure)?)?;
        let name = resolve_string(
            manager,
            localized_tags,
            localized_cache,
            &strings,
            display + 0x04,
        )
        .unwrap_or_default();
        let description = resolve_string(
            manager,
            localized_tags,
            localized_cache,
            &strings,
            display + 0x0C,
        )
        .unwrap_or_default();

        let gate_hashes = activity_gate_hashes(&definitions, definition_start)?;
        activities.push(ActivityContext {
            hash: u64::from(hash),
            definition_start,
            name,
            description,
            gate_hashes,
        });
    }

    let mut distinct_starts = activities
        .iter()
        .map(|activity| activity.definition_start)
        .collect::<Vec<_>>();
    distinct_starts.sort_unstable();
    distinct_starts.dedup();
    let references_by_start = distinct_starts
        .iter()
        .enumerate()
        .map(|(index, &start)| {
            let end = distinct_starts
                .get(index + 1)
                .copied()
                .unwrap_or(definitions.len());
            Ok((
                start,
                scan_condition_expressions_in(&definitions, start, end)?,
            ))
        })
        .collect::<Result<HashMap<_, _>, String>>()?;

    for activity in &activities {
        let Some(references) = references_by_start.get(&activity.definition_start) else {
            continue;
        };
        attach_condition_context(
            flag_definitions,
            value_definitions,
            references,
            &activity_progression_context(activity, ProgressionContextKind::Activity),
        );
    }

    let availability = activity_availability_references(manager, root)?;
    for activity in &activities {
        let mut references = ConditionReferences::default();
        for gate_hash in &activity.gate_hashes {
            if let Some(gate_references) = availability.get(gate_hash) {
                merge_condition_references(&mut references, gate_references.clone());
            }
        }
        attach_condition_context(
            flag_definitions,
            value_definitions,
            &references,
            &activity_progression_context(activity, ProgressionContextKind::ActivityAvailability),
        );
    }
    attach_location_definition_release_contexts(
        locations,
        &activities,
        flag_definitions,
        value_definitions,
    )?;
    attach_location_release_condition_contexts(
        manager,
        root,
        locations,
        &activities,
        flag_definitions,
        value_definitions,
    )?;
    Ok(())
}

fn attach_location_definition_release_contexts(
    locations: &[LocationContext],
    activities: &[ActivityContext],
    flag_definitions: &mut [UnlockDefinition],
    value_definitions: &mut [UnlockDefinition],
) -> Result<(), String> {
    for location in locations {
        for release in &location.releases {
            let activity = match release.activity_index {
                Some(index) => Some(activities.get(index).ok_or_else(|| {
                    format!("Location release has out-of-range activity index {index}")
                })?),
                None => None,
            };
            attach_condition_context(
                flag_definitions,
                value_definitions,
                &release.references,
                &location_release_progression_context(location, activity),
            );
        }
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct LocationReleaseConditionRow {
    location_index: usize,
    references: ConditionReferences,
    activity_index: Option<usize>,
}

fn location_release_condition_row_at(
    data: &[u8],
    row: usize,
) -> Result<LocationReleaseConditionRow, String> {
    let location_offset = row
        .checked_add(LOCATION_RELEASE_LOCATION_INDEX_OFFSET)
        .ok_or("Location-release location offset overflowed")?;
    let condition_offset = row
        .checked_add(LOCATION_RELEASE_CONDITIONS_OFFSET)
        .ok_or("Location-release condition offset overflowed")?;
    let activity_offset = row
        .checked_add(LOCATION_RELEASE_CONDITION_ACTIVITY_INDEX_OFFSET)
        .ok_or("Location-release activity offset overflowed")?;
    let location_index = usize::try_from(u32_at(data, location_offset)?)
        .map_err(|_| "Location-release location index is too large")?;
    let activity_index = usize::from(u16_at(data, activity_offset)?);
    Ok(LocationReleaseConditionRow {
        location_index,
        references: condition_references_at(data, condition_offset)?,
        activity_index: (activity_index != usize::from(u16::MAX)).then_some(activity_index),
    })
}

fn location_release_activity(
    activities: &[ActivityContext],
    activity_index: Option<usize>,
    row_index: usize,
) -> Result<Option<&ActivityContext>, String> {
    activity_index
        .map(|activity_index| {
            activities.get(activity_index).ok_or_else(|| {
                format!(
                    "Location-release condition row {row_index} has out-of-range activity index {activity_index}"
                )
            })
        })
        .transpose()
}

fn attach_location_release_condition_contexts(
    manager: &PackageManager,
    root: &[u8],
    locations: &[LocationContext],
    activities: &[ActivityContext],
    flag_definitions: &mut [UnlockDefinition],
    value_definitions: &mut [UnlockDefinition],
) -> Result<(), String> {
    let data = manager
        .read_tag(TagHash(u32_at(
            root,
            8 + LOCATION_RELEASE_CONDITION_TABLE_SLOT * 16,
        )?))
        .map_err(|error| format!("Could not read location-release conditions: {error}"))?;
    let (count, rows, row_class) = array_at(&data, 8)?;
    if row_class != LOCATION_RELEASE_CONDITION_ROW_CLASS {
        return Err(format!(
            "The installed location-release condition table has unexpected row class 0x{row_class:08X}"
        ));
    }
    for index in 0..count {
        let row = rows
            .checked_add(
                index
                    .checked_mul(LOCATION_RELEASE_CONDITION_ROW_SIZE)
                    .ok_or("Location-release condition row offset overflowed")?,
            )
            .ok_or("Location-release condition row offset overflowed")?;
        let release = location_release_condition_row_at(&data, row)?;
        let Some(location) = locations.get(release.location_index) else {
            return Err(format!(
                "Location-release condition row {index} has an out-of-range location index"
            ));
        };
        let activity = location_release_activity(activities, release.activity_index, index)?;
        attach_condition_context(
            flag_definitions,
            value_definitions,
            &release.references,
            &location_release_progression_context(location, activity),
        );
    }
    Ok(())
}

fn location_release_progression_context(
    location: &LocationContext,
    activity: Option<&ActivityContext>,
) -> ProgressionContextDef {
    let activity = activity.filter(|activity| !activity.name.trim().is_empty());
    ProgressionContextDef {
        hash: activity.map_or(location.hash, |activity| activity.hash),
        kind: ProgressionContextKind::LocationRelease,
        name: activity.map_or_else(|| location.name.clone(), |activity| activity.name.clone()),
        type_name: String::new(),
        description: activity.map_or_else(
            || location.description.clone(),
            |activity| activity.description.clone(),
        ),
        paths: if activity.is_some() && !location.name.trim().is_empty() {
            vec![vec![location.name.clone()]]
        } else {
            Vec::new()
        },
    }
}

fn activity_progression_context(
    activity: &ActivityContext,
    kind: ProgressionContextKind,
) -> ProgressionContextDef {
    ProgressionContextDef {
        hash: activity.hash,
        kind,
        name: activity.name.clone(),
        type_name: String::new(),
        description: activity.description.clone(),
        paths: Vec::new(),
    }
}

fn activity_gate_hashes(data: &[u8], definition_start: usize) -> Result<Vec<u32>, String> {
    let descriptor = definition_start
        .checked_add(ACTIVITY_GATE_LIST_OFFSET)
        .ok_or("Activity gate-list offset overflowed")?;
    if u64_at(data, descriptor)? == 0 {
        return Ok(Vec::new());
    }
    let (count, rows, row_class) = array_at(data, descriptor)?;
    if row_class != ACTIVITY_GATE_ROW_CLASS {
        return Err(format!(
            "Activity has unexpected gate-list row class 0x{row_class:08X}"
        ));
    }
    let end = rows
        .checked_add(
            count
                .checked_mul(4)
                .ok_or("Activity gate-list size overflowed")?,
        )
        .ok_or("Activity gate-list row offset overflowed")?;
    if end > data.len() {
        return Err("Activity gate list extends beyond its package data".into());
    }
    let mut hashes = (0..count)
        .map(|index| u32_at(data, rows + index * 4))
        .collect::<Result<Vec<_>, _>>()?;
    hashes.sort_unstable();
    hashes.dedup();
    Ok(hashes)
}

fn activity_availability_references(
    manager: &PackageManager,
    root: &[u8],
) -> Result<HashMap<u32, ConditionReferences>, String> {
    let data = manager
        .read_tag(TagHash(u32_at(
            root,
            8 + ACTIVITY_AVAILABILITY_TABLE_SLOT * 16,
        )?))
        .map_err(|error| format!("Could not read activity availability: {error}"))?;
    let (count, rows, row_class) = array_at(&data, ACTIVITY_AVAILABILITY_LIST_OFFSET)?;
    if row_class != ACTIVITY_AVAILABILITY_ROW_CLASS {
        return Err(format!(
            "The installed activity-availability table has unexpected row class 0x{row_class:08X}"
        ));
    }
    let mut by_gate = HashMap::new();
    for index in 0..count {
        let row = rows
            .checked_add(
                index
                    .checked_mul(ACTIVITY_AVAILABILITY_ROW_SIZE)
                    .ok_or("Activity-availability row offset overflowed")?,
            )
            .ok_or("Activity-availability row offset overflowed")?;
        let gate_hash = u32_at(&data, row + ACTIVITY_AVAILABILITY_GATE_HASH_OFFSET)?;
        let mut references = ConditionReferences::default();
        for offset in ACTIVITY_AVAILABILITY_GROUP_OFFSETS {
            let descriptor = row + offset;
            if u64_at(&data, descriptor)? == 0 {
                continue;
            }
            let (group_count, group_rows, group_class) = array_at(&data, descriptor)?;
            if group_class != ACTIVITY_AVAILABILITY_GROUP_ROW_CLASS {
                return Err(format!(
                    "Activity-availability row {index} has an unexpected condition group"
                ));
            }
            for group_index in 0..group_count {
                let group = group_rows
                    .checked_add(
                        group_index
                            .checked_mul(ACTIVITY_AVAILABILITY_GROUP_ROW_SIZE)
                            .ok_or("Activity-availability group offset overflowed")?,
                    )
                    .ok_or("Activity-availability group offset overflowed")?;
                merge_condition_references(&mut references, condition_references_at(&data, group)?);
            }
        }
        merge_condition_references(by_gate.entry(gate_hash).or_default(), references);
    }
    Ok(by_gate)
}

pub(super) fn scan_record_objective_owners(
    package: &mut ProgressionPackageData<'_>,
    presentation_nodes: &[PresentationNodeDef],
    objectives: &mut [ObjectiveDef],
    flag_definitions: &mut [UnlockDefinition],
    value_definitions: &mut [UnlockDefinition],
) -> Result<(), String> {
    let ProgressionPackageData {
        manager,
        root,
        globals,
        localized_tags,
        localized_cache,
    } = package;
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
        let paths = presentation_paths(presentation_nodes, &parents);
        let mut condition_references = ConditionReferences::default();
        for offset in RECORD_CONDITION_OFFSETS {
            merge_condition_references(
                &mut condition_references,
                condition_references_at(&definitions, definition + offset)?,
            );
        }
        attach_condition_context(
            flag_definitions,
            value_definitions,
            &condition_references,
            &ProgressionContextDef {
                hash: u64::from(hash),
                kind: ProgressionContextKind::Record,
                name: name.clone(),
                type_name: String::new(),
                description: String::new(),
                paths: paths.clone(),
            },
        );

        for objective_index in record_objective_indices(&definitions, definition, objectives.len())
            .map_err(|error| format!("Record {index}: {error}"))?
        {
            add_objective_owner(
                objectives,
                objective_index,
                ObjectiveOwnerDef {
                    hash: u64::from(hash),
                    kind: ObjectiveOwnerKind::Record,
                    name: name.clone(),
                    type_name: "Record".into(),
                    description: String::new(),
                    traits: Vec::new(),
                    paths: paths.clone(),
                },
            );
        }
    }
    Ok(())
}

fn record_objective_indices(
    definitions: &[u8],
    definition: usize,
    objective_count: usize,
) -> Result<Vec<usize>, String> {
    let mut indices = objective_indices_from_array(
        definitions,
        definition + RECORD_OBJECTIVE_LIST_OFFSET,
        RECORD_OBJECTIVE_INDEX_ROW_CLASS,
        size_of::<u16>(),
        0,
        objective_count,
        "objective list",
    )?;
    indices.extend(objective_indices_from_array(
        definitions,
        definition + RECORD_INTERVAL_OBJECTIVE_LIST_OFFSET,
        RECORD_INTERVAL_OBJECTIVE_ROW_CLASS,
        RECORD_INTERVAL_OBJECTIVE_ROW_SIZE,
        RECORD_INTERVAL_OBJECTIVE_INDEX_OFFSET,
        objective_count,
        "interval objective list",
    )?);
    indices.sort_unstable();
    indices.dedup();
    Ok(indices)
}

#[allow(clippy::too_many_arguments)]
fn objective_indices_from_array(
    data: &[u8],
    descriptor: usize,
    expected_class: u32,
    row_size: usize,
    index_offset: usize,
    objective_count: usize,
    label: &str,
) -> Result<Vec<usize>, String> {
    if u64_at(data, descriptor)? == 0 {
        return Ok(Vec::new());
    }
    let (count, rows, class) = array_at(data, descriptor)?;
    if class != expected_class {
        return Err(format!("unexpected {label} row class 0x{class:08X}"));
    }
    let rows_end = rows
        .checked_add(
            count
                .checked_mul(row_size)
                .ok_or_else(|| format!("{label} size overflowed"))?,
        )
        .ok_or_else(|| format!("{label} offset overflowed"))?;
    if rows_end > data.len() || index_offset + size_of::<u16>() > row_size {
        return Err(format!("{label} extends beyond its package data"));
    }

    let mut indices = Vec::with_capacity(count);
    for index in 0..count {
        let objective_index = usize::from(u16_at(data, rows + index * row_size + index_offset)?);
        if objective_index < objective_count {
            indices.push(objective_index);
        }
    }
    Ok(indices)
}

pub(super) fn item_objective_indices(item: &[u8], objective_count: usize) -> Vec<usize> {
    let mut indices = Vec::new();
    let Ok(relative) = i64_at(item, ITEM_OBJECTIVE_RESOURCE_POINTER_OFFSET) else {
        return indices;
    };
    if relative == 0 {
        return indices;
    }
    let Ok(resource) = relative_offset(ITEM_OBJECTIVE_RESOURCE_POINTER_OFFSET, 0, relative) else {
        return indices;
    };
    let Some(resource_class_offset) = resource.checked_sub(size_of::<u32>()) else {
        return indices;
    };
    if u32_at(item, resource_class_offset) != Ok(ITEM_OBJECTIVE_RESOURCE_CLASS) {
        return indices;
    }
    let Ok((count, rows, class)) = array_at(item, resource) else {
        return indices;
    };
    if class != ITEM_OBJECTIVE_INDEX_ROW_CLASS || count > objective_count {
        return indices;
    }
    let Some(byte_count) = count.checked_mul(size_of::<u16>()) else {
        return indices;
    };
    if rows
        .checked_add(byte_count)
        .is_none_or(|end| end > item.len())
    {
        return indices;
    }
    for index in 0..count {
        if let Ok(index) = u16_at(item, rows + index * size_of::<u16>()) {
            let index = usize::from(index);
            if index < objective_count {
                indices.push(index);
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

    fn write_condition_array(
        data: &mut [u8],
        descriptor: usize,
        header: usize,
        rows: &[(u32, u32)],
    ) {
        let count = u64::try_from(rows.len()).unwrap();
        let pointer = descriptor + 8;
        let relative = i64::try_from(header).unwrap() - i64::try_from(pointer).unwrap();
        data[descriptor..descriptor + 8].copy_from_slice(&count.to_le_bytes());
        data[pointer..pointer + 8].copy_from_slice(&relative.to_le_bytes());
        data[header..header + 8].copy_from_slice(&count.to_le_bytes());
        data[header + 8..header + 12]
            .copy_from_slice(&CONDITION_EXPRESSION_ROW_CLASS.to_le_bytes());
        for (index, (kind, operand)) in rows.iter().copied().enumerate() {
            let row = header + 16 + index * CONDITION_EXPRESSION_ROW_SIZE;
            data[row..row + 4].copy_from_slice(&kind.to_le_bytes());
            data[row + 4..row + 8].copy_from_slice(&operand.to_le_bytes());
        }
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
                tested_by: Vec::new(),
            },
            UnlockDefinition {
                hash: 0x2222_2222,
                code: 0x0001,
                compact_slot: Some(58),
                name: None,
                description: None,
                tested_by: Vec::new(),
            },
            UnlockDefinition {
                hash: 0x3333_3333,
                code: 0x0002,
                compact_slot: Some(58),
                name: None,
                description: None,
                tested_by: Vec::new(),
            },
            UnlockDefinition {
                hash: 0x4444_4444,
                code: 0x0001,
                compact_slot: None,
                name: None,
                description: None,
                tested_by: Vec::new(),
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
    fn objective_conditions_use_row_plus_08_and_ignore_plus_10_decoy() {
        const ACTUAL_HEADER: usize = 0x60;
        const DECOY_HEADER: usize = 0x100;
        const DECOY_COUNT: usize = ACTUAL_HEADER - 0x10;
        let mut definitions =
            vec![0_u8; DECOY_HEADER + 16 + DECOY_COUNT * CONDITION_EXPRESSION_ROW_SIZE];

        definitions[OBJECTIVE_CONDITIONS_OFFSET..OBJECTIVE_CONDITIONS_OFFSET + 8]
            .copy_from_slice(&1_u64.to_le_bytes());
        definitions[OBJECTIVE_CONDITIONS_OFFSET + 8..OBJECTIVE_CONDITIONS_OFFSET + 16]
            .copy_from_slice(&i64::try_from(DECOY_COUNT).unwrap().to_le_bytes());
        definitions[0x18..0x20]
            .copy_from_slice(&i64::try_from(DECOY_HEADER - 0x18).unwrap().to_le_bytes());

        definitions[ACTUAL_HEADER..ACTUAL_HEADER + 8].copy_from_slice(&1_u64.to_le_bytes());
        definitions[ACTUAL_HEADER + 8..ACTUAL_HEADER + 12]
            .copy_from_slice(&CONDITION_EXPRESSION_ROW_CLASS.to_le_bytes());
        definitions[ACTUAL_HEADER + 16..ACTUAL_HEADER + 20]
            .copy_from_slice(&CONDITION_FLAG_KIND.to_le_bytes());
        definitions[ACTUAL_HEADER + 20..ACTUAL_HEADER + 24].copy_from_slice(&3_u32.to_le_bytes());

        definitions[DECOY_HEADER..DECOY_HEADER + 8]
            .copy_from_slice(&u64::try_from(DECOY_COUNT).unwrap().to_le_bytes());
        definitions[DECOY_HEADER + 8..DECOY_HEADER + 12]
            .copy_from_slice(&CONDITION_EXPRESSION_ROW_CLASS.to_le_bytes());
        definitions[DECOY_HEADER + 16..DECOY_HEADER + 20]
            .copy_from_slice(&CONDITION_VALUE_KIND.to_le_bytes());
        definitions[DECOY_HEADER + 20..DECOY_HEADER + 24].copy_from_slice(&9_u32.to_le_bytes());

        assert_eq!(
            objective_condition_references_at(&definitions, 0).unwrap(),
            ConditionReferences {
                flags: vec![3],
                values: Vec::new(),
            }
        );
        assert_eq!(
            condition_references_at(&definitions, 0x10).unwrap(),
            ConditionReferences {
                flags: Vec::new(),
                values: vec![9],
            }
        );
    }

    #[test]
    fn location_definition_releases_use_activity_u16_at_12_and_own_conditions() {
        const FIRST_RELEASE: usize = 0;
        const SECOND_RELEASE: usize = LOCATION_RELEASE_ROW_SIZE;
        const FIRST_HEADER: usize = 0xB0;
        const SECOND_HEADER: usize = 0xD0;
        let mut definitions = vec![0_u8; 0xF0];
        write_condition_array(
            &mut definitions,
            FIRST_RELEASE,
            FIRST_HEADER,
            &[(CONDITION_FLAG_KIND, 4)],
        );
        write_condition_array(
            &mut definitions,
            SECOND_RELEASE,
            SECOND_HEADER,
            &[(CONDITION_VALUE_KIND, 6)],
        );
        definitions[FIRST_RELEASE + 0x10..FIRST_RELEASE + 0x12]
            .copy_from_slice(&0xBEEF_u16.to_le_bytes());
        definitions[FIRST_RELEASE + LOCATION_RELEASE_ACTIVITY_INDEX_OFFSET
            ..FIRST_RELEASE + LOCATION_RELEASE_ACTIVITY_INDEX_OFFSET + 2]
            .copy_from_slice(&0x1234_u16.to_le_bytes());
        definitions[SECOND_RELEASE + LOCATION_RELEASE_ACTIVITY_INDEX_OFFSET
            ..SECOND_RELEASE + LOCATION_RELEASE_ACTIVITY_INDEX_OFFSET + 2]
            .copy_from_slice(&u16::MAX.to_le_bytes());

        let first = location_definition_release_at(&definitions, FIRST_RELEASE).unwrap();
        let second = location_definition_release_at(&definitions, SECOND_RELEASE).unwrap();

        assert_eq!(first.activity_index, Some(0x1234));
        assert_eq!(first.references.flags, vec![4]);
        assert!(first.references.values.is_empty());
        assert_eq!(second.activity_index, None);
        assert!(second.references.flags.is_empty());
        assert_eq!(second.references.values, vec![6]);
    }

    #[test]
    fn location_release_condition_rows_use_exact_fields_and_reject_bad_activity_indices() {
        const HEADER: usize = 0x40;
        let mut data = vec![0_u8; 0x58];
        data[LOCATION_RELEASE_LOCATION_INDEX_OFFSET..LOCATION_RELEASE_LOCATION_INDEX_OFFSET + 4]
            .copy_from_slice(&7_u32.to_le_bytes());
        write_condition_array(
            &mut data,
            LOCATION_RELEASE_CONDITIONS_OFFSET,
            HEADER,
            &[(CONDITION_FLAG_KIND, 11)],
        );
        data[0x18..0x1C].copy_from_slice(&0xDEAD_BEEF_u32.to_le_bytes());
        data[LOCATION_RELEASE_CONDITION_ACTIVITY_INDEX_OFFSET
            ..LOCATION_RELEASE_CONDITION_ACTIVITY_INDEX_OFFSET + 2]
            .copy_from_slice(&2_u16.to_le_bytes());

        let release = location_release_condition_row_at(&data, 0).unwrap();
        assert_eq!(release.location_index, 7);
        assert_eq!(release.references.flags, vec![11]);
        assert!(release.references.values.is_empty());
        assert_eq!(release.activity_index, Some(2));

        let activities = vec![ActivityContext {
            hash: 1,
            definition_start: 0,
            name: "Only activity".into(),
            description: String::new(),
            gate_hashes: Vec::new(),
        }];
        let error = location_release_activity(&activities, release.activity_index, 5).unwrap_err();
        assert!(error.contains("row 5"));
        assert!(error.contains("activity index 2"));

        data[LOCATION_RELEASE_CONDITION_ACTIVITY_INDEX_OFFSET
            ..LOCATION_RELEASE_CONDITION_ACTIVITY_INDEX_OFFSET + 2]
            .copy_from_slice(&u16::MAX.to_le_bytes());
        let release = location_release_condition_row_at(&data, 0).unwrap();
        assert_eq!(release.activity_index, None);
        assert!(
            location_release_activity(&activities, release.activity_index, 5)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn presentation_paths_keep_immediate_parent_first_and_preserve_branches() {
        let node = |hash, name: &str, parents| PresentationNodeDef {
            hash,
            name: name.into(),
            parents,
            objective_index: None,
            condition_references: ConditionReferences::default(),
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
    fn metric_traits_use_the_authored_u16_list_without_a_row_cap() {
        const METRIC_DEFINITION: usize = 0x20;
        const HEADER: usize = 0x80;
        const ROWS: usize = HEADER + 0x10;
        let descriptor = METRIC_DEFINITION + METRIC_TRAIT_LIST_OFFSET;
        let mut definitions = vec![0_u8; ROWS + 6];
        definitions[descriptor..descriptor + 8].copy_from_slice(&3_u64.to_le_bytes());
        definitions[descriptor + 8..descriptor + 16]
            .copy_from_slice(&((HEADER - (descriptor + 8)) as i64).to_le_bytes());
        definitions[HEADER..HEADER + 8].copy_from_slice(&3_u64.to_le_bytes());
        definitions[HEADER + 8..HEADER + 12]
            .copy_from_slice(&METRIC_TRAIT_INDEX_ROW_CLASS.to_le_bytes());
        definitions[ROWS..ROWS + 2].copy_from_slice(&5_u16.to_le_bytes());
        definitions[ROWS + 2..ROWS + 4].copy_from_slice(&71_u16.to_le_bytes());
        definitions[ROWS + 4..ROWS + 6].copy_from_slice(&74_u16.to_le_bytes());

        assert_eq!(
            metric_trait_indices(&definitions, METRIC_DEFINITION, 75).unwrap(),
            vec![5, 71, 74]
        );

        definitions[HEADER + 8..HEADER + 12].copy_from_slice(&0xDEAD_BEEF_u32.to_le_bytes());
        let error = metric_trait_indices(&definitions, METRIC_DEFINITION, 75).unwrap_err();
        assert!(error.contains("Unexpected metric trait row class 0xDEADBEEF"));
    }

    #[test]
    fn item_objective_lists_follow_the_exact_resource_pointer_and_deduplicate() {
        const RESOURCE: usize = 0x80;
        const HEADER: usize = 0xA0;
        const ROWS: usize = HEADER + 0x10;
        let mut item = vec![0_u8; ROWS + 6];
        item[ITEM_OBJECTIVE_RESOURCE_POINTER_OFFSET..ITEM_OBJECTIVE_RESOURCE_POINTER_OFFSET + 8]
            .copy_from_slice(
                &((RESOURCE - ITEM_OBJECTIVE_RESOURCE_POINTER_OFFSET) as i64).to_le_bytes(),
            );
        item[RESOURCE - 4..RESOURCE].copy_from_slice(&ITEM_OBJECTIVE_RESOURCE_CLASS.to_le_bytes());
        item[RESOURCE..RESOURCE + 8].copy_from_slice(&3_u64.to_le_bytes());
        item[RESOURCE + 8..RESOURCE + 16]
            .copy_from_slice(&((HEADER - (RESOURCE + 8)) as i64).to_le_bytes());
        item[HEADER..HEADER + 8].copy_from_slice(&3_u64.to_le_bytes());
        item[HEADER + 8..HEADER + 12]
            .copy_from_slice(&ITEM_OBJECTIVE_INDEX_ROW_CLASS.to_le_bytes());
        item[ROWS..ROWS + 2].copy_from_slice(&5_u16.to_le_bytes());
        item[ROWS + 2..ROWS + 4].copy_from_slice(&2_u16.to_le_bytes());
        item[ROWS + 4..ROWS + 6].copy_from_slice(&5_u16.to_le_bytes());

        assert_eq!(item_objective_indices(&item, 10), vec![2, 5]);

        item[RESOURCE - 4..RESOURCE].copy_from_slice(&0_u32.to_le_bytes());
        assert!(item_objective_indices(&item, 10).is_empty());
    }

    #[test]
    fn item_objective_lists_ignore_decoy_arrays_outside_the_resource_pointer() {
        let mut item = vec![0_u8; 96];
        item[0..8].copy_from_slice(&1_u64.to_le_bytes());
        item[8..16].copy_from_slice(&8_i64.to_le_bytes());
        item[16..24].copy_from_slice(&1_u64.to_le_bytes());
        item[24..28].copy_from_slice(&ITEM_OBJECTIVE_INDEX_ROW_CLASS.to_le_bytes());
        item[32..34].copy_from_slice(&5_u16.to_le_bytes());

        assert!(item_objective_indices(&item, 10).is_empty());
    }

    #[test]
    fn record_objective_lists_merge_direct_and_shadowkeep_interval_rows() {
        const DIRECT_HEADER: usize = 0x80;
        const DIRECT_ROWS: usize = DIRECT_HEADER + 0x10;
        const INTERVAL_HEADER: usize = 0xA0;
        const INTERVAL_ROWS: usize = INTERVAL_HEADER + 0x10;
        let mut definitions = vec![0_u8; INTERVAL_ROWS + 2 * RECORD_INTERVAL_OBJECTIVE_ROW_SIZE];

        definitions[RECORD_OBJECTIVE_LIST_OFFSET..RECORD_OBJECTIVE_LIST_OFFSET + 8]
            .copy_from_slice(&2_u64.to_le_bytes());
        definitions[RECORD_OBJECTIVE_LIST_OFFSET + 8..RECORD_OBJECTIVE_LIST_OFFSET + 16]
            .copy_from_slice(
                &((DIRECT_HEADER - (RECORD_OBJECTIVE_LIST_OFFSET + 8)) as i64).to_le_bytes(),
            );
        definitions[DIRECT_HEADER..DIRECT_HEADER + 8].copy_from_slice(&2_u64.to_le_bytes());
        definitions[DIRECT_HEADER + 8..DIRECT_HEADER + 12]
            .copy_from_slice(&RECORD_OBJECTIVE_INDEX_ROW_CLASS.to_le_bytes());
        definitions[DIRECT_ROWS..DIRECT_ROWS + 2].copy_from_slice(&5_u16.to_le_bytes());
        definitions[DIRECT_ROWS + 2..DIRECT_ROWS + 4].copy_from_slice(&7_u16.to_le_bytes());

        definitions
            [RECORD_INTERVAL_OBJECTIVE_LIST_OFFSET..RECORD_INTERVAL_OBJECTIVE_LIST_OFFSET + 8]
            .copy_from_slice(&2_u64.to_le_bytes());
        definitions
            [RECORD_INTERVAL_OBJECTIVE_LIST_OFFSET + 8..RECORD_INTERVAL_OBJECTIVE_LIST_OFFSET + 16]
            .copy_from_slice(
                &((INTERVAL_HEADER - (RECORD_INTERVAL_OBJECTIVE_LIST_OFFSET + 8)) as i64)
                    .to_le_bytes(),
            );
        definitions[INTERVAL_HEADER..INTERVAL_HEADER + 8].copy_from_slice(&2_u64.to_le_bytes());
        definitions[INTERVAL_HEADER + 8..INTERVAL_HEADER + 12]
            .copy_from_slice(&RECORD_INTERVAL_OBJECTIVE_ROW_CLASS.to_le_bytes());
        definitions[INTERVAL_ROWS..INTERVAL_ROWS + 2].copy_from_slice(&2_u16.to_le_bytes());
        definitions[INTERVAL_ROWS + RECORD_INTERVAL_OBJECTIVE_ROW_SIZE
            ..INTERVAL_ROWS + RECORD_INTERVAL_OBJECTIVE_ROW_SIZE + 2]
            .copy_from_slice(&5_u16.to_le_bytes());

        assert_eq!(
            record_objective_indices(&definitions, 0, 10).unwrap(),
            vec![2, 5, 7]
        );
    }

    #[test]
    fn collectible_item_paths_use_the_authored_u16_item_index_and_presentation_parents() {
        let nodes = vec![
            PresentationNodeDef {
                hash: 1,
                name: "Items".into(),
                parents: Vec::new(),
                objective_index: None,
                condition_references: ConditionReferences::default(),
            },
            PresentationNodeDef {
                hash: 2,
                name: "Majestic Solstice Suit".into(),
                parents: vec![0],
                objective_index: None,
                condition_references: ConditionReferences::default(),
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

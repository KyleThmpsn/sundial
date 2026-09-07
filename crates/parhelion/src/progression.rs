#[cfg(test)]
mod tests;

mod unlocks;
use unlocks::*;
pub(crate) use unlocks::{
    append_unlock, append_unlock_display, append_unlock_flag_bank_row, first_free_unlock_slot,
    unlock_flag_bank_descriptor, unlock_sorted_index_position,
    validate_authored_unlock_display_row, validate_authored_unlock_flag_bank_row,
};

mod conditions;
use conditions::*;
pub(crate) use conditions::{
    classify_sunrise_count_pools, numeric_program_layout, numeric_program_stack_depth,
    patch_project_acquired_count_programs, patch_project_collection_objectives,
    shared_numeric_instruction_template, validate_shared_expression_table,
};

mod collectibles;
#[cfg(test)]
use collectibles::*;
pub(crate) use collectibles::{
    append_collectible, append_collectible_display, collection_unlock_index,
    donor_weapon_collection_page, template_presentation_parents,
    validate_authored_collectible_display_row, validate_authored_collectible_nested_isolation,
    validate_weapon_material_sets,
};

use std::{
    collections::{BTreeMap, BTreeSet},
    mem::size_of,
};

pub(crate) use sundial::package_authoring::investment_schema::{
    COLLECTIBLE_DEFINITION_ROW_CLASS, COLLECTIBLE_DEFINITION_ROW_SIZE as COLLECTIBLE_ROW_SIZE,
    COLLECTIBLE_DISPLAY_DESCRIPTION_REFERENCE_OFFSET, COLLECTIBLE_DISPLAY_ICON_INDEX_OFFSET,
    COLLECTIBLE_DISPLAY_NAME_REFERENCE_OFFSET, COLLECTIBLE_DISPLAY_REQUIREMENT_REFERENCE_OFFSET,
    COLLECTIBLE_DISPLAY_ROW_CLASS, COLLECTIBLE_DISPLAY_ROW_SIZE,
    COLLECTIBLE_DISPLAY_SOURCE_REFERENCE_OFFSET, COLLECTIBLE_HASH_OFFSET,
    COLLECTIBLE_INVENTORY_ITEM_INDEX_OFFSET as COLLECTIBLE_ITEM_INDEX_OFFSET,
    COLLECTIBLE_MATERIAL_REQUIREMENT_SET_INDEX_OFFSET as COLLECTIBLE_MATERIAL_SET_OFFSET,
    COLLECTIBLE_PRESENTATION_NODE_PARENTS_OFFSET,
    CONDITION_EXPRESSION_ROW_CLASS as NUMERIC_PROGRAM_ROW_CLASS,
    CONDITION_EXPRESSION_ROW_SIZE as NUMERIC_INSTRUCTION_ROW_SIZE, MATERIAL_REQUIREMENT_ROW_CLASS,
    MATERIAL_REQUIREMENT_ROW_SIZE, MATERIAL_REQUIREMENT_SET_ROW_CLASS,
    MATERIAL_REQUIREMENT_SET_ROW_SIZE, NESTED_ARRAY_TRAILER, OBJECTIVE_COMPLETION_VALUE_OFFSET,
    OBJECTIVE_DEFINITION_ROW_CLASS as OBJECTIVE_ROW_CLASS,
    OBJECTIVE_DEFINITION_ROW_SIZE as OBJECTIVE_ROW_SIZE, PRESENTATION_NODE_DEFINITION_ROW_CLASS,
    PRESENTATION_NODE_DEFINITION_ROW_SIZE as PRESENTATION_NODE_ROW_SIZE,
    PRESENTATION_NODE_HASH_OFFSET, PRESENTATION_NODE_INDEX_ROW_CLASS,
    PRESENTATION_NODE_OBJECTIVE_INDEX_OFFSET, PRESENTATION_NODE_STRING_ROW_SIZE,
    SHARED_EXPRESSION_POOL_COUNT,
    SHARED_EXPRESSION_POOL_HASHED_EXPRESSION_OFFSET as SHARED_EXPRESSION_DESCRIPTOR_OFFSET,
    SHARED_EXPRESSION_POOL_HASHED_ROW_CLASS as SHARED_EXPRESSION_POOL_ROW_CLASS,
    SHARED_EXPRESSION_POOL_HASHED_ROW_SIZE as SHARED_EXPRESSION_POOL_ROW_SIZE,
    SHARED_EXPRESSION_POOL_PARALLEL_ROW_CLASS as SHARED_EXPRESSION_PARALLEL_ROW_CLASS,
};
use sundial::package_authoring::investment_schema::{
    UNLOCK_FLAG_DEFINITION_ROW_CLASS, UNLOCK_FLAG_DEFINITION_ROW_SIZE,
    UNLOCK_FLAG_DISPLAY_CONTENT_ROW_CLASS, UNLOCK_FLAG_DISPLAY_CONTENT_ROW_SIZE,
    UNLOCK_FLAG_DISPLAY_ROW_CLASS, UNLOCK_FLAG_DISPLAY_ROW_SIZE,
    UNLOCK_FLAG_SORTED_INDEX_ROW_CLASS, UNLOCK_FLAG_SORTED_INDEX_ROW_SIZE,
};

use crate::{
    AuthoringError, AuthoringResult,
    error::{invalid, validation},
    tag_payload::{
        array_at, read_array, read_i32, read_u16, read_u32, read_u64, relative_target,
        set_array_count, write_i32, write_localized_reference, write_relative_pointer, write_u16,
        write_u32, write_u64,
    },
    weapon::WeaponCloneIdentity,
};

pub(crate) const COLLECTIBLE_CURATED_ACQUISITION_FLAG_OFFSET: usize = 0x00;
pub(crate) const COLLECTIBLE_CURATED_ACQUISITION_FLAG: u8 = 1;
pub(crate) const COLLECTIBLE_REACQUISITION_STATE_OFFSET: usize = 0x98;
pub(crate) const COLLECTIBLE_REACQUISITION_ENABLED: u16 = 0;
pub(crate) const COLLECTIBLE_EXOTIC_WEAPON_MATERIAL_SET: u16 = 97;
pub(crate) const COLLECTIBLE_CURATED_WEAPON_MATERIAL_SET: u16 = 145;
pub(crate) const CURATED_WEAPON_MATERIAL_SET_HASH: u32 = 0x415A_1D3B;
pub(crate) const CURATED_WEAPON_MATERIAL_REQUIREMENTS: [(u32, u32); 4] =
    [(123, 500), (128, 5), (1_869, 5), (1_414, 7)];
pub(crate) const EXOTIC_WEAPON_MATERIAL_SET_HASH: u32 = 0x727C_AEEE;
pub(crate) const EXOTIC_WEAPON_MATERIAL_REQUIREMENTS: [(u32, u32); 3] =
    [(123, 777), (128, 7), (1_869, 7)];
pub(crate) const COLLECTIBLE_CONDITION_OFFSET: usize = 0x70;
// Native reacquisition applies these by socket type, overriding the item's own defaults.
pub(crate) const COLLECTIBLE_SOCKET_OVERRIDES_OFFSET: usize = 0xA8;
pub(crate) const COLLECTIBLE_SOCKET_OVERRIDE_ROW_CLASS: u32 = 0x8080_3062;
pub(crate) const COLLECTIBLE_SOCKET_OVERRIDE_ROW_SIZE: usize = 12;
pub(crate) const COLLECTIBLE_POINTER_FIELDS: [usize; 7] =
    [0x18, 0x30, 0x40, 0x50, 0x60, 0x70, 0xA8];
pub(crate) const BLANK_LOCALIZED_REFERENCE_TABLE_INDEX: u32 = 0x0000_FFFF;
pub(crate) const BLANK_LOCALIZED_REFERENCE_HASH: u32 = sundial::package_authoring::FNV1_EMPTY_HASH;
pub(crate) const COLLECTIBLE_DISPLAY_CONDITION_OFFSET: usize = 0x48;
pub(crate) const PRESENTATION_NODE_POINTER_FIELDS: [usize; 8] =
    [0x18, 0x30, 0x40, 0x58, 0x68, 0x78, 0x88, 0x98];
pub(crate) const PRESENTATION_NODE_RECORD_INDEX_OFFSET: usize = 0x52;
pub(crate) const PRESENTATION_NODE_CHILD_NODES_OFFSET: usize = 0x68;
pub(crate) const PRESENTATION_NODE_CHILD_NODE_ROW_SIZE: usize = 0x18;
pub(crate) const PRESENTATION_NODE_CHILD_NODE_ROW_CLASS: u32 = 0x8080_306A;
pub(crate) const PRESENTATION_NODE_COLLECTIBLES_OFFSET: usize = 0x78;
pub(crate) const PRESENTATION_NODE_COLLECTIBLE_ROW_SIZE: usize = 4;
pub(crate) const PRESENTATION_NODE_COLLECTIBLE_ROW_CLASS: u32 = 0x8080_3068;
pub(crate) const PRESENTATION_NODE_STRING_ROW_CLASS: u32 = 0x8080_2E15;
pub(crate) const PRESENTATION_NODE_STRING_ICON_OFFSET: usize = 0x04;
pub(crate) const PRESENTATION_NODE_STRING_NAME_REFERENCE_OFFSET: usize = 0x08;
pub(crate) const PRESENTATION_NODE_STRING_DESCRIPTION_REFERENCE_OFFSET: usize = 0x10;
pub(crate) const STOCK_PRESENTATION_NODE_COUNT: usize = 924;
pub(crate) const BADGES_ROOT_NODE_INDEX: usize = 457;
pub(crate) const BADGES_ROOT_NODE_HASH: u32 = 0x1DB2_1A03;
pub(crate) const BADGES_ROOT_OBJECTIVE_INDEX: u16 = 1385;
pub(crate) const NUMERIC_FLAG_INSTRUCTION: u8 = 1;
pub(crate) const NUMERIC_AND_INSTRUCTION: u8 = 4;
pub(crate) const NUMERIC_VALUE_INSTRUCTION: u8 = 10;
pub(crate) const NUMERIC_POOL_INSTRUCTION: u8 = 12;
pub(crate) const NUMERIC_ADD_INSTRUCTION: u8 = 17;
const NESTED_ARRAY_MARKER: [u8; 4] = [0xBD, 0x9F, 0x80, 0x80];
pub(crate) const UNLOCK_ROW_SIZE: usize = UNLOCK_FLAG_DEFINITION_ROW_SIZE;
pub(crate) const UNLOCK_FLAG_BANK_ROW_SIZE: usize = 8;
pub(crate) const UNLOCK_FLAG_BANK_ROW_CLASS: u32 = 0x8080_7D48;
pub(crate) const UNLOCK_DISPLAY_ROW_SIZE: usize = UNLOCK_FLAG_DISPLAY_ROW_SIZE;

struct UnlockTableLayout {
    count: usize,
    primary_header: usize,
    primary_rows: usize,
    primary_end: usize,
    secondary_header: usize,
    secondary_rows: usize,
    order: Vec<usize>,
    ordered_hashes: Vec<u32>,
}

#[derive(Clone, Debug)]
struct CollectibleNestedClone {
    field: usize,
    count: usize,
    class: u32,
    bytes: Vec<u8>,
    retargeted_source_flags: usize,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct CollectibleUnlockClone {
    pub(crate) source_index: usize,
    pub(crate) authored_index: u16,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct AuthoredCollectibleSpec<'a> {
    pub(crate) collectible_hash: u32,
    pub(crate) item_index: u16,
    pub(crate) unlock: CollectibleUnlockClone,
    pub(crate) material_set_index: u16,
    pub(crate) presentation_parents: &'a [u16],
    pub(crate) require_donor_parent_subset: bool,
}

#[derive(Debug)]
pub(crate) struct NumericProgramLayout {
    pub(crate) count: usize,
    pub(crate) header: usize,
    pub(crate) rows_end: usize,
    pub(crate) segment_end: usize,
    pub(crate) instructions: Vec<NumericInstruction>,
    pub(crate) tokens: Vec<(u8, u16)>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct NumericInstruction {
    pub(crate) opcode: u8,
    pub(crate) operand: u16,
    pub(crate) serialized: [u8; NUMERIC_INSTRUCTION_ROW_SIZE],
}

impl NumericInstruction {
    fn read(data: &[u8], row: usize) -> AuthoringResult<Self> {
        let serialized = read_array(data, row)?;
        Ok(Self {
            opcode: serialized[0],
            operand: u16::from_le_bytes([serialized[4], serialized[5]]),
            serialized,
        })
    }

    pub(crate) fn with_semantics(mut self, opcode: u8, operand: u16) -> Self {
        self.opcode = opcode;
        self.operand = operand;
        self.serialized[0] = opcode;
        self.serialized[4..6].copy_from_slice(&operand.to_le_bytes());
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AcquiredCountPoolLink {
    pool_index: usize,
    source_count: usize,
    source_opcode: u8,
    source_operand: u16,
    direct_additive_flag: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct SunriseAcquiredPoolSelection {
    selected: BTreeSet<usize>,
    excluded_badge: BTreeSet<usize>,
}

#[derive(Clone, Debug)]
pub(crate) struct ProjectAuthoredRow {
    pub(crate) donor_collectible_index: usize,
    pub(crate) authored_collectible_index: usize,
    pub(crate) weapon_page: u16,
    pub(crate) source_acquired_flag: u16,
    pub(crate) authored_unlock_index: u16,
    pub(crate) count_selection: SunriseAcquiredPoolSelection,
}

#[derive(Clone, Copy)]
struct NumericStackNode {
    contains_target: bool,
    additive_target_path: bool,
}

struct UnlockDisplayTableLayout {
    count: usize,
    primary_header: usize,
    primary_rows: usize,
    primary_end: usize,
    content_header: usize,
    content_rows: usize,
    content_count: usize,
}

use std::{
    cmp::{Ordering, Reverse},
    collections::{HashMap, HashSet},
};

use eframe::egui;
use serde_json::{Map, Value};

#[cfg(test)]
use crate::catalog::{
    ObjectiveOwnerDef, ObjectiveOwnerKind, ObjectiveOwnerTraitDef, ProgressionContextKind,
};
use crate::{
    catalog::{
        Catalog, ObjectiveDef, ProgressionContextDef, ProgressionDefinition, ProgressionScope,
        UnlockDefinition,
    },
    hash::format_hash_hex,
};

#[cfg(test)]
use super::inspector::objective_goal_text;
use super::{
    glyphs::Glyph,
    inspector::{
        HashInspectionState, MetadataSelection, OverrideFilter, ProgressionInspectorState,
        definition_has_undecoded_opcodes, definition_hash_hex_text, definition_identity,
        definition_metadata_tooltip, definition_name, draw_catalog_hash_window, draw_hash_hex_cell,
        draw_hash_link, draw_progression_metadata_workspace, flag_override_state_help,
        flag_override_state_label, meaningful_definition_contexts, objective_description,
        objective_details_tooltip, objective_owner_type, objective_table_text,
        objective_target_text, objective_traits_text, override_filter_matches, override_meaning,
        override_meaning_contexts, progression_context_kind_label, progression_type_label,
        resolved_objective_table_text, take_definition_context as take_hash_inspection_context,
        take_definition_request as take_hash_inspection_request,
    },
    ui::{
        TABLE_CELL_HEIGHT, TABLE_COLUMN_GAP, hierarchy_branch_cell as draw_hierarchy_branch_cell,
        hierarchy_leaf_cell as draw_hierarchy_leaf_cell, sortable_header_cell, table_cell,
        toolbar as progression_toolbar,
    },
};

mod add_dialogs;
mod browser;
mod document;
mod evaluation;
mod hierarchy;
mod mutations;
mod native;
mod override_tables;
mod page;
pub(in crate::app) mod seasonal;
mod state;
mod table_ui;
mod unlock_tables;
mod workspace;

#[cfg(test)]
mod tests;

use document::{
    ACCOUNT_FLAG_BANK, ACCOUNT_FLAG_CAPACITY, ACCOUNT_OBJECTIVE_BANK, CHARACTER_FLAG_BANK,
    CHARACTER_FLAG_CAPACITY, CHARACTER_OBJECT_FLAG_BANK, CHARACTER_OBJECT_FLAG_CAPACITY,
    CHARACTER_OBJECT_VALUE_CAPACITY, CHARACTER_OBJECTIVE_BANK, FAMILY5_FLAG_SLOT_MAXIMUM,
    FAMILY5_FLAG_VALUE_MAXIMUM, FAMILY5_OVERRIDE_CAPACITY, FAMILY5_VALUE_SLOT_MAXIMUM, FlagIndex,
    FlagOverride, FlagRun, IndexedValue, InvestmentPolicy, OBJECTIVE_VALUE_CAPACITY,
    PROFILE_FLAG_BANK, PROFILE_FLAG_CAPACITY, PROGRESSION_DEFINITION_CAPACITY, Progression,
    ProgressionValue, RESERVED_CHARACTER_OBJECTIVE_VALUES, UnlockPolicy, ValueOverride,
    compress_flag_slots, expanded_flag_slots, parse, parse_document_unlocks, parse_investment,
};
pub(super) use document::{
    CollectionStateSnapshot, collection_flag_state_text, collection_state_snapshot,
    collection_value_state_text, validate,
};

const RESPONSIVE_HASH_COLUMN_BREAKPOINT: f32 = 680.0;
const TABLE_ROW_GAP: f32 = 2.0;
const TABLE_ROW_STRIDE: f32 = TABLE_CELL_HEIGHT + TABLE_ROW_GAP;
const TABLE_ACTION_WIDTH: f32 = 24.0;
const CANONICAL_ROOTS: [&str; 5] = ["Items", "Triumphs", "Metrics", "Activities", "Presentation"];

pub(super) use hierarchy::progression_display_name;
pub(in crate::app) use mutations::remove_authored_collection_state;
pub(super) use mutations::{set_collection_flag, set_collection_value};
pub(super) use page::draw_content;
pub(super) use state::{UiState, View};
pub(super) use unlock_tables::{progression_target, saved_progression_lanes};

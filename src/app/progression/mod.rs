#[cfg(test)]
use super::inspector::{
    meaningful_definition_contexts, objective_details_tooltip, objective_traits_text,
    override_meaning,
};
use std::{
    cmp::Reverse,
    collections::{HashMap, HashSet},
};

use eframe::egui;
use serde_json::Value;

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
        definition_hash_hex_text, definition_identity, definition_metadata_tooltip,
        definition_name, draw_catalog_hash_window, draw_progression_metadata_workspace,
        flag_override_state_help, objective_description, objective_owner_type,
        objective_table_text, objective_target_text, override_filter_matches,
        progression_context_kind_label, progression_type_label, resolved_objective_table_text,
        take_definition_context as take_hash_inspection_context,
        take_definition_request as take_hash_inspection_request,
    },
    ui::{
        TABLE_CELL_HEIGHT, TABLE_COLUMN_GAP, destiny_text,
        hierarchy_branch_cell as draw_hierarchy_branch_cell,
        hierarchy_leaf_cell as draw_hierarchy_leaf_cell, hierarchy_selection_cell,
        sortable_header_cell, table_cell, toolbar as progression_toolbar,
    },
};

mod add_dialogs;
use crate::persistence::progression as document;
mod evaluation;
mod hierarchy;
pub(in crate::app) mod impact;
mod labels;
mod mutations;
mod native;
mod page;
mod rank_state;
mod rewards;
pub(in crate::app) mod seasonal;
mod sorting;
mod state;
mod storage;
mod table_ui;
mod triumphs;
mod unlocks;
mod workspace;

#[cfg(test)]
mod tests;

#[cfg(test)]
use document::{ACCOUNT_FLAG_BANK, ACCOUNT_OBJECTIVE_BANK, IndexedValue};
use document::{
    ACCOUNT_FLAG_CAPACITY, CHARACTER_OBJECT_FLAG_CAPACITY, FAMILY5_FLAG_SLOT_MAXIMUM,
    FAMILY5_FLAG_VALUE_MAXIMUM, FAMILY5_OVERRIDE_CAPACITY, FAMILY5_VALUE_SLOT_MAXIMUM,
    InvestmentPolicy, PROFILE_FLAG_CAPACITY, Progression, ProgressionValue,
    RESERVED_CHARACTER_OBJECTIVE_VALUES, UnlockPolicy, expanded_flag_slots, parse,
};
pub(super) use document::{
    CollectionStateSnapshot, collection_flag_state_text, collection_state_snapshot,
    collection_value_state_text, validate,
};

const TABLE_ROW_GAP: f32 = 2.0;
const CANONICAL_ROOTS: [&str; 5] = ["Items", "Triumphs", "Metrics", "Activities", "Presentation"];

pub(super) use hierarchy::progression_display_name;
pub(super) use mutations::{set_collection_flag, set_collection_value};
pub(super) use page::draw_content;
pub(super) use rank_state::{progression_target, saved_progression_lanes};
pub(super) use state::{UiState, View};

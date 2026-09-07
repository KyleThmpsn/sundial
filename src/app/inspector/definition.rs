//! Cross-page catalog definition inspector.
//!
//! The controller owns its navigation state and renders read-only catalog
//! relationships. Page-specific editors only open it through the inspector
//! facade; they do not own or depend on its implementation.

use eframe::egui;
use serde_json::Value;

use crate::{
    catalog::*,
    hash::{format_hash_hex, format_hash_hex_and_decimal, parse_hash_hex},
};

use super::super::{
    progression::{
        CollectionStateSnapshot, collection_state_snapshot, progression_display_name,
        progression_target, saved_progression_lanes, set_collection_flag, set_collection_value,
    },
    ui::{TABLE_CELL_HEIGHT, TABLE_COLUMN_GAP, table_cell},
};
use super::{
    catalog_hash_hex_and_decimal_field, condition_opcode_label, condition_token_resolution,
    draw_catalog_hash_link, draw_hash_hex_and_decimal_cells, draw_hash_hex_cell,
    draw_hash_wrapped_detail, draw_metadata_paths, draw_named_catalog_hash_link, hash_detail_field,
    hash_hex_and_decimal_field, hash_metadata_section, item_class_type_label,
    item_definition_name_cell, metadata_label_text, metadata_path_text, metadata_subsection,
    metadata_text, objective_description, objective_owner_display_label,
    objective_owner_kind_label, progression_context_kind_label, progression_scope_label,
    take_definition_request as take_hash_inspection_request, yes_no,
};

mod collections;
mod controller;
mod instance;
mod item;
mod item_details;
mod matches;
mod materials;
mod progression;
mod runtime;
mod state;
mod unlocks;

use collections::{
    collectible_item_name, draw_hash_collection_matches, draw_hash_condition_programs,
    draw_hash_package_paths,
};
use controller::HashInspectorAction;
pub(in crate::app) use controller::draw_catalog_hash_window;
use item::draw_hash_item_matches;
use matches::{CatalogHashMatches, CatalogMatchGroup};
use materials::{draw_hash_material_requirement_set, draw_hash_material_requirements};
use progression::draw_hash_progression_matches;
pub(in crate::app) use state::HashInspectionState;
use unlocks::draw_hash_unlock_matches;

const HASH_RELATIONSHIP_AUTO_EXPAND_LIMIT: usize = 8;
const HASH_INSPECTOR_WIDE_SUMMARY_WIDTH: f32 = 720.0;
const TABLE_ROW_GAP: f32 = 2.0;

fn hash_inspector_uses_wide_summary(available_width: f32) -> bool {
    available_width >= HASH_INSPECTOR_WIDE_SUMMARY_WIDTH
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InspectorProgressionEdit {
    Flag {
        definition_index: usize,
        set: bool,
    },
    Value {
        definition_index: usize,
        value: i32,
    },
    Collectible {
        collectible_index: u16,
        acquired: bool,
    },
}

fn hash_state_field(ui: &mut egui::Ui, value: impl Into<String>, tooltip: impl Into<String>) {
    ui.label(metadata_label_text(ui, "Current State"));
    ui.add(egui::Label::new(value.into()).wrap())
        .on_hover_text(tooltip.into());
    ui.end_row();
}

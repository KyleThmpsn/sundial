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
    progression::{progression_display_name, progression_target, saved_progression_lanes},
    ui::{TABLE_CELL_HEIGHT, TABLE_COLUMN_GAP, table_cell},
};
use super::{
    condition_opcode_label, condition_token_resolution, draw_hash_hex_and_decimal_cells,
    draw_hash_hex_cell, draw_hash_link, draw_hash_wrapped_detail, draw_metadata_paths,
    hash_detail_field, hash_hex_and_decimal_field, hash_metadata_section, item_class_type_label,
    item_definition_name_cell, metadata_path_text, metadata_subsection, metadata_text,
    objective_description, objective_owner_display_label, objective_owner_kind_label,
    progression_context_kind_label, progression_scope_label,
    take_definition_request as take_hash_inspection_request, unresolved_name_cell, yes_no,
};

mod collections;
mod controller;
mod item;
mod matches;
mod materials;
mod progression;
mod sandbox_perk;
mod state;
mod unlocks;

use collections::{
    draw_hash_collection_matches, draw_hash_condition_programs, draw_hash_package_paths,
};
pub(in crate::app) use controller::draw_catalog_hash_window;
use item::draw_hash_item_matches;
use matches::CatalogHashMatches;
use materials::{draw_hash_material_requirement_set, draw_hash_material_requirements};
use progression::draw_hash_progression_matches;
use sandbox_perk::{draw_hash_sandbox_perk_definition, hash_inspector_uses_wide_summary};
pub(in crate::app) use state::HashInspectionState;
use unlocks::draw_hash_unlock_matches;

const HASH_RELATIONSHIP_AUTO_EXPAND_LIMIT: usize = 8;
const SANDBOX_PERK_NAME_UNRESOLVED_HELP: &str = "No validated package bridge connects this historical sandbox-perk hash to a localized display name. Same-value item hashes are not treated as proof of identity.";
const TABLE_ROW_GAP: f32 = 2.0;

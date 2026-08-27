//! Shared inspector infrastructure and cross-page definition inspection.
//!
//! This facade stays intentionally small. Focused modules own layout,
//! definition requests, and semantic name resolution; domain inspectors live
//! beside the data they present.

mod definition;
mod hash_names;
mod layout;
mod metadata;
mod progression;
mod requests;

pub(super) use definition::{HashInspectionState, draw_catalog_hash_window};
pub(super) use hash_names::item_definition_name_cell;
pub(super) use layout::{heading, uses_side_workspace, workspace};
pub(super) use metadata::{
    catalog_hash_hex_and_decimal_field, draw_catalog_hash_link, draw_hash_hex_and_decimal_cells,
    draw_hash_hex_cell, draw_hash_link, draw_hash_wrapped_detail, draw_metadata_paths,
    draw_named_catalog_hash_link, hash_detail_field, hash_hex_and_decimal_field,
    hash_metadata_section, item_class_type_label, metadata_field, metadata_label_text,
    metadata_path_text, metadata_section, metadata_subsection, metadata_text,
    objective_owner_kind_label, progression_context_kind_label, progression_scope_label, yes_no,
};
pub(super) use progression::{
    MetadataSelection, OverrideFilter, ProgressionInspectorState, condition_opcode_label,
    condition_token_resolution, definition_has_undecoded_opcodes, definition_hash_hex_text,
    definition_identity, definition_metadata_tooltip, definition_name,
    draw_progression_metadata_workspace, flag_override_state_help, flag_override_state_label,
    meaningful_definition_contexts, objective_description, objective_details_tooltip,
    objective_owner_display_label, objective_owner_type, objective_table_text,
    objective_target_text, objective_traits_text, override_filter_matches, override_meaning,
    override_meaning_contexts, progression_type_label, resolved_objective_table_text,
};
#[cfg(test)]
pub(super) use progression::{
    objective_goal_text, objective_target_tooltip, objective_traits_tooltip,
};
pub(super) use requests::{
    DefinitionInspectionContext, request_definition, request_definition_with_context,
    take_definition_context, take_definition_request,
};

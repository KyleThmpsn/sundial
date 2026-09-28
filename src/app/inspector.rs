//! Shared inspector infrastructure and cross-page definition inspection.

mod definition;
mod hash_names;
mod layout;
pub(super) mod look;
mod metadata;
mod progression;
mod requests;

pub(super) use definition::{HashInspectionState, draw_catalog_hash_window};
pub(super) use hash_names::item_definition_name_cell;
pub(super) use layout::{heading, uses_side_workspace, workspace};
pub(super) use metadata::{
    catalog_hash_hex_and_decimal_field, copy_menu_buttons, definition_context_menu,
    draw_catalog_hash_link, draw_metadata_paths, draw_named_catalog_hash_link,
    hash_metadata_section, inspect_menu_button, item_class_type_label, metadata_field,
    metadata_label_text, metadata_section, metadata_subsection, metadata_text,
    objective_owner_kind_label, progression_context_kind_label, progression_scope_label, yes_no,
};
pub(super) use progression::{
    MetadataSelection, OverrideFilter, ProgressionInspectorState, condition_opcode_label,
    condition_token_resolution, definition_hash_hex_text, definition_identity,
    definition_metadata_tooltip, definition_name, draw_progression_metadata_workspace,
    flag_override_state_help, objective_description, objective_owner_type, objective_table_text,
    objective_target_text, override_filter_matches, progression_type_label,
    resolved_objective_table_text,
};
pub(super) use requests::{
    DefinitionInspectionContext, clear_owned_quantities, publish_owned_quantities,
    request_definition, request_definition_with_context, request_progression_selection,
    take_definition_context, take_definition_request, take_owned_quantities_request,
    take_progression_selection,
};

/// Shown for a definition whose name does not resolve.
const UNNAMED: &str = "Unnamed";

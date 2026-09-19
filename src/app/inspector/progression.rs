//! Progression metadata inspection and semantic presentation.
//!
//! The progression page owns editing and table state. This feature owns the
//! read-only drill-in experience, including its navigation history and the
//! condition/objective language shared by definition inspection.

mod conditions;
mod definition_details;
mod definitions;
mod objective_details;
mod objectives;
mod overrides;
mod panel;
mod state;

pub(in crate::app) use conditions::{condition_opcode_label, condition_token_resolution};
pub(in crate::app) use definitions::{
    definition_hash_hex_text, definition_identity, definition_metadata_tooltip, definition_name,
    flag_override_state_help,
};
#[cfg(test)]
pub(in crate::app) use objectives::objective_goal_text;
pub(in crate::app) use objectives::{
    objective_description, objective_owner_display_label, objective_owner_type,
    objective_table_text, objective_target_text, progression_type_label,
    resolved_objective_table_text,
};
pub(in crate::app) use overrides::{OverrideFilter, override_filter_matches};
pub(in crate::app) use panel::draw_progression_metadata_workspace;
pub(in crate::app) use state::{MetadataSelection, ProgressionInspectorState};

#[cfg(test)]
pub(in crate::app) use objectives::{
    meaningful_definition_contexts, objective_details_tooltip, objective_traits_text,
    override_meaning,
};

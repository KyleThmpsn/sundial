//! Custom-perk authoring owns recipe mutations, window navigation and parameter drafts.
//! Native recipe types remain unchanged; only this feature's UI/state lives here.
use super::*;
use sundial::package_authoring::sandbox_perk::entity::{self, Selection as ProjectileSelection};

mod mutations;
mod picked;
pub(super) mod workbench;
pub(super) use picked::{
    attach_picked_perk, choice_conflicts, repair_socket_picks, resolve_picked_perk,
};

// Test-only compatibility helpers are also used by the workbench integration tests.
#[cfg(test)]
pub(super) use mutations::remove_private_perk_runtime_values;

pub(super) use mutations::reconcile_socket_plug_variants;
#[cfg(test)]
pub(super) use mutations::upsert_private_perk_runtime_values;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(super) struct PerkEditorKey {
    pub(super) socket_index: u16,
    pub(super) choice_index: u16,
    pub(super) source_plug_hash: u32,
    pub(super) source_perk_index: u16,
}

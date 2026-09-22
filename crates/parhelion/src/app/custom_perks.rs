//! Custom-perk authoring owns recipe mutations, window navigation and parameter drafts.
//! Native recipe types remain unchanged; only this feature's UI/state lives here.
use super::*;
use sundial::package_authoring::sandbox_perk::projectile::{
    self, Selection as ProjectileSelection,
};

mod editor;
mod mutations;
mod picked;
pub(super) mod workbench;
pub(super) use picked::{
    attach_picked_perk, choice_conflicts, repair_socket_picks, resolve_picked_perk,
};

// Test-only compatibility helpers are also used by the workbench integration tests.
#[cfg(test)]
pub(super) use mutations::{private_perk_runtime_values, remove_private_perk_runtime_values};

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

#[derive(Clone, Debug)]
struct PrivatePerkRuntimeGraph {
    action_tag: u32,
    action_payload: Vec<u8>,
    /// Readable account of the native action, when its payload decodes.
    summary: Option<sundial::package_authoring::sandbox_perk::action::ActionSummary>,
    /// The editable program recovered from the action, or why none can be recovered.
    program: Option<Result<sundial::package_authoring::sandbox_perk::program::Program, String>>,
    graphs: Vec<(u32, WeaponRuntimeGraph)>,
    warnings: Vec<String>,
    /// Failures that prevent validating runtime edits, separate from reference-index notices.
    graph_errors: Vec<String>,
    /// Assets the loading index cannot serve, checked the moment they are chosen so the
    /// build never has to be the one that says no.
    loading_issues: Vec<String>,
    projectile_slots: Vec<(u32, u32)>,
    projectile_catalog: Arc<projectile::catalog::Catalog>,
    native_assets: Vec<sundial::package_authoring::tft::Reference>,
}

enum PrivatePerkGraphEvent {
    Finished(Result<PrivatePerkRuntimeGraph, String>),
    Preview(
        editor::conversion::Input,
        Result<editor::conversion::Preview, String>,
    ),
}

pub(super) struct PerkEditor {
    history: workbench::history::History<editor::history::Snapshot>,
    activation: Option<sundial::package_authoring::sandbox_perk::activation::PerkActivation>,
    preview: Option<(
        editor::conversion::Input,
        Result<editor::conversion::Preview, String>,
    )>,
    entity_source: Option<u32>,
    key: PerkEditorKey,
    plug_label: String,
    packages: PathBuf,
    draft: Vec<WeaponRuntimeValueOverride>,
    action_draft: Vec<crate::WeaponSandboxPerkActionFloatRecipe>,
    projectile_draft: Vec<ProjectileSelection>,
    original_projectile_draft: Vec<ProjectileSelection>,
    projectile_labels: BTreeMap<u16, String>,
    /// Weapon names by item hash, for assets named after the pattern that fires them.
    item_names: BTreeMap<u32, projectile::catalog::ItemName>,
    projectile_query: String,
    pending_movement: Option<(u32, Vec<(projectile::parameters::Kind, u32)>)>,
    original_draft: Vec<WeaponRuntimeValueOverride>,
    original_action_draft: Vec<crate::WeaponSandboxPerkActionFloatRecipe>,
    parameter_error: Option<String>,
    graph: Option<Arc<PrivatePerkRuntimeGraph>>,
    error: Option<String>,
    receiver: Option<Receiver<PrivatePerkGraphEvent>>,
    worker: Option<thread::JoinHandle<()>>,
    query: String,
    value_text: BTreeMap<(WeaponRuntimeFieldLocator, u8), String>,
    show_all_native_values: bool,
    /// A conversion the user requested. The workbench applies it and closes the editor.
    conversion: Option<sundial::package_authoring::sandbox_perk::program::Program>,
}

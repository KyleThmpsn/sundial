//! Custom-perk authoring owns recipe mutations, window navigation and parameter drafts.
//! Native recipe types remain unchanged; only this feature's UI/state lives here.
use super::*;

mod editor;
mod mutations;
mod reuse;
mod window;
pub(super) use reuse::ReusePicker;

// Release gate for editor entry points only; recipe parsing and compilation remain supported.
fn authoring_available() -> bool {
    false
}

// Test-only compatibility helpers are also used by the workbench integration tests.
#[cfg(test)]
pub(super) use mutations::{private_perk_runtime_values, remove_private_perk_runtime_values};

use mutations::private_perk;
pub(super) use mutations::{reconcile_socket_plug_variants, upsert_private_perk_runtime_values};

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
    graphs: Vec<(u32, WeaponRuntimeGraph)>,
    warnings: Vec<String>,
}

enum PrivatePerkGraphEvent {
    Finished(Result<PrivatePerkRuntimeGraph, String>),
}

pub(super) struct PerkEditor {
    key: PerkEditorKey,
    plug_label: String,
    packages: PathBuf,
    draft: Vec<WeaponRuntimeValueOverride>,
    action_draft: Vec<crate::WeaponSandboxPerkActionFloatRecipe>,
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
}

enum PerkEditorAction {
    Apply {
        key: PerkEditorKey,
        values: Vec<WeaponRuntimeValueOverride>,
        action_values: Vec<crate::WeaponSandboxPerkActionFloatRecipe>,
    },
    Cancel,
}

impl PackageAuthoringApp {
    pub(super) fn draw_perk_editor(&mut self, ctx: &egui::Context) {
        if !authoring_available() {
            return;
        }
        let action = self
            .perk_editor
            .as_mut()
            .and_then(|editor| editor.show(ctx, self.show_experimental_options));
        match action {
            Some(PerkEditorAction::Apply {
                key,
                values,
                action_values,
            }) => {
                upsert_private_perk_runtime_values(&mut self.recipe, key, values)
                    .action_float_values = action_values;
                self.perk_editor = None;
            }
            Some(PerkEditorAction::Cancel) => self.perk_editor = None,
            None => {}
        }
    }
}

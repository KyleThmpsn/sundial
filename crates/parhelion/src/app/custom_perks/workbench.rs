//! Standalone perk authoring. Weapon attachment is an explicit copy operation.
use super::*;
use crate::perk::{
    PerkRecipe,
    library::{DraftsWrite, Entry, Library},
};
use serde::{Deserialize, Serialize};
use std::time::SystemTime;

pub(super) mod assets;
mod attachment;
mod behaviors;
pub(super) mod canvas;
mod cards;
mod catalog_insert;
mod controls;
mod diagnostics;
mod discovery;
mod duplicate;
mod engine;
mod forms;
mod guidance;
pub(in crate::app::custom_perks) mod history;
use crate::artwork_browser as icons;
mod library;
mod markers;
mod parameters;
pub(super) use crate::app::pickers;
pub(in crate::app) use program::native::remember_abilities;
mod program;
mod properties;
mod reading;
mod referrers;
mod selection;
mod stats;
mod stock;
mod templates;
mod test_plan;
#[cfg(test)]
pub(crate) mod tests;
mod validation;
mod verification_ui;
use parameters::{PerkEditor, PrivatePerkRuntimeGraph};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app) enum Request {
    EditChoice {
        socket: usize,
        choice: usize,
    },
    SelectChoice {
        socket: usize,
        choice: usize,
    },
    /// A custom perk of a subclass ability or node.
    Ability {
        place: crate::subclass::Place,
        perk: AbilityPerk,
    },
}

/// Which custom perk of a subclass ability or node the workbench opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app) enum AbilityPerk {
    New,
    /// One of its custom perks, by its place among them.
    Custom(usize),
    /// A copy of one of its stock perks, which takes that perk's place once applied.
    Stock(u16),
}

impl Request {
    pub(in crate::app) fn socket(self) -> Option<usize> {
        match self {
            Self::EditChoice { socket, .. } | Self::SelectChoice { socket, .. } => Some(socket),
            Self::Ability { .. } => None,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct Document {
    #[serde(skip)]
    history: history::History,
    recipe: PerkRecipe,
    baseline: Option<Vec<u8>>,
    /// `baseline` parsed. Assign `baseline` through `set_baseline` so the two agree.
    #[serde(skip)]
    saved: Option<PerkRecipe>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    origin: Option<PerkRecipe>,
    #[serde(skip)]
    target: Option<attachment::Target>,
    /// Opened from a weapon socket as a copy of the perk there.
    #[serde(skip)]
    from_socket: bool,
    /// The subclass ability or node it goes on.
    #[serde(skip)]
    ability: Option<attachment::AbilityTarget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pending_effect: Option<EffectDraft>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    modified: Option<SystemTime>,
    /// The last problem and warning check. The checks compile every effect, so they run again
    /// only when the recipe or the discovery they read changes, not on every frame.
    #[serde(skip)]
    issues: Option<IssueCache>,
}

/// A document's checked recipe with what the check found.
#[derive(Clone)]
struct IssueCache {
    recipe: PerkRecipe,
    /// Whether discovery was idle at the check. Its results feed the problem check.
    ready: bool,
    problem: Option<String>,
    warning: Option<String>,
}

impl Document {
    fn new(recipe: PerkRecipe, baseline: Option<Vec<u8>>) -> Self {
        Self {
            history: history::History::default(),
            issues: None,
            modified: baseline.is_none().then(SystemTime::now),
            origin: Some(recipe.clone()),
            recipe,
            saved: parse_baseline(baseline.as_deref()),
            baseline,
            target: None,
            from_socket: false,
            ability: None,
            pending_effect: None,
        }
    }

    fn set_baseline(&mut self, baseline: Option<Vec<u8>>) {
        self.baseline = baseline;
        self.read_baseline();
    }

    /// Parses `baseline` again, for a document read from the drafts file.
    fn read_baseline(&mut self) {
        self.saved = parse_baseline(self.baseline.as_deref());
    }

    fn restored(&self) -> PerkRecipe {
        self.saved
            .clone()
            .or_else(|| self.origin.clone())
            .unwrap_or_else(|| self.recipe.clone())
    }

    /// Whether the recipe differs from what `restored` returns.
    fn changed(&self) -> bool {
        self.saved
            .as_ref()
            .or(self.origin.as_ref())
            .is_some_and(|restored| *restored != self.recipe)
    }

    /// Whether the recipe matches its saved library copy.
    fn unchanged(&self) -> bool {
        self.saved
            .as_ref()
            .is_some_and(|saved| *saved == self.recipe)
    }

    /// A socket's perk opened as a copy and not changed or saved since. The library list, the
    /// Select Custom Perk choices and the drafts file leave it out.
    fn untouched_copy(&self) -> bool {
        self.from_socket
            && self.baseline.is_none()
            && self.pending_effect.is_none()
            && !self.changed()
    }
}

fn parse_baseline(bytes: Option<&[u8]>) -> Option<PerkRecipe> {
    bytes.and_then(|bytes| serde_json::from_slice(bytes).ok())
}

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
struct EffectDraft {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    action: Option<usize>,
    index: u16,
    values: Vec<WeaponRuntimeValueOverride>,
    action_values: Vec<crate::WeaponSandboxPerkActionFloatRecipe>,
    projectiles: Vec<ProjectileSelection>,
    field_text: Vec<((WeaponRuntimeFieldLocator, u8), String)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pending_movement: Option<(u32, Vec<(entity::projectile::parameters::Kind, u32)>)>,
}

/// Whether the Details row of the open perk is unfolded. The description, icon and category
/// matter most for a new perk, so a new one opens on Basics and a copied one on Effects.
#[derive(Clone, Copy, Default, PartialEq)]
enum Page {
    Basics,
    #[default]
    Effects,
}

#[derive(Default)]
pub(in crate::app) struct Workbench {
    pub open: bool,
    /// The installed runtime, named by warnings about what it reads.
    pub(in crate::app) branding: crate::branding::Branding,
    initialized: bool,
    draft_baseline: Option<Vec<u8>>,
    drafts_writable: bool,
    /// Why the last draft write failed. Autosave tries again on the next edit.
    drafts_error: Option<String>,
    /// This window's own drafts file, named after it, once another window wrote the shared
    /// drafts file since this one last did. Autosave writes there for the rest of the session.
    drafts_set_aside: Option<String>,
    library: Option<Library>,
    entries: Vec<Entry>,
    /// Each saved perk's problem, by path, with the modified time and size it was checked at.
    /// A library holds hundreds of perks, too many to check on every frame.
    library_issues: BTreeMap<PathBuf, (Option<SystemTime>, usize, Option<String>)>,
    /// Whether discovery was idle when `library_issues` was filled. Its results feed the check.
    library_issues_ready: bool,
    documents: Vec<Document>,
    selected: usize,
    /// The destination last chosen. A selected document without one takes it.
    destination: Option<attachment::Target>,
    page: Page,
    query: String,
    library_order: library::Order,
    template_query: String,
    effect_query: String,
    effect_purpose: guidance::Purpose,
    effect_editing: guidance::EditingFilter,
    effect_order: guidance::EffectOrder,
    stat_query: String,
    icon_query: String,
    icons: icons::Picker,
    asset_query: String,
    keys: program::Keys,
    behaviors: behaviors::Picker,
    /// Stock perk names by finished perk index, for labelling assets the perks reference.
    perk_names: BTreeMap<u16, String>,
    ingredients: Option<(usize, usize, Arc<sundial::investment::IngredientCatalog>)>,
    /// Stock effect names for Add from Perk, read from one ingredient catalog.
    effect_names: Option<forms::EffectNames>,
    ingredient_source: Option<sundial::investment::IngredientSource>,
    /// Weapon names by item hash, for assets named after the pattern that fires them.
    item_names: BTreeMap<u32, entity::catalog::ItemName>,
    /// Names are cached by native catalog identity and invalidated when source labels change.
    asset_label_source: Option<Arc<entity::catalog::Catalog>>,
    /// The game-name registration the labels were computed with.
    asset_label_generation: u64,
    asset_labels: BTreeMap<u32, String>,
    properties: properties::Properties,
    editor: Option<PerkEditor>,
    retired_editors: Vec<PerkEditor>,
    templates: Option<Vec<WeaponSandboxPerkChoice>>,
    /// The stock New from Perk rows, built once per catalog from `templates`.
    template_rows: Option<Vec<templates::Row>>,
    authored_templates: Option<Vec<templates::AuthoredTemplate>>,
    editing_effect: Option<u16>,
    editing_program_action: Option<usize>,
    discovery: discovery::Discovery,
    engine: engine::EngineCatalog,
    diagnostics_open: bool,
    diagnostic_cache: Option<diagnostics::Cache>,
    consolidation: (usize, usize),
    verification: verification_ui::Window,
    message: Option<String>,
    message_path: Option<PathBuf>,
    error: Option<String>,
    picker: Option<selection::Picker>,
    /// A perk the reader asked to delete from Custom Perks, awaiting confirmation.
    pending_delete: Option<library::PendingDelete>,
    /// Bundled custom perks captured before the reader confirms a guarded restore.
    pending_restore_defaults: Option<crate::perk::library::RestoreDefaults>,
    reveal_document: bool,
    header_action: Option<HeaderAction>,
    duplicating: Option<duplicate::Pending>,
    /// Stock effects with overrides, converted so their cards edit in place.
    stock_programs: Vec<stock::Prepared>,
    reveal_problem: Option<validation::Location>,
    reveal_action: Option<usize>,
    /// The open recipe's kind, set each frame, for text that names the item a perk goes on.
    item_kind: crate::ItemKind,
    /// Whether the open weapon recipe has unsaved changes, set by the host each frame.
    pub(in crate::app) recipe_unsaved: bool,
    /// The footer asked the host to save the weapon recipe.
    pub(in crate::app) save_recipe_requested: bool,
    /// A subclass recipe's abilities and path nodes with their names, set by the host each
    /// frame, which its perks can go on.
    pub(in crate::app) ability_places: Vec<(crate::subclass::Place, String)>,
}

enum HeaderAction {
    History(bool),
    Save(bool),
    Discard,
    Delete,
}

#[cfg(test)]
impl Workbench {
    /// A workbench that never opens the reader's Custom Perks library or writes drafts.
    pub(in crate::app) fn offline() -> Self {
        Self {
            initialized: true,
            ..Self::default()
        }
    }

    /// The open perk, for a test that edits it the way the Basics and Stats forms do.
    pub(in crate::app) fn open_perk_mut(&mut self) -> Option<&mut PerkRecipe> {
        self.documents
            .get_mut(self.selected)
            .map(|document| &mut document.recipe)
    }

    /// Whether the background perk discovery that effect checks read has finished.
    pub(in crate::app) fn discovery_settled(&self) -> bool {
        !self.discovery.busy() && (self.discovery.data.is_some() || self.discovery.error.is_some())
    }

    /// Removes the open perk's effects the workbench flags, as a reader does before applying.
    pub(in crate::app) fn remove_flagged_effects(&mut self) {
        let discovery = &self.discovery;
        if let Some(document) = self.documents.get_mut(self.selected) {
            document
                .recipe
                .effects
                .retain(|effect| discovery.perk_issue(effect.source_perk_index).is_none());
        }
    }
}

impl Workbench {
    pub(in crate::app) fn saved_weapons_changed(&mut self) {
        self.authored_templates = None;
    }

    /// The perk's first problem, one that keeps it from being applied or chosen. Warnings, which
    /// never block, come from `validation::perk_warning`.
    fn perk_issue(&self, recipe: &PerkRecipe) -> Option<String> {
        recipe
            .validate()
            .err()
            .or_else(|| {
                recipe.effects.iter().find_map(|effect| {
                    self.discovery
                        .perk_issue(effect.source_perk_index)
                        .map(str::to_owned)
                })
            })
            .or_else(|| {
                recipe
                    .effects
                    .iter()
                    .find_map(|effect| validation::counter_issue(effect.program.as_ref()))
            })
            .or_else(|| {
                recipe
                    .effects
                    .iter()
                    .find_map(|effect| validation::choice_issue(effect.program.as_ref()))
            })
    }

    pub(in crate::app) fn open_engine_catalog(&mut self) {
        self.engine.open = true;
    }

    pub(in crate::app) fn busy(&self) -> bool {
        self.discovery.busy()
            || self.engine.export.busy()
            || self.verification.busy()
            || self.duplicating.is_some()
            || self.preparing_stock_effect()
            || self.properties.busy()
            || self.icons.busy()
            || self
                .editor
                .as_ref()
                .is_some_and(PerkEditor::has_background_work)
            || self
                .retired_editors
                .iter()
                .any(PerkEditor::has_background_work)
    }

    /// Stops reads that only feed a view, so work that needs the packages to itself can go
    /// ahead. The Markers view can read again after package work finishes.
    pub(in crate::app) fn stop_optional_reads(&mut self) {
        self.discovery.stop();
        self.engine.markers.stop();
        self.engine.referrers.stop();
    }

    pub(in crate::app) fn editing(&self) -> bool {
        self.open && self.editor.is_some()
    }

    /// Parameter editors belong to one document, including while their window is closed.
    fn select_document(&mut self, index: usize) {
        if self.selected == index {
            return;
        }
        self.capture_effect_draft();
        self.retire_editor();
        self.editing_effect = None;
        self.editing_program_action = None;
        self.selected = index;
        self.reveal_document = true;
        // An image still being imported belongs to the perk it was asked for.
        self.properties.discard_hud_import();
        // A result or an error belongs to the perk it came from.
        self.message = None;
        self.message_path = None;
        self.error = None;
    }

    fn discard_changes(&mut self) {
        self.retire_editor();
        self.editing_effect = None;
        self.editing_program_action = None;
        if let Some(document) = self.documents.get_mut(self.selected) {
            let restored = document.restored();
            if restored != document.recipe {
                document.history.record_step(document.recipe.clone());
            }
            document.recipe = restored;
            document.pending_effect = None;
            document.modified = None;
        }
        self.error = None;
        self.message = Some("Changes discarded.".into());
        self.message_path = None;
        self.persist_drafts();
    }

    pub(in crate::app) fn invalidate(&mut self) {
        self.capture_effect_draft();
        self.discovery.invalidate();
        // The marker index describes the packages that were open, so a reload drops it.
        self.engine.markers.stop();
        self.engine.markers.invalidate();
        self.engine.referrers.stop();
        self.engine.referrers.invalidate();
        self.icons.invalidate();
        self.behaviors = behaviors::Picker::default();
        self.keys = program::Keys::default();
        self.ingredients = None;
        self.effect_names = None;
        self.clear_stock_programs();
        self.retire_editor();
        self.templates = None;
        self.template_rows = None;
        self.authored_templates = None;
        self.editing_effect = None;
        self.editing_program_action = None;
    }

    fn refresh_editor_context(&mut self) {
        let context_name = self.editor.as_ref().and_then(|editor| {
            let tag = editor.entity_source?;
            let program = self
                .documents
                .get(self.selected)?
                .recipe
                .effects
                .iter()
                .find(|effect| Some(effect.source_perk_index) == self.editing_effect)?
                .program
                .as_ref()?;
            self.program_asset_labels(program.native.as_ref()?, &program.name)
                .remove(&tag)
        });
        if let Some(editor) = &mut self.editor {
            if editor.item_names != self.item_names {
                editor.item_names = self.item_names.clone();
            }
            if editor.projectile_labels != self.perk_names {
                editor.projectile_labels = self.perk_names.clone();
            }
            if let Some(tag) = editor.entity_source {
                if let Some(graph) = &editor.graph {
                    self.properties.remember(tag, graph.clone());
                }
                if let Some(name) = context_name {
                    editor.plug_label = name;
                } else if let Some(entry) =
                    self.discovery.data.as_ref().and_then(|data| {
                        data.effects.entries.iter().find(|entry| entry.graph == tag)
                    })
                {
                    editor.plug_label = entry.discovery_label_with(
                        |index| self.perk_names.get(&index).cloned(),
                        |item| self.item_names.get(&item).cloned(),
                    );
                }
            }
        }
    }

    fn retry_discovery(&mut self, packages: &Path, ctx: &egui::Context) {
        if std::mem::take(&mut self.engine.retry_requested) {
            self.discovery.invalidate();
            self.discovery.start(packages, ctx);
        }
    }

    /// Advances every background job the workbench owns before a frame is drawn.
    fn poll_background_work(
        &mut self,
        ctx: &egui::Context,
        packages: &Path,
        choices: &[WeaponSandboxPerkChoice],
    ) {
        self.poll_duplicate(ctx, choices);
        self.engine.export.poll();
        self.verification.poll();
        self.poll_stock_programs();
        if let Some(editor) = &mut self.editor {
            editor.poll();
        }
        for editor in &mut self.retired_editors {
            editor.poll();
        }
        self.retired_editors.retain(PerkEditor::has_background_work);
        if (self.open || self.engine.open) && packages.is_dir() {
            self.discovery.start(packages, ctx);
        }
        self.discovery.poll();
        // Native discovery feeds the other catalog views. Let it finish before this optional
        // whole-game read starts competing for the same package readers.
        if self.engine.wants_markers() && packages.is_dir() && !self.discovery.busy() {
            self.engine.markers.start(packages, ctx);
        } else {
            // Only the Markers view shows this read, so it stops when that view is left
            // rather than holding every other job that waits on the packages.
            self.engine.markers.cancel();
        }
        self.engine.markers.poll();
        // Every use of a resource is read only when a resource page asks, and the read runs
        // to the end wherever the reader goes next, since the packages it finished are kept.
        if self.engine.referrers.wanted() && packages.is_dir() && !self.discovery.busy() {
            self.engine.referrers.start(packages, ctx);
        }
        self.engine.referrers.poll();
        // An effect opened before discovery finished shows the index notice until the
        // index exists; once discovery has data, the index does.
        if self.discovery.data.is_some()
            && let Some(editor) = &mut self.editor
        {
            editor.refresh_asset_index();
        }
        self.icons.poll();
    }

    fn show(
        &mut self,
        ctx: &egui::Context,
        packages: &Path,
        catalog: Option<&InvestmentCatalog>,
        choices: &[WeaponSandboxPerkChoice],
        experimental: bool,
        attachment: (&WeaponRecipe, Option<&WeaponDonor>),
    ) -> Option<attachment::Applied> {
        let (weapon, donor) = attachment;
        self.item_kind = weapon.kind;
        self.poll_background_work(ctx, packages, choices);
        program::native::label_choices(
            ctx,
            self.discovery.labels.clone(),
            self.discovery.label_error.clone(),
        );
        program::native::script_choices(
            ctx,
            self.discovery
                .data
                .as_ref()
                .map(|data| data.scripts.clone()),
        );
        let ingredients = catalog.map(|catalog| {
            let abilities = self
                .discovery
                .data
                .as_ref()
                .map_or(&[][..], |data| data.abilities.as_slice());
            if self.ingredients.as_ref().is_none_or(|(count, sources, _)| {
                *count != choices.len() || *sources != abilities.len()
            }) {
                self.ingredients = Some((
                    choices.len(),
                    abilities.len(),
                    Arc::new(catalog.perk_ingredients(choices, abilities)),
                ));
            }
            Arc::clone(&self.ingredients.as_ref().expect("ingredient catalog").2)
        });
        self.behaviors.count_carried(ingredients.clone());
        self.keys
            .sync(self.discovery.keys.as_ref(), ingredients.as_ref());
        let choices = ingredients
            .as_ref()
            .map_or(choices, |data| data.choices.as_slice());
        if self.perk_names.len() != choices.len() {
            self.perk_names = choices
                .iter()
                .map(|choice| (choice.perk_index, choice.representative_name.clone()))
                .collect();
            self.asset_label_source = None;
        }
        // Discovery can finish while the native editor is open. Its names must not
        // depend on returning to the effect list or on when the editor was created.
        self.refresh_item_names(catalog);
        self.properties.sync(
            packages,
            self.discovery.data.as_ref().map(|data| &data.effects),
        );
        self.refresh_editor_context();
        if self.busy() || self.engine.markers.busy() || self.engine.referrers.busy() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        if self.engine.open {
            self.refresh_asset_labels();
        }
        let empty_sources = sundial::investment::PerkSources::default();
        // Taken before the call, because `show` borrows the catalog mutably while the object
        // page needs the same index the Markers view read.
        let marker_index = self.engine.markers.index();
        self.engine.insertion.destination = self
            .documents
            .get(self.selected)
            .filter(|document| self.editor.is_none() && document.pending_effect.is_none())
            .map(|document| document.recipe.clone());
        self.engine.show(
            ctx,
            choices,
            ingredients
                .as_ref()
                .map_or(&empty_sources, |data| &data.references),
            assets::Browser {
                catalog,
                discovery: &self.discovery,
                perk_names: &self.perk_names,
                item_names: &self.item_names,
                asset_labels: &self.asset_labels,
                markers: marker_index.as_deref(),
                carried: ingredients.as_ref().map(|data| &data.sources),
            },
            experimental,
        );
        if self.engine.stop_requested {
            self.engine.stop_requested = false;
            self.discovery.stop();
        }
        self.retry_discovery(packages, ctx);
        if let Some(request) = self.engine.insertion.requested.take()
            && let Some(document) = self.documents.get_mut(self.selected)
        {
            let before = document.recipe.clone();
            match request.apply(&mut document.recipe) {
                Ok(()) => {
                    document.history.record_step(before);
                    document.modified = Some(SystemTime::now());
                    self.message =
                        Some("Inserted the native configuration into the current perk.".into());
                    self.open = true;
                    self.page = Page::Effects;
                    self.persist_drafts();
                }
                Err(error) => self.error = Some(error),
            }
        }
        if let Some(index) = self.engine.copy_requested.take()
            && let Some(choice) = choices.iter().find(|choice| choice.perk_index == index)
        {
            self.initialize();
            self.copy_behavior(choice);
        }
        if !self.open {
            self.show_diagnostics(ctx);
            if let Some(document) = self.documents.get(self.selected) {
                self.verification.show(
                    ctx,
                    &document.recipe,
                    self.item_kind,
                    self.library.as_ref().map(Library::root),
                );
            }
            return None;
        }
        self.initialize();
        self.restore_effect_draft(ctx, packages, choices);
        let mut open = self.open;
        let mut attachment = None;
        let window = egui::Window::new("Custom Perk Workbench")
            .id(egui::Id::new("global-custom-perk-workbench"))
            .open(&mut open)
            .collapsible(false)
            // Open into the room that is actually there. A fixed 720 high window left most
            // of a tall screen empty and cut the editor off mid effect, and the size is
            // remembered once a reader drags it, so this only sets where they start.
            .default_size(egui::vec2(
                (ctx.screen_rect().width() - 80.0).clamp(700.0, 1600.0),
                (ctx.screen_rect().height() - 120.0).clamp(480.0, 1200.0),
            ))
            .min_width(700.0_f32.min((ctx.screen_rect().width() - 40.0).max(320.0)))
            .max_width((ctx.screen_rect().width() - 40.0).max(320.0))
            .max_height((ctx.screen_rect().height() - 64.0).max(360.0))
            .show(ctx, |ui| {
                crate::app::style::perk_workbench_style(ui);
                // Respect the requested window size and reserve the destination footer.
                // The body follows the window, so dragging the window taller shows more of
                // the editor instead of stopping at a fixed height on a tall screen.
                let tallest = (ctx.screen_rect().height() - 140.0).max(240.0);
                let footer_id = ui.id().with("attachment-height");
                let footer_height = ctx
                    .data(|data| data.get_temp::<f32>(footer_id))
                    .unwrap_or(40.0);
                let body_height = (ui.available_height() - footer_height).clamp(240.0, tallest);
                let width = ui.available_width();
                let library_width = (width * 0.26).clamp(220.0, 290.0).min(width * 0.40);
                let editor_width =
                    (width - library_width - ui.spacing().item_spacing.x * 3.0 - 2.0).max(120.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), body_height),
                    egui::Layout::left_to_right(egui::Align::Min),
                    |ui| {
                        ui.allocate_ui_with_layout(
                            egui::vec2(library_width, body_height),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.set_width(library_width);
                                self.draw_library(ui, catalog, donor.map(|donor| (weapon, donor)));
                            },
                        );
                        ui.separator();
                        ui.allocate_ui_with_layout(
                            egui::vec2(editor_width, body_height),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.set_width(editor_width);
                                let Some(document) = self.documents.get(self.selected) else {
                                    return;
                                };
                                let mut recipe = document.recipe.clone();
                                let before = recipe.clone();
                                let document_id = recipe.id.clone();
                                let header_height =
                                    self.draw_document_header(ui, &mut recipe, catalog);
                                if self.editor.is_some() {
                                    self.draw_effect_editor(
                                        ui,
                                        ctx,
                                        &mut recipe,
                                        experimental,
                                        body_height - header_height,
                                    );
                                } else {
                                    egui::ScrollArea::vertical()
                                        .id_salt(("independent-perk-body", &document_id))
                                        .max_height((body_height - header_height - 8.0).max(120.0))
                                        .auto_shrink([false, false])
                                        .show(ui, |ui| {
                                            self.draw_effects(
                                                ui,
                                                packages,
                                                catalog,
                                                choices,
                                                &mut recipe,
                                                experimental,
                                            );
                                        });
                                }
                                if recipe != before {
                                    if let Some(document) = self
                                        .documents
                                        .iter_mut()
                                        .find(|document| document.recipe.id == document_id)
                                    {
                                        document.history.record(before, ctx);
                                        document.recipe = recipe;
                                        document.modified = Some(SystemTime::now());
                                    }
                                    self.message = None;
                                    self.message_path = None;
                                    self.persist_drafts();
                                }
                            },
                        );
                    },
                );
                let footer_top = ui.cursor().top();
                ui.separator();
                attachment = if weapon.kind == crate::ItemKind::Subclass {
                    self.draw_ability_attachment(ui)
                        .map(|change| attachment::Applied::Ability(Box::new(change)))
                } else {
                    self.draw_attachment(ui, weapon, donor, catalog)
                        .map(|change| attachment::Applied::Socket(Box::new(change)))
                };
                ctx.data_mut(|data| {
                    data.insert_temp(footer_id, (ui.cursor().top() - footer_top + 8.0).max(40.0))
                });
            })
            .map(|window| window.response.layer_id);
        if open && window.is_some_and(|layer| history::owns_shortcuts(ctx, layer)) {
            self.handle_save_shortcut(ctx);
            self.history_shortcuts(ctx);
        }
        match self.header_action.take() {
            Some(HeaderAction::History(redo)) => self.restore_history(redo),
            Some(HeaderAction::Save(copy)) => self.save(copy),
            Some(HeaderAction::Discard) => self.discard_changes(),
            Some(HeaderAction::Delete) => {
                self.confirm_delete(library::PerkSource::Document(self.selected))
            }
            None => {}
        }
        self.open = open;
        if !open {
            self.message = None;
            self.message_path = None;
        }
        self.capture_effect_draft();
        self.show_diagnostics(ctx);
        if let Some(document) = self.documents.get(self.selected) {
            self.verification.show(
                ctx,
                &document.recipe,
                self.item_kind,
                self.library.as_ref().map(Library::root),
            );
        }
        if self.busy() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        attachment
    }

    /// The name, the actions that act on this perk, its status and any message. Returns the
    /// height used, so the body below can take the rest.
    fn draw_document_header(
        &mut self,
        ui: &mut egui::Ui,
        recipe: &mut PerkRecipe,
        catalog: Option<&InvestmentCatalog>,
    ) -> f32 {
        let top = ui.cursor().top();
        let editing = self.editor.is_some();
        let save_issue = self.save_issue();
        let saveable = save_issue.is_none();
        let dirty = self
            .documents
            .get(self.selected)
            .is_some_and(|document| document.changed() || document.pending_effect.is_some())
            || editing;
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                crate::app::style::more_menu(ui, "Perk", |ui| {
                    for (label, redo) in [("Undo", false), ("Redo", true)] {
                        let available = self.editor.as_ref().map_or_else(
                            || {
                                self.documents.get(self.selected).is_some_and(|d| {
                                    d.pending_effect.is_none() && d.history.available(redo)
                                })
                            },
                            |editor| editor.history_available(redo),
                        );
                        if ui
                            .add_enabled(available, egui::Button::new(label))
                            .clicked()
                        {
                            self.header_action = Some(HeaderAction::History(redo));
                            ui.close_menu();
                        }
                    }
                    ui.separator();
                    if ui.button("Perk Diagnostics…").clicked() {
                        self.diagnostics_open = true;
                        ui.close_menu();
                    }
                    if ui.button("Gameplay Verification…").clicked() {
                        self.verification.open = true;
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(
                            !editing && !recipe.effects.is_empty(),
                            egui::Button::new("Copy Test Plan"),
                        )
                        .clicked()
                    {
                        ui.ctx().copy_text(test_plan::render(
                            recipe,
                            self.item_kind,
                            &self.perk_names,
                            Some(&self.keys.catalog),
                            &self.asset_labels,
                        ));
                        self.message = Some("Copied the in-game test plan.".into());
                        self.message_path = None;
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(saveable, egui::Button::new("Save as New Perk"))
                        .clicked()
                    {
                        self.header_action = Some(HeaderAction::Save(true));
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(dirty, egui::Button::new("Discard Changes"))
                        .clicked()
                    {
                        self.header_action = Some(HeaderAction::Discard);
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui
                        .add_enabled(!editing, egui::Button::new("Delete Perk…"))
                        .clicked()
                    {
                        self.header_action = Some(HeaderAction::Delete);
                        ui.close_menu();
                    }
                });
                let save = crate::app::style::primary(ui, "Save to Library");
                if ui
                    .add_enabled(saveable, save)
                    .on_hover_text("Ctrl+S")
                    .on_disabled_hover_text(save_issue.unwrap_or_default())
                    .clicked()
                {
                    self.header_action = Some(HeaderAction::Save(false));
                }
                // The same reading and colours as the weapon recipe's save status.
                let status = self.documents.get(self.selected).map(|document| {
                    if document.saved.is_none() {
                        crate::app::ui_state::RecipeSaveStatus::NotSavedYet
                    } else if dirty {
                        crate::app::ui_state::RecipeSaveStatus::UnsavedChanges
                    } else {
                        crate::app::ui_state::RecipeSaveStatus::Saved
                    }
                });
                if let Some(status) = status {
                    let visuals = ui.visuals();
                    let color = match status {
                        crate::app::ui_state::RecipeSaveStatus::NotSavedYet => visuals.text_color(),
                        crate::app::ui_state::RecipeSaveStatus::UnsavedChanges => {
                            visuals.warn_fg_color
                        }
                        crate::app::ui_state::RecipeSaveStatus::Saved => {
                            crate::app::style::success_color(visuals)
                        }
                    };
                    ui.label(egui::RichText::new(status.label()).color(color));
                }
                if let Some(message) = &self.message {
                    let detail = self.message_path.as_ref().map_or_else(
                        || message.clone(),
                        |path| format!("{message}\n{}", path.display()),
                    );
                    // Shown inline so an action says what it did, held to a width that leaves
                    // the name field room. The saved path stays on hover.
                    let color = crate::app::style::success_color(ui.visuals());
                    let width = 260.0_f32.min(ui.available_width() * 0.4);
                    let height = ui.spacing().interact_size.y;
                    ui.allocate_ui_with_layout(
                        egui::vec2(width, height),
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            // Right to left, so the message sits at the edge and the check
                            // reads before it.
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(message.as_str()).color(color),
                                )
                                .truncate(),
                            )
                            .on_hover_text(detail);
                            ui.label(
                                crate::app::style::icon(ui, egui_phosphor::regular::CHECK)
                                    .color(color),
                            );
                        },
                    );
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    if !self
                        .icons
                        .preview(ui, self.discovery.packages(), recipe.icon.as_ref())
                        && let Some(catalog) = catalog
                    {
                        catalog.draw_perk_icon(
                            ui,
                            recipe.template_plug.parse_u32().unwrap_or_default(),
                            ui.spacing().interact_size.y,
                        );
                    }
                    if editing {
                        ui.add(
                            egui::Label::new(egui::RichText::new(recipe.name.as_str()).strong())
                                .truncate(),
                        );
                    } else {
                        let response = ui.add(
                            egui::TextEdit::singleline(&mut recipe.name)
                                .hint_text("Perk Name")
                                .desired_width(f32::INFINITY),
                        );
                        crate::app::style::named_control(response, "Perk Name");
                    }
                });
            });
        });
        if !editing {
            self.draw_basics(ui, catalog, recipe);
        }
        // A draft with no editor open, such as one left when Experimental Features turns off.
        let stranded = !editing
            && self
                .documents
                .get(self.selected)
                .is_some_and(|document| document.pending_effect.is_some());
        if stranded {
            let discard = ui
                .horizontal_wrapped(|ui| {
                    ui.colored_label(ui.visuals().warn_fg_color, "Unapplied parameter edits.");
                    ui.button("Discard Parameter Edits").clicked()
                })
                .inner;
            if discard {
                if let Some(document) = self.documents.get_mut(self.selected) {
                    document.pending_effect = None;
                }
                self.persist_drafts();
            }
        }
        if let Some(error) = &self.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        ui.separator();
        ui.cursor().top() - top
    }

    fn retire_editor(&mut self) {
        if let Some(editor) = self.editor.take()
            && editor.has_background_work()
        {
            self.retired_editors.push(editor);
        }
    }

    fn capture_effect_draft(&mut self) {
        let (Some(editor), Some(index), Some(document)) = (
            &self.editor,
            self.editing_effect,
            self.documents.get_mut(self.selected),
        ) else {
            return;
        };
        let draft = EffectDraft {
            action: self.editing_program_action,
            index,
            values: editor.draft.clone(),
            action_values: editor.action_draft.clone(),
            projectiles: editor.projectile_draft.clone(),
            pending_movement: editor.pending_movement.clone(),
            field_text: editor
                .value_text
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        };
        if document.pending_effect.as_ref() != Some(&draft) {
            document.pending_effect = Some(draft);
            document.modified = Some(SystemTime::now());
            self.persist_drafts();
        }
    }

    fn restore_effect_draft(
        &mut self,
        ctx: &egui::Context,
        packages: &Path,
        choices: &[WeaponSandboxPerkChoice],
    ) {
        if self.editor.is_some() {
            return;
        }
        let Some(document) = self.documents.get(self.selected) else {
            return;
        };
        let Some(draft) = document.pending_effect.clone() else {
            return;
        };
        if !document
            .recipe
            .effects
            .iter()
            .any(|effect| effect.source_perk_index == draft.index)
        {
            return;
        }
        let key = PerkEditorKey {
            socket_index: 0,
            choice_index: 0,
            source_plug_hash: document
                .recipe
                .template_plug
                .parse_u32()
                .unwrap_or_default(),
            source_perk_index: draft.index,
        };
        if let Some(action_index) = draft.action {
            if let Some(asset) = document
                .recipe
                .effects
                .iter()
                .find(|effect| effect.source_perk_index == draft.index)
                .and_then(|effect| effect.program.as_ref())
                .and_then(|program| program.asset(action_index))
            {
                let mut editor = PerkEditor::open_entity(
                    packages.to_owned(),
                    key,
                    sundial::package_authoring::tft::asset_label(&asset.path),
                    asset.graph,
                    draft.values,
                    ctx,
                );
                editor.value_text = draft.field_text.into_iter().collect();
                self.editor = Some(editor);
                self.editing_effect = Some(draft.index);
                self.editing_program_action = Some(action_index);
            }
            return;
        }
        let mut editor = PerkEditor::open(
            packages.to_owned(),
            key,
            self.stock_effect_name(choices, draft.index),
            draft.values,
            draft.action_values,
            draft.projectiles,
            ctx,
        );
        editor.value_text = draft.field_text.into_iter().collect();
        editor.pending_movement = draft.pending_movement;
        editor.activation = document
            .recipe
            .effects
            .iter()
            .find(|effect| effect.source_perk_index == draft.index)
            .and_then(|effect| effect.activation);
        editor.projectile_labels = choices
            .iter()
            .map(|choice| (choice.perk_index, choice.representative_name.clone()))
            .collect();
        editor.item_names = self.item_names.clone();
        self.editor = Some(editor);
        self.editing_effect = Some(draft.index);
    }

    /// Resolves the weapon names the asset catalog refers to, once the catalog and the
    /// native assets are both available.
    fn refresh_item_names(&mut self, catalog: Option<&InvestmentCatalog>) {
        let (Some(catalog), Some(data)) = (catalog, &self.discovery.data) else {
            return;
        };
        if self.item_names.len() == data.pattern_items.len() {
            return;
        }
        self.item_names = data
            .pattern_items
            .iter()
            .map(|&item| {
                let name = entity::catalog::ItemName::new(
                    catalog.plug_label(item, false),
                    catalog.item_type_name(item).unwrap_or_default(),
                );
                (item, name)
            })
            .collect();
        self.asset_label_source = None;
    }

    fn refresh_asset_labels(&mut self) {
        let Some(data) = &self.discovery.data else {
            self.asset_labels.clear();
            self.asset_label_source = None;
            return;
        };
        let generation = entity::catalog::game_names_generation();
        if self
            .asset_label_source
            .as_ref()
            .is_some_and(|source| Arc::ptr_eq(source, &data.effects))
            && self.asset_label_generation == generation
        {
            return;
        }
        // A perk an ability grants is listed as "Ability <list> / <entry>", a placeholder
        // the naming ignores, so what it attaches is named after the ability instead.
        self.asset_labels = data.effects.discovery_labels_with(
            |index| match self.perk_names.get(&index) {
                Some(name) if !name.starts_with("Ability ") => Some(name.clone()),
                listed => crate::app::ability_names::perk_owner(index).or_else(|| listed.cloned()),
            },
            |item| self.item_names.get(&item).cloned(),
        );
        self.asset_label_source = Some(data.effects.clone());
        self.asset_label_generation = generation;
    }
}

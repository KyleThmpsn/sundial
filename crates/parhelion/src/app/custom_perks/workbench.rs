//! Standalone perk authoring. Weapon attachment is an explicit copy operation.
use super::*;
use crate::perk::{
    PerkRecipe,
    library::{Entry, Library},
};
use serde::{Deserialize, Serialize};
use std::time::SystemTime;

pub(super) mod assets;
mod attachment;
mod behaviors;
pub(super) mod canvas;
mod controls;
mod discovery;
mod engine;
mod forms;
mod guidance;
use crate::artwork_browser as icons;
mod library;
pub(super) use crate::app::pickers;
mod program;
mod properties;
mod reading;
mod selection;
mod stats;
mod templates;
mod test_plan;
#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app) enum Request {
    EditChoice { socket: usize, choice: usize },
    SelectChoice { socket: usize, choice: usize },
}

impl Request {
    pub(in crate::app) fn socket(self) -> usize {
        match self {
            Self::EditChoice { socket, .. } | Self::SelectChoice { socket, .. } => socket,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct Document {
    recipe: PerkRecipe,
    baseline: Option<Vec<u8>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    origin: Option<PerkRecipe>,
    #[serde(skip)]
    target: Option<attachment::Target>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pending_effect: Option<EffectDraft>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    modified: Option<SystemTime>,
}

impl Document {
    fn new(recipe: PerkRecipe, baseline: Option<Vec<u8>>) -> Self {
        Self {
            modified: baseline.is_none().then(SystemTime::now),
            origin: Some(recipe.clone()),
            recipe,
            baseline,
            target: None,
            pending_effect: None,
        }
    }

    fn restored(&self) -> PerkRecipe {
        self.baseline
            .as_deref()
            .and_then(|bytes| serde_json::from_slice(bytes).ok())
            .or_else(|| self.origin.clone())
            .unwrap_or_else(|| self.recipe.clone())
    }
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
    pending_movement: Option<(u32, Vec<(projectile::parameters::Kind, u32)>)>,
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
    initialized: bool,
    draft_baseline: Option<Vec<u8>>,
    drafts_writable: bool,
    library: Option<Library>,
    entries: Vec<Entry>,
    documents: Vec<Document>,
    selected: usize,
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
    property_query: String,
    removal_query: String,
    keys: program::Keys,
    behaviors: behaviors::Picker,
    /// Stock perk names by finished perk index, for labelling assets the perks reference.
    perk_names: BTreeMap<u16, String>,
    ingredients: Option<(usize, usize, Arc<sundial::investment::IngredientCatalog>)>,
    ingredient_source: Option<sundial::investment::IngredientSource>,
    /// Weapon names by item hash, for assets named after the pattern that fires them.
    item_names: BTreeMap<u32, projectile::catalog::ItemName>,
    /// Names are cached by native catalog identity and invalidated when source labels change.
    asset_label_source: Option<Arc<projectile::catalog::Catalog>>,
    asset_labels: BTreeMap<u32, String>,
    properties: properties::Properties,
    editor: Option<PerkEditor>,
    retired_editors: Vec<PerkEditor>,
    templates: Option<Vec<WeaponSandboxPerkChoice>>,
    authored_templates: Option<Vec<templates::AuthoredTemplate>>,
    editing_effect: Option<u16>,
    editing_program_action: Option<usize>,
    discovery: discovery::Discovery,
    engine: engine::EngineCatalog,
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
}

enum HeaderAction {
    Save(bool),
    Discard,
    Delete,
}

impl Workbench {
    pub(in crate::app) fn saved_weapons_changed(&mut self) {
        self.authored_templates = None;
    }

    fn perk_issue(&self, recipe: &PerkRecipe) -> Option<String> {
        recipe.validate().err().or_else(|| {
            recipe.effects.iter().find_map(|effect| {
                self.discovery
                    .perk_issue(effect.source_perk_index)
                    .map(str::to_owned)
            })
        })
    }

    pub(in crate::app) fn open_engine_catalog(&mut self) {
        self.engine.open = true;
    }

    pub(in crate::app) fn busy(&self) -> bool {
        self.discovery.busy()
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
    }

    fn discard_changes(&mut self) {
        self.retire_editor();
        self.editing_effect = None;
        self.editing_program_action = None;
        if let Some(document) = self.documents.get_mut(self.selected) {
            document.recipe = document.restored();
            document.pending_effect = None;
            document.modified = None;
        }
        self.error = None;
        self.message = Some("Changes discarded.".into());
        self.persist_drafts();
    }

    pub(in crate::app) fn invalidate(&mut self) {
        self.capture_effect_draft();
        self.discovery.invalidate();
        self.icons.invalidate();
        self.behaviors = behaviors::Picker::default();
        self.keys = program::Keys::default();
        self.ingredients = None;
        self.retire_editor();
        self.templates = None;
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

    fn show(
        &mut self,
        ctx: &egui::Context,
        packages: &Path,
        catalog: Option<&InvestmentCatalog>,
        choices: &[WeaponSandboxPerkChoice],
        experimental: bool,
        attachment: (&WeaponRecipe, Option<&WeaponDonor>),
    ) -> Option<attachment::Change> {
        let (weapon, donor) = attachment;
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
        self.icons.poll();
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
        if self.busy() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        if self.engine.open {
            self.refresh_asset_labels();
        }
        let empty_sources = sundial::investment::PerkSources::default();
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
            },
            experimental,
        );
        if let Some(index) = self.engine.copy_requested.take()
            && let Some(choice) = choices.iter().find(|choice| choice.perk_index == index)
        {
            self.initialize();
            self.copy_behavior(choice);
        }
        if !self.open {
            return None;
        }
        self.initialize();
        if !experimental && self.editing_program_action.is_some() {
            self.capture_effect_draft();
            self.retire_editor();
            self.editing_effect = None;
            self.editing_program_action = None;
        }
        let restore_allowed = experimental
            || self
                .documents
                .get(self.selected)
                .and_then(|document| document.pending_effect.as_ref())
                .is_none_or(|draft| draft.action.is_none());
        if restore_allowed {
            self.restore_effect_draft(ctx, packages, choices);
        }
        let mut open = self.open;
        let mut attachment = None;
        egui::Window::new("Custom Perk Workbench")
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
                crate::app::style::workbench_style(ui);
                // Respect the requested window size and reserve the destination footer.
                // The body follows the window, so dragging the window taller shows more of
                // the editor instead of stopping at a fixed height on a tall screen.
                let tallest = (ctx.screen_rect().height() - 140.0).max(240.0);
                let body_height = (ui.available_height() - 40.0).clamp(240.0, tallest);
                let width = ui.available_width();
                let library_width = (width * 0.26).clamp(260.0, 290.0).min(width * 0.46);
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
                                self.draw_library(
                                    ui,
                                    catalog,
                                    experimental,
                                    donor.map(|donor| (weapon, donor)),
                                );
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
                                        document.recipe = recipe;
                                        document.modified = Some(SystemTime::now());
                                    }
                                    self.message = None;
                                    self.persist_drafts();
                                }
                            },
                        );
                    },
                );
                ui.separator();
                attachment = self.draw_attachment(ui, weapon, donor, catalog);
            });
        if open {
            self.handle_save_shortcut(ctx);
        }
        match self.header_action.take() {
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
        let dirty = self.documents.get(self.selected).is_some_and(|document| {
            document.recipe != document.restored() || document.pending_effect.is_some()
        }) || editing;
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                crate::app::style::more_menu(ui, |ui| {
                    if ui
                        .add_enabled(
                            !editing && !recipe.effects.is_empty(),
                            egui::Button::new("Copy Test Plan"),
                        )
                        .on_hover_text(
                            "Copy an in-game checklist derived from this perk's triggers, actions and lifetime.",
                        )
                        .clicked()
                    {
                        ui.ctx().copy_text(test_plan::render(
                            recipe,
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
                    .on_hover_text("Save this perk in Custom Perks for reuse (Ctrl+S). Weapon copies stay unchanged. Apply to Weapon updates the selected socket.")
                    .on_disabled_hover_text(save_issue.unwrap_or_default())
                    .clicked()
                {
                    self.header_action = Some(HeaderAction::Save(false));
                }
                if let Some(message) = &self.message {
                    let detail = self.message_path.as_ref().map_or_else(
                        || message.clone(),
                        |path| format!("{message}\n{}", path.display()),
                    );
                    ui.label(
                        egui::RichText::new(egui_phosphor::regular::CHECK)
                            .color(crate::app::style::success_color(ui.visuals())),
                    )
                    .on_hover_text(detail);
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    if !self.icons.preview(ui, self.discovery.packages(), recipe.icon.as_ref())
                        && let Some(catalog) = catalog {
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
                let name = projectile::catalog::ItemName::new(
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
        if self
            .asset_label_source
            .as_ref()
            .is_some_and(|source| Arc::ptr_eq(source, &data.effects))
        {
            return;
        }
        self.asset_labels = data.effects.discovery_labels_with(
            |index| self.perk_names.get(&index).cloned(),
            |item| self.item_names.get(&item).cloned(),
        );
        self.asset_label_source = Some(data.effects.clone());
    }
}

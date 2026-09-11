//! Standalone perk authoring. Weapon attachment is an explicit copy operation.
use super::*;
use crate::perk::{
    PerkRecipe,
    library::{Entry, Library},
};
use serde::{Deserialize, Serialize};

mod attachment;
mod discovery;
mod forms;
mod library;
pub(super) mod pickers;
mod program;
mod selection;
mod templates;
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
}

impl Document {
    fn new(recipe: PerkRecipe, baseline: Option<Vec<u8>>) -> Self {
        Self {
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
    template_query: String,
    effect_query: String,
    stat_query: String,
    icon_query: String,
    category_query: String,
    asset_query: String,
    editor: Option<PerkEditor>,
    retired_editors: Vec<PerkEditor>,
    templates: Option<Vec<WeaponSandboxPerkChoice>>,
    authored_templates: Option<Vec<templates::AuthoredTemplate>>,
    editing_effect: Option<u16>,
    editing_program_action: Option<usize>,
    discovery: discovery::Discovery,
    message: Option<String>,
    message_path: Option<PathBuf>,
    error: Option<String>,
    picker: Option<selection::Picker>,
}

fn draw_experimental_banner(ui: &mut egui::Ui) {
    let (background, foreground) = if ui.visuals().dark_mode {
        (
            egui::Color32::from_rgb(77, 53, 32),
            egui::Color32::from_rgb(255, 218, 174),
        )
    } else {
        (
            egui::Color32::from_rgb(252, 234, 210),
            egui::Color32::from_rgb(105, 59, 17),
        )
    };
    egui::Frame::new()
        .fill(background)
        .corner_radius(6)
        .inner_margin(10)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(egui::Label::new(egui::RichText::new(
                "This feature is in early development! Some of the UI may not work, and some effects and combinations may behave unexpectedly or fail to work in game. Support and reliability will improve in future releases."
            ).color(foreground)).wrap());
        });
    ui.add_space(4.0);
}

impl Workbench {
    fn perk_issue(&self, recipe: &PerkRecipe) -> Option<String> {
        recipe.validate().err().or_else(|| {
            recipe.effects.iter().find_map(|effect| {
                self.discovery
                    .perk_issue(effect.source_perk_index)
                    .map(str::to_owned)
            })
        })
    }

    pub(in crate::app) fn open_assets(&mut self) {
        self.discovery.open = true;
    }

    pub(in crate::app) fn busy(&self) -> bool {
        self.discovery.busy()
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
    }

    fn discard_changes(&mut self) {
        self.retire_editor();
        self.editing_effect = None;
        self.editing_program_action = None;
        if let Some(document) = self.documents.get_mut(self.selected) {
            document.recipe = document.restored();
            document.pending_effect = None;
        }
        self.error = None;
        self.message = Some("Discarded changes to this perk.".into());
        self.persist_drafts();
    }

    pub(in crate::app) fn invalidate(&mut self) {
        self.capture_effect_draft();
        self.discovery.invalidate();
        self.retire_editor();
        self.templates = None;
        self.authored_templates = None;
        self.editing_effect = None;
        self.editing_program_action = None;
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
        self.discovery.poll();
        if self.busy() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        if !experimental {
            self.discovery.open = false;
        }
        if experimental && (self.open || self.discovery.open) {
            self.discovery.start(packages, ctx);
        }
        self.discovery.show(ctx, choices);
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
            .default_size(egui::vec2(1020.0, 720.0))
            .min_width(520.0_f32.min((ctx.screen_rect().width() - 40.0).max(320.0)))
            .max_width((ctx.screen_rect().width() - 40.0).max(320.0))
            .max_height((ctx.screen_rect().height() - 64.0).max(360.0))
            .show(ctx, |ui| {
                crate::app::style::perk_style(ui);
                draw_experimental_banner(ui);
                self.draw_toolbar(ui, catalog, experimental);
                if let Some(error) = &self.error {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                }
                if let Some(message) = &self.message {
                    let response =
                        ui.colored_label(crate::app::style::success_color(ui.visuals()), message);
                    if let Some(path) = &self.message_path {
                        response.on_hover_text(path.display().to_string());
                    }
                }
                ui.separator();
                let body_height = (ui.available_height() - 46.0).max(240.0);
                let library_width = (ui.available_width() * 0.22).clamp(180.0, 230.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), body_height),
                    egui::Layout::left_to_right(egui::Align::Min),
                    |ui| {
                        ui.allocate_ui_with_layout(
                            egui::vec2(library_width, body_height),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.set_max_width(library_width);
                                if let (Some(catalog), Some(donor)) = (catalog, donor) {
                                    self.draw_weapon_perks(ui, weapon, donor, catalog);
                                    ui.separator();
                                }
                                self.draw_library(ui);
                            },
                        );
                        ui.separator();
                        ui.vertical(|ui| {
                            ui.set_min_width((ui.available_width() - 8.0).max(300.0));
                            let Some(document) = self.documents.get(self.selected) else {
                                return;
                            };
                            let mut recipe = document.recipe.clone();
                            let before = recipe.clone();
                            if self.editor.is_some() {
                                self.draw_effect_editor(
                                    ui,
                                    ctx,
                                    &mut recipe,
                                    experimental,
                                    body_height,
                                );
                            } else {
                                ui.horizontal(|ui| {
                                    ui.label("Perk Name");
                                    let response = ui.add(
                                        egui::TextEdit::singleline(&mut recipe.name)
                                            .desired_width(f32::INFINITY),
                                    );
                                    crate::app::style::named_control(response, "Perk Name");
                                });
                                ui.add_space(4.0);
                                ui.horizontal_wrapped(|ui| {
                                    ui.selectable_value(
                                        &mut self.page,
                                        Page::Effects,
                                        "Effect Builder",
                                    );
                                    ui.selectable_value(&mut self.page, Page::Basics, "Identity");
                                });
                                ui.separator();
                                egui::ScrollArea::vertical()
                                    .id_salt("independent-perk-body")
                                    .max_height(body_height - 78.0)
                                    .auto_shrink([false, false])
                                    .show(ui, |ui| match self.page {
                                        Page::Basics => self.draw_basics(ui, catalog, &mut recipe),
                                        Page::Effects => self.draw_effects(
                                            ui,
                                            packages,
                                            catalog,
                                            choices,
                                            &mut recipe,
                                            experimental,
                                        ),
                                    });
                            }
                            if recipe != before {
                                self.documents[self.selected].recipe = recipe;
                                self.message = None;
                                self.persist_drafts();
                            }
                        });
                    },
                );
                ui.separator();
                attachment = self.draw_attachment(ui, weapon, donor, catalog);
            });
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

    fn draw_toolbar(
        &mut self,
        ui: &mut egui::Ui,
        catalog: Option<&InvestmentCatalog>,
        experimental: bool,
    ) {
        ui.horizontal_wrapped(|ui| {
            let editing = self.editor.is_some();
            if ui
                .add_enabled(!editing, egui::Button::new("New Perk"))
                .clicked()
            {
                let mut recipe = PerkRecipe::new();
                if experimental {
                    recipe.effects.push(program::new_effect(421));
                }
                self.add_document(Document::new(recipe, None));
                self.page = Page::Effects;
            }
            if let Some(catalog) = catalog {
                ui.add_enabled_ui(!editing, |ui| self.draw_templates(ui, catalog));
            }
            ui.separator();
            if ui
                .add_enabled(
                    !editing && self.library.is_some(),
                    egui::Button::new("Save Perk"),
                )
                .clicked()
            {
                self.save(false);
            }
            if ui
                .add_enabled(
                    !editing && self.library.is_some(),
                    egui::Button::new("Save as New Perk"),
                )
                .on_hover_text("Save a separate perk and keep the original unchanged.")
                .on_disabled_hover_text("Apply or discard the open parameter edits first.")
                .clicked()
            {
                self.save(true);
            }
            ui.add_enabled_ui(!editing, |ui| {
                ui.menu_button("More", |ui| {
                    crate::app::style::perk_style(ui);
                    if ui.button("Import…").clicked() {
                        self.import();
                        ui.close_menu();
                    }
                    if ui.button("Export…").clicked() {
                        self.export();
                        ui.close_menu();
                    }
                });
            });
            let dirty = self.documents.get(self.selected).is_some_and(|document| {
                document.recipe != document.restored() || document.pending_effect.is_some()
            }) || editing;
            if ui
                .add_enabled(dirty, egui::Button::new("Discard Changes"))
                .on_hover_text(
                    "Restore the last saved perk, or the original copy for an unsaved perk.",
                )
                .clicked()
            {
                self.discard_changes();
            }
        });
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
                .and_then(|program| program.actions.get(action_index))
                .map(|action| action.asset())
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
            forms::effect_name(choices, draft.index),
            draft.values,
            draft.action_values,
            draft.projectiles,
            ctx,
        );
        editor.value_text = draft.field_text.into_iter().collect();
        editor.pending_movement = draft.pending_movement;
        editor.projectile_labels = choices
            .iter()
            .map(|choice| (choice.perk_index, choice.representative_name.clone()))
            .collect();
        self.editor = Some(editor);
        self.editing_effect = Some(draft.index);
    }
}

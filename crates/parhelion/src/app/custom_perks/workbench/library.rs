use super::*;
mod order;
pub(super) use order::Order;

/// Where a Custom Perks row comes from: an open document, or a saved perk not opened yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PerkSource {
    Document(usize),
    Entry(usize),
}

/// What the reader picked from a row's right-click menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RowAction {
    Duplicate,
    Delete,
}

/// A perk the reader asked to delete, shown in a confirmation before anything is removed.
#[derive(Clone, Debug)]
pub(super) struct PendingDelete {
    name: String,
    source: PerkSource,
}

/// The name a row shows for a recipe, with a stand-in for an empty one.
fn display_name(name: &str) -> &str {
    if name.trim().is_empty() {
        "Untitled Perk"
    } else {
        name
    }
}

/// The right-click menu of one Custom Perks row.
fn draw_row_menu(
    response: &egui::Response,
    source: PerkSource,
    saved: bool,
) -> Option<(PerkSource, RowAction)> {
    let mut action = None;
    response.context_menu(|ui| {
        crate::app::style::workbench_style(ui);
        if ui
            .button("Duplicate")
            .on_hover_text("Open a copy as a new draft. Save to Library keeps it in Custom Perks.")
            .clicked()
        {
            action = Some((source, RowAction::Duplicate));
            ui.close_menu();
        }
        let delete = if saved {
            "Delete…"
        } else {
            "Delete Draft…"
        };
        if ui.button(delete).clicked() {
            action = Some((source, RowAction::Delete));
            ui.close_menu();
        }
    });
    action
}

impl Workbench {
    pub(super) fn initialize(&mut self) {
        if self.initialized {
            return;
        }
        self.initialized = true;
        match Library::open_default() {
            Ok(library) => {
                let draft_path = library.root().join("workbench-drafts.json");
                match std::fs::read(&draft_path) {
                    Ok(bytes) => match serde_json::from_slice::<Vec<Document>>(&bytes) {
                        Ok(documents) if documents.iter().all(|document| document.recipe.validate_draft().is_ok()) => {
                            self.documents = documents;
                            for document in &mut self.documents { if document.origin.is_none() { document.origin = Some(document.recipe.clone()); } }
                            self.draft_baseline = Some(bytes);
                            self.drafts_writable = true;
                        }
                        _ => self.error = Some("Saved workbench drafts could not be read. The original file is preserved and draft autosave is paused. Save to Library or Export keeps your new work.".into()),
                    },
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => self.drafts_writable = true,
                    Err(error) => self.error = Some(format!("Could not read workbench drafts: {error}. Draft autosave is paused.")),
                }
                self.library = Some(library);
                self.refresh_library();
            }
            Err(error) => self.error = Some(error),
        }
        if self.documents.is_empty() {
            self.documents.push(Document::new(PerkRecipe::new(), None));
        }
    }

    pub(super) fn refresh_library(&mut self) {
        let warnings = self.scan_library();
        if !warnings.is_empty() {
            self.error = Some(warnings.join("\n"));
        }
    }

    /// A failed refresh must not offer stale saved versions as current library entries.
    pub(super) fn scan_library(&mut self) -> Vec<String> {
        self.authored_templates = None;
        self.entries.clear();
        let Some(library) = &self.library else {
            return vec![
                "Custom Perks is unavailable. Workbench drafts and weapon recipes are still available."
                    .into(),
            ];
        };
        match library.scan() {
            Ok(scan) => {
                self.entries = scan.entries;
                scan.errors
            }
            Err(error) => vec![error],
        }
    }

    pub(super) fn persist_drafts(&mut self) {
        if !self.drafts_writable {
            return;
        }
        let Some(library) = &self.library else {
            return;
        };
        let result = serde_json::to_vec_pretty(&self.documents)
            .map_err(|error| error.to_string())
            .and_then(|bytes| {
                library.save_drafts(&bytes, self.draft_baseline.as_deref())?;
                Ok(bytes)
            });
        match result {
            Ok(bytes) => self.draft_baseline = Some(bytes),
            Err(error) => {
                self.drafts_writable = false;
                self.error = Some(format!("Draft autosave paused: {error}"));
            }
        }
    }

    pub(super) fn add_document(&mut self, document: Document) {
        self.message = None;
        self.message_path = None;
        self.query.clear();
        if let Some(index) = self
            .documents
            .iter()
            .position(|existing| existing.recipe.id == document.recipe.id)
        {
            self.select_document(index);
        } else {
            let index = self.documents.len();
            self.documents.push(document);
            self.select_document(index);
            self.persist_drafts();
        }
    }

    pub(super) fn draw_library(
        &mut self,
        ui: &mut egui::Ui,
        catalog: Option<&InvestmentCatalog>,
        experimental: bool,
        weapon: Option<(&WeaponRecipe, &WeaponDonor)>,
    ) {
        ui.horizontal(|ui| {
            ui.set_min_height(ui.spacing().interact_size.y);
            ui.strong("Custom Perks");
        });
        ui.horizontal_wrapped(|ui| {
            crate::app::style::compact_controls(ui);
            self.draw_library_actions(ui, catalog, experimental);
        });
        self.draw_library_search(ui);
        if let (Some(catalog), Some((weapon, donor))) = (catalog, weapon) {
            egui::CollapsingHeader::new("Current Weapon Perks").show(ui, |ui| {
                self.draw_weapon_perks(ui, weapon, donor, catalog);
            });
        }
        if !self.drafts_writable {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                "Draft autosave is paused. Use Save to Library or Export to keep your changes.",
            );
        }
        let query = self.query.trim().to_lowercase();
        let mut picked = None;
        let mut selected = None;
        let mut action = None;
        let order = self.library_rows();
        egui::ScrollArea::vertical()
            .id_salt("perk-library")
            .max_height(ui.available_height().max(80.0))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let mut visible = 0;
                ui.add_enabled_ui(self.editor.is_none(), |ui| {
                    for source in order {
                        match source {
                            PerkSource::Document(index) => {
                                let document = &self.documents[index];
                                if !document.recipe.name.to_lowercase().contains(&query) {
                                    continue;
                                }
                                let unchanged = document
                                    .baseline
                                    .as_ref()
                                    .and_then(|bytes| {
                                        serde_json::from_slice::<PerkRecipe>(bytes).ok()
                                    })
                                    .is_some_and(|saved| saved == document.recipe);
                                let name = display_name(&document.recipe.name);
                                let label =
                                    format!("{name}{}", if unchanged { "" } else { " · Draft" });
                                visible += 1;
                                let response = perk_row(
                                    ui,
                                    catalog,
                                    &document.recipe,
                                    self.selected == index,
                                    &label,
                                );
                                if self.reveal_document && self.selected == index {
                                    response.scroll_to_me(Some(egui::Align::Min));
                                }
                                if response.clicked() {
                                    selected = Some(index);
                                }
                                action = action.or(draw_row_menu(
                                    &response,
                                    PerkSource::Document(index),
                                    document.baseline.is_some(),
                                ));
                            }
                            PerkSource::Entry(index) => {
                                let entry = &self.entries[index];
                                if !entry.recipe.name.to_lowercase().contains(&query) {
                                    continue;
                                }
                                visible += 1;
                                let response =
                                    perk_row(ui, catalog, &entry.recipe, false, &entry.recipe.name);
                                if response.clicked() {
                                    picked = Some(Document::new(
                                        entry.recipe.clone(),
                                        Some(entry.baseline.clone()),
                                    ));
                                }
                                action = action.or(draw_row_menu(
                                    &response,
                                    PerkSource::Entry(index),
                                    true,
                                ));
                            }
                        }
                    }
                });
                if visible == 0 {
                    ui.weak("No perks match this search.");
                }
            });
        self.reveal_document = false;
        if let Some(index) = selected {
            self.select_document(index);
            self.page = Page::Effects;
            self.message = None;
            self.message_path = None;
        }
        if let Some(document) = picked {
            self.add_document(document);
        }
        match action {
            Some((source, RowAction::Duplicate)) => self.duplicate(source),
            Some((source, RowAction::Delete)) => {
                self.confirm_delete(source);
            }
            None => {}
        }
        self.draw_delete_confirmation(ui.ctx());
        self.draw_restore_defaults_confirmation(ui.ctx());
    }

    /// Compact actions alongside the Custom Perks heading.
    fn draw_library_actions(
        &mut self,
        ui: &mut egui::Ui,
        catalog: Option<&InvestmentCatalog>,
        experimental: bool,
    ) {
        let editing = self.editor.is_some();
        ui.add_enabled_ui(!editing, |ui| {
            if ui
                .button("New Perk")
                .on_hover_text("Start an empty perk.")
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
                self.draw_templates(ui, catalog);
            }
            crate::app::style::more_menu(ui, |ui| {
                crate::app::style::workbench_style(ui);
                if ui.button("Import…").clicked() {
                    self.import();
                    ui.close_menu();
                }
                if ui.button("Export…").clicked() {
                    self.export();
                    ui.close_menu();
                }
                if ui.button("Refresh Library").clicked() {
                    self.refresh_library();
                    ui.close_menu();
                }
                ui.separator();
                let has_edits = self.has_unsaved_bundled_edits();
                if ui
                    .add_enabled(
                        self.library.is_some() && !has_edits,
                        egui::Button::new("Restore Default Custom Perks…"),
                    )
                    .on_hover_text(if has_edits {
                        "Save or discard open edits to a default custom perk first."
                    } else {
                        "Back up changed defaults, restore missing defaults, and leave your custom perks unchanged."
                    })
                    .clicked()
                {
                    let result = self
                        .library
                        .as_ref()
                        .ok_or("Custom Perks is unavailable".to_owned())
                        .and_then(Library::prepare_restore_defaults);
                    match result {
                        Ok(preview) => self.pending_restore_defaults = Some(preview),
                        Err(error) => self.error = Some(error),
                    }
                    ui.close_menu();
                }
            });
        });
    }

    fn has_unsaved_bundled_edits(&self) -> bool {
        let Some(library) = &self.library else {
            return false;
        };
        let Ok(ids) = library.bundled_ids() else {
            return false;
        };
        self.documents.iter().any(|document| {
            ids.contains(&document.recipe.id)
                && document
                    .baseline
                    .as_deref()
                    .and_then(|bytes| serde_json::from_slice::<PerkRecipe>(bytes).ok())
                    .is_none_or(|saved| saved != document.recipe)
        })
    }

    fn draw_restore_defaults_confirmation(&mut self, ctx: &egui::Context) {
        let Some(preview) = self.pending_restore_defaults.clone() else {
            return;
        };
        let has_edits = self.has_unsaved_bundled_edits();
        let mut restore = false;
        let mut cancel = false;
        let response = egui::Modal::new("restore_default_custom_perks".into()).show(ctx, |ui| {
            crate::app::style::workbench_style(ui);
            ui.set_width(400.0);
            ui.heading("Restore Default Custom Perks?");
            ui.label("This replaces saved edits to bundled custom perks and restores missing defaults. Changed files are backed up first. Your other custom perks and weapon recipes stay unchanged.");
            if has_edits {
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    "Save or discard open edits to a default custom perk first.",
                );
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                restore = ui
                    .add_enabled(!has_edits, egui::Button::new("Back Up & Restore"))
                    .clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
        cancel |= response.should_close();
        if restore {
            self.pending_restore_defaults = None;
            let result = self
                .library
                .as_ref()
                .ok_or("Custom Perks is unavailable".to_owned())
                .and_then(|library| library.restore_defaults(&preview));
            match result {
                Ok(backup) => {
                    self.refresh_library();
                    self.reload_bundled_documents();
                    self.error = None;
                    self.message_path = backup.clone();
                    self.message = Some(match backup {
                        Some(path) => {
                            format!("Default custom perks restored. Backup: {}", path.display())
                        }
                        None => "Default custom perks are already up to date.".into(),
                    });
                }
                Err(error) => self.error = Some(error),
            }
        } else if cancel {
            self.pending_restore_defaults = None;
        }
    }

    fn reload_bundled_documents(&mut self) {
        let Some(library) = &self.library else {
            return;
        };
        let Ok(ids) = library.bundled_ids() else {
            return;
        };
        let restored = self
            .entries
            .iter()
            .filter(|entry| ids.contains(&entry.recipe.id))
            .map(|entry| {
                (
                    entry.recipe.id.clone(),
                    (entry.recipe.clone(), entry.baseline.clone()),
                )
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        for document in &mut self.documents {
            let Some((recipe, baseline)) = restored.get(&document.recipe.id) else {
                continue;
            };
            document.recipe = recipe.clone();
            document.origin = Some(recipe.clone());
            document.baseline = Some(baseline.clone());
            document.pending_effect = None;
            document.modified = None;
        }
        self.persist_drafts();
    }

    fn recipe_at(&self, source: PerkSource) -> Option<&PerkRecipe> {
        match source {
            PerkSource::Document(index) => {
                self.documents.get(index).map(|document| &document.recipe)
            }
            PerkSource::Entry(index) => self.entries.get(index).map(|entry| &entry.recipe),
        }
    }

    /// Opens a copy of a perk as a new draft with its own id, so the original stays as it
    /// is until the reader saves the copy.
    pub(super) fn duplicate(&mut self, source: PerkSource) {
        let Some(mut recipe) = self.recipe_at(source).cloned() else {
            return;
        };
        recipe.id = PerkRecipe::new().id;
        recipe.name = format!("{} Copy", display_name(&recipe.name));
        let name = recipe.name.clone();
        self.add_document(Document::new(recipe, None));
        self.page = Page::Effects;
        self.error = None;
        self.message = Some(format!("Opened {name} as a new draft."));
    }

    pub(super) fn confirm_delete(&mut self, source: PerkSource) {
        if let Some(recipe) = self.recipe_at(source) {
            self.pending_delete = Some(PendingDelete {
                name: display_name(&recipe.name).to_owned(),
                source,
            });
        }
    }

    fn draw_delete_confirmation(&mut self, ctx: &egui::Context) {
        let Some(pending) = self.pending_delete.clone() else {
            return;
        };
        let saved = match pending.source {
            PerkSource::Document(index) => self
                .documents
                .get(index)
                .is_some_and(|document| document.baseline.is_some()),
            PerkSource::Entry(_) => true,
        };
        let mut confirm = false;
        let mut cancel = false;
        let response = egui::Modal::new("perk-workbench-delete".into()).show(ctx, |ui| {
            crate::app::style::workbench_style(ui);
            ui.set_width(380.0);
            ui.heading(if saved {
                "Delete This Perk?"
            } else {
                "Delete This Draft?"
            });
            ui.strong(&pending.name);
            ui.label(if saved {
                "The saved copy in Custom Perks is removed. Weapon recipes that already carry this perk keep their own copy."
            } else {
                "This draft was never saved to Custom Perks, so it cannot be brought back."
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                confirm = ui.button("Delete").clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
        cancel |= response.should_close();
        if confirm {
            self.pending_delete = None;
            self.delete(pending.source);
        } else if cancel {
            self.pending_delete = None;
        }
    }

    /// Deletes a perk: its saved file when it has one, and its open document. A file that
    /// changed outside the workbench is left alone and reported.
    pub(super) fn delete(&mut self, source: PerkSource) {
        let (recipe, baseline) = match source {
            PerkSource::Document(index) => {
                let Some(document) = self.documents.get(index) else {
                    return;
                };
                (document.recipe.clone(), document.baseline.clone())
            }
            PerkSource::Entry(index) => {
                let Some(entry) = self.entries.get(index) else {
                    return;
                };
                (entry.recipe.clone(), Some(entry.baseline.clone()))
            }
        };
        if let Some(baseline) = baseline {
            let Some(library) = &self.library else {
                self.error = Some(
                    "Custom Perks is unavailable, so the saved copy cannot be deleted.".into(),
                );
                return;
            };
            if let Err(error) = library.delete(&recipe, &baseline) {
                self.error = Some(error);
                return;
            }
        }
        if let PerkSource::Document(index) = source {
            self.remove_document(index);
        }
        self.error = None;
        self.message = Some(format!("Deleted {}.", display_name(&recipe.name)));
        self.message_path = None;
        self.refresh_library();
    }

    /// Closes a document. The list always keeps one perk open and selected.
    fn remove_document(&mut self, index: usize) {
        if index >= self.documents.len() {
            return;
        }
        if self.selected == index {
            self.retire_editor();
            self.editing_effect = None;
            self.editing_program_action = None;
        }
        self.documents.remove(index);
        if self.documents.is_empty() {
            self.documents.push(Document::new(PerkRecipe::new(), None));
        }
        if self.selected > index {
            self.selected -= 1;
        }
        self.selected = self.selected.min(self.documents.len() - 1);
        self.persist_drafts();
    }

    pub(super) fn save(&mut self, copy: bool) {
        if let Some(issue) = self.save_issue() {
            self.error = Some(issue.to_owned());
            return;
        }
        let Some(library) = self.library.clone() else {
            return;
        };
        let Some(document) = self.documents.get(self.selected).cloned() else {
            return;
        };
        let mut recipe = document.recipe;
        if copy {
            recipe.id = PerkRecipe::new().id;
        }
        match library.save(
            &recipe,
            if copy {
                None
            } else {
                document.baseline.as_deref()
            },
        ) {
            Ok(entry) => {
                if copy {
                    self.documents
                        .push(Document::new(entry.recipe, Some(entry.baseline)));
                    self.select_document(self.documents.len() - 1);
                } else {
                    self.documents[self.selected].baseline = Some(entry.baseline);
                    self.documents[self.selected].origin = Some(recipe.clone());
                    self.documents[self.selected].modified = entry.modified;
                }
                self.error = None;
                self.message = Some(format!("Saved {} to Custom Perks.", recipe.name));
                self.message_path = Some(entry.path);
                self.persist_drafts();
                self.refresh_library();
            }
            Err(error) => self.error = Some(error),
        }
    }

    pub(super) fn save_issue(&self) -> Option<&'static str> {
        if self.editor.is_some()
            || self
                .documents
                .get(self.selected)
                .is_some_and(|document| document.pending_effect.is_some())
        {
            Some("Apply or discard the open parameter edits first.")
        } else if self.library.is_none() {
            Some("Custom Perks is unavailable, so this perk cannot be saved to the library.")
        } else {
            None
        }
    }

    pub(super) fn handle_save_shortcut(&mut self, ctx: &egui::Context) {
        if ctx.memory(|memory| memory.top_modal_layer().is_some() || memory.any_popup_open()) {
            return;
        }
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::S)) {
            self.header_action = Some(HeaderAction::Save(false));
        }
    }

    pub(super) fn import(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Custom Perk", &["json"])
            .pick_file()
        else {
            return;
        };
        match Library::read(&path) {
            Ok(mut entry) => {
                entry.recipe.id = PerkRecipe::new().id;
                self.add_document(Document::new(entry.recipe, None));
            }
            Err(error) => self.error = Some(error),
        }
    }

    pub(super) fn export(&mut self) {
        let Some(library) = &self.library else {
            return;
        };
        let Some(document) = self.documents.get(self.selected) else {
            return;
        };
        let Some(path) = rfd::FileDialog::new()
            .set_file_name(format!("{}.perk.json", document.recipe.id))
            .save_file()
        else {
            return;
        };
        let result = library.export(&document.recipe, &path);
        match result {
            Ok(()) => {
                self.message = Some(format!("Exported {}.", document.recipe.name));
                self.message_path = Some(path);
            }
            Err(error) => self.error = Some(error),
        }
    }
}

fn perk_row(
    ui: &mut egui::Ui,
    catalog: Option<&InvestmentCatalog>,
    recipe: &PerkRecipe,
    selected: bool,
    label: &str,
) -> egui::Response {
    match catalog {
        Some(catalog) => catalog.draw_perk_row_with_icon(
            ui,
            recipe.template_plug.parse_u32().unwrap_or_default(),
            label,
            selected,
            sundial::investment::PlugTooltip {
                name: Some(display_name(&recipe.name)),
                description: Some(&recipe.description),
                classification_hash: recipe
                    .classification
                    .as_ref()
                    .and_then(|hash| hash.parse_u32().ok()),
            },
            crate::artwork_browser::preview::icon(ui, catalog, recipe.icon.as_ref()),
        ),
        None => crate::app::style::list_row(ui, selected, label),
    }
}

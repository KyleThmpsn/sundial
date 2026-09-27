use super::*;
mod order;
pub(super) use order::Order;

/// Saved perks checked for problems per frame, so opening a large library never stalls a frame.
const ISSUE_CHECKS_PER_FRAME: usize = 16;

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

/// A perk the reader asked to delete, shown in a confirmation before anything is removed. It
/// is found again by id when the reader confirms, since the list can be rescanned meanwhile.
#[derive(Clone, Debug)]
pub(super) struct PendingDelete {
    name: String,
    id: String,
    saved: bool,
}

/// The name a row shows for a recipe, with a stand-in for an empty one.
fn display_name(name: &str) -> &str {
    if name.trim().is_empty() {
        "Untitled Perk"
    } else {
        name
    }
}

/// A perk's name as a file name: the characters no file system takes are replaced, and a
/// name that is only those falls back to the stand-in.
fn file_stem(name: &str) -> String {
    let stem = name
        .trim()
        .chars()
        .map(|character| match character {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => ' ',
            control if control.is_control() => ' ',
            other => other,
        })
        .collect::<String>();
    let stem = stem.split_whitespace().collect::<Vec<_>>().join(" ");
    let stem = stem.trim_end_matches('.').trim().to_owned();
    if stem.is_empty() {
        "Untitled Perk".to_owned()
    } else {
        stem
    }
}

/// "1 draft" or "N drafts".
fn drafts(count: usize) -> String {
    if count == 1 {
        "1 draft".to_owned()
    } else {
        format!("{count} drafts")
    }
}

/// The documents in a drafts file that still open, and the entries that no longer do. A file
/// that is not a list of drafts at all is one rejected entry, so it is set aside whole.
fn read_drafts(bytes: &[u8]) -> (Vec<Document>, Vec<serde_json::Value>) {
    let Ok(entries) = serde_json::from_slice::<Vec<serde_json::Value>>(bytes) else {
        let whole = serde_json::from_slice::<serde_json::Value>(bytes)
            .unwrap_or_else(|_| serde_json::Value::String(String::from_utf8_lossy(bytes).into()));
        return (Vec::new(), vec![whole]);
    };
    let mut documents = Vec::new();
    let mut rejected = Vec::new();
    for entry in entries {
        match serde_json::from_value::<Document>(entry.clone()) {
            Ok(document) if document.recipe.validate_draft().is_ok() => documents.push(document),
            _ => rejected.push(entry),
        }
    }
    (documents, rejected)
}

/// The right-click menu of one Custom Perks row.
fn draw_row_menu(
    response: &egui::Response,
    source: PerkSource,
    saved: bool,
) -> Option<(PerkSource, RowAction)> {
    let mut action = None;
    response.context_menu(|ui| {
        crate::app::style::perk_workbench_style(ui);
        if ui.button("Duplicate").clicked() {
            action = Some((source, RowAction::Duplicate));
            ui.close_menu();
        }
        let delete = if saved {
            "Delete Perk…"
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
        self.initialize_from(Library::open_default());
    }

    /// Opens the workbench on `library`: its saved perks, and the drafts file beside them.
    pub(super) fn initialize_from(&mut self, library: Result<Library, String>) {
        self.initialized = true;
        match library {
            Ok(library) => {
                let draft_path = library.root().join("workbench-drafts.json");
                match std::fs::read(&draft_path) {
                    Ok(bytes) => {
                        let (documents, rejected) = read_drafts(&bytes);
                        self.documents = documents;
                        for document in &mut self.documents {
                            if document.origin.is_none() {
                                document.origin = Some(document.recipe.clone());
                            }
                            document.read_baseline();
                        }
                        self.draft_baseline = Some(bytes);
                        self.drafts_writable = true;
                        // A draft the workbench cannot read is set aside, so the ones it
                        // can read open and autosave carries on.
                        if !rejected.is_empty() {
                            let aside = library.root().join("workbench-drafts.rejected.json");
                            let kept = serde_json::to_vec_pretty(&rejected)
                                .map_err(|error| error.to_string())
                                .and_then(|bytes| {
                                    std::fs::write(&aside, bytes).map_err(|error| error.to_string())
                                });
                            self.error = Some(match kept {
                                Ok(()) => format!(
                                    "{} could not be opened. Kept in {}.",
                                    drafts(rejected.len()),
                                    aside.display().to_string().trim_start_matches(r"\\?\")
                                ),
                                Err(error) => format!(
                                    "{} could not be opened or set aside: {error}",
                                    drafts(rejected.len())
                                ),
                            });
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        self.drafts_writable = true
                    }
                    Err(error) => {
                        self.error = Some(format!(
                            "Could not read workbench drafts: {error}. Draft autosave is paused."
                        ))
                    }
                }
                let refresh = library.defaults_refresh().cloned();
                self.library = Some(library);
                self.refresh_library();
                if let Some(refresh) = refresh {
                    self.reload_bundled_documents();
                    self.message_path = Some(refresh.backup.clone());
                    self.message = Some(refresh.summary("perks"));
                }
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
        self.adopt_library_changes();
    }

    /// Brings open documents up to date with their files after a rescan. A document with no
    /// edits takes the file as it is now. One with edits keeps them and measures them
    /// against the file as it is now, so it can be saved in place or discarded to it.
    fn adopt_library_changes(&mut self) {
        let on_disk = self
            .entries
            .iter()
            .map(|entry| (entry.recipe.id.as_str(), entry))
            .collect::<std::collections::BTreeMap<_, _>>();
        for document in &mut self.documents {
            let Some(entry) = on_disk.get(document.recipe.id.as_str()) else {
                continue;
            };
            if document.baseline.as_deref() == Some(entry.baseline.as_slice()) {
                continue;
            }
            if document.unchanged() {
                document.recipe = entry.recipe.clone();
                document.origin = Some(entry.recipe.clone());
                document.modified = entry.modified;
            }
            document.set_baseline(Some(entry.baseline.clone()));
        }
    }

    /// A failed refresh must not offer stale saved versions as current library entries.
    pub(super) fn scan_library(&mut self) -> Vec<String> {
        self.authored_templates = None;
        self.entries.clear();
        let Some(library) = &self.library else {
            return vec!["Custom Perks is unavailable.".into()];
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
        let documents = self
            .documents
            .iter()
            .filter(|document| !document.untouched_copy())
            .collect::<Vec<_>>();
        let result = serde_json::to_vec_pretty(&documents)
            .map_err(|error| error.to_string())
            .and_then(|bytes| {
                library.save_drafts(&bytes, self.draft_baseline.as_deref())?;
                Ok(bytes)
            });
        match result {
            Ok(bytes) => {
                self.draft_baseline = Some(bytes);
                self.drafts_error = None;
            }
            Err(error) => {
                // A failed write is tried again on the next edit rather than pausing autosave
                // for the session. The drafts file is this reader's scratch, so whatever
                // another instance wrote there is taken as the baseline for that next write.
                self.drafts_error = Some(error);
                self.draft_baseline =
                    std::fs::read(library.root().join("workbench-drafts.json")).ok();
            }
        }
    }

    pub(super) fn add_document(&mut self, document: Document) {
        self.message = None;
        self.message_path = None;
        self.error = None;
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
        weapon: Option<(&WeaponRecipe, &WeaponDonor)>,
    ) {
        ui.horizontal(|ui| {
            ui.set_min_height(ui.spacing().interact_size.y);
            ui.strong("Custom Perks");
        });
        ui.horizontal_wrapped(|ui| {
            crate::app::style::compact_controls(ui);
            self.draw_library_actions(ui, catalog);
        });
        self.draw_library_search(ui);
        if let (Some(catalog), Some((weapon, donor))) = (catalog, weapon) {
            egui::CollapsingHeader::new(format!("Current {} Perks", weapon.kind.label())).show(
                ui,
                |ui| {
                    self.draw_weapon_perks(ui, weapon, donor, catalog);
                },
            );
        }
        if !self.drafts_writable {
            ui.colored_label(ui.visuals().warn_fg_color, "Draft autosave is paused.");
        } else if let Some(error) = &self.drafts_error {
            ui.colored_label(ui.visuals().warn_fg_color, "Draft autosave failed.")
                .on_hover_text(error);
        }
        let query = self.query.trim().to_lowercase();
        let mut picked = None;
        let mut selected = None;
        let mut action = None;
        let order = self.library_rows();
        let issues = self.row_issues(ui.ctx(), &order);
        egui::ScrollArea::vertical()
            .id_salt("perk-library")
            .max_height(ui.available_height().max(80.0))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let mut visible = 0;
                ui.add_enabled_ui(self.editor.is_none(), |ui| {
                    for (source, issue) in order.into_iter().zip(&issues) {
                        match source {
                            PerkSource::Document(index) => {
                                let document = &self.documents[index];
                                if !document.recipe.name.to_lowercase().contains(&query) {
                                    continue;
                                }
                                let name = display_name(&document.recipe.name);
                                let draft = if document.unchanged() {
                                    ""
                                } else {
                                    " · Draft"
                                };
                                let label = format!("{name}{draft}");
                                visible += 1;
                                let response = perk_row(
                                    ui,
                                    catalog,
                                    &document.recipe,
                                    self.selected == index,
                                    &label,
                                    issue.as_deref(),
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
                                let response = perk_row(
                                    ui,
                                    catalog,
                                    &entry.recipe,
                                    false,
                                    &entry.recipe.name,
                                    issue.as_deref(),
                                );
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
                    ui.weak(if query.is_empty() {
                        "No custom perks yet."
                    } else {
                        "No perks match this search."
                    });
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

    /// Each row's problem or warning, the one the status bar names when the perk is open. Open
    /// drafts are checked on every frame. Saved perks are checked a few at a time and kept until
    /// the file changes, or until discovery, whose results the check reads, finishes or restarts.
    fn row_issues(&mut self, ctx: &egui::Context, order: &[PerkSource]) -> Vec<Option<String>> {
        let ready = !self.discovery.busy();
        if ready != self.library_issues_ready {
            self.library_issues.clear();
            self.library_issues_ready = ready;
        }
        let mut budget = ISSUE_CHECKS_PER_FRAME;
        let mut issues = Vec::with_capacity(order.len());
        for source in order {
            let issue = match *source {
                PerkSource::Document(index) => {
                    let recipe = &self.documents[index].recipe;
                    self.perk_issue(recipe)
                        .or_else(|| validation::perk_warning(recipe))
                }
                PerkSource::Entry(index) => {
                    let entry = &self.entries[index];
                    let key = (entry.modified, entry.baseline.len());
                    match self.library_issues.get(&entry.path) {
                        Some((modified, size, issue)) if (*modified, *size) == key => issue.clone(),
                        _ if budget == 0 => {
                            ctx.request_repaint();
                            None
                        }
                        _ => {
                            budget -= 1;
                            let issue = self
                                .perk_issue(&entry.recipe)
                                .or_else(|| validation::perk_warning(&entry.recipe));
                            self.library_issues
                                .insert(entry.path.clone(), (key.0, key.1, issue.clone()));
                            issue
                        }
                    }
                }
            };
            issues.push(issue);
        }
        if self.library_issues.len() > self.entries.len() {
            let paths = self
                .entries
                .iter()
                .map(|entry| entry.path.clone())
                .collect::<BTreeSet<_>>();
            self.library_issues.retain(|path, _| paths.contains(path));
        }
        issues
    }

    /// Compact actions alongside the Custom Perks heading.
    fn draw_library_actions(&mut self, ui: &mut egui::Ui, catalog: Option<&InvestmentCatalog>) {
        let editing = self.editor.is_some();
        ui.add_enabled_ui(!editing, |ui| {
            if ui.button("New Perk").clicked() {
                self.add_document(Document::new(PerkRecipe::new(), None));
                self.page = Page::Basics;
            }
            if let Some(catalog) = catalog {
                self.draw_templates(ui, catalog);
            }
            crate::app::style::more_menu(ui, "Library", |ui| {
                crate::app::style::perk_workbench_style(ui);
                if ui.button("Import…").clicked() {
                    self.import();
                    ui.close_menu();
                }
                let edit_issue = self.edit_issue();
                if ui
                    .add_enabled(edit_issue.is_none(), egui::Button::new("Export…"))
                    .on_disabled_hover_text(edit_issue.unwrap_or_default())
                    .clicked()
                {
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
                    .on_disabled_hover_text(if has_edits {
                        "Save or discard edits to default perks first."
                    } else {
                        "Custom Perks is unavailable."
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
        self.documents
            .iter()
            .any(|document| ids.contains(&document.recipe.id) && !document.unchanged())
    }

    fn draw_restore_defaults_confirmation(&mut self, ctx: &egui::Context) {
        let Some(preview) = self.pending_restore_defaults.clone() else {
            return;
        };
        let has_edits = self.has_unsaved_bundled_edits();
        let mut restore = false;
        let mut cancel = false;
        let response = egui::Modal::new("restore_default_custom_perks".into()).show(ctx, |ui| {
            crate::app::style::perk_workbench_style(ui);
            ui.set_width(400.0);
            ui.heading("Restore Default Custom Perks?");
            ui.label("Replaces edited default custom perks and restores missing ones.");
            if has_edits {
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    "Save or discard edits to default perks first.",
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
                        Some(_) => "Default custom perks restored.".into(),
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
            // Edits that were never saved go on as a draft copy under a new id, the way the
            // library keeps a saved edit as a copy, rather than vanishing under the new
            // default.
            if document.changed() || document.pending_effect.is_some() {
                document.recipe.id = PerkRecipe::new().id;
                document.recipe.name = format!("{} Copy", display_name(&document.recipe.name));
                document.origin = Some(document.recipe.clone());
                document.set_baseline(None);
                document.target = None;
                document.from_socket = false;
                continue;
            }
            document.recipe = recipe.clone();
            document.origin = Some(recipe.clone());
            document.set_baseline(Some(baseline.clone()));
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
        let saved = match source {
            PerkSource::Document(index) => self
                .documents
                .get(index)
                .is_some_and(|document| document.baseline.is_some()),
            PerkSource::Entry(_) => true,
        };
        if let Some(recipe) = self.recipe_at(source) {
            self.pending_delete = Some(PendingDelete {
                name: display_name(&recipe.name).to_owned(),
                id: recipe.id.clone(),
                saved,
            });
        }
    }

    /// Where the perk with this id is now: its open document first, then its saved entry.
    pub(super) fn source_of(&self, id: &str) -> Option<PerkSource> {
        self.documents
            .iter()
            .position(|document| document.recipe.id == id)
            .map(PerkSource::Document)
            .or_else(|| {
                self.entries
                    .iter()
                    .position(|entry| entry.recipe.id == id)
                    .map(PerkSource::Entry)
            })
    }

    fn draw_delete_confirmation(&mut self, ctx: &egui::Context) {
        let Some(pending) = self.pending_delete.clone() else {
            return;
        };
        let saved = pending.saved;
        let mut confirm = false;
        let mut cancel = false;
        let response = egui::Modal::new("perk-workbench-delete".into()).show(ctx, |ui| {
            crate::app::style::perk_workbench_style(ui);
            ui.set_width(380.0);
            ui.heading(if saved {
                "Delete This Perk?"
            } else {
                "Delete This Draft?"
            });
            ui.strong(&pending.name);
            ui.label(if saved {
                "Removes the saved copy. Recipes that carry this perk keep their own."
            } else {
                "This draft was never saved, so it cannot be brought back."
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
            if let Some(source) = self.source_of(&pending.id) {
                self.delete(source);
            }
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
                self.error = Some("Custom Perks is unavailable.".into());
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
        // A draft that was never saved has no original to keep, so a copy of it is the draft
        // itself saved, not a second document beside it.
        let copy = copy && document.baseline.is_some();
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
                    self.documents[self.selected].set_baseline(Some(entry.baseline));
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

    pub(super) fn edit_issue(&self) -> Option<&'static str> {
        if self.copying_selected_effect() {
            Some("Wait for the effect copy to finish.")
        } else if self.editor.is_some()
            || self
                .documents
                .get(self.selected)
                .is_some_and(|document| document.pending_effect.is_some())
        {
            Some("Apply or discard the open parameter edits first.")
        } else {
            None
        }
    }

    pub(super) fn save_issue(&self) -> Option<&'static str> {
        if let Some(issue) = self.edit_issue() {
            Some(issue)
        } else if self.library.is_none() {
            Some("Custom Perks is unavailable.")
        } else if self
            .documents
            .get(self.selected)
            .is_some_and(|document| document.recipe.name.trim().is_empty())
        {
            Some("Name the perk first.")
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
        self.import_from(&path);
    }

    /// Opens the perk in `path` as a new draft under its own id.
    pub(super) fn import_from(&mut self, path: &Path) {
        match Library::read(path) {
            Ok(mut entry) => {
                entry.recipe.id = PerkRecipe::new().id;
                self.add_document(Document::new(entry.recipe, None));
            }
            Err(error) => self.error = Some(error),
        }
    }

    pub(super) fn export(&mut self) {
        let Some(document) = self.documents.get(self.selected) else {
            return;
        };
        let Some(path) = rfd::FileDialog::new()
            .set_file_name(format!(
                "{}.perk.json",
                file_stem(display_name(&document.recipe.name))
            ))
            .save_file()
        else {
            return;
        };
        self.export_to(&path);
    }

    /// Writes the open perk to `path`, outside the library.
    pub(super) fn export_to(&mut self, path: &Path) {
        let Some(library) = &self.library else {
            return;
        };
        let Some(document) = self.documents.get(self.selected) else {
            return;
        };
        match library.export(&document.recipe, path) {
            Ok(()) => {
                self.message = Some(format!("Exported {}.", document.recipe.name));
                self.message_path = Some(path.to_owned());
                self.error = None;
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
    issue: Option<&str>,
) -> egui::Response {
    let Some(issue) = issue else {
        return perk_row_body(ui, catalog, recipe, selected, label);
    };
    // The icon keeps its own room at the end of the row, so a long name truncates before it.
    let icon = ui.text_style_height(&egui::TextStyle::Body) + ui.spacing().item_spacing.x;
    ui.horizontal(|ui| {
        let width = (ui.available_width() - icon).max(0.0);
        let row = ui
            .allocate_ui_with_layout(
                egui::vec2(width, 0.0),
                egui::Layout::top_down_justified(egui::Align::Min),
                |ui| perk_row_body(ui, catalog, recipe, selected, label),
            )
            .inner;
        let color = crate::app::style::secondary(ui.visuals());
        ui.label(egui::RichText::new(egui_phosphor::regular::WARNING).color(color))
            .on_hover_text(issue);
        row
    })
    .inner
}

fn perk_row_body(
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

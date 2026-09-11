use super::*;

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
                        _ => self.error = Some("Saved workbench drafts could not be read. The original file is preserved and draft autosave is paused. Save Perk or Export keeps your new work.".into()),
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

    fn refresh_library(&mut self) {
        self.authored_templates = None;
        let Some(library) = &self.library else {
            return;
        };
        match library.scan() {
            Ok(scan) => {
                self.entries = scan.entries;
                if !scan.errors.is_empty() {
                    self.error = Some(scan.errors.join("\n"));
                }
            }
            Err(error) => self.error = Some(error),
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
        if let Some(index) = self
            .documents
            .iter()
            .position(|existing| existing.recipe.id == document.recipe.id)
        {
            self.selected = index;
        } else {
            self.selected = self.documents.len();
            self.documents.push(document);
            self.persist_drafts();
        }
    }

    pub(super) fn draw_library(&mut self, ui: &mut egui::Ui) {
        ui.strong("My Perks");
        let response = ui.add(
            egui::TextEdit::singleline(&mut self.query)
                .hint_text("Search My Perks")
                .desired_width(f32::INFINITY),
        );
        crate::app::style::named_control(response, "Search My Perks");
        let query = self.query.trim().to_lowercase();
        let mut picked = None;
        egui::ScrollArea::vertical()
            .id_salt("perk-library")
            .max_height((ui.available_height() - 66.0).max(80.0))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let mut visible = 0;
                ui.add_enabled_ui(self.editor.is_none(), |ui| {
                    for (index, document) in self.documents.iter().enumerate() {
                        if !document.recipe.name.to_lowercase().contains(&query) {
                            continue;
                        }
                        let saved = document
                            .baseline
                            .as_ref()
                            .and_then(|bytes| serde_json::from_slice::<PerkRecipe>(bytes).ok())
                            .is_some_and(|saved| saved == document.recipe);
                        let name = if document.recipe.name.trim().is_empty() {
                            "Untitled Perk"
                        } else {
                            &document.recipe.name
                        };
                        let label = format!("{name}{}", if saved { "" } else { " *" });
                        visible += 1;
                        if crate::app::style::list_row(ui, self.selected == index, &label).clicked()
                        {
                            self.selected = index;
                            self.page = Page::Effects;
                            self.message = None;
                            self.message_path = None;
                        }
                    }
                    let unloaded = self.entries.iter().filter(|entry| {
                        !self
                            .documents
                            .iter()
                            .any(|document| document.recipe.id == entry.recipe.id)
                    });
                    for entry in unloaded {
                        if !entry.recipe.name.to_lowercase().contains(&query) {
                            continue;
                        }
                        visible += 1;
                        if crate::app::style::list_row(ui, false, &entry.recipe.name).clicked() {
                            picked = Some(Document::new(
                                entry.recipe.clone(),
                                Some(entry.baseline.clone()),
                            ));
                        }
                    }
                });
                if visible == 0 {
                    ui.weak("No matching perks.");
                }
            });
        if let Some(document) = picked {
            self.add_document(document);
        }
        ui.small(if self.drafts_writable {
            "* Unsaved Changes"
        } else {
            "Draft autosave is paused. Use Save Perk or Export to keep your changes."
        })
        .on_hover_text("Drafts are kept automatically. Save Perk updates the copy in My Perks.");
        if ui.button("Refresh Library").clicked() {
            self.refresh_library();
        }
    }

    pub(super) fn save(&mut self, copy: bool) {
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
                    self.selected = self.documents.len() - 1;
                } else {
                    self.documents[self.selected].baseline = Some(entry.baseline);
                    self.documents[self.selected].origin = Some(recipe.clone());
                }
                self.error = None;
                self.message = Some(format!("Saved {} to My Perks.", recipe.name));
                self.message_path = Some(entry.path);
                self.persist_drafts();
                self.refresh_library();
            }
            Err(error) => self.error = Some(error),
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

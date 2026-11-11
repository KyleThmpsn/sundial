use super::*;

/// What a library listing shows: its rows, and the export choices it hides.
#[derive(Clone, Copy)]
struct Shown {
    matches: usize,
    hidden: usize,
}

impl PackageAuthoringApp {
    /// `open_only` leaves recipes open to opening while the rest of the library waits for the
    /// importer's sweep or source read.
    pub(super) fn draw_library_browser(
        &mut self,
        ctx: &egui::Context,
        busy: bool,
        open_only: bool,
    ) {
        let mut open = self.library_open;
        let mut action = None;
        egui::Window::new("Recipe Library")
            .open(&mut open)
            .default_width(620.0)
            .default_height(720.0)
            .min_width(420.0)
            .resizable(true)
            .show(ctx, |ui| {
                workbench_style(ui);
                if self.draw_library_controls(ui, busy, &mut action) {
                    return;
                }
                #[cfg(feature = "d2-model-importer")]
                if open_only {
                    self.draw_library_importer_work(ui);
                }
                self.draw_library_notice(ui);
                self.draw_library_search(ui);
                ui.separator();
                let shown = self.draw_library_entries(ui, busy, open_only, &mut action);
                ui.separator();
                self.draw_library_footer(ui, busy, shown, &mut action);
            });
        self.library_open = open;
        if !open {
            self.restore_defaults_preview = None;
            self.library_state.export_selection = None;
        }
        if let Some(action) = action {
            self.apply_library_action(ctx, action);
        }
    }

    fn draw_library_search(&mut self, ui: &mut egui::Ui) {
        super::draw_recipe_search(
            ui,
            "library-sort",
            &mut self.library_query,
            &mut self.library_state,
            (&self.recipe_entries, &self.library_donors),
            &mut self.recipe_search_focus_pending,
        );
    }

    /// The importer's running work, such as its sweep or a source read, while it holds the
    /// library.
    #[cfg(feature = "d2-model-importer")]
    fn draw_library_importer_work(&self, ui: &mut egui::Ui) {
        let color = ui.visuals().weak_text_color();
        for (label, _, details) in self.importer.activities() {
            let response = ui
                .horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(12.0).color(color));
                    ui.label(egui::RichText::new(label).color(color));
                })
                .response;
            if !details.is_empty() {
                response.on_hover_ui(|ui| {
                    for line in &details {
                        ui.weak(line);
                    }
                });
            }
        }
    }

    /// Draws the rows the search and filters show. Returns how many show and, while recipes are
    /// being chosen for export, how many chosen ones they hide.
    fn draw_library_entries(
        &mut self,
        ui: &mut egui::Ui,
        busy: bool,
        open_only: bool,
        action: &mut Option<LibraryAction>,
    ) -> Shown {
        let query = self.library_query.trim().to_lowercase();
        let listing =
            self.library_state
                .take_listing(&self.recipe_entries, &self.library_donors, &query);
        let shown = listing
            .shown
            .iter()
            .map(|(index, details)| (&self.recipe_entries[*index], details.as_str()))
            .collect::<Vec<_>>();
        // Activity recipes have their own editor. Item bundles do not carry activities.
        let shown_maps = if self.library_state.export_selection.is_some() {
            Vec::new()
        } else {
            super::matching_maps(&self.map_entries, &self.library_state, &query)
        };
        if let Some(selected) = &mut self.library_state.export_selection {
            ui.horizontal(|ui| {
                ui.strong("Choose Recipes to Export");
                ui.add_enabled_ui(!busy, |ui| {
                    super::draw_select_all_shown(
                        ui,
                        shown.iter().map(|(entry, _)| &entry.path),
                        selected,
                    );
                });
            });
        }
        // Opening only reads a saved recipe, so it need not wait for the sweep or a source read.
        let can_open = !busy || (open_only && self.library_state.export_selection.is_none());
        let footer_height = super::pinned_footer_height(ui);
        egui::ScrollArea::vertical()
            .id_salt("recipe-library-results")
            .max_height(super::windowed_list_height(ui, footer_height))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for &(entry, details) in &shown {
                    let current = self.recipe_path.as_ref() == Some(&entry.path);
                    let response = ui
                        .add_enabled_ui(can_open, |ui| {
                            draw_library_row(
                                ui,
                                self.catalog.as_ref(),
                                &mut self.library_icons,
                                entry,
                                details,
                                LibraryRowState {
                                    current,
                                    inclusion: self
                                        .library_state
                                        .export_selection
                                        .as_ref()
                                        .map(|selected| selected.contains(&entry.path)),
                                    highlighted: self
                                        .library_state
                                        .highlighted
                                        .contains(&entry.path),
                                    reveal: self.library_state.reveal.as_ref() == Some(&entry.path),
                                    can_restore: !(current && self.recipe_dirty),
                                    installed: self.installed.contains(entry.identity_hash),
                                    library_busy: busy,
                                },
                            )
                        })
                        .inner;
                    if self.library_state.reveal.as_ref() == Some(&entry.path) {
                        self.library_state.reveal = None;
                    }
                    if let Some(selected) = &mut self.library_state.export_selection {
                        if response.activated && !selected.remove(&entry.path) {
                            selected.insert(entry.path.clone());
                        }
                    } else if let Some(entry_action) = response
                        .action
                        .or_else(|| response.activated.then_some(EntryAction::Open))
                    {
                        *action = Some(LibraryAction::Entry(entry.path.clone(), entry_action));
                    }
                }
                if shown_maps.is_empty() && shown.is_empty() {
                    ui.label("No Matching Results");
                }
                for map in &shown_maps {
                    let clicked = ui
                        .add_enabled_ui(can_open, |ui| super::draw_map_row(ui, map, None))
                        .inner;
                    #[cfg(feature = "d2-model-importer")]
                    if clicked {
                        *action = Some(LibraryAction::OpenActivity(map.path.clone()));
                    }
                    #[cfg(not(feature = "d2-model-importer"))]
                    let _ = clicked;
                }
            });
        let counts = Shown {
            matches: shown.len() + shown_maps.len(),
            hidden: self
                .library_state
                .export_selection
                .as_ref()
                .map_or(0, |selected| {
                    super::hidden_selection(selected, shown.iter().map(|(entry, _)| &entry.path))
                }),
        };
        drop(shown);
        self.library_state.keep_listing(listing);
        counts
    }

    fn draw_library_footer(
        &mut self,
        ui: &mut egui::Ui,
        busy: bool,
        Shown { matches, hidden }: Shown,
        action: &mut Option<LibraryAction>,
    ) {
        if let Some(selected) = &self.library_state.export_selection {
            let count = selected.len();
            let mut cancel = false;
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!busy && count > 0, egui::Button::new("Export Bundle…"))
                    .on_hover_text("Saves the selected recipes, with open edits, in one file.")
                    .clicked()
                {
                    *action = Some(LibraryAction::ExportSelected);
                }
                cancel = ui.add_enabled(!busy, egui::Button::new("Cancel")).clicked();
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    super::draw_selection_count(ui, count, hidden, matches);
                });
            });
            if cancel {
                self.library_state.export_selection = None;
            }
        } else {
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let noun = if matches == 1 { "recipe" } else { "recipes" };
                    ui.weak(format!("{matches} {noun}"));
                });
            });
        }
    }
}

use super::*;

impl PackageAuthoringApp {
    pub(super) fn draw_library_browser(&mut self, ctx: &egui::Context, busy: bool) {
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
                self.draw_library_notice(ui);
                self.draw_library_search(ui);
                ui.separator();
                let matches = self.draw_library_entries(ui, busy, &mut action);
                ui.separator();
                self.draw_library_footer(ui, busy, matches, &mut action);
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
            &mut self.library_state.sort,
            &mut self.library_state.kind,
            &mut self.library_state.activities_only,
            &mut self.recipe_search_focus_pending,
        );
    }

    fn draw_library_entries(
        &mut self,
        ui: &mut egui::Ui,
        busy: bool,
        action: &mut Option<LibraryAction>,
    ) -> usize {
        let query = self.library_query.trim().to_lowercase();
        let mut shown =
            self.library_state
                .matching_entries(&self.recipe_entries, &self.library_donors, &query);
        self.library_state
            .sort_entries(&mut shown, &self.library_donors);
        // Activity recipes have their own editor. Item bundles do not carry activities.
        let shown_maps = if self.library_state.export_selection.is_some() {
            Vec::new()
        } else {
            super::matching_maps(&self.map_entries, self.library_state.kind, &query)
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
        let footer_height = super::pinned_footer_height(ui);
        egui::ScrollArea::vertical()
            .id_salt("recipe-library-results")
            .max_height(super::windowed_list_height(ui, footer_height))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (entry, details) in &shown {
                    let current = self.recipe_path.as_ref() == Some(&entry.path);
                    let response = ui
                        .add_enabled_ui(!busy, |ui| {
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
                        .add_enabled_ui(!busy, |ui| super::draw_map_row(ui, map, None))
                        .inner;
                    #[cfg(feature = "d2-model-importer")]
                    if clicked {
                        *action = Some(LibraryAction::OpenActivity(map.path.clone()));
                    }
                    #[cfg(not(feature = "d2-model-importer"))]
                    let _ = clicked;
                }
            });
        shown.len() + shown_maps.len()
    }

    fn draw_library_footer(
        &mut self,
        ui: &mut egui::Ui,
        busy: bool,
        matches: usize,
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
                    ui.weak(format!("{count} selected · {matches} shown"));
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

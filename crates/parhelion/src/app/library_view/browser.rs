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
                ui.add_space(
                    (ui.available_height() - ui.spacing().interact_size.y - 12.0).max(0.0),
                );
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
        ui.horizontal(|ui| {
            let search = named_control(
                ui.add(
                    egui::TextEdit::singleline(&mut self.library_query)
                        .hint_text("Search Recipes…")
                        .desired_width((ui.available_width() - 175.0).max(100.0)),
                ),
                "Search recipes",
            )
            .on_hover_text("Search by name, weapon type, element or ammo.");
            if std::mem::take(&mut self.recipe_search_focus_pending) {
                search.request_focus();
            }
            egui::ComboBox::from_id_salt("library-sort")
                .width(155.0)
                .selected_text(format!(
                    "Sort: {}",
                    match self.library_state.sort {
                        SortOrder::RecentlyModified => "Recent",
                        order => order.label(),
                    }
                ))
                .show_ui(ui, |ui| {
                    for order in SortOrder::ALL {
                        ui.selectable_value(&mut self.library_state.sort, order, order.label());
                    }
                });
        });
    }

    fn draw_library_entries(
        &mut self,
        ui: &mut egui::Ui,
        busy: bool,
        action: &mut Option<LibraryAction>,
    ) -> usize {
        let query = self.library_query.trim().to_lowercase();
        let mut shown =
            matching_library_entries(&self.recipe_entries, &self.donor_summaries, &query);
        self.library_state
            .sort_entries(&mut shown, &self.donor_summaries);
        if let Some(selected) = &mut self.library_state.export_selection {
            ui.horizontal(|ui| {
                ui.strong("Choose Recipes To Export");
                ui.add_enabled_ui(!busy, |ui| {
                    if ui
                        .small_button("Select All")
                        .on_hover_text("Select all recipes shown by this search.")
                        .clicked()
                    {
                        selected.extend(shown.iter().map(|(entry, _)| entry.path.clone()));
                    }
                    if ui
                        .small_button("Clear All")
                        .on_hover_text("Clear all recipes shown by this search.")
                        .clicked()
                    {
                        for (entry, _) in &shown {
                            selected.remove(&entry.path);
                        }
                    }
                });
            });
        }
        egui::ScrollArea::vertical()
            .id_salt("recipe-library-results")
            .max_height((ui.available_height() - 45.0).max(120.0))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (entry, details) in &shown {
                    let current = self.recipe_path.as_ref() == Some(&entry.path);
                    let response = ui
                        .add_enabled_ui(!busy, |ui| {
                            draw_library_row(
                                ui,
                                &self.library_icons,
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
                if shown.is_empty() {
                    ui.label("No matching recipes. Try a different search.");
                }
            });
        shown.len()
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
                    .on_hover_text("Save selected recipes in one bundle, including open edits if that recipe is selected.")
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
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let noun = if matches == 1 { "recipe" } else { "recipes" };
                ui.weak(format!("{matches} {noun}"));
            });
        }
    }
}

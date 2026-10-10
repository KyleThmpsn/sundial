//! Workbench preferences and a separate, read-only activity log.
use super::*;

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub(super) enum PreferencesPage {
    #[default]
    EditorLibrary,
    BuildsBackups,
}

impl PreferencesPage {
    pub(super) const ALL: [Self; 2] = [Self::EditorLibrary, Self::BuildsBackups];

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::EditorLibrary => "Editor & Library",
            Self::BuildsBackups => "Builds & Backups",
        }
    }
}

impl PackageAuthoringApp {
    pub(super) fn apply_preview_preferences(&self, ctx: &egui::Context) {
        sundial::ui::model_preview::set_options(
            ctx,
            sundial::ui::model_preview::Options {
                show_fps: self.show_preview_fps,
                play_animations: self.play_preview_animations,
            },
        );
    }

    pub(super) fn draw_preview_preferences(&mut self, ui: &mut egui::Ui) -> bool {
        preference_heading(ui, "Model Previews");
        let mut changed = ui
            .checkbox(&mut self.show_preview_fps, "Show Preview FPS")
            .on_hover_text("Show a small frame rate counter in model previews.")
            .changed();
        changed |= ui
            .checkbox(
                &mut self.play_preview_animations,
                "Play Preview Animations by Default",
            )
            .on_hover_text(
                "Start embedded previews playing. Use Play or Pause on a preview to change it.",
            )
            .changed();
        if changed {
            self.apply_preview_preferences(ui.ctx());
        }
        changed
    }

    pub(super) fn draw_preferences_window(&mut self, ctx: &egui::Context) {
        if !self.preferences_open {
            return;
        }
        let mut open = true;
        let mut done = false;
        let available =
            (ctx.content_rect().size() - egui::vec2(48.0, 48.0)).max(egui::vec2(320.0, 260.0));
        egui::Window::new("Parhelion Preferences")
            .id(egui::Id::new("parhelion-preferences"))
            .collapsible(false)
            .resizable(true)
            .min_size(egui::vec2(720.0, 520.0).min(available))
            .default_size(egui::vec2(960.0, 640.0).min(available))
            .max_size(available)
            .open(&mut open)
            .show(ctx, |ui| {
                workbench_style(ui);
                ui.heading("Preferences");
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    for page in PreferencesPage::ALL {
                        ui.selectable_value(&mut self.preferences_page, page, page.label());
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical()
                    .id_salt(("parhelion-preferences-content", self.preferences_page))
                    .max_height((ui.available_height() - 42.0).max(100.0))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.add_space(6.0);
                        if let Some(error) = &self.preferences_error {
                            ui.colored_label(ui.visuals().error_fg_color, error);
                            ui.label(
                                "Changes apply to this session until preferences can be saved.",
                            );
                            ui.add_space(8.0);
                        }
                        self.draw_preferences_page(ui);
                    });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Activity Log…").clicked() {
                        self.activity_log_open = true;
                    }
                    if ui.link("Sundial Preferences").clicked() {
                        self.open_sundial_preferences = true;
                        done = true;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        done |= ui.button("Done").clicked();
                    });
                });
            });
        self.preferences_open = open && !done;
    }

    pub(super) fn draw_preferences_page(&mut self, ui: &mut egui::Ui) {
        match self.preferences_page {
            PreferencesPage::EditorLibrary => self.draw_editor_library_preferences(ui),
            PreferencesPage::BuildsBackups => self.draw_build_backup_preferences(ui),
        }
    }

    fn package_preferences_editable(&self) -> bool {
        self.build_receiver.is_none()
            && self.install_receiver.is_none()
            && !(self.build_status_open && self.build_dialog_step == BuildDialogStep::ReviewInstall)
    }

    fn draw_editor_library_preferences(&mut self, ui: &mut egui::Ui) {
        if self.draw_preview_preferences(ui) {
            self.save_workbench_preferences();
        }
        ui.add_space(12.0);
        preference_heading(ui, "Experimental");
        #[cfg(feature = "d2-model-importer")]
        self.draw_importer_preference(ui);
        let mut show = self.show_experimental_options;
        if ui
            .checkbox(&mut show, "Enable Experimental Features")
            .on_hover_text("Shows advanced behavior, inventory, socket, material and package controls. Saved overrides stay active when off.")
            .changed()
        {
            self.set_show_experimental_options(show);
        }
        ui.add_space(12.0);
        let editable = self.package_preferences_editable();
        preference_heading(ui, "Recipe Library");
        if let Some(library) = self.recipe_library.clone() {
            draw_preference_path(ui, library.root());
            ui.horizontal_wrapped(|ui| {
                if ui.button("Open Recipe Folder").clicked()
                    && let Err(error) = open_directory(library.root())
                {
                    self.log.push(LogEntry::error(error));
                }
                if ui
                    .add_enabled(
                        editable && !self.has_background_work(),
                        egui::Button::new("Refresh Library"),
                    )
                    .clicked()
                {
                    self.refresh_recipe_library();
                }
            });
        } else {
            ui.label("Recipe library unavailable. See the Activity Log.");
        }
        ui.add_space(12.0);
        preference_heading(ui, "Weapon Catalog");
        if let Some(progress) = self.catalog_progress {
            ui.label(format!(
                "{} ({}/{})",
                progress.message, progress.completed, progress.total
            ));
        } else if self.catalog.is_some() {
            ui.label(format!(
                "{} weapon donors · {} in Collections",
                self.donor_summaries.len(),
                self.donor_summaries
                    .iter()
                    .filter(|donor| donor.collection_backed)
                    .count()
            ));
        } else {
            ui.label("Catalog unavailable");
        }
        if ui
            .add_enabled(
                editable && !self.has_background_work(),
                egui::Button::new("Reload Catalog"),
            )
            .clicked()
        {
            self.reset_catalog_load();
        }
    }

    fn draw_build_backup_preferences(&mut self, ui: &mut egui::Ui) {
        let editable = self.package_preferences_editable();
        if !editable {
            ui.label("Locked during a build, installation or review.");
        }
        ui.add_enabled_ui(editable, |ui| {
            preference_heading(ui, "Build Files");
            ui.label("Game Packages · Selected in Sundial");
            draw_preference_path(ui, &self.packages);
            let mut changed = fields::path_row(
                ui,
                "Staging Folder",
                &mut self.staging,
                "Each build gets its own folder here.",
            );
            changed |= ui
                .checkbox(
                    &mut self.ignore_installed,
                    "Build from a Temporary Stock Package View",
                )
                .changed();
            ui.add_space(12.0);
            preference_heading(ui, "Package Backups");
            changed |= fields::path_row(
                ui,
                "Backup Folder",
                &mut self.backup_root,
                "Replaced packages are backed up here.",
            );
            let mut backups_changed = false;
            ui.horizontal_wrapped(|ui| {
                backups_changed |= ui
                    .checkbox(&mut self.limit_package_backups, "Keep Last")
                    .changed();
                let retention = named_control(
                    ui.add_enabled(
                        self.limit_package_backups,
                        egui::DragValue::new(&mut self.package_backup_retention)
                            .range(1..=MAX_PACKAGE_BACKUP_RETENTION),
                    ),
                    "Package backups to keep",
                );
                // Saves when a drag or text edit ends.
                backups_changed |= retention.drag_stopped()
                    || retention.lost_focus()
                    || (retention.changed() && !retention.dragged() && !retention.has_focus());
                ui.label("automatic package backups");
            });
            backups_changed |= ui
                .checkbox(
                    &mut self.backup_recipe_snapshots,
                    "Include Recipe Snapshots in Package Backups",
                )
                .on_hover_text("Saves the built recipes with each backup.")
                .changed();
            if backups_changed {
                self.save_workbench_preferences();
            }
            if changed {
                self.invalidate_results();
            }
        });
        ui.add_space(12.0);
        preference_heading(ui, "Installed Custom Packages");
        ui.label("Removes installed custom packages, and optionally their items and progression. Backed up first.");
        if ui
            .add_enabled(
                editable
                    && !self.has_background_work()
                    && !self.perk_workbench.editing()
                    && self.icon_editor.is_none()
                    && !self.presentation_editor.editing()
                    && !self.build_status_open,
                egui::Button::new("Uninstall Custom Packages…"),
            )
            .on_disabled_hover_text(
                "Finish the current operation and close its review window first.",
            )
            .clicked()
        {
            self.start_uninstall_review(false);
        }
    }

    pub(super) fn draw_activity_log_window(&mut self, ctx: &egui::Context) {
        if !self.activity_log_open {
            return;
        }
        let mut open = true;
        egui::Window::new("Activity Log")
            .id(egui::Id::new("parhelion-activity-log"))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .min_width(360.0)
            .default_width(720.0)
            .default_height(480.0)
            .show(ctx, |ui| {
                workbench_style(ui);
                ui.horizontal_wrapped(|ui| {
                    ui.label(format!("{} recent events · newest first", self.log.len()))
                        .on_hover_text(format!(
                            "Latest {ACTIVITY_LOG_CAPACITY} events. Times in UTC"
                        ));
                    if ui.button("Copy Log").clicked() {
                        ui.ctx().copy_text(self.activity_log_text());
                    }
                    if ui.button("Open Log Folder").clicked() {
                        self.log.file.open_folder();
                    }
                });
                if let Some(error) = self.log.file.error() {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                }
                ui.separator();
                egui::ScrollArea::vertical()
                    .id_salt("parhelion-activity-log-entries")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for entry in self.log.iter().rev() {
                            let color = if entry.error {
                                ui.visuals().error_fg_color
                            } else {
                                ui.visuals().text_color()
                            };
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(activity_entry_text(entry)).color(color),
                                )
                                .wrap()
                                .selectable(true),
                            );
                            ui.separator();
                        }
                    });
            });
        self.activity_log_open = open;
    }

    pub(super) fn activity_log_text(&self) -> String {
        self.log
            .iter()
            .rev()
            .map(activity_entry_text)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn draw_preference_path(ui: &mut egui::Ui, path: &Path) {
    ui.add(
        egui::Label::new(egui::RichText::new(path.display().to_string()).monospace())
            .wrap()
            .selectable(true),
    );
}

fn activity_entry_text(entry: &LogEntry) -> String {
    entry.formatted()
}

fn preference_heading(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let style = egui::TextStyle::Name("Section Heading".into());
    let text = egui::RichText::new(text).strong();
    let text = if ui.style().text_styles.contains_key(&style) {
        text.text_style(style)
    } else {
        text
    };
    ui.label(text)
}

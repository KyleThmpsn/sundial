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
    pub(super) fn draw_preferences_window(&mut self, ctx: &egui::Context) {
        if !self.preferences_open {
            return;
        }
        let mut open = true;
        let mut done = false;
        egui::Window::new("Parhelion Preferences")
            .id(egui::Id::new("parhelion-preferences"))
            .collapsible(false)
            .resizable(true)
            .min_width(420.0)
            .default_width(660.0)
            .default_height(460.0)
            .open(&mut open)
            .show(ctx, |ui| {
                workbench_style(ui);
                ui.horizontal_wrapped(|ui| {
                    for page in PreferencesPage::ALL {
                        ui.selectable_value(&mut self.preferences_page, page, page.label());
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical()
                    .id_salt(("parhelion-preferences-content", self.preferences_page))
                    .max_height((ui.available_height() - 65.0).max(100.0))
                    .auto_shrink([false, false])
                    .show(ui, |ui| self.draw_preferences_page(ui));
                ui.separator();
                ui.label("Changes apply immediately.");
                ui.horizontal(|ui| {
                    if ui.button("Activity Log…").clicked() {
                        self.activity_log_open = true;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        done = ui.button("Done").clicked();
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
        ui.heading("Editor");
        let mut show = self.show_experimental_options;
        if ui
            .checkbox(&mut show, "Show advanced technical controls (experimental)")
            .changed()
        {
            self.set_show_experimental_options(show);
        }
        ui.label("Shows detailed behavior, inventory, socket, material, and raw package fields. Existing overrides stay active when these controls are hidden.");
        ui.label(
            "Build validation checks the package data. Test new gameplay combinations in game.",
        );
        ui.separator();
        let editable = self.package_preferences_editable();
        ui.heading("Recipe Library");
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
            ui.label("Recipe library unavailable. Check the activity log for details.");
        }
        ui.separator();
        ui.heading("Weapon Catalog");
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
        ui.label("Reload weapon data after changing the installed packages.");
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
            ui.label("Build and backup options are locked during a package operation or installation review.");
        }
        ui.add_enabled_ui(editable, |ui| {
            ui.heading("Build Files");
            ui.label("Game packages · selected in Sundial");
            draw_preference_path(ui, &self.packages);
            let mut changed = path_row(ui, "Staging Folder", &mut self.staging, "Each build creates a separate folder here. Staging does not install packages.");
            changed |= ui.checkbox(&mut self.ignore_installed, "Build from a temporary stock package view").changed();
            ui.label("Ignores recognized Parhelion overlays while building. Installed packages are not moved or changed.");
            ui.separator();
            ui.heading("Package Backups");
            changed |= path_row(ui, "Backup Folder", &mut self.backup_root, "Installation backs up affected authored packages here before replacing them.");
            let mut backups_changed = false;
            ui.horizontal_wrapped(|ui| {
                backups_changed |= ui.checkbox(&mut self.limit_package_backups, "Keep last").changed();
                backups_changed |= named_control(ui.add_enabled(self.limit_package_backups,
                    egui::DragValue::new(&mut self.package_backup_retention).range(1..=MAX_PACKAGE_BACKUP_RETENTION)), "Package backups to keep").changed();
                ui.label("automatic package backups");
            });
            backups_changed |= ui.checkbox(&mut self.backup_recipe_snapshots, "Include recipe snapshots in package backups")
                .on_hover_text("Copies the normalized recipes recorded by the staged manifest into the matching backup generation.").changed();
            ui.label("The installer only prunes its own automatic package backups. Manually named snapshots are preserved. Settings backups are managed separately in Sundial.");
            if backups_changed {
                self.save_backup_preferences();
            }
            if changed {
                self.invalidate_results();
            }
        });
        ui.separator();
        ui.heading("Installed Custom Packages");
        ui.label("Remove Parhelion's installed package set. The review lets you also remove its items and progression from the selected account. Stock packages, recipes and unrelated account data are kept, with a recovery backup.");
        if ui
            .add_enabled(
                editable
                    && !self.has_background_work()
                    && self.perk_editor.is_none()
                    && self.icon_editor.is_none()
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
                    ui.label(format!("{} recent events · newest first", self.log.len()));
                    if ui.button("Copy Log").clicked() {
                        ui.ctx().copy_text(self.activity_log_text());
                    }
                    if ui.button("Open Log Folder").clicked() {
                        self.log.file.open_folder();
                    }
                });
                ui.label(format!(
                    "Latest {ACTIVITY_LOG_CAPACITY} events · timestamps in UTC."
                ));
                ui.label("Log files keep recent sessions: 5 MB each, with two older files.");
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

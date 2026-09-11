//! Runtime comparison and explicit selection. Inspection alone never changes files.
use super::{
    SundialApp, account_workspace::WorkspaceDocument, platform, preferences::SettingsLayout,
};
use crate::package_runtime::installation::{
    RuntimeCopy, RuntimeInspection, RuntimeLocation, RuntimeRestorePlan, SettingsResetPlan,
    archive_other_runtime, preview_runtime_restore, restore_runtime,
};
use eframe::egui;

#[derive(Default)]
pub(super) struct RuntimeChoice {
    pub inspection: RuntimeInspection,
    pub open: bool,
    pub error: Option<String>,
    pub pending_restore: Option<RuntimeRestorePlan>,
    pub pending_defaults: Option<SettingsResetPlan>,
}

impl RuntimeChoice {
    pub(super) fn inspect(install: &std::path::Path) -> Self {
        let inspection = RuntimeInspection::inspect(install);
        Self {
            open: inspection.duplicates(),
            inspection,
            error: None,
            pending_restore: None,
            pending_defaults: None,
        }
    }
}

impl SundialApp {
    pub(super) fn refresh_runtime_inspection(&mut self) {
        let inspection = RuntimeInspection::inspect(&self.install_path);
        if inspection.duplicates() && !self.runtime_choice.inspection.duplicates() {
            self.runtime_choice.open = true;
        }
        self.sunrise_version = inspection
            .copies
            .first()
            .and_then(|copy| copy.version.clone())
            .unwrap_or_else(|| "Not detected".into());
        self.runtime_choice.inspection = inspection;
    }

    pub(super) fn draw_runtime_banner(&mut self, ctx: &egui::Context) {
        if let Some(problem) = self
            .runtime_choice
            .inspection
            .for_settings(&self.settings_path)
            .and_then(|copy| {
                copy.persistence_problem(self.document.json()).or_else(|| {
                    copy.dawn_runtime
                        .as_ref()
                        .and_then(|dawn| dawn.validate(self.document.json()).err())
                })
            })
        {
            egui::TopBottomPanel::top("runtime_account_schema_warning").show(ctx, |ui| {
                ui.colored_label(ui.visuals().warn_fg_color, "Runtime Compatibility");
                ui.label(problem);
            });
        }
        if self.runtime_choice.inspection.duplicates() {
            egui::TopBottomPanel::top("duplicate_runtime_warning").show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(ui.visuals().warn_fg_color, "Two Runtime Copies");
                    ui.label("steam_api64.dll exists in the game folder and bin/x64.");
                    if ui.button("Review Copies").clicked() {
                        self.refresh_runtime_inspection();
                        self.runtime_choice.open = true;
                    }
                });
            });
        }
    }

    pub(super) fn draw_runtime_preferences(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            if ui.button("Recheck Runtime Copies").clicked() {
                self.refresh_runtime_inspection();
            }
            if ui.button("Restore Runtime Backup…").clicked() {
                self.request_runtime_restore();
            }
            if self.runtime_choice.inspection.duplicates()
                && ui.button("Choose Runtime Copy…").clicked()
            {
                self.refresh_runtime_inspection();
                self.runtime_choice.open = true;
            }
        });
        for copy in &self.runtime_choice.inspection.copies {
            ui.label(format!(
                "{}: Sunrise {}",
                copy.location.label(),
                copy.version.as_deref().unwrap_or("version unavailable")
            ));
            for detail in &copy.compatibility {
                ui.label(detail);
            }
            if let Some(problem) = &copy.selection_problem {
                ui.colored_label(ui.visuals().warn_fg_color, problem);
            }
        }
    }

    pub(super) fn draw_runtime_choice(&mut self, ctx: &egui::Context) {
        if self.runtime_choice.pending_defaults.is_some() {
            self.draw_runtime_defaults(ctx);
            return;
        }
        if self.runtime_choice.pending_restore.is_some() {
            self.draw_runtime_restore(ctx);
            return;
        }
        if !self.runtime_choice.open
            || self.confirmation.is_some()
            || self.pending_install_choice.is_some()
        {
            return;
        }
        let mut selected = None;
        let mut reset = None;
        let mut refresh = false;
        let mut close = false;
        let blocked = self.runtime_choice_blocker();
        let response = egui::Modal::new("choose_runtime_copy".into()).show(ctx, |ui| {
            ui.set_width((ctx.screen_rect().width() - 64.0).clamp(240.0, 740.0));
            ui.heading("Choose a Runtime Copy");
            egui::ScrollArea::vertical().max_height((ctx.screen_rect().height() - 145.0).max(150.0)).show(ui, |ui| {
                ui.label("Choose the settings and saved data you want to keep. The game-folder DLL takes precedence when both copies are present.");
                let copies = &self.runtime_choice.inspection.copies;
                if copies.len() == 2 && copies[0].dll_hash.is_some() && copies[0].dll_hash == copies[1].dll_hash {
                    ui.add_space(6.0);
                    ui.weak("Identical DLLs • Settings and saved data may differ");
                }
                for copy in copies {
                    ui.add_space(8.0);
                    egui::Frame::group(ui.style()).inner_margin(12.0).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        draw_copy(ui, copy);
                        ui.add_space(8.0);
                        let enabled = blocked.is_none() && copy.selection_problem.is_none() && copies.len() == 2;
                        let label = match copy.location {
                            RuntimeLocation::Root => "Keep Game-Folder Copy",
                            RuntimeLocation::BinX64 => "Keep bin/x64 Copy",
                        };
                        ui.horizontal_wrapped(|ui| {
                            if ui.add_enabled(enabled, egui::Button::new(label).wrap()).clicked() { selected = Some(copy.location); }
                            if ui.add_enabled(blocked.is_none() && copy.bundled_schema.is_some() && copy.settings_hash.is_some(), egui::Button::new("Restore Default Settings…")).clicked() { reset = Some(copy.clone()); }
                        });
                    });
                }
                ui.add_space(8.0);
                ui.label("Keeping a copy backs up the other DLL and its Sunrise folder to .sunrise/backups. You can undo this with Restore Runtime Backup in Installation preferences.");
                if let Some(reason) = blocked { ui.colored_label(ui.visuals().warn_fg_color, reason); }
                if let Some(error) = &self.runtime_choice.error { ui.colored_label(ui.visuals().error_fg_color, error); }
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                refresh = ui.button("Refresh Comparison").clicked();
                close = ui.button("Decide Later").clicked();
            });
        });
        if close || response.should_close() {
            self.runtime_choice.open = false;
        }
        if refresh {
            self.refresh_runtime_inspection();
            self.runtime_choice.error = None;
        }
        if let Some(copy) = reset {
            match SettingsResetPlan::prepare(&self.install_path, &copy) {
                Ok(plan) => {
                    self.runtime_choice.pending_defaults = Some(plan);
                    self.runtime_choice.error = None;
                }
                Err(error) => self.runtime_choice.error = Some(error),
            }
        }
        if let Some(keep) = selected {
            self.select_runtime_copy(keep);
        }
    }

    fn draw_runtime_defaults(&mut self, ctx: &egui::Context) {
        let Some(plan) = self.runtime_choice.pending_defaults.clone() else {
            return;
        };
        let blocked = self.runtime_choice_blocker();
        let mut apply = false;
        let mut cancel = false;
        let response = egui::Modal::new("restore_runtime_defaults".into()).show(ctx, |ui| {
            ui.set_width((ctx.screen_rect().width() - 64.0).clamp(240.0, 580.0));
            ui.heading("Restore Default Settings");
            egui::ScrollArea::vertical().max_height((ctx.screen_rect().height() - 180.0).max(120.0)).show(ui, |ui| {
                ui.label(format!("Restore settings v{} bundled with Sunrise {} in {}?", plan.schema,
                    plan.copy().version.as_deref().unwrap_or("Unavailable"), plan.copy().location.label()));
                ui.add_space(8.0);
                ui.label(plan.copy().settings_path.display().to_string());
                ui.label("The current settings.json will be backed up to .sunrise/backups before replacement. This does not choose or remove either runtime copy.");
                if plan.schema < 18 {
                    ui.colored_label(ui.visuals().warn_fg_color, "These defaults include the JSON account. Its saved progress and unlocks will return to defaults.");
                    ui.label("If you have custom Parhelion packages installed, reinstall them afterward to restore their Collections entries and unlocks.");
                } else {
                    ui.label("The account in investment.sqlite3 is preserved.");
                }
                if let Some(reason) = blocked { ui.colored_label(ui.visuals().warn_fg_color, reason); }
                if let Some(error) = &self.runtime_choice.error { ui.colored_label(ui.visuals().error_fg_color, error); }
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                apply = ui.add_enabled(blocked.is_none(), egui::Button::new("Back Up and Restore Defaults")).clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
        if cancel || response.should_close() {
            self.runtime_choice.pending_defaults = None;
        }
        if apply && self.runtime_choice_blocker().is_none() {
            match plan.apply(|| {
                if platform::destiny_is_running()? {
                    Err("Close Destiny 2 before restoring settings.".into())
                } else {
                    Ok(())
                }
            }) {
                Ok(backup) => {
                    self.runtime_choice.pending_defaults = None;
                    self.runtime_choice.error = None;
                    if self.settings_path == plan.copy().settings_path {
                        self.reload();
                    }
                    self.refresh_runtime_inspection();
                    self.set_status(
                        format!(
                            "Restored this DLL's default settings. Previous settings: {}",
                            backup.display()
                        ),
                        false,
                    );
                }
                Err(error) => self.runtime_choice.error = Some(error),
            }
        }
    }

    fn runtime_choice_blocker(&self) -> Option<&'static str> {
        if self.has_unsaved_changes() {
            Some("Save or reload your changes before choosing a runtime.")
        } else if self.package_authoring_open || self.package_authoring_busy {
            Some("Close Parhelion before choosing a runtime.")
        } else if self.catalog_task.is_some() {
            Some("Wait for the installation to finish loading.")
        } else {
            None
        }
    }

    fn request_runtime_restore(&mut self) {
        let Some(backup) = rfd::FileDialog::new()
            .set_title("Select a Runtime Backup Folder")
            .set_directory(self.install_path.join(".sunrise/backups"))
            .pick_folder()
        else {
            return;
        };
        match preview_runtime_restore(&self.install_path, &backup) {
            Ok(plan) => {
                self.runtime_choice.pending_restore = Some(plan);
                self.runtime_choice.error = None;
            }
            Err(error) => self.set_status(format!("Runtime backup not selected: {error}"), true),
        }
    }

    fn draw_runtime_restore(&mut self, ctx: &egui::Context) {
        if self.confirmation.is_some() {
            return;
        }
        let Some(plan) = self.runtime_choice.pending_restore.clone() else {
            return;
        };
        let blocked = self.runtime_choice_blocker();
        let mut restore = false;
        let mut cancel = false;
        let response = egui::Modal::new("restore_runtime_backup".into()).show(ctx, |ui| {
            ui.set_width((ctx.screen_rect().width() - 64.0).clamp(240.0, 620.0));
            ui.heading("Restore Runtime Backup");
            egui::ScrollArea::vertical().max_height((ctx.screen_rect().height() - 145.0).max(150.0)).show(ui, |ui| {
                ui.label(format!("Backup: {}", plan.backup.display()));
                ui.label("These archived files and folders will move back to their original locations:");
                for path in &plan.paths { ui.label(path.display().to_string()); }
                ui.label("Existing files will not be replaced. Restoring a second runtime copy will reopen the runtime comparison.");
                if let Some(reason) = blocked { ui.colored_label(ui.visuals().warn_fg_color, reason); }
                if let Some(error) = &self.runtime_choice.error { ui.colored_label(ui.visuals().error_fg_color, error); }
            });
            ui.horizontal(|ui| {
                restore = ui.add_enabled(blocked.is_none(), egui::Button::new("Restore Runtime Files")).clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
        if cancel || response.should_close() {
            self.runtime_choice.pending_restore = None;
        }
        if restore {
            match restore_runtime(&plan, || {
                if platform::destiny_is_running()? {
                    Err("Close Destiny 2 before restoring runtime files.".into())
                } else {
                    Ok(())
                }
            }) {
                Ok(()) => {
                    self.runtime_choice.pending_restore = None;
                    self.runtime_choice.error = None;
                    self.refresh_sunrise_version();
                    self.set_status("Runtime backup restored. Review the runtime copies before starting Destiny 2.", false);
                }
                Err(error) => {
                    self.runtime_choice.error = Some(error.clone());
                    self.set_status(error, true);
                }
            }
        }
    }

    fn select_runtime_copy(&mut self, keep: RuntimeLocation) {
        if let Some(reason) = self.runtime_choice_blocker() {
            self.runtime_choice.error = Some(reason.into());
            return;
        }
        let path = keep
            .directory(&self.install_path)
            .join("Sunrise/settings.json");
        // Resolve the chosen document before touching files. The archive operation rechecks its hash.
        let json = match super::settings::load_workspace_json(&path) {
            Ok(value) => value,
            Err(error) => {
                self.runtime_choice.error = Some(error);
                return;
            }
        };
        let result = archive_other_runtime(
            &self.install_path,
            &self.runtime_choice.inspection,
            keep,
            || {
                if platform::destiny_is_running()? {
                    Err("Close Destiny 2 before choosing a runtime copy.".into())
                } else {
                    Ok(())
                }
            },
        );
        match result {
            Ok(backup) => {
                self.settings_path = path;
                self.settings_layout = match keep {
                    RuntimeLocation::Root => SettingsLayout::Root,
                    RuntimeLocation::BinX64 => SettingsLayout::BinX64,
                };
                self.persistence_compatibility =
                    super::persistence_compatibility::PersistenceCompatibility::inspect(
                        &self.install_path,
                    );
                self.replace_loaded_document(WorkspaceDocument::load(json, &self.settings_path));
                self.runtime_choice.open = false;
                self.runtime_choice.error = None;
                let preference_error = self.save_preferences().err();
                let note = preference_error.as_ref().map_or(String::new(), |e| {
                    format!(" The choice could not be remembered: {e}")
                });
                self.set_status(
                    format!(
                        "Runtime selected. The other copy is backed up at {}.{note}",
                        backup.display()
                    ),
                    preference_error.is_some(),
                );
            }
            Err(error) => {
                self.runtime_choice.error = Some(error.clone());
                self.set_status(error, true);
            }
        }
    }
}

fn draw_copy(ui: &mut egui::Ui, copy: &RuntimeCopy) {
    ui.horizontal_wrapped(|ui| {
        ui.strong(copy.location.label());
        ui.weak(format!(
            "Sunrise {}",
            copy.version.as_deref().unwrap_or("Unavailable")
        ));
    });
    ui.add_space(4.0);
    ui.label(format!(
        "Settings {}  •  DLL Defaults {}",
        copy.schema
            .map_or("Unavailable".into(), |v| format!("v{v}")),
        copy.bundled_schema
            .map_or("Unavailable".into(), |v| format!("v{v}"))
    ));
    if let Some(problem) = &copy.selection_problem {
        ui.colored_label(ui.visuals().warn_fg_color, problem);
        ui.weak("Restore this DLL's default settings to resolve a schema mismatch.");
    } else {
        ui.weak("Available to keep");
    }
    ui.push_id(copy.location.label(), |ui| {
        ui.collapsing("File Details", |ui| {
            ui.label(format!("DLL: {}", copy.dll_path.display()));
            ui.label(format!("Modified: {}", copy.dll_modified));
            ui.label(format!("Settings: {}", copy.settings_path.display()));
            ui.label(format!("Modified: {}", copy.settings_modified));
            for detail in &copy.compatibility {
                ui.label(detail);
            }
        });
    });
}

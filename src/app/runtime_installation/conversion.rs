//! Opt-in conversion and recovery. Previewing only writes temporary files.
mod plan;
mod recovery;
mod storage;
#[cfg(test)]
mod tests;

use super::*;
use crate::{app::settings, persistence::sqlite_account as native};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
};

pub(super) enum Dialog {
    Convert {
        plan: Box<plan::Plan>,
        accepted: bool,
    },
    Restore(Box<recovery::Plan>),
}

impl SundialApp {
    pub(super) fn draw_conversion_button(&mut self, ui: &mut egui::Ui) {
        if self
            .runtime_choice
            .inspection
            .launch_copy()
            .is_some_and(|copy| plan::supported(&self.document, copy))
            && ui.button("Convert Account…").clicked()
        {
            self.request_account_conversion();
        }
    }

    fn conversion_blocker(&self) -> Option<&'static str> {
        if self.json_editor.has_unapplied_changes() {
            Some("Apply or discard the JSON editor changes first.")
        } else if self.package_authoring_open || self.package_authoring_busy {
            Some("Close Parhelion before converting an account.")
        } else if self.catalog_task.is_some() {
            Some("Wait for the installation to finish loading.")
        } else {
            None
        }
    }

    fn request_account_conversion(&mut self) {
        let result = (|| {
            if let Some(reason) = self.conversion_blocker() {
                return Err(reason.into());
            }
            self.refresh_runtime_inspection();
            let target = self
                .runtime_choice
                .inspection
                .launch_copy()
                .ok_or("The runtime is missing.")?
                .clone();
            let defaults = settings::load_installed_sunrise_defaults(&self.install_path)?;
            let native_defaults = if target.bundled_schema == Some(18) {
                Some(settings::load_installed_account_defaults(
                    &self.install_path,
                )?)
            } else {
                None
            };
            plan::Plan::prepare(
                &self.install_path,
                &self.settings_path,
                &self.document,
                &self.persisted_document,
                target,
                defaults,
                native_defaults.as_ref(),
            )
        })();
        match result {
            Ok(plan) => {
                self.runtime_choice.pending_conversion = Some(Dialog::Convert {
                    plan: Box::new(plan),
                    accepted: false,
                });
                self.runtime_choice.error = None;
            }
            Err(error) => self.set_status(format!("Conversion unavailable: {error}"), true),
        }
    }

    pub(super) fn request_conversion_restore(&mut self) {
        let Some(folder) = rfd::FileDialog::new()
            .set_title("Select an Account Conversion Backup")
            .set_directory(self.install_path.join(".sunrise/backups"))
            .pick_folder()
        else {
            return;
        };
        match recovery::Plan::prepare(&self.install_path, &folder) {
            Ok(plan) => {
                self.runtime_choice.pending_conversion = Some(Dialog::Restore(Box::new(plan)));
                self.runtime_choice.error = None;
            }
            Err(error) => self.set_status(error, true),
        }
    }

    pub(super) fn draw_account_conversion(&mut self, ctx: &egui::Context) {
        if self.confirmation.is_some() || self.pending_install_choice.is_some() {
            return;
        }
        let Some(mut dialog) = self.runtime_choice.pending_conversion.take() else {
            return;
        };
        let blocked = match &dialog {
            Dialog::Convert { plan, .. } => self.conversion_blocker().or_else(|| {
                (self.document != plan.document)
                    .then_some("The account changed. Cancel and review the conversion again.")
            }),
            Dialog::Restore(_) => self.runtime_choice_blocker(),
        };
        let mut apply = false;
        let mut cancel = false;
        let response = egui::Modal::new("account_conversion".into()).show(ctx, |ui| {
            ui.set_width((ctx.screen_rect().width() - 64.0).clamp(240.0, 620.0));
            let ready = match &mut dialog {
                Dialog::Convert { plan, accepted } => {
                    ui.heading("Experimental Conversion");
                    egui::ScrollArea::vertical().max_height((ctx.screen_rect().height() - 250.0).max(120.0)).show(ui, |ui| {
                        ui.strong(format!("Convert to {} Settings v{}", plan.target.name(), plan.target.bundled_schema.unwrap_or_default()));
                        ui.label("Conversion may lose data or fail in game. You may need to restore the backup.");
                        ui.label(format!("Save to: {}", plan.target.settings_path.display()));
                        ui.add_space(8.0);
                        for note in &plan.notes { ui.label(format!("• {note}")); }
                        ui.add_space(8.0);
                        ui.label("Your account, including unsaved edits, is backed up to .sunrise/backups first. To undo conversion, use Restore Conversion Backup in Installation preferences.");
                    });
                    ui.checkbox(accepted, "I Understand This Is Experimental");
                    *accepted
                }
                Dialog::Restore(plan) => {
                    ui.heading("Restore Conversion Backup");
                    ui.label(format!("Backup: {}", plan.folder.display()));
                    ui.label(format!("Restore: {}", plan.receipt.install.join(&plan.receipt.target_path).display()));
                    ui.label("This replaces progress made since conversion. Your current files will be backed up first. Switch back to the original runtime before starting the game.");
                    true
                }
            };
            if let Some(reason) = blocked { ui.colored_label(ui.visuals().warn_fg_color, reason); }
            if let Some(error) = &self.runtime_choice.error { ui.colored_label(ui.visuals().error_fg_color, error); }
            ui.horizontal_wrapped(|ui| {
                let label = if matches!(dialog, Dialog::Convert { .. }) { "Back Up and Convert" } else { "Restore Backup" };
                apply = ui.add_enabled(ready && blocked.is_none(), egui::Button::new(label)).clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
        if cancel || response.should_close() {
            return;
        }
        if apply {
            let result = match &dialog {
                Dialog::Convert { plan, .. } => plan.apply(check_game_closed).map(|backup| (plan.receipt.target_path.clone(), format!("Converted to {} settings v{}. Backup: {}", plan.target.name(), plan.target.bundled_schema.unwrap_or_default(), backup.display()))),
                Dialog::Restore(plan) => plan.apply(check_game_closed).map(|backup| (plan.receipt.source_path.clone(), format!("Conversion backup restored. Switch back to the original runtime before starting the game.{}", backup.map_or(String::new(), |path| format!(" Previous files: {}", path.display()))))),
            };
            match result {
                Ok((relative, message)) => {
                    self.settings_path = self.install_path.join(&relative);
                    if let Some(layout) = SettingsLayout::ALL
                        .into_iter()
                        .find(|layout| layout.relative_path() == relative)
                    {
                        self.settings_layout = layout;
                    }
                    self.runtime_choice.error = None;
                    self.runtime_choice.open = false;
                    if self.reload() {
                        self.refresh_runtime_inspection();
                        match self.save_preferences() {
                            Ok(()) => self.set_status(message, false),
                            Err(error) => self.set_status(
                                format!("{message} Could not remember the settings path: {error}"),
                                true,
                            ),
                        }
                    }
                    return;
                }
                Err(error) => self.runtime_choice.error = Some(error),
            }
        }
        self.runtime_choice.pending_conversion = Some(dialog);
    }
}

fn check_game_closed() -> Result<(), String> {
    if platform::destiny_is_running()? {
        Err("Close Destiny 2 before converting or restoring an account.".into())
    } else {
        Ok(())
    }
}

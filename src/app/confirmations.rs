//! Confirmation dialogs for explicit user actions.
mod parhelion;
use super::account_workspace::AccountSourceKind;
use super::change_review::collect_change_summaries;
use super::preferences::{PlugSelectionMode, SettingsLayout};
use super::save_support::SaveAction;
use super::settings::settings_path_for_install;
use super::{
    CHANGE_REVIEW_LIMIT, ConfirmationDialog, SundialApp, draw_future_schema_warning, equipment,
};
use eframe::egui;

impl SundialApp {
    pub(super) fn draw_pending_install_choice(&mut self, ctx: &egui::Context) {
        if let Some(install_path) = self.pending_install_choice.clone() {
            let mut selected = None;
            let mut cancel = false;
            let response = egui::Modal::new("choose_sunrise_settings".into()).show(ctx, |ui| {
                ui.set_width(500.0);
                ui.heading("Choose Sunrise Settings");
                ui.add_space(6.0);
                ui.label("Multiple settings.json files were found. Choose the one Project Sunrise uses for this installation.");
                ui.add_space(10.0);
                for layout in SettingsLayout::ALL {
                    let path = settings_path_for_install(&install_path, layout);
                    if !path.is_file() {
                        continue;
                    }
                    if ui
                        .button(format!("Use {}", layout.relative_path().display()))
                        .clicked()
                    {
                        selected = Some((layout, path.clone()));
                    }
                    ui.label(
                        egui::RichText::new(path.display().to_string())
                            .weak()
                            .small(),
                    );
                    ui.add_space(8.0);
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
            cancel |= response.should_close();
            if let Some((layout, path)) = selected {
                self.pending_install_choice = None;
                self.load_install(ctx, install_path, path, layout);
            } else if cancel {
                self.pending_install_choice = None;
            }
        }
    }

    pub(super) fn draw_future_schema_confirmation(&mut self, ctx: &egui::Context) {
        if let Some(pending) = self.pending_future_schema.clone() {
            let mut proceed = false;
            let mut cancel = false;
            let response = egui::Modal::new("future_schema_warning".into()).show(ctx, |ui| {
                ui.set_width(500.0);
                draw_future_schema_warning(ui, &pending);
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.button("Proceed with Caution").clicked() {
                        proceed = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
            cancel |= response.should_close();
            self.pending_future_schema = if !proceed && !cancel {
                Some(pending.clone())
            } else {
                None
            };
            if proceed {
                self.load_future_schema_install(ctx, pending);
            }
        }
    }

    pub(super) fn draw_reset_defaults_confirmation(&mut self, ctx: &egui::Context) {
        if self.confirmation == Some(ConfirmationDialog::ResetDefaults) {
            let mut reset = false;
            let mut cancel = false;
            let account_source = self.document.source_info().kind;
            let response = egui::Modal::new("restore_sunrise_defaults".into()).show(ctx, |ui| {
                ui.set_width(500.0);
                ui.heading("Restore Sunrise Defaults?");
                ui.add_space(6.0);
                ui.label(match account_source {
                    AccountSourceKind::Json => {
                        "This replaces the entire settings.json with the default bundled in your installed Project Sunrise version."
                    }
                    AccountSourceKind::Sqlite | AccountSourceKind::Blocked => {
                        "This restores bundled settings.json defaults while preserving its inactive legacy /state/account and /state/characters data. It does not change investment.sqlite3."
                    }
                });
                ui.add_space(6.0);
                ui.label("Your current file will be preserved as settings.json.bak and as a timestamped Sundial backup. Any unsaved changes will be discarded.");
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(self.settings_path.display().to_string())
                        .monospace()
                        .color(super::ui::secondary_text_color(ui)),
                );
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.button("Restore Defaults").clicked() {
                        reset = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
            cancel |= response.should_close();
            self.confirmation = (!reset && !cancel).then_some(ConfirmationDialog::ResetDefaults);
            if reset {
                self.reset_to_sunrise_defaults();
            }
        }
    }

    pub(super) fn draw_sqlite_restore_confirmation(&mut self, ctx: &egui::Context) {
        if self.confirmation == Some(ConfirmationDialog::RestoreSqliteBackup) {
            if let Some(backup) = self.pending_sqlite_restore.clone() {
                let mut restore = false;
                let mut cancel = false;
                let response =
                    egui::Modal::new("restore_sqlite_backup".into()).show(ctx, |ui| {
                        ui.set_width(560.0);
                        ui.heading("Restore this account database backup?");
                        ui.add_space(6.0);
                        ui.label("Sundial will replace investment.sqlite3 with the selected compatible backup. Before replacement, it creates and integrity-checks a recovery snapshot of the current database.");
                        ui.add_space(6.0);
                        ui.label("Destiny 2 must be closed. Any unsaved Sundial changes will be discarded after the restored workspace reloads. settings.json is not changed or synchronized.");
                        ui.add_space(8.0);
                        ui.label(egui::RichText::new("Selected backup").strong());
                        ui.label(
                            egui::RichText::new(backup.display().to_string())
                                .weak()
                                .small(),
                        );
                        ui.add_space(12.0);
                        ui.horizontal(|ui| {
                            if ui.button("Restore account database").clicked() {
                                restore = true;
                            }
                            if ui.button("Cancel").clicked() {
                                cancel = true;
                            }
                        });
                    });
                cancel |= response.should_close();
                if restore {
                    self.confirmation = None;
                    self.restore_selected_sqlite_backup();
                } else if cancel {
                    self.confirmation = None;
                    self.pending_sqlite_restore = None;
                    self.set_status("Database restore cancelled; no files were changed", false);
                }
            } else {
                self.confirmation = None;
            }
        }
    }

    pub(super) fn draw_unsafe_mode_confirmation(&mut self, ctx: &egui::Context) {
        if self.confirmation == Some(ConfirmationDialog::ReallyUnsafe) {
            let mut enable = false;
            let mut cancel = false;
            let account_source = self.document.source_info().kind;
            let response = egui::Modal::new("really_unsafe_confirmation".into()).show(ctx, |ui| {
                ui.set_width(500.0);
                ui.heading("Show all plugs?");
                ui.add_space(6.0);
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    "Incompatible plugs can prevent Destiny 2 from loading or cause crashes.",
                );
                ui.add_space(8.0);
                ui.label("All mode makes every discovered plug available in every socket, including combinations the item does not support.");
                ui.add_space(8.0);
                ui.label("Sundial backs up each account file before saving changes.");
                ui.label(match account_source {
                    AccountSourceKind::Json => {
                        "If the game no longer loads, use Preferences > Saving & Recovery to restore the installed Sunrise defaults. This resets the account. The current file is backed up first."
                    }
                    AccountSourceKind::Sqlite => {
                        "If an account edit prevents loading, use Preferences > Saving & Recovery to restore a verified account database backup. The current database is backed up first."
                    }
                    AccountSourceKind::Blocked => {
                        "Account editing is currently blocked, so Sundial will not write the incompatible investment.sqlite3."
                    }
                });
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.button("Show all plugs").clicked() {
                        enable = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
            cancel |= response.should_close();
            self.confirmation = (!enable && !cancel).then_some(ConfirmationDialog::ReallyUnsafe);
            if enable {
                self.plug_selection_mode = PlugSelectionMode::AnyPlug;
                if self.remember_plug_selection_mode_after_confirmation {
                    self.preferences.default_plug_selection_mode = PlugSelectionMode::AnyPlug;
                }
                self.preferences.really_unsafe_warning_acknowledged = true;
                self.remember_plug_selection_mode_after_confirmation = false;
                if let Err(error) = self.save_preferences() {
                    self.set_status(
                        format!(
                            "All plugs enabled, but the preference could not be saved: {error}"
                        ),
                        true,
                    );
                }
            } else if cancel {
                self.remember_plug_selection_mode_after_confirmation = false;
            }
        }
    }

    pub(super) fn draw_save_review_confirmation(&mut self, ctx: &egui::Context) {
        if self.confirmation == Some(ConfirmationDialog::ReviewSave) {
            let mut changes = self
                .document
                .account_change_summaries(&self.persisted_document, CHANGE_REVIEW_LIMIT + 1);
            if self.document.account_changed_from(&self.persisted_document) && changes.is_empty() {
                changes.push("investment.sqlite3: account data changed".to_owned());
            }
            if changes.len() <= CHANGE_REVIEW_LIMIT {
                changes.extend(collect_change_summaries(
                    self.persisted_document.json(),
                    self.document.json(),
                    CHANGE_REVIEW_LIMIT + 1 - changes.len(),
                ));
            }
            let truncated = changes.len() > CHANGE_REVIEW_LIMIT;
            changes.truncate(CHANGE_REVIEW_LIMIT);
            let total = changes.len();
            let mut confirm = false;
            let mut cancel = false;
            let action = self.pending_save_action.unwrap_or(SaveAction::Save);
            let review_width = (ctx.screen_rect().width() - 40.0).clamp(280.0, 760.0);
            let review_height = (ctx.screen_rect().height() - 180.0).clamp(120.0, 430.0);
            let response = egui::Modal::new("review_settings_changes".into()).show(ctx, |ui| {
                ui.set_width(review_width);
                ui.heading(if action == SaveAction::SaveAndExit {
                    "Review Changes Before Saving and Exiting"
                } else {
                    "Review Changes Before Saving"
                });
                ui.add_space(6.0);
                let source_label = match (
                    self.document.json_changed_from(&self.persisted_document),
                    self.document.account_changed_from(&self.persisted_document),
                ) {
                    (true, true) => "settings.json and investment.sqlite3",
                    (false, true) => "investment.sqlite3",
                    _ => "settings.json",
                };
                ui.label(if truncated {
                    format!(
                        "Reviewing the first {total} changes that will be written to {source_label}."
                    )
                } else {
                    format!(
                        "Sundial will write {total} change{} to {source_label}.",
                        if total == 1 { "" } else { "s" }
                    )
                });
                ui.add_space(8.0);
                egui::ScrollArea::vertical()
                    .id_salt("settings-change-review")
                    .max_height(review_height)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        for change in &changes {
                            ui.add(egui::Label::new(egui::RichText::new(change).monospace().size(13.0)).wrap().selectable(true));
                        }
                    });
                if truncated {
                    ui.label(
                        egui::RichText::new(
                            "The review is capped. Additional changed fields may not be listed.",
                        )
                        .color(super::ui::secondary_text_color(ui)),
                    );
                }
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    let save_label = if action == SaveAction::SaveAndExit {
                        "Save and Exit"
                    } else {
                        "Save Changes"
                    };
                    if ui.button(save_label).clicked() {
                        confirm = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
            cancel |= response.should_close();
            if confirm {
                self.confirmation = None;
                self.pending_save_action = None;
                self.perform_save_action(ctx, action);
            } else if cancel {
                self.confirmation = None;
                self.pending_save_action = None;
                self.set_status("Save cancelled. No files were changed", false);
            }
        }
    }

    pub(super) fn draw_delete_equipment_confirmation(&mut self, ctx: &egui::Context) {
        if self.confirmation == Some(ConfirmationDialog::DeleteEquipment) {
            let pending = self.pending_equipment_delete.clone();
            let mut delete = false;
            let mut cancel = false;
            if let Some(pending) = pending.as_ref() {
                let response = egui::Modal::new("delete_equipped_item".into()).show(ctx, |ui| {
                    ui.set_width(460.0);
                    ui.heading(format!("Delete {}?", pending.item_name));
                    ui.add_space(6.0);
                    ui.label(format!(
                        "This empties the {} slot and does not move the item to inventory.",
                        equipment::equipment_slot_label(&pending.slot)
                    ));
                    ui.label("You can Undo this change until the settings are saved.");
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if ui.button("Delete item").clicked() {
                            delete = true;
                        }
                        if ui.button("Cancel").clicked() {
                            cancel = true;
                        }
                    });
                });
                cancel |= response.should_close();
            } else {
                cancel = true;
            }
            if delete {
                if let Some(pending) = self.pending_equipment_delete.take() {
                    self.empty_weapon(pending.character_index, &pending.slot);
                }
                self.confirmation = None;
            } else if cancel {
                self.pending_equipment_delete = None;
                self.confirmation = None;
            }
        }
    }

    pub(super) fn draw_reload_confirmation(&mut self, ctx: &egui::Context) {
        if self.confirmation == Some(ConfirmationDialog::Reload) {
            let mut discard = false;
            let mut cancel = false;
            let response = egui::Modal::new("reload_confirmation".into()).show(ctx, |ui| {
                ui.heading("Discard Unsaved Changes?");
                ui.add_space(6.0);
                ui.label("Reloading will discard changes that have not been saved.");
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Discard and Reload").clicked() {
                        discard = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
            cancel |= response.should_close();
            self.confirmation = (!discard && !cancel).then_some(ConfirmationDialog::Reload);
            if discard {
                self.reload();
            }
        }
    }

    pub(super) fn draw_exit_confirmation(&mut self, ctx: &egui::Context) {
        if self.confirmation == Some(ConfirmationDialog::Exit) {
            let mut save_and_exit = false;
            let mut discard_and_exit = false;
            let mut cancel = false;
            let response = egui::Modal::new("exit_confirmation".into()).show(ctx, |ui| {
                ui.heading("Unsaved Changes");
                ui.add_space(6.0);
                ui.label("Save your changes before closing Sundial?");
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Save and Exit").clicked() {
                        save_and_exit = true;
                    }
                    if ui.button("Discard and Exit").clicked() {
                        discard_and_exit = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
            cancel |= response.should_close();
            self.confirmation = (!save_and_exit && !discard_and_exit && !cancel)
                .then_some(ConfirmationDialog::Exit);
            if save_and_exit {
                self.request_save(ctx, SaveAction::SaveAndExit);
            } else if discard_and_exit {
                if let Some(message) = self.package_authoring_exit_blocker() {
                    self.set_status(message, true);
                } else {
                    self.exit_confirmed = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
    }
}

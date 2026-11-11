//! Confirmation dialogs for explicit user actions.
mod parhelion;
mod recovery;
use super::account_workspace::{self as account, AccountSourceKind};
use super::change_review::collect_change_summaries;
use super::preferences::{PlugSelectionMode, SettingsLayout};
use super::saving::SaveAction;
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
                ui.label("Choose the settings.json Project Sunrise uses.");
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
        if matches!(self.confirmation, Some(ConfirmationDialog::ResetDefaults)) {
            let mut reset = false;
            let mut cancel = false;
            let account_source = self.document.source_kind();
            let runtime = self.runtime_name();
            // Dawn's account is player-state.db whatever its state, and Sunrise's investment.sqlite3.
            let database = if self.document.account_is_dawn() {
                crate::persistence::DAWN_DATABASE_NAME
            } else {
                "investment.sqlite3"
            };
            let response = egui::Modal::new("restore_sunrise_defaults".into()).show(ctx, |ui| {
                ui.set_width(500.0);
                ui.heading(format!("Restore {runtime} Defaults?"));
                ui.add_space(6.0);
                ui.label(match account_source {
                    AccountSourceKind::Json => {
                        format!("Replaces all of settings.json with your {runtime} defaults.")
                    }
                    AccountSourceKind::Sqlite | AccountSourceKind::Dawn | AccountSourceKind::Blocked => {
                        format!("Restores settings.json defaults and keeps its inactive /state/account and /state/characters data. {database} is unchanged.")
                    }
                });
                ui.add_space(6.0);
                ui.label("The current file is backed up to settings.json.bak and a Sundial backup. Unsaved changes are discarded.");
                recovery::draw_parhelion_reset_note(ui, account_source == AccountSourceKind::Json);
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
        let Some(ConfirmationDialog::RestoreSqliteBackup(backup)) = &self.confirmation else {
            return;
        };
        let mut restore = false;
        let mut cancel = false;
        let response = egui::Modal::new("restore_sqlite_backup".into()).show(ctx, |ui| {
            ui.set_width(560.0);
            ui.heading("Restore This Account Database Backup?");
            ui.add_space(6.0);
            ui.label("Replaces investment.sqlite3 with this backup. The current database is saved and checked first.");
            ui.add_space(6.0);
            ui.label("Close Destiny 2 first. Unsaved changes are discarded. settings.json is unchanged.");
            ui.add_space(8.0);
            ui.strong("Selected Backup");
            ui.label(
                egui::RichText::new(backup.display().to_string())
                    .weak()
                    .small(),
            );
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                restore = ui.button("Restore Account Database").clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
        cancel |= response.should_close();
        if restore
            && let Some(ConfirmationDialog::RestoreSqliteBackup(backup)) = self.confirmation.take()
        {
            self.restore_selected_sqlite_backup(backup);
        } else if cancel {
            self.confirmation = None;
            self.set_status("Database restore cancelled. No files were changed", false);
        }
    }

    pub(super) fn draw_unsafe_mode_confirmation(&mut self, ctx: &egui::Context) {
        if let Some(ConfirmationDialog::ReallyUnsafe { remember }) = self.confirmation {
            let mut enable = false;
            let mut cancel = false;
            let account_source = self.document.source_kind();
            let response = egui::Modal::new("really_unsafe_confirmation".into()).show(ctx, |ui| {
                ui.set_width(500.0);
                ui.heading("Show All Plugs?");
                ui.add_space(6.0);
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    "Incompatible plugs can prevent Destiny 2 from loading or cause crashes.",
                );
                ui.add_space(8.0);
                ui.label("All offers every plug in every socket, including unsupported ones.");
                ui.add_space(8.0);
                ui.label("Sundial backs up each account file before saving changes.");
                ui.label(match account_source {
                    AccountSourceKind::Json => {
                        "If the game stops loading, restore Sunrise defaults in Preferences > Saving & Recovery. This resets the account after backing it up."
                    }
                    AccountSourceKind::Sqlite => {
                        "If the game stops loading, restore an account backup in Preferences > Saving & Recovery. The current database is backed up first."
                    }
                    AccountSourceKind::Dawn => {
                        "Dawn account edits are saved to player-state.db."
                    }
                    AccountSourceKind::Blocked => {
                        "Account editing is blocked. The incompatible investment.sqlite3 is not written."
                    }
                });
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.button("Show All Plugs").clicked() {
                        enable = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
            cancel |= response.should_close();
            if enable || cancel {
                self.confirmation = None;
            }
            if enable {
                self.plug_selection_mode = PlugSelectionMode::AnyPlug;
                if remember {
                    self.preferences.default_plug_selection_mode = PlugSelectionMode::AnyPlug;
                }
                self.preferences.really_unsafe_warning_acknowledged = true;
                if let Err(error) = self.save_preferences() {
                    self.set_status(
                        format!(
                            "All plugs enabled, but the preference could not be saved: {error}"
                        ),
                        true,
                    );
                }
            }
        }
    }

    pub(super) fn draw_save_review_confirmation(&mut self, ctx: &egui::Context) {
        if let Some(ConfirmationDialog::ReviewSave(action)) = self.confirmation {
            let mut changes = self
                .document
                .account_change_summaries(&self.persisted_document, CHANGE_REVIEW_LIMIT + 1);
            if self.document.account_changed_from(&self.persisted_document) && changes.is_empty() {
                changes.push(format!(
                    "{}: account data changed",
                    self.document.source_info().label
                ));
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
            let review_width = (ctx.content_rect().width() - 40.0).clamp(280.0, 760.0);
            let review_height = (ctx.content_rect().height() - 180.0).clamp(120.0, 430.0);
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
                    (true, true) => {
                        format!("settings.json and {}", self.document.source_info().label)
                    }
                    (false, true) => self.document.source_info().label.to_owned(),
                    _ => "settings.json".to_owned(),
                };
                ui.label(if truncated {
                    format!("First {total} changes to {source_label}.")
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
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(change).monospace().size(13.0),
                                )
                                .wrap()
                                .selectable(true),
                            );
                        }
                    });
                if truncated {
                    ui.label(
                        egui::RichText::new("More changes are not listed.")
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
                self.perform_save_action(ctx, action);
            } else if cancel {
                self.confirmation = None;
                self.set_status("Save cancelled. No files were changed", false);
            }
        }
    }

    pub(super) fn draw_delete_equipment_confirmation(&mut self, ctx: &egui::Context) {
        let Some(ConfirmationDialog::DeleteEquipment(pending)) = &self.confirmation else {
            return;
        };
        let mut delete = false;
        let mut cancel = false;
        let response = egui::Modal::new("delete_equipped_item".into()).show(ctx, |ui| {
            ui.set_width(460.0);
            ui.heading(format!("Delete {}?", pending.item_name));
            ui.add_space(6.0);
            ui.label(format!(
                "Empties the {} slot. The item is not moved to inventory.",
                equipment::equipment_slot_label(pending.slot)
            ));
            ui.label("Undo works until you save.");
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                delete = ui.button("Delete Item").clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
        cancel |= response.should_close();
        if delete
            && let Some(ConfirmationDialog::DeleteEquipment(pending)) = self.confirmation.take()
        {
            if self.settings_path == pending.settings_path
                && account::character_soid(&self.document, pending.character_index)
                    == pending.character_soid
                && account::equipped_item_snapshots(&self.document, pending.character_index)
                    .is_ok_and(|items| {
                        items.into_iter().any(|item| {
                            item.slot == pending.slot
                                && item.instance_soid == Some(pending.instance_soid)
                                && item.definition_hash == Some(pending.definition_hash)
                        })
                    })
            {
                self.empty_weapon(pending.character_index, pending.slot);
            } else {
                self.set_status(
                    "The equipped item changed. Review the slot before deleting.",
                    true,
                );
            }
        } else if cancel {
            self.confirmation = None;
        }
    }

    pub(super) fn draw_reload_confirmation(&mut self, ctx: &egui::Context) {
        if matches!(self.confirmation, Some(ConfirmationDialog::Reload)) {
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
        if matches!(self.confirmation, Some(ConfirmationDialog::Exit)) {
            let mut save_and_exit = false;
            let mut discard_and_exit = false;
            let mut cancel = false;
            let response = egui::Modal::new("exit_confirmation".into()).show(ctx, |ui| {
                ui.heading("Unsaved Changes");
                ui.add_space(6.0);
                ui.label("Save Your Changes before Closing Sundial?");
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
                } else if self.confirm_authoring_work_saved() {
                    self.exit_confirmed = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
    }

    /// Asked on the way out while Parhelion work kept outside its recipe, such as perk drafts,
    /// could not be saved. Trying again exits once the work saves.
    pub(super) fn draw_unsaved_authoring_work_confirmation(&mut self, ctx: &egui::Context) {
        let Some(ConfirmationDialog::UnsavedAuthoringWork(reason)) = &self.confirmation else {
            return;
        };
        let mut retry = false;
        let mut exit = false;
        let mut cancel = false;
        let response = egui::Modal::new("unsaved_authoring_work".into()).show(ctx, |ui| {
            ui.heading("Unsaved Parhelion Work");
            ui.add_space(6.0);
            ui.label(reason.as_str());
            ui.label("Exiting now loses the edits made since the last save.");
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Try Again").clicked() {
                    retry = true;
                }
                if ui.button("Exit Anyway").clicked() {
                    exit = true;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        });
        cancel |= response.should_close();
        if retry {
            self.confirmation = None;
            exit = self.confirm_authoring_work_saved();
        } else if exit || cancel {
            self.confirmation = None;
        }
        if exit {
            if let Some(message) = self.package_authoring_exit_blocker() {
                self.set_status(message, true);
            } else {
                self.exit_confirmed = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
}

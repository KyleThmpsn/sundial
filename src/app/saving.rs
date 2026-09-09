//! UI save orchestration; persistence transactions live in workspace_save.
use super::account_validation::validate_new_account_catalog_issues;
use super::account_workspace::WorkspaceDocument;
use super::preferences::normalized_automatic_backup_limit;
use super::save_support::{SaveAction, settings_save_note};
use super::settings::{
    backups_path, create_adjacent_backup, repair_known_ability_pairs, validate_workspace_document,
    verify_workspace_source_unchanged,
};
use super::workspace_save::save_changed_sources;
use super::{ConfirmationDialog, SundialApp, has_save_work, platform};
use crate::backups::prune_automatic_backups;
use crate::persistence::json_account::ensure_schema_v8_preferences;
use eframe::egui;

impl SundialApp {
    pub(super) fn save_sources(&mut self) -> bool {
        if self.document.uses_json_account()
            && ensure_schema_v8_preferences(self.document.json_mut())
        {
            self.dirty = true;
        }
        let repaired_ability_pairs = match repair_known_ability_pairs(&mut self.document) {
            Ok(repaired) => repaired,
            Err(error) => {
                self.set_status(format!("Not saved: {error}"), true);
                return false;
            }
        };
        if repaired_ability_pairs > 0 {
            self.dirty = true;
        }
        let json_changed = self.document.json_changed_from(&self.persisted_document);
        let account_changed = self.document.account_changed_from(&self.persisted_document);
        let json_account_changed = self
            .document
            .json_account_changed_from(&self.persisted_document);
        if let Err(error) =
            self.verify_save_preconditions(json_changed, account_changed, json_account_changed)
        {
            self.set_status(format!("Not saved: {error}"), true);
            return false;
        }
        let current_warning = match self.validation_warning_for_write(&self.document) {
            Ok(warning) => warning,
            Err(error) => {
                self.set_status(format!("Not saved: {error}"), true);
                return false;
            }
        };
        let detected_warning = self
            .source_warning
            .clone()
            .or_else(|| current_warning.clone());
        let safety_backup = if json_changed && detected_warning.is_some() {
            match create_adjacent_backup(&self.settings_path) {
                Ok(path) => Some(path),
                Err(error) => {
                    self.set_status(
                        format!(
                            "Not saved: the file contains an unexpected setting and its safety copy could not be created: {error}"
                        ),
                        true,
                    );
                    return false;
                }
            }
        } else {
            None
        };
        let source_receipt = match save_changed_sources(
            &mut self.document,
            &self.persisted_document,
            &self.settings_path,
            json_changed,
            account_changed,
        ) {
            Ok(receipt) => receipt,
            Err(error) => {
                let suffix = safety_backup.map_or_else(String::new, |path| {
                    format!(" The untouched JSON source is at {}.", path.display())
                });
                let rollback_note = match error.sqlite_rollback {
                    Some(Ok(())) => {
                        " The SQLite write was rolled back from its verified backup.".to_owned()
                    }
                    Some(Err(rollback_error)) => format!(
                        " CRITICAL: the JSON save failed after SQLite was written, and SQLite rollback also failed: {rollback_error}"
                    ),
                    None => String::new(),
                };
                self.set_status(
                    format!("Save incomplete: {}{suffix}{rollback_note}", error.message),
                    true,
                );
                return false;
            }
        };
        let json_result = source_receipt.json;
        #[cfg(feature = "sqlite-account")]
        let sqlite_receipt = source_receipt.sqlite;
        #[cfg(feature = "sqlite-account")]
        let sqlite_checkpoint_warning = sqlite_receipt
            .as_ref()
            .and_then(|receipt| receipt.checkpoint_warning.as_deref());
        #[cfg(feature = "sqlite-account")]
        let sqlite_checkpoint_failed = sqlite_checkpoint_warning.is_some();
        #[cfg(feature = "sqlite-account")]
        let sqlite_checkpoint_note = sqlite_checkpoint_warning
            .map_or_else(String::new, |warning| {
                format!(" SQLite checkpoint warning: {warning}.")
            });
        #[cfg(not(feature = "sqlite-account"))]
        let (sqlite_checkpoint_failed, sqlite_checkpoint_note) = (false, String::new());
        let automatic_backup_created = json_result.is_some();
        #[cfg(feature = "sqlite-account")]
        let automatic_backup_created = automatic_backup_created || sqlite_receipt.is_some();
        let (retention_note, retention_failed) = if automatic_backup_created {
            self.apply_backup_retention()
        } else {
            (String::new(), false)
        };

        let json_durability_warning = json_result
            .as_ref()
            .is_some_and(|result| result.durability_warning.is_some());
        let safe_to_close = !sqlite_checkpoint_failed && !json_durability_warning;
        self.persisted_document = self.document.clone();
        self.source_warning = current_warning;
        self.dirty = false;
        self.progression_ui.mark_saved();
        if self.raw_json_document == *self.document.json() {
            // Saving unchanged editor content should preserve its formatting and undo history.
            self.json_editor.mark_synced();
        } else {
            self.sync_raw_json();
        }
        let repair_note = match repaired_ability_pairs {
            0 => String::new(),
            1 => " Corrected one invalid ability pairing.".to_owned(),
            count => format!(" Corrected {count} invalid ability pairings."),
        };
        let size_note = json_result
            .as_ref()
            .map_or_else(String::new, settings_save_note);
        let mut backups = Vec::new();
        if let Some(result) = &json_result {
            backups.push(format!(
                "Backup: {}",
                result
                    .backup
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
            ));
        }
        #[cfg(feature = "sqlite-account")]
        if let Some(receipt) = &sqlite_receipt {
            backups.push(format!(
                "investment.sqlite3 backup: {}",
                receipt
                    .backup
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
            ));
        }
        let backup_note = if backups.is_empty() {
            String::new()
        } else {
            format!(" {}.", backups.join(" · "))
        };
        if let (Some(warning), Some(safety_backup)) = (detected_warning, safety_backup) {
            self.set_status(
                format!(
                    "Saved after detecting an unexpected JSON setting ({warning}).{repair_note}{size_note} The untouched JSON source is at {}.{backup_note}{sqlite_checkpoint_note}{retention_note}",
                    safety_backup.display()
                ),
                true,
            );
        } else {
            self.set_status(
                format!(
                    "Saved.{repair_note}{size_note}{backup_note}{sqlite_checkpoint_note}{retention_note}"
                ),
                retention_failed || sqlite_checkpoint_failed || json_durability_warning,
            );
        }
        safe_to_close
    }

    pub(super) fn verify_save_preconditions(
        &self,
        json_changed: bool,
        account_changed: bool,
        json_account_changed: bool,
    ) -> Result<(), String> {
        if json_changed || account_changed {
            self.document.verify_account_source_unchanged()?;
        }
        if json_changed {
            verify_workspace_source_unchanged(
                &self.settings_path,
                self.persisted_document.json(),
                self.persisted_document.uses_json_account(),
            )?;
        }
        if json_changed || account_changed {
            super::settings::require_game_closed(platform::destiny_is_running())?;
        }
        if account_changed || json_account_changed {
            validate_new_account_catalog_issues(
                &self.document,
                &self.persisted_document,
                &self.manifest,
                self.preferences.experimental_cross_class_subclasses,
            )?;
        }
        Ok(())
    }

    pub(super) fn validation_warning_for_write(
        &self,
        candidate: &WorkspaceDocument,
    ) -> Result<Option<String>, String> {
        let warning = validate_workspace_document(candidate).err();
        // An unchanged JSON source may accompany a SQLite-only save. Once JSON is
        // edited, require valid known settings: first-error equality cannot prove
        // that another invalid field was not introduced later in validation.
        if !candidate.json_changed_from(&self.persisted_document) {
            return Ok(warning);
        }
        match warning {
            None => Ok(None),
            Some(error) => Err(format!(
                "invalid settings: {error}. Correct invalid known settings before applying or saving edited JSON"
            )),
        }
    }

    pub(super) fn save_all_edits(&mut self) -> bool {
        if self.json_editor.has_unapplied_changes() && !self.apply_raw_json() {
            return false;
        }
        self.save_sources()
    }

    pub(super) fn has_unsaved_changes(&self) -> bool {
        has_save_work(self.dirty, self.json_editor.has_unapplied_changes())
    }

    pub(super) fn package_authoring_exit_blocker(&self) -> Option<&'static str> {
        if !self.package_authoring_open {
            None
        } else if self.package_authoring_busy {
            Some("Wait for Parhelion's active operation to finish before closing Sundial")
        } else if self.package_authoring_dirty {
            Some("Save or discard Parhelion's recipe changes before closing Sundial")
        } else {
            None
        }
    }

    pub(super) fn request_save(&mut self, ctx: &egui::Context, action: SaveAction) {
        if self.json_editor.has_unapplied_changes() && !self.apply_raw_json() {
            return;
        }
        let document_changed = self.document != self.persisted_document;
        if !has_save_work(document_changed, false) {
            self.dirty = false;
            self.set_status("There are no changes to save", false);
            if action == SaveAction::SaveAndExit {
                self.perform_save_action(ctx, action);
            }
            return;
        }
        if self.preferences.review_changes_before_saving && document_changed {
            // Validate the same repairable settings that the save path accepts,
            // without changing the document or writing backups during review.
            let mut candidate = self.document.clone();
            let validation = repair_known_ability_pairs(&mut candidate)
                .and_then(|_| self.validation_warning_for_write(&candidate));
            if let Err(error) = validation {
                self.set_status(format!("Not saved: {error}"), true);
                return;
            }
            self.pending_save_action = Some(action);
            self.confirmation = Some(ConfirmationDialog::ReviewSave);
        } else {
            self.perform_save_action(ctx, action);
        }
    }

    pub(super) fn perform_save_action(&mut self, ctx: &egui::Context, action: SaveAction) {
        let safe_to_close = self.save_all_edits();
        if action == SaveAction::SaveAndExit && !self.has_unsaved_changes() && safe_to_close {
            if let Some(message) = self.package_authoring_exit_blocker() {
                self.set_status(message, true);
                return;
            }
            self.exit_confirmed = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    pub(super) fn apply_backup_retention(&self) -> (String, bool) {
        if !self.preferences.limit_automatic_backups {
            return (String::new(), false);
        }
        let result = backups_path()
            .ok_or_else(|| "Could not locate Sundial's backups folder".to_owned())
            .and_then(|root| {
                let keep = usize::from(normalized_automatic_backup_limit(
                    self.preferences.automatic_backup_limit,
                ));
                let json_removed = prune_automatic_backups(&root, &self.settings_path, keep)?;
                #[cfg(feature = "sqlite-account")]
                let sqlite_removed = prune_automatic_backups(
                    &root,
                    &self.document.source_info().database_path,
                    keep,
                )?;
                #[cfg(not(feature = "sqlite-account"))]
                let sqlite_removed = 0;
                Ok(json_removed + sqlite_removed)
            });
        match result {
            Ok(0) => (String::new(), false),
            Ok(1) => (" Removed one older automatic backup.".to_owned(), false),
            Ok(removed) => (
                format!(" Removed {removed} older automatic backups."),
                false,
            ),
            Err(error) => (
                format!(" Automatic backup cleanup needs attention: {error}."),
                true,
            ),
        }
    }
}

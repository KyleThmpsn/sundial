//! Explicit workspace reset and backup-recovery actions.
use crate::app::account_workspace as account;

use super::save_support::settings_save_note;
use super::settings::{
    create_adjacent_backup, load_installed_sunrise_defaults, save_json,
    validate_workspace_document, verify_workspace_source_unchanged,
};
use super::{ConfirmationDialog, platform, settings::backups_path};
use super::{SundialApp, preserve_inactive_json_account_domains};

impl SundialApp {
    pub(super) fn request_sqlite_backup_restore(&mut self) {
        let mut dialog = rfd::FileDialog::new()
            .set_title("Select a Sundial investment.sqlite3 backup")
            .add_filter("SQLite database", &["sqlite3"]);
        if let Some(path) = backups_path() {
            dialog = dialog.set_directory(path);
        }
        let Some(path) = dialog.pick_file() else {
            return;
        };
        match self.document.validate_sqlite_backup(&path) {
            Ok(()) => {
                self.pending_sqlite_restore = Some(path);
                self.confirmation = Some(ConfirmationDialog::RestoreSqliteBackup);
            }
            Err(error) => self.set_status(
                format!("Backup not selected: {error}. No files were changed"),
                true,
            ),
        }
    }

    pub(super) fn restore_selected_sqlite_backup(&mut self) {
        let Some(backup) = self.pending_sqlite_restore.take() else {
            return;
        };
        match platform::destiny_is_running() {
            Ok(true) => {
                self.set_status(
                    "Not restored: close Destiny 2 before replacing investment.sqlite3, then try again",
                    true,
                );
                return;
            }
            Ok(false) => {}
            Err(error) => {
                self.set_status(format!("Not restored: {error}"), true);
                return;
            }
        }
        match self.document.restore_sqlite_backup_safely(&backup) {
            Ok(safety_backup) => {
                if self.reload() {
                    let warning = self
                        .source_warning
                        .clone()
                        .map_or_else(String::new, |value| {
                            format!(" Reloaded with an unrelated settings.json warning: {value}.")
                        });
                    self.set_status(
                        format!(
                            "Restored investment.sqlite3 from {}. The replaced database is preserved at {}.{warning}",
                            backup.display(),
                            safety_backup.display()
                        ),
                        self.source_warning.is_some(),
                    );
                } else {
                    let reload_error = self.status.clone();
                    self.set_status(
                        format!(
                            "Restored investment.sqlite3 from {}, but Sundial could not reload the workspace: {reload_error}. The replaced database is preserved at {}",
                            backup.display(),
                            safety_backup.display()
                        ),
                        true,
                    );
                }
            }
            Err(error) => self.set_status(format!("Not restored: {error}"), true),
        }
    }

    pub(super) fn reset_to_sunrise_defaults(&mut self) {
        if let Err(error) = require_game_closed_for_reset(super::platform::destiny_is_running()) {
            self.set_status(format!("Defaults not restored: {error}"), true);
            return;
        }
        if let Err(error) = self.document.verify_account_source_unchanged() {
            self.set_status(format!("Defaults not restored: {error}"), true);
            return;
        }
        if let Err(error) = verify_workspace_source_unchanged(
            &self.settings_path,
            self.persisted_document.json(),
            self.persisted_document.uses_json_account(),
        ) {
            self.set_status(format!("Defaults not restored: {error}"), true);
            return;
        }
        let mut default_document = match load_installed_sunrise_defaults(&self.install_path) {
            Ok(document) => document,
            Err(error) => {
                self.set_status(error, true);
                return;
            }
        };
        if !self.document.uses_json_account() {
            preserve_inactive_json_account_domains(
                &mut default_document,
                self.persisted_document.json(),
            );
        }
        let adjacent_backup = match create_adjacent_backup(&self.settings_path) {
            Ok(path) => path,
            Err(error) => {
                self.set_status(
                    format!("Defaults not restored because the safety copy failed: {error}"),
                    true,
                );
                return;
            }
        };
        match save_json(
            &self.settings_path,
            &default_document,
            self.persisted_document.json(),
            self.persisted_document.uses_json_account(),
        ) {
            Ok(result) => {
                let size_note = settings_save_note(&result);
                let (retention_note, retention_failed) = self.apply_backup_retention();
                self.document.replace_json(default_document.clone());
                self.progression_ui.invalidate_document();
                self.persisted_document.replace_json(default_document);
                self.refresh_sunrise_version();
                self.source_warning = validate_workspace_document(&self.document).err();
                self.class_armor_defaults = account::class_armor_default_characters(&self.document);
                self.selected_character = self
                    .selected_character
                    .min(self.character_count().saturating_sub(1));
                self.clear_picker_state();
                self.sync_raw_json();
                self.dirty = self.document != self.persisted_document;
                self.set_status(
                    format!(
                        "Restored the defaults bundled with the installed Project Sunrise.{size_note} Original: {}. Backup: {}.{retention_note}",
                        adjacent_backup.display(),
                        result.backup.display()
                    ),
                    retention_failed || result.durability_warning.is_some(),
                );
            }
            Err(error) => {
                self.set_status(
                    format!(
                        "Defaults not restored: {error}. The untouched source is at {}",
                        adjacent_backup.display()
                    ),
                    true,
                );
            }
        }
    }
}

fn require_game_closed_for_reset(running: Result<bool, String>) -> Result<(), String> {
    if running? {
        return Err("Close Destiny 2 before restoring Sunrise defaults, then try again".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_requires_a_confirmed_closed_game() {
        assert!(require_game_closed_for_reset(Ok(false)).is_ok());
        assert!(require_game_closed_for_reset(Ok(true)).is_err());
        assert!(require_game_closed_for_reset(Err("process scan failed".into())).is_err());
    }
}

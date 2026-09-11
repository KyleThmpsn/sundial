use super::*;
use crate::persistence::sqlite_account::ResetPlan;

impl SundialApp {
    pub(in crate::app) fn request_sqlite_defaults_reset(&mut self) {
        self.pending_sqlite_reset = None;
        if self.document.uses_json_account() {
            self.set_status("The active account uses settings.json. Use Reset to Sunrise Defaults for this installation", true);
            return;
        }
        let result = super::super::settings::load_installed_account_defaults(&self.install_path)
            .and_then(|defaults| {
                ResetPlan::prepare(&self.document.source_info().database_path, &defaults)
                    .map_err(|error| error.to_string())
            });
        match result {
            Ok(plan) => {
                self.pending_sqlite_reset = Some(plan);
                self.confirmation = Some(ConfirmationDialog::ResetSqliteDefaults);
            }
            Err(error) => self.set_status(format!("Account reset unavailable: {error}"), true),
        }
    }

    pub(in crate::app) fn reset_sqlite_to_sunrise_defaults(&mut self) {
        let Some(plan) = self.pending_sqlite_reset.take() else {
            return;
        };
        if self.document.uses_json_account()
            || plan.path() != self.document.source_info().database_path
        {
            self.set_status("Account not reset because the selected installation changed. Open the reset confirmation again", true);
            return;
        }
        if self.package_authoring_open || self.package_authoring_busy {
            self.set_status(
                "Close Parhelion before resetting the account database, then try again",
                true,
            );
            return;
        }
        if let Err(error) = require_game_closed_for_reset(platform::destiny_is_running()) {
            self.set_status(format!("Account not reset: {error}"), true);
            return;
        }
        match plan.apply() {
            Ok(receipt) => {
                if self.reload() {
                    let warning = self
                        .source_warning
                        .as_deref()
                        .map_or(String::new(), |warning| {
                            format!(" Settings warning: {warning}")
                        });
                    self.set_status(format!("Reset investment.sqlite3 to the installed Sunrise defaults. Recovery backup: {}.{warning}", receipt.safety_backup.display()), self.source_warning.is_some());
                } else {
                    self.set_status(format!("Reset investment.sqlite3, but the workspace could not reload: {}. Recovery backup: {}", self.status, receipt.safety_backup.display()), true);
                }
            }
            Err(error) => self.set_status(format!("Account not reset: {error}"), true),
        }
    }
}

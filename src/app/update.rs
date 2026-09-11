//! Restart admission uses the same document and Parhelion state as normal closing.
use super::{SundialApp, save_support::SaveAction};
use crate::updates::Action;
use eframe::egui;

impl SundialApp {
    pub(super) fn update_restart_blocker(&self) -> Option<&'static str> {
        if self.package_authoring_busy {
            Some("Wait for Parhelion's active operation to finish before restarting.")
        } else if self.package_authoring_dirty {
            Some("Save or discard Parhelion's recipe changes before restarting.")
        } else if self.catalog_task.is_some() {
            Some("Wait for the current catalog operation to finish before restarting.")
        } else if self.confirmation.is_some() {
            Some("Finish or cancel the open confirmation before restarting.")
        } else if self.has_unsaved_changes() || self.document != self.persisted_document {
            Some("Save or discard your changes before restarting. The downloaded update will wait.")
        } else {
            None
        }
    }

    pub(super) fn draw_update_window(&mut self, ctx: &egui::Context) {
        let blocker = self.update_restart_blocker();
        let can_save = self.has_unsaved_changes() && self.confirmation.is_none();
        match self.update_check.draw(ctx, blocker, can_save) {
            Action::None => {}
            Action::Save => self.request_save(ctx, SaveAction::Save),
            Action::Prepare if self.update_restart_blocker().is_none() => {
                self.update_check
                    .prepare_restart(ctx, self.install_path.clone());
            }
            Action::Commit if self.update_restart_blocker().is_none() => {
                if self.update_check.commit_restart() {
                    self.exit_confirmed = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            Action::Prepare | Action::Commit => {}
        }
    }
}

use super::*;

pub(super) fn draw_parhelion_reset_note(ui: &mut egui::Ui, resets_account: bool) {
    ui.add_space(8.0);
    ui.colored_label(ui.visuals().warn_fg_color, if resets_account {
        "If you have custom Parhelion packages installed, reinstall them in Parhelion after this reset to restore their Collections entries and unlocks."
    } else {
        "If you also reset investment.sqlite3, reinstall any custom Parhelion packages afterward to restore their Collections entries and unlocks."
    });
}

impl SundialApp {
    pub(in crate::app) fn draw_sqlite_reset_confirmation(&mut self, ctx: &egui::Context) {
        if self.confirmation != Some(ConfirmationDialog::ResetSqliteDefaults) {
            return;
        }
        let Some(plan) = self.pending_sqlite_reset.as_ref() else {
            self.confirmation = None;
            return;
        };
        let mut reset = false;
        let mut cancel = false;
        let response = egui::Modal::new("reset_account_defaults".into()).show(ctx, |ui| {
            ui.set_width((ctx.screen_rect().width() - 60.0).clamp(260.0, 500.0));
            ui.heading("Reset Account Database?");
            ui.add_space(6.0);
            ui.label("This replaces all data in investment.sqlite3 with the defaults bundled in your installed Project Sunrise version, including characters, inventory, equipment, unlocks, progression, and account preferences.");
            ui.add_space(6.0);
            ui.label("Sundial creates and verifies a full recovery backup first. settings.json is not changed.");
            ui.add_space(6.0);
            ui.label("Close Destiny 2 and Parhelion before continuing. Any unsaved Sundial changes will be discarded.");
            draw_parhelion_reset_note(ui, true);
            ui.add_space(6.0);
            ui.add(egui::Label::new(egui::RichText::new(plan.path().display().to_string()).monospace()).wrap());
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                reset = ui.button("Reset Account Database").clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
        cancel |= response.should_close();
        if cancel {
            self.confirmation = None;
            self.pending_sqlite_reset = None;
        } else if reset {
            self.confirmation = None;
            self.reset_sqlite_to_sunrise_defaults();
        }
    }
}

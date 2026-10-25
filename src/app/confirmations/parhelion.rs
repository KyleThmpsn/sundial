//! The introduction shown before Parhelion first opens.
use super::{ConfirmationDialog, SundialApp};
use eframe::egui;

impl SundialApp {
    pub(in crate::app) fn request_open_parhelion(&mut self, ctx: &egui::Context) {
        if !self.preferences.parhelion_warning_acknowledged {
            self.confirmation = Some(ConfirmationDialog::OpenParhelion);
            return;
        }
        self.open_package_authoring(ctx);
    }

    pub(in crate::app) fn finish_parhelion_confirmation(&mut self, accepted: bool) {
        self.confirmation = None;
        if accepted {
            self.preferences.parhelion_warning_acknowledged = true;
        }
    }

    pub(in crate::app) fn draw_parhelion_confirmation(&mut self, ctx: &egui::Context) {
        if self.confirmation != Some(ConfirmationDialog::OpenParhelion) {
            return;
        }
        let Some(accepted) = introduction(ctx) else {
            return;
        };
        self.finish_parhelion_confirmation(accepted);
        if !accepted {
            return;
        }
        let saved = self.save_preferences();
        self.open_package_authoring(ctx);
        if let Err(error) = saved
            && self.package_authoring_open
        {
            self.set_status(
                format!("Opened Parhelion, but the preference could not be saved: {error}"),
                true,
            );
        }
    }
}

fn introduction(ctx: &egui::Context) -> Option<bool> {
    let mut decision = None;
    let response = egui::Modal::new("enable_parhelion_confirmation".into()).show(ctx, |ui| {
        ui.set_width(560.0_f32.min((ctx.content_rect().width() - 48.0).max(240.0)));
        ui.heading("Before You Open Parhelion");
        ui.add_space(8.0);
        egui::ScrollArea::vertical()
            .max_height((ctx.content_rect().height() - 180.0).max(120.0))
            .show(ui, |ui| {
                ui.label("Parhelion builds custom weapons from stock stats, perks, behavior and appearances. It is experimental. Test every build in game.");
                ui.add_space(10.0);
                ui.label("Custom weapons use additional packages. Stock packages are preserved.");
                ui.strong("To remove custom packages, open Parhelion Preferences > Builds & Backups > Uninstall Custom Packages.");
                ui.add_space(10.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label("Report bugs through");
                    ui.hyperlink_to("GitHub Issues", format!("{}/issues", super::super::PROJECT_URL));
                });
                ui.add_space(10.0);
                ui.label("Experimental Features can freeze or crash the game with invalid combinations.");
            });
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            if ui.button("I Understand").clicked() {
                decision = Some(true);
            }
            if ui.button("Cancel").clicked() {
                decision = Some(false);
            }
        });
    });
    decision.or_else(|| response.should_close().then_some(false))
}

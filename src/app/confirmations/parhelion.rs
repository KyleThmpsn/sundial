//! First-enable introduction for the experimental weapon workbench.
use super::{ConfirmationDialog, SundialApp};
use eframe::egui;

impl SundialApp {
    pub(in crate::app) fn request_parhelion_enabled(&mut self, enabled: bool) -> bool {
        if enabled && !self.preferences.parhelion_warning_acknowledged {
            self.confirmation = Some(ConfirmationDialog::EnableParhelion);
            return false;
        }
        let changed = self.preferences.experimental_package_authoring != enabled;
        self.preferences.experimental_package_authoring = enabled;
        changed
    }

    pub(in crate::app) fn finish_parhelion_confirmation(&mut self, accepted: bool) {
        self.confirmation = None;
        if accepted {
            self.preferences.parhelion_warning_acknowledged = true;
            self.preferences.experimental_package_authoring = true;
        }
    }

    pub(in crate::app) fn draw_parhelion_confirmation(&mut self, ctx: &egui::Context) {
        if self.confirmation != Some(ConfirmationDialog::EnableParhelion) {
            return;
        }
        let Some(accepted) = introduction(ctx) else {
            return;
        };
        self.finish_parhelion_confirmation(accepted);
        if accepted && let Err(error) = self.save_preferences() {
            self.set_status(
                format!("Parhelion enabled, but the preference could not be saved: {error}"),
                true,
            );
        }
    }
}

fn introduction(ctx: &egui::Context) -> Option<bool> {
    let mut decision = None;
    let response = egui::Modal::new("enable_parhelion_confirmation".into()).show(ctx, |ui| {
        ui.set_width(560.0_f32.min((ctx.screen_rect().width() - 48.0).max(240.0)));
        ui.heading("Before you enable Parhelion");
        ui.add_space(8.0);
        egui::ScrollArea::vertical()
            .max_height((ctx.screen_rect().height() - 180.0).max(120.0))
            .show(ui, |ui| {
                ui.label("Parhelion combines weapon stats, perks, behavior, and appearance into custom weapons. It is experimental. A successful build still needs an in-game test.");
                ui.add_space(10.0);
                ui.label("Custom weapons use additional packages. Stock packages are preserved.");
                ui.strong("To remove custom packages, open Parhelion Preferences > Builds & Backups > Uninstall Custom Packages.");
                ui.add_space(10.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label("Report bugs through");
                    ui.hyperlink_to("GitHub Issues", format!("{}/issues", super::super::PROJECT_URL));
                });
                ui.add_space(10.0);
                ui.label("Custom perk editing is planned for a future release. Existing custom perks can be reused from saved recipes.");
                ui.add_space(10.0);
                ui.label("Advanced technical controls expose experimental game data. Invalid combinations can freeze or crash the game.");
            });
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            if ui.button("I understand").clicked() {
                decision = Some(true);
            }
            if ui.button("Cancel").clicked() {
                decision = Some(false);
            }
        });
    });
    decision.or_else(|| response.should_close().then_some(false))
}

#[cfg(test)]
mod tests;

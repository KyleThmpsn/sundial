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
                ui.label("Parhelion is an experimental package creation tool. This release allows many different weapon combinations to be built. We try to block combinations that definitely will not work, but bugs are expected.");
                ui.add_space(10.0);
                ui.label("Parhelion adds packages; it does not edit your existing ones.");
                ui.strong("If something goes wrong, you can uninstall from Parhelion Preferences > Uninstall Custom Packages and revert your game packages back to stock.");
                ui.add_space(10.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label("Many bugs are known and being worked out. Please report the ones you find through");
                    ui.hyperlink_to("GitHub Issues", format!("{}/issues", super::super::PROJECT_URL));
                });
                ui.add_space(10.0);
                ui.label("Some features, such as custom perk creation, are very early in exploration. They will change substantially in future releases as their behavior is mapped out.");
                ui.add_space(10.0);
                ui.label("Extremely early features are locked behind ‘Show advanced technical controls (experimental)’ in Parhelion Preferences. These are not recommended for use; many can outright freeze the game.");
                ui.add_space(10.0);
                ui.label("More substantial features will be added in future releases.");
            });
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            if ui.button("I understand").clicked() {
                decision = Some(true);
            }
            if ui.button("Nevermind").clicked() {
                decision = Some(false);
            }
        });
    });
    decision.or_else(|| response.should_close().then_some(false))
}

#[cfg(test)]
mod tests;

//! The review, its progress and any error share one bounded dialog and action row.
use super::super::style;
use super::UpgradeUi;
use std::path::Path;

#[derive(Default)]
pub(super) enum Action {
    #[default]
    None,
    Build,
    Review,
    Recover,
    Close,
}

pub(super) fn show(ctx: &egui::Context, state: &UpgradeUi, backup: &str) -> Action {
    let mut action = Action::None;
    let available =
        (ctx.content_rect().size() - egui::vec2(48.0, 48.0)).max(egui::vec2(280.0, 220.0));
    let width = 600.0_f32.min(available.x - 40.0);
    egui::Window::new("Upgrade Parhelion Installation")
        .id(egui::Id::new("parhelion-upgrade"))
        .collapsible(false)
        .title_bar(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .default_width(width)
        .frame(egui::Frame::window(&ctx.global_style()).inner_margin(20))
        .show(ctx, |ui| {
            style::workbench_style(ui);
            ui.set_width(width);
            ui.spacing_mut().item_spacing.y = 8.0;
            hint(ui, "Sunrise · Installation");
            heading(ui, state);
            ui.add_space(4.0);
            egui::ScrollArea::vertical()
                .id_salt("upgrade-content")
                .max_height((available.y - 160.0).max(60.0))
                .auto_shrink([false, true])
                .show(ui, |ui| content(ui, state, backup));
            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| actions(ui, state, &mut action));
        });
    action
}

fn heading(ui: &mut egui::Ui, state: &UpgradeUi) {
    let title = if state.review.is_some() && state.recovering {
        "Recovering Installation"
    } else if state.review.is_some() {
        "Checking Installation"
    } else if state.error.is_some() {
        "Installation Needs Attention"
    } else if state.plan.is_some() {
        "Rebuild for Sunrise Mods"
    } else {
        "No Upgrade Needed"
    };
    ui.label(egui::RichText::new(title).size(24.0).strong());
}

fn content(ui: &mut egui::Ui, state: &UpgradeUi, backup: &str) {
    if state.busy() {
        ui.horizontal_wrapped(|ui| {
            ui.spinner();
            ui.label(if state.recovering {
                "Recovering the interrupted operation and checking your installation."
            } else {
                "Checking the installed Sunrise version and your custom packages."
            });
        });
        hint(ui, "Keep Parhelion open until this finishes.");
    } else if let Some(error) = &state.error {
        ui.label("The operation could not finish. Review the details before trying again.");
        style::block(ui.style()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(
                egui::Label::new(egui::RichText::new(error).color(ui.visuals().error_fg_color))
                    .wrap(),
            );
        });
        ui.label("If an earlier install was interrupted, close Destiny and choose Recover Interrupted Operation.");
    } else if let Some(plan) = &state.plan {
        ui.label("On Sunrise 0.6 or later, builds install to the mod folder. Packages from earlier builds can't move there, so build and install again to bring your items over.");
        ui.add_space(4.0);
        style::block(ui.style()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.strong(format!(
                "{} Packages From an Earlier Build",
                plan.artifacts().len()
            ));
            path(ui, "In", plan.target());
            ui.add_space(4.0);
            path(ui, "New Builds Install To", &plan.mod_directory());
        });
        path_block(ui, "Backup Folder", Path::new(backup));
        ui.label("Installing backs up and removes these packages. Your recipes are kept.");
        ui.collapsing("Package Files", |ui| {
            for artifact in plan.artifacts() {
                ui.add(
                    egui::Label::new(egui::RichText::new(&artifact.file_name).monospace()).wrap(),
                );
            }
        });
        ui.add_space(4.0);
        hint(
            ui,
            "Keep every installed item in the build so it stays in your account.",
        );
    } else {
        ui.label("This installation does not need a mod-folder upgrade. You can continue using Parhelion.");
    }
}

fn path_block(ui: &mut egui::Ui, label: &str, value: &Path) {
    style::block(ui.style()).show(ui, |ui| {
        ui.set_width(ui.available_width());
        path(ui, label, value);
    });
}

fn hint(ui: &mut egui::Ui, text: &str) {
    ui.add(
        egui::Label::new(egui::RichText::new(text).color(style::secondary(ui.visuals()))).wrap(),
    );
}

fn path(ui: &mut egui::Ui, label: &str, value: &Path) {
    ui.strong(label);
    ui.add(egui::Label::new(egui::RichText::new(value.display().to_string()).monospace()).wrap());
}

fn actions(ui: &mut egui::Ui, state: &UpgradeUi, action: &mut Action) {
    ui.spacing_mut().button_padding = egui::vec2(12.0, 7.0);
    if state.busy() {
        ui.add_enabled(false, egui::Button::new("Please Wait…"));
        return;
    }
    if state.error.is_some() {
        if ui.add(style::primary(ui, "Review Again")).clicked() {
            *action = Action::Review;
        }
        if ui.button("Recover Interrupted Operation").clicked() {
            *action = Action::Recover;
        }
    } else if state.plan.is_some() && ui.add(style::primary(ui, "Build & Stage")).clicked() {
        *action = Action::Build;
    }
    let close = if state.plan.is_some() && state.error.is_none() {
        "Later"
    } else {
        "Close"
    };
    if ui.button(close).clicked() {
        *action = Action::Close;
    }
}

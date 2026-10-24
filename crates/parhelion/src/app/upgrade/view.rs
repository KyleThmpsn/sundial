//! The review, its progress and any error share one bounded dialog and action row.
use super::super::style;
use super::UpgradeUi;
use std::path::Path;

#[derive(Default)]
pub(super) enum Action {
    #[default]
    None,
    Build,
    Restore,
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
    } else if let Some(kept) = state.plan.as_ref().and_then(|plan| plan.kept()) {
        if kept.mod_folder {
            "Restore Your Sunrise 0.6 Build"
        } else {
            "Restore Your Build for This Runtime"
        }
    } else if state.plan.is_some() {
        "Rebuild for Sunrise Mods"
    } else {
        "No Upgrade Needed"
    };
    ui.label(egui::RichText::new(title).size(24.0).strong());
}

fn content(ui: &mut egui::Ui, state: &UpgradeUi, backup: &str) {
    if state.busy() {
        let (message, completed, total) = state.progress();
        ui.horizontal_wrapped(|ui| {
            ui.spinner();
            ui.label(message);
        });
        hint(ui, &format!("Step {} of {total}", completed + 1));
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
    } else if let Some(kept) = state.plan.as_ref().and_then(|plan| plan.kept()) {
        let age = age(kept.installed);
        ui.label(if kept.mod_folder {
            format!("Parhelion kept the Sunrise 0.6 build you installed {age}. Restoring installs it again without a rebuild, and backs up and removes the packages a Dawn or earlier Sunrise install put in stock packages, so the two builds never load together.")
        } else {
            format!("Parhelion kept the build you installed for this runtime {age}. Restoring installs it again without a rebuild, and moves your Sunrise 0.6 build out of the mod folder, so the two never load together.")
        });
        path_block(ui, "Kept Build", &kept.run_directory);
        ui.label("The review checks it against this installation, your other mods and your account before anything changes. If it no longer fits, build again instead.");
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

/// When a build was installed, in words.
fn age(installed: std::time::SystemTime) -> String {
    let days = installed
        .elapsed()
        .map_or(0, |elapsed| elapsed.as_secs() / 86_400);
    match days {
        0 => "in the last day".to_owned(),
        1 => "a day ago".to_owned(),
        days => format!("{days} days ago"),
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
    } else if state
        .plan
        .as_ref()
        .is_some_and(|plan| plan.kept().is_some())
    {
        if ui.add(style::primary(ui, "Review Restore")).clicked() {
            *action = Action::Restore;
        }
        if ui.button("Build & Stage").clicked() {
            *action = Action::Build;
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

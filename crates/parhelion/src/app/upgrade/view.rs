//! The review, progress and result share one bounded dialog and action row.
use super::super::style;
use super::UpgradeUi;
use std::path::Path;

#[derive(Default)]
pub(super) enum Action {
    #[default]
    None,
    Upgrade,
    Review,
    Recover,
    OpenBackup,
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
    let title = if state.worker.is_some() {
        "Upgrading Installation"
    } else if state.review.is_some() && state.recovering {
        "Recovering Installation"
    } else if state.review.is_some() {
        "Checking Installation"
    } else if state.report.is_some() {
        "Installation Upgraded"
    } else if state.error.is_some() {
        "Installation Needs Attention"
    } else if state.plan.is_some() {
        "Move to Sunrise Mods"
    } else {
        "No Upgrade Needed"
    };
    let mut title = egui::RichText::new(title).size(24.0).strong();
    if state.report.is_some() {
        title = title.color(style::success_color(ui.visuals()));
    }
    ui.label(title);
}

fn content(ui: &mut egui::Ui, state: &UpgradeUi, backup: &str) {
    if state.busy() {
        ui.horizontal_wrapped(|ui| {
            ui.spinner();
            ui.label(if state.worker.is_some() {
                "Backing up your packages and moving them into the mod folder."
            } else if state.recovering {
                "Recovering the interrupted operation and checking your installation."
            } else {
                "Checking the installed Sunrise version and your custom packages."
            });
        });
        hint(ui, "Keep Parhelion open until this finishes.");
    } else if let Some(report) = &state.report {
        ui.label(format!(
            "{} packages are now in the Sunrise mod folder. Future installs will update this folder.",
            report.package_count
        ));
        path_block(ui, "Mod Folder", &report.mod_directory);
        path_block(ui, "Recovery Backup", &report.backup_directory);
        ui.label("Your account and recipes are kept. Start Destiny again to load the mod.");
    } else if let Some(error) = &state.error {
        ui.label("The operation could not finish. Review the details before trying again.");
        style::block(ui.style()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(
                egui::Label::new(egui::RichText::new(error).color(ui.visuals().error_fg_color))
                    .wrap(),
            );
        });
        ui.label("If an earlier install or upgrade was interrupted, close Destiny and choose Recover Interrupted Operation.");
    } else if let Some(plan) = &state.plan {
        ui.label("Sunrise 0.6 or later uses a mod folder for your custom content. Move your existing Parhelion packages before installing another build.");
        ui.add_space(4.0);
        style::block(ui.style()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.strong(format!("{} Packages to Move", plan.artifacts().len()));
            path(ui, "From", plan.target());
            ui.add_space(4.0);
            path(ui, "To the Mod Folder", &plan.mod_directory());
        });
        path_block(ui, "Backup Folder", Path::new(backup));
        ui.label("A verified backup is saved before the move. Your account, recipes and package contents are kept.");
        ui.collapsing("Package Files", |ui| {
            for artifact in plan.artifacts() {
                ui.add(
                    egui::Label::new(egui::RichText::new(&artifact.file_name).monospace()).wrap(),
                );
            }
        });
        ui.add_space(4.0);
        ui.label(egui::RichText::new("Close Destiny before upgrading.").strong());
        hint(ui, "Start it again afterward to load the mod.");
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
    if state.report.is_some() {
        if ui.add(style::primary(ui, "Done")).clicked() {
            *action = Action::Close;
        }
        if ui.button("Open Backup Folder").clicked() {
            *action = Action::OpenBackup;
        }
        return;
    }
    if state.error.is_some() {
        if ui.add(style::primary(ui, "Review Again")).clicked() {
            *action = Action::Review;
        }
        if ui.button("Recover Interrupted Operation").clicked() {
            *action = Action::Recover;
        }
    } else if state.plan.is_some() && ui.add(style::primary(ui, "Back Up and Upgrade")).clicked() {
        *action = Action::Upgrade;
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

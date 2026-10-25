//! Build locations, backup policy and installed-content actions.
use super::*;

impl PackageAuthoringApp {
    pub(super) fn draw_build_backup_preferences(&mut self, ui: &mut egui::Ui) {
        let editable = self.package_preferences_editable();
        if !editable {
            ui.label("Locked during a build, installation or review.");
        }
        ui.add_enabled_ui(editable, |ui| {
            if ui.available_width() >= 780.0 {
                ui.columns(2, |columns| {
                    self.draw_build_files(&mut columns[0]);
                    self.draw_package_backups(&mut columns[1]);
                });
            } else {
                self.draw_build_files(ui);
                ui.add_space(12.0);
                self.draw_package_backups(ui);
            }
        });
        ui.add_space(16.0);
        preference_card(ui, "Installed Custom Content", |ui| {
            let enabled = editable
                && !self.has_background_work()
                && !self.perk_workbench.editing()
                && self.icon_editor.is_none()
                && !self.presentation_editor.editing()
                && !self.build_status_open;
            if action_row(
                ui,
                "Sunrise Mod Folder",
                "On Sunrise 0.6 or later, packages from earlier builds must be built again for the mod folder. Installing backs them up and removes them.",
                "Review Sunrise Mod Upgrade…",
                enabled,
            ) {
                self.start_upgrade_review(false);
            }
            ui.add_space(6.0);
            ui.separator();
            ui.add_space(6.0);
            if action_row(
                ui,
                "Remove Custom Content",
                "Remove installed custom packages and choose whether to remove their items and progression. Review the changes before uninstalling.",
                "Uninstall Custom Packages…",
                enabled,
            ) {
                self.start_uninstall_review(false);
            }
        });
    }

    fn draw_build_files(&mut self, ui: &mut egui::Ui) {
        preference_card(ui, "Build Files", |ui| {
            hint(ui, "Choose where new builds are staged.");
            ui.add_space(4.0);
            ui.strong("Game Packages");
            draw_preference_path(ui, &self.packages);
            hint(ui, "Selected in Sundial.");
            ui.add_space(8.0);
            let mut changed = fields::path_row(
                ui,
                "Staging Folder",
                &mut self.staging,
                "Each build gets its own folder here.",
            );
            ui.add_space(8.0);
            changed |= ui
                .checkbox(
                    &mut self.ignore_installed,
                    "Build from a Temporary Stock Package View",
                )
                .changed();
            if changed {
                self.invalidate_results();
            }
        });
    }

    fn draw_package_backups(&mut self, ui: &mut egui::Ui) {
        preference_card(ui, "Package Backups", |ui| {
            hint(ui, "Keep recovery copies when installed content changes.");
            ui.add_space(4.0);
            let changed = fields::path_row(
                ui,
                "Backup Folder",
                &mut self.backup_root,
                "Package and mod-folder recovery backups are saved here.",
            );
            ui.add_space(8.0);
            let mut backups_changed = false;
            ui.horizontal_wrapped(|ui| {
                backups_changed |= ui
                    .checkbox(&mut self.limit_package_backups, "Keep Last")
                    .changed();
                let retention = named_control(
                    ui.add_enabled(
                        self.limit_package_backups,
                        egui::DragValue::new(&mut self.package_backup_retention)
                            .range(1..=MAX_PACKAGE_BACKUP_RETENTION),
                    ),
                    "Package backups to keep",
                );
                backups_changed |= retention.drag_stopped()
                    || retention.lost_focus()
                    || (retention.changed() && !retention.dragged() && !retention.has_focus());
                ui.label("automatic package backups");
            });
            backups_changed |= ui
                .checkbox(
                    &mut self.backup_recipe_snapshots,
                    "Include Recipe Snapshots in Package Backups",
                )
                .on_hover_text("Saves the built recipes with each backup.")
                .changed();
            ui.add_space(4.0);
            hint(
                ui,
                "The limit includes completed mod backups. Recovery copies for interrupted operations, account changes, upgrades, and removals are kept.",
            );
            if backups_changed {
                self.save_workbench_preferences();
            }
            if changed {
                self.invalidate_results();
            }
        });
    }
}

fn preference_card(ui: &mut egui::Ui, title: &str, content: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::group(ui.style())
        .inner_margin(12)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                preference_heading(ui, title);
                ui.add_space(8.0);
                content(ui);
            });
        });
}

fn hint(ui: &mut egui::Ui, text: &str) {
    ui.add(
        egui::Label::new(egui::RichText::new(text).color(style::secondary(ui.visuals()))).wrap(),
    );
}

fn action_row(
    ui: &mut egui::Ui,
    title: &str,
    description: &str,
    command: &str,
    enabled: bool,
) -> bool {
    let mut clicked = false;
    let button = |ui: &mut egui::Ui| {
        ui.add_enabled(enabled, egui::Button::new(command))
            .on_disabled_hover_text(
                "Finish the current operation and close its review or editor window first.",
            )
            .clicked()
    };
    if ui.available_width() >= 680.0 {
        ui.horizontal(|ui| {
            let description_width = (ui.available_width() - 245.0).max(240.0);
            ui.allocate_ui_with_layout(
                egui::vec2(description_width, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.strong(title);
                    ui.add(egui::Label::new(description).wrap());
                },
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                clicked = button(ui);
            });
        });
    } else {
        ui.strong(title);
        ui.add(egui::Label::new(description).wrap());
        ui.add_space(4.0);
        clicked = button(ui);
    }
    clicked
}

//! Build, review, and installation presentation.
use super::*;

mod progress;
pub(super) use progress::message as progress_message;
pub(super) use progress::{Activity, InstallStatus};

impl PackageAuthoringApp {
    pub(super) fn draw_build_status_window(&mut self, ctx: &egui::Context) {
        if !self.build_status_open {
            return;
        }

        let building = self.build_receiver.is_some();
        let can_install = !building
            && matches!(self.latest_build, Some(Ok(_)))
            && self.install_receiver.is_none()
            && self.catalog_receiver.is_none()
            && self.catalog_worker.is_none()
            && !self.catalog_reload_pending;
        let staging_directory = self
            .latest_build
            .as_ref()
            .and_then(|result| result.as_ref().ok())
            .map(|report| report.run_directory.clone());
        let mut open = true;
        let mut close_requested = false;
        let mut open_staging_requested = false;
        let mut install_requested = false;

        egui::Window::new("Build & Install")
            .id(egui::Id::new("parhelion_build_status"))
            .collapsible(false)
            .resizable(true)
            .default_width(820.0)
            .default_height(620.0)
            .max_size(ctx.screen_rect().size() - egui::vec2(48.0, 48.0))
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_min_width(340.0);
                workbench_style(ui);
                progress::draw_steps(ui, self);
                match self.build_dialog_step {
                    BuildDialogStep::ReviewInstall => {
                        self.draw_install_confirmation(ui);
                        return;
                    }
                    BuildDialogStep::Install => {
                        self.draw_install_status(ui);
                        return;
                    }
                    BuildDialogStep::Build => {}
                }
                if building {
                    if let Some(current) = &self.build_progress {
                        progress::draw_build(ui, current, self.build_elapsed(Instant::now()), true);
                    } else {
                        ui.heading("Starting Build");
                        ui.spinner();
                    }
                    ui.add_space(12.0);
                    self.build_activity.draw(ui, "build-progress-activity");
                    ui.weak(
                        "You can close this window. The build will continue in the background.",
                    );
                } else {
                    if let Some(progress) = &self.build_progress {
                        ui.label(
                            egui::RichText::new(format!(
                                "Elapsed {}",
                                format_elapsed(progress.elapsed)
                            ))
                            .weak(),
                        );
                    }
                    egui::ScrollArea::vertical()
                        .id_salt("parhelion-build-status-report")
                        .max_height((ui.available_height() - 110.0).max(120.0))
                        .auto_shrink([false, true])
                        .show(ui, |ui| match self.latest_build.as_ref() {
                            Some(Ok(report)) => draw_build_report(ui, report),
                            Some(Err(error)) => {
                                ui.heading(
                                    egui::RichText::new("Build Blocked")
                                        .color(ui.visuals().error_fg_color),
                                );
                                ui.colored_label(ui.visuals().error_fg_color, error);
                                if ui.button("Copy Error").clicked() {
                                    ui.ctx().copy_text(error.clone());
                                }
                            }
                            None => {
                                ui.label("No build result is available.");
                            }
                        });
                }

                if !building {
                    egui::CollapsingHeader::new("Build Activity")
                        .default_open(matches!(self.latest_build, Some(Err(_))))
                        .show(ui, |ui| {
                            self.build_activity.draw(ui, "build-progress-activity")
                        });
                }
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(5.0);
                ui.horizontal_wrapped(|ui| {
                    let mut install_button =
                        egui::Button::new(egui::RichText::new("Review Installation").strong());
                    if can_install {
                        install_button = install_button.fill(ui.visuals().selection.bg_fill);
                    }
                    if ui.add_enabled(can_install, install_button).clicked() {
                        install_requested = true;
                    }
                    if ui
                        .add_enabled(
                            staging_directory.is_some(),
                            egui::Button::new("Open Staging Folder"),
                        )
                        .clicked()
                    {
                        open_staging_requested = true;
                    }
                    if ui
                        .button("Close")
                        .on_hover_text("Closing this window does not stop the build.")
                        .clicked()
                    {
                        close_requested = true;
                    }
                });
            });

        if open_staging_requested
            && let Some(path) = staging_directory.as_deref()
            && let Err(error) = open_directory(path)
        {
            self.log.push(LogEntry::error(error));
        }
        if install_requested {
            self.start_replacement_review();
            self.build_dialog_step = BuildDialogStep::ReviewInstall;
        }
        if close_requested {
            open = false;
        }
        self.build_status_open &= open;
    }

    pub(super) fn draw_install_confirmation(&mut self, ui: &mut egui::Ui) {
        self.poll_replacement_review();
        let Some(Ok(build)) = self.latest_build.as_ref() else {
            self.build_dialog_step = BuildDialogStep::Build;
            return;
        };
        ui.heading("Review Installation");
        egui::ScrollArea::vertical()
            .id_salt("install-review-contents")
            .max_height((ui.available_height() - 56.0).max(120.0))
            .auto_shrink([false, true])
            .show(ui, |ui| {
                reports::draw_summary(ui, build.weapons.len(), build.artifacts.len(),
                    match &self.replacement_review {
                        Some(Ok(_)) if self.replacement_receiver.is_none() => "Ready To Install",
                        Some(Err(_)) => "Needs Attention",
                        _ => "Checking Account Changes",
                    });
                ui.label("This replaces your installed custom weapon set. Include every weapon you want to keep.");
                ui.colored_label(ui.visuals().warn_fg_color, "Close Destiny 2 before installing.");
                ui.add_space(8.0);
                egui::Frame::group(ui.style()).inner_margin(12).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    self.draw_account_changes(ui);
                });
                ui.add_space(8.0);
                egui::CollapsingHeader::new("Installation Details").show(ui, |ui| {
                    reports::path_row(ui, "Staged Run", &build.run_directory);
                    reports::path_row(ui, "Target Packages", &self.packages);
                    reports::path_row(ui, "Backup Folder", Path::new(self.backup_root.trim()));
                    ui.add_space(5.0);
                    ui.strong(format!("Files to Install ({})", build.artifacts.len()));
                    for artifact in &build.artifacts { ui.monospace(&artifact.file_name); }
                });
            });
        ui.add_space(6.0);
        ui.separator();
        let mut install_requested = false;
        ui.horizontal_wrapped(|ui| {
            let label = if self
                .replacement_review
                .as_ref()
                .and_then(|r| r.as_ref().ok())
                .is_some_and(|r| r.removes_account_data())
            {
                "Back Up, Remove & Install"
            } else if self
                .replacement_review
                .as_ref()
                .and_then(|r| r.as_ref().ok())
                .is_some_and(|r| r.changes_account())
            {
                "Back Up, Update & Install"
            } else {
                "Back Up & Install"
            };
            install_requested = ui
                .add_enabled(
                    self.install_receiver.is_none()
                        && matches!(self.replacement_review, Some(Ok(_)))
                        && self.replacement_receiver.is_none()
                        && self.catalog_receiver.is_none()
                        && self.catalog_worker.is_none()
                        && !self.catalog_reload_pending,
                    egui::Button::new(label).fill(ui.visuals().selection.bg_fill),
                )
                .clicked();
            if ui.button("Back to Build").clicked() {
                self.build_dialog_step = BuildDialogStep::Build;
            }
        });
        if install_requested {
            self.start_install();
        }
    }

    pub(super) fn draw_account_changes(&self, ui: &mut egui::Ui) {
        if self.replacement_receiver.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Checking saved weapons and custom perks…");
            });
            ui.add(
                sundial::investment::progress_bar(0.0)
                    .animate(true)
                    .text("Reviewing Account Changes…"),
            );
            ui.ctx().request_repaint_after(Duration::from_millis(100));
            return;
        }
        let review = match &self.replacement_review {
            Some(Ok(review)) => review,
            Some(Err(error)) => {
                ui.colored_label(ui.visuals().error_fg_color, error);
                return;
            }
            None => return,
        };
        if !review.changes_account() {
            ui.label(
                "No saved items, equipment slots, socket selections, or unlocks need changes.",
            );
            return;
        }
        let Some(cleanup) = review.account_cleanup() else {
            return;
        };
        ui.strong("Account Changes");
        for movement in &cleanup.slot_moves {
            let name = self
                .catalog
                .as_ref()
                .map(|catalog| catalog.plug_label(movement.definition_hash, false))
                .unwrap_or_else(|| format!("Item 0x{:08X}", movement.definition_hash));
            let location = format!(
                "Character {} · {}",
                movement.character_index + 1,
                movement.source_label()
            );
            match movement.outcome {
                sundial::investment::AuthoredMoveOutcome::MovedToInventory => {
                    ui.label(format!(
                        "Move {name} from {location} to {} inventory.",
                        movement.destination_label()
                    ));
                }
                sundial::investment::AuthoredMoveOutcome::DeletedInventoryFull => {
                    ui.colored_label(ui.visuals().error_fg_color, format!("Delete {name} from {location}. There is no room in inventory for its new {} slot.", movement.destination_label()));
                }
            }
        }
        if !cleanup.slot_moves.is_empty() {
            ui.label("Moved weapons keep their instance IDs, rolls, and saved item details. Their previous equipment slots will be empty. Deleted copies are included in the account backup.");
        }
        let count = cleanup.removed_items.values().sum::<usize>();
        if count > 0 {
            ui.label(format!(
                "Remove {count} saved {}:",
                if count == 1 { "item" } else { "items" }
            ));
            for (hash, count) in &cleanup.removed_items {
                let name = self
                    .catalog
                    .as_ref()
                    .map(|catalog| catalog.plug_label(*hash, false));
                ui.label(format!(
                    "{} × {count}",
                    name.unwrap_or_else(|| format!("Item 0x{hash:08X}"))
                ));
            }
            ui.label("Applies to every character, including equipped items. Removed equipment leaves empty slots.");
        }
        for (count, label) in [
            (cleanup.cleared_plugs, "custom plug references"),
            (cleanup.cleared_unlocks, "Collections unlocks"),
            (cleanup.removed_reward_rules, "reward rules"),
        ] {
            if count > 0 {
                ui.label(format!("Clear {count} {label}."));
            }
        }
        for change in review.socket_changes() {
            let Some(count) = cleanup.resized_items.get(&change.definition_hash) else {
                continue;
            };
            let name = self
                .catalog
                .as_ref()
                .map(|catalog| catalog.plug_label(change.definition_hash, false))
                .unwrap_or_else(|| format!("Item 0x{:08X}", change.definition_hash));
            ui.label(format!(
                "Update {count} saved {name} socket lists from {} to {} sockets.",
                change.previous_socket_count,
                change.default_plugs.len(),
            ));
            ui.label(if change.default_plugs.len() > change.previous_socket_count {
                "Existing socket selections are kept. Added sockets use the new definition's defaults."
            } else {
                "Selections in the remaining sockets are kept. Selections in removed sockets are deleted."
            });
        }
        reports::path_row(ui, "Account", &cleanup.settings_path);
    }

    pub(super) fn draw_install_status(&mut self, ui: &mut egui::Ui) {
        if self.install_receiver.is_some() {
            self.install_status.draw(ui, true);
            ui.weak(
                "Keep Destiny 2 closed. You can close this window while installation continues.",
            );
            return;
        }
        ui.weak(format!(
            "Elapsed {}",
            format_elapsed(self.install_status.elapsed)
        ));
        egui::ScrollArea::vertical()
            .id_salt("install-result-contents")
            .max_height((ui.available_height() - 90.0).max(120.0))
            .auto_shrink([false, true])
            .show(ui, |ui| match &self.latest_install {
                Some(Ok(report)) => {
                    draw_install_report(ui, report);
                }
                Some(Err(error)) => {
                    ui.heading("Installation Blocked");
                    ui.colored_label(ui.visuals().error_fg_color, error);
                }
                None => {
                    ui.label("No installation result is available.");
                }
            });
        egui::CollapsingHeader::new("Installation Activity")
            .default_open(matches!(self.latest_install, Some(Err(_))))
            .show(ui, |ui| {
                self.install_status
                    .activity
                    .draw(ui, "install-progress-activity")
            });
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            if ui.button("Close").clicked() {
                self.build_status_open = false;
            }
            if matches!(self.latest_install, Some(Err(_))) && ui.button("Review Again").clicked() {
                self.start_replacement_review();
                self.build_dialog_step = BuildDialogStep::ReviewInstall;
            }
        });
    }
}

//! Build, review, and installation presentation.
use super::*;

mod progress;
pub(super) use progress::{Activity, BuildFraction, InstallStatus};
#[cfg(test)]
mod tests;

/// A library recipe a build stopped on, and whether it is the one open in the editor.
struct BuildBlocker {
    path: PathBuf,
    name: String,
    open: bool,
}

fn draw_mods(ui: &mut egui::Ui, mods: &[crate::ModReport]) {
    if mods.is_empty() {
        return;
    }
    ui.strong("Managed Mods");
    ui.label("Each import source has its own mod, which can be removed on its own. Parhelion holds a source's rows of data Sunrise can't merge, and the source keeps everything else and needs Parhelion to load. Changed folders are backed up together.");
    if mods.iter().any(|member| {
        member.id == sundial::package_authoring::mod_content::ID
            && member.status == crate::ModStatus::Updated
    }) && mods.iter().any(|member| {
        member.id != sundial::package_authoring::mod_content::ID
            && member.status == crate::ModStatus::Installed
    }) {
        ui.label("Imported resources move into the new importer mods listed below. Parhelion and the importer mods change in one transaction.");
    }
    for member in mods {
        let title = format!("{}: {}", member.name, member.status.label());
        if member.packages.is_empty() {
            ui.label(title);
        } else {
            ui.collapsing(title, |ui| {
                for package in &member.packages {
                    ui.monospace(package);
                }
            });
        }
        if let Some(note) = &member.note {
            ui.weak(note);
        }
    }
}

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
        let blocker = self.build_blocker_entry();
        let mut open = true;
        let mut close_requested = false;
        let mut open_staging_requested = false;
        let mut install_requested = false;
        let mut remove_blocker_requested = false;
        let mut open_blocker_requested = false;

        egui::Window::new("Build & Install")
            .id(egui::Id::new("parhelion_build_status"))
            .collapsible(false)
            .resizable(true)
            .default_width(820.0)
            .default_height(620.0)
            .max_size(ctx.content_rect().size() - egui::vec2(48.0, 48.0))
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_min_width(340.0);
                // Keep the height selected at the window border, even when a phase has little text.
                ui.set_min_height(ui.available_height());
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
                egui::Panel::bottom("build-status-footer")
                    .frame(egui::Frame::NONE)
                    .show_separator_line(false)
                    .show(ui, |ui| {
                        ui.add_space(8.0);
                        ui.separator();
                        ui.add_space(5.0);
                        ui.horizontal_wrapped(|ui| {
                            let mut install_button = egui::Button::new(
                                egui::RichText::new("Review Installation").strong(),
                            );
                            if can_install {
                                install_button =
                                    install_button.fill(ui.visuals().selection.bg_fill);
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
                            // A build runs to its end once started. It writes only its own staging
                            // run, so leaving it running is safe, and the window says so.
                            let close = if building {
                                ui.button("Hide").on_hover_text(
                                    "The build keeps running. Reopen this window from Build & Install Status.",
                                )
                            } else {
                                ui.button("Close")
                            };
                            if close.clicked() {
                                close_requested = true;
                            }
                        });
                    });
                if building {
                    if let Some(current) = &self.build_progress {
                        progress::draw_build(ui, current, self.build_elapsed(Instant::now()), true);
                    } else {
                        ui.heading("Starting Build");
                        ui.spinner();
                    }
                    ui.add_space(12.0);
                    self.build_activity
                        .draw(ui, "build-progress-activity", Some(0.0));
                } else {
                    if let Some(progress) = &self.build_progress {
                        ui.weak(format!("Elapsed {}", format_elapsed(progress.elapsed)));
                    }
                    egui::ScrollArea::vertical()
                        .id_salt("parhelion-build-status-report")
                        .max_height(ui.available_height().max(0.0))
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            match self.latest_build.as_ref() {
                                Some(Ok(report)) => draw_build_report(ui, report),
                                Some(Err(error)) => {
                                    ui.heading(
                                        egui::RichText::new("Build Blocked")
                                            .color(ui.visuals().error_fg_color),
                                    );
                                    ui.colored_label(ui.visuals().error_fg_color, error);
                                    ui.horizontal_wrapped(|ui| {
                                        if ui.button("Copy Error").clicked() {
                                            ui.ctx().copy_text(error.clone());
                                        }
                                        let Some(blocker) = &blocker else {
                                            return;
                                        };
                                        if ui.button("Remove from Build").clicked() {
                                            remove_blocker_requested = true;
                                        }
                                        if !blocker.open && ui.button("Open Recipe").clicked() {
                                            open_blocker_requested = true;
                                        }
                                    });
                                }
                                None => {
                                    ui.label("No build result.");
                                }
                            }
                            egui::CollapsingHeader::new("Build Activity")
                                .default_open(matches!(self.latest_build, Some(Err(_))))
                                .show(ui, |ui| {
                                    self.build_activity
                                        .draw(ui, "build-progress-activity", None)
                                });
                        });
                }
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
        if let Some(blocker) = blocker {
            if remove_blocker_requested {
                self.remove_from_build(&blocker);
            } else if open_blocker_requested {
                self.build_status_open = false;
                self.request_recipe_action(PendingRecipeAction::Open(blocker.path));
            }
        }
    }

    /// The recipe the latest build stopped on, while it is still in the build.
    fn build_blocker_entry(&self) -> Option<BuildBlocker> {
        let namespace = self.build_blocker.as_deref()?;
        let (path, name) = self
            .recipe_entries
            .iter()
            .find(|entry| entry.namespace == namespace)
            .map(|entry| (entry.path.clone(), entry.name.clone()))
            .or_else(|| {
                let path = self.recipe_path.clone()?;
                (self.recipe.namespace == namespace).then(|| (path, self.recipe.name.clone()))
            })?;
        self.enabled_recipe_paths
            .contains(&path)
            .then(|| BuildBlocker {
                open: self.recipe_path.as_ref() == Some(&path),
                path,
                name,
            })
    }

    /// Leaves a recipe out of the build selection.
    fn remove_from_build(&mut self, blocker: &BuildBlocker) {
        let mut selected = self.enabled_recipe_paths.clone();
        selected.remove(&blocker.path);
        match self.apply_build_selection(selected) {
            Ok(()) => self.log.push(LogEntry::info(format!(
                "Removed {} from the build",
                blocker.name
            ))),
            Err(error) => self.log.push(LogEntry::error(error)),
        }
    }

    pub(super) fn draw_install_confirmation(&mut self, ui: &mut egui::Ui) {
        self.poll_replacement_review();
        if self.install_candidate().is_none() {
            self.build_dialog_step = BuildDialogStep::Build;
            return;
        }
        let mut install_requested = false;
        egui::Panel::bottom("install-review-footer")
            .frame(egui::Frame::NONE)
            .show_separator_line(false)
            .show(ui, |ui| {
                ui.add_space(6.0);
                ui.separator();
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
                        .is_some_and(|r| r.changes_account() || r.grants_items())
                    {
                        "Back Up, Update & Install"
                    } else {
                        "Back Up & Install"
                    };
                    install_requested = ui
                        .add_enabled(
                            self.install_receiver.is_none()
                                && self.account_resync_receiver.is_none()
                                && matches!(self.replacement_review, Some(Ok(_)))
                                && self.replacement_receiver.is_none()
                                && self.catalog_receiver.is_none()
                                && self.catalog_worker.is_none()
                                && !self.catalog_reload_pending
                                && !self.technical_markers_busy()
                                && !self.importer_busy(),
                            egui::Button::new(label).fill(ui.visuals().selection.bg_fill),
                        )
                        .clicked();
                    if ui.button("Back to Build").clicked() {
                        self.restore = None;
                        self.build_dialog_step = BuildDialogStep::Build;
                    }
                });
            });
        let restoring = self.restore.is_some();
        let Some(build) = self.install_candidate() else {
            return;
        };
        ui.heading(if restoring {
            "Review Restore"
        } else {
            "Review Installation"
        });
        if restoring {
            ui.label(
                "Installs the build you last installed for this runtime again, without a rebuild.",
            );
        }
        egui::ScrollArea::vertical()
            .id_salt("install-review-contents")
            .max_height(ui.available_height().max(0.0))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                reports::draw_summary(
                    ui,
                    build,
                    match &self.replacement_review {
                        Some(Ok(_)) if self.replacement_receiver.is_none() => "Ready to Install",
                        Some(Err(_)) => "Needs Attention",
                        _ => "Checking Account Changes",
                    },
                );
                ui.label(
                    "Replaces your installed custom items. Include every item you want to keep.",
                );
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    "Close Destiny 2 before installing.",
                );
                ui.add_space(8.0);
                egui::Frame::group(ui.style())
                    .inner_margin(12)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        self.draw_account_changes(ui);
                    });
                if self.replacement_status.progress.is_some() {
                    egui::CollapsingHeader::new("Account Review Activity").show(ui, |ui| {
                        self.replacement_status
                            .activity
                            .draw(ui, "review-progress-activity", None);
                    });
                }
                ui.add_space(8.0);
                egui::CollapsingHeader::new("Installation Details").show(ui, |ui| {
                    reports::path_row(
                        ui,
                        if restoring {
                            "Kept Build"
                        } else {
                            "Staged Run"
                        },
                        &build.run_directory,
                    );
                    reports::path_row(ui, "Target Packages", &self.packages);
                    reports::path_row(ui, "Backup Folder", Path::new(self.backup_root.trim()));
                    ui.add_space(5.0);
                    ui.strong(format!("Files to Install ({})", build.artifacts.len()));
                    for artifact in &build.artifacts {
                        ui.monospace(&artifact.file_name);
                    }
                });
            });
        if install_requested {
            self.start_install();
        }
    }

    pub(super) fn draw_account_changes(&self, ui: &mut egui::Ui) {
        if self.replacement_receiver.is_some() {
            let status = &self.replacement_status;
            let elapsed = status
                .started
                .map_or(status.elapsed, |started| started.elapsed());
            let progress = status.progress.as_ref();
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(
                    progress
                        .and_then(|p| p.current_artifact.as_deref())
                        .unwrap_or("Starting account review…"),
                );
            });
            ui.weak(format!("Elapsed {}", format_elapsed(elapsed)));
            let fraction = progress.filter(|p| p.total > 0).map_or(0.0, |p| {
                (p.completed as f32 / p.total as f32).clamp(0.0, 1.0)
            });
            ui.add(
                sundial::investment::progress_bar(fraction)
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
        draw_mods(ui, review.mods());
        let grant_error = matches!(review.item_grants(), Some(Err(_)));
        if !review.changes_account()
            && !review.grants_items()
            && !grant_error
            && review.collection_unlocks() == 0
        {
            ui.label("No account changes needed.");
            return;
        }
        ui.strong("Account Changes");
        self.draw_cleanup(ui, review);
        self.draw_grants(ui, review);
    }

    /// The saved items, references and sockets the install removes or updates.
    fn draw_cleanup(&self, ui: &mut egui::Ui, review: &crate::install::ReplacementReview) {
        if !review.changes_account() {
            return;
        }
        let Some(cleanup) = review.account_cleanup() else {
            return;
        };
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
                sundial::package_authoring::account::AuthoredMoveOutcome::MovedToInventory => {
                    ui.label(format!(
                        "Move {name} from {location} to {} inventory.",
                        movement.destination_label()
                    ));
                }
                sundial::package_authoring::account::AuthoredMoveOutcome::DeletedInventoryFull => {
                    ui.colored_label(
                        ui.visuals().error_fg_color,
                        format!(
                            "Delete {name} from {location}. No inventory room for its new {} slot.",
                            movement.destination_label()
                        ),
                    );
                }
            }
        }
        if !cleanup.slot_moves.is_empty() {
            ui.label("Moved weapons keep their rolls. Their old slots will be empty. Deleted copies are in the account backup.");
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
            ui.label("Includes equipped items on every character. Their slots will be empty.");
        }
        for (count, singular, plural) in [
            (
                cleanup.cleared_plugs,
                "custom plug reference",
                "custom plug references",
            ),
            (
                cleanup.cleared_unlocks,
                "Collections unlock",
                "Collections unlocks",
            ),
            (cleanup.removed_reward_rules, "reward rule", "reward rules"),
        ] {
            if count > 0 {
                let label = if count == 1 { singular } else { plural };
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
            if change.default_plugs.len() == change.previous_socket_count {
                ui.label(format!("Update {count} saved {name} to new default plugs."));
                continue;
            }
            ui.label(format!(
                "Update {count} saved {name} from {} to {} sockets.",
                change.previous_socket_count,
                change.default_plugs.len(),
            ));
            ui.label(
                if change.default_plugs.len() > change.previous_socket_count {
                    "Existing choices are kept. New sockets use their defaults."
                } else {
                    "Choices in removed sockets are deleted."
                },
            );
        }
        reports::path_row(ui, "Account", &cleanup.settings_path);
    }

    /// The subclasses and shaders the install adds after its cleanup, the characters that equip
    /// them, and the Collections unlocks it sets. A copy the account already holds is left
    /// where it is, so a repeat install keeps each character's equipment and ability choices.
    fn draw_grants(&self, ui: &mut egui::Ui, review: &crate::install::ReplacementReview) {
        let unlocks = review.collection_unlocks();
        if unlocks > 0 {
            ui.label(format!(
                "Unlock {unlocks} custom {} in Collections where not already unlocked.",
                if unlocks == 1 { "item" } else { "items" }
            ));
        }
        let grants = match review.item_grants() {
            Some(Ok(grants)) => grants,
            Some(Err(error)) => {
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    format!("Installs, but custom Subclasses and shaders can't be added: {error}"),
                );
                return;
            }
            None => return,
        };
        let name = |hash: u32| {
            self.install_candidate()
                .and_then(|build| build.weapons.iter().find(|item| item.item_hash == hash))
                .map(|item| item.name.clone())
                .or_else(|| {
                    self.catalog
                        .as_ref()
                        .map(|catalog| catalog.plug_label(hash, false))
                })
                .unwrap_or_else(|| format!("Item 0x{hash:08X}"))
        };
        let place = |outcome: &sundial::package_authoring::account::AuthoredGrantOutcome| {
            outcome.character_index.map_or_else(
                || "the profile inventory".to_owned(),
                |index| format!("Character {}", index + 1),
            )
        };
        for outcome in &grants.added {
            let equipped = grants.equipped.contains(outcome);
            let reset = grants.abilities_reset.contains(outcome);
            let mut line = if equipped {
                format!(
                    "Add and equip {} on {}.",
                    name(outcome.item_hash),
                    place(outcome)
                )
            } else {
                format!("Add {} to {}.", name(outcome.item_hash), place(outcome))
            };
            if reset {
                line.push_str(" Ability choices there start at the defaults.");
            }
            ui.label(line);
        }
        for outcome in &grants.full {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                format!(
                    "No room to add {} to {}.",
                    name(outcome.item_hash),
                    place(outcome)
                ),
            );
        }
        if !grants.added.is_empty() {
            ui.label("Characters that already hold these items keep their equipment.");
            reports::path_row(ui, "Account", &grants.account_path);
        }
    }

    pub(super) fn draw_install_status(&mut self, ui: &mut egui::Ui) {
        let installing = self.install_receiver.is_some();
        egui::Panel::bottom("install-status-footer")
            .frame(egui::Frame::NONE)
            .show_separator_line(false)
            .show(ui, |ui| {
                ui.separator();
                if installing {
                    ui.weak("Keep Destiny 2 closed.");
                    return;
                }
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Close").clicked() {
                        self.build_status_open = false;
                    }
                    if matches!(self.latest_install, Some(Err(_)))
                        && ui.button("Review Again").clicked()
                    {
                        self.start_replacement_review();
                        self.build_dialog_step = BuildDialogStep::ReviewInstall;
                    }
                });
            });
        if installing {
            self.install_status.draw(ui, true);
            return;
        }
        ui.weak(format!(
            "Elapsed {}",
            format_elapsed(self.install_status.elapsed)
        ));
        egui::ScrollArea::vertical()
            .id_salt("install-result-contents")
            .max_height(ui.available_height().max(0.0))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                match &self.latest_install {
                    Some(Ok(report)) => draw_install_report(ui, report),
                    Some(Err(error)) => {
                        ui.heading("Installation Blocked");
                        ui.colored_label(ui.visuals().error_fg_color, error);
                    }
                    None => {
                        ui.label("No installation result.");
                    }
                }
                egui::CollapsingHeader::new("Installation Activity")
                    .default_open(matches!(self.latest_install, Some(Err(_))))
                    .show(ui, |ui| {
                        self.install_status
                            .activity
                            .draw(ui, "install-progress-activity", None)
                    });
            });
    }
}

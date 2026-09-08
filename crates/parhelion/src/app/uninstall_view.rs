//! Review, confirmation and result share one window. No filesystem mutation on review.
use super::*;
use crate::install::{
    RecoveryRequest, UninstallPlan, UninstallReport, preview_uninstall_with_account_cleanup,
    recover_interrupted_install, uninstall_custom_packages,
};

#[derive(Default)]
pub(super) struct UninstallUi {
    pub open: bool,
    acknowledged: bool,
    cleanup_account: bool,
    review_receiver: Option<Receiver<Result<UninstallPlan, String>>>,
    remove_receiver: Option<Receiver<Result<UninstallReport, String>>>,
    plan: Option<UninstallPlan>,
    error: Option<String>,
    report: Option<UninstallReport>,
}

impl UninstallUi {
    pub(super) fn busy(&self) -> bool {
        self.review_receiver.is_some() || self.remove_receiver.is_some()
    }
}

impl PackageAuthoringApp {
    pub(super) fn start_uninstall_review(&mut self, recover: bool) {
        if self.has_background_work() || self.perk_editor.is_some() || self.icon_editor.is_some() {
            return;
        }
        let packages = self.packages.clone();
        let backup_root = PathBuf::from(self.backup_root.trim());
        if recover {
            self.drop_loaded_catalog();
        }
        let (sender, receiver) = mpsc::channel();
        self.uninstall = UninstallUi {
            open: true,
            review_receiver: Some(receiver),
            ..Default::default()
        };
        thread::spawn(move || {
            let result = (|| {
                if recover {
                    recover_interrupted_install(&RecoveryRequest {
                        target_packages_directory: packages.clone(),
                        backup_root,
                        game_running_check: sundial::package_authoring::destiny_is_running,
                    })
                    .map_err(|error| error.to_string())?;
                }
                preview_uninstall_with_account_cleanup(&packages).map_err(|error| error.to_string())
            })();
            let _ = sender.send(result);
        });
        if recover {
            self.packages_changed = true;
            self.catalog_force_rebuild = true;
            self.log.push(LogEntry::info(
                "Started recovery of an interrupted package operation",
            ));
        }
    }

    fn start_uninstall(&mut self) {
        if !self.uninstall.acknowledged || self.has_background_work() {
            return;
        }
        let Some(plan) = self.uninstall.plan.clone() else {
            return;
        };
        let plan = if self.uninstall.cleanup_account {
            plan
        } else {
            plan.without_account_cleanup()
        };
        let backup = PathBuf::from(self.backup_root.trim());
        self.drop_loaded_catalog();
        self.catalog_load_requested = true;
        let (sender, receiver) = mpsc::channel();
        self.uninstall.remove_receiver = Some(receiver);
        thread::spawn(move || {
            let result = uninstall_custom_packages(
                &plan,
                &backup,
                sundial::package_authoring::destiny_is_running,
            )
            .map_err(|error| error.to_string());
            let _ = sender.send(result);
        });
        self.log.push(LogEntry::info(
            if self.uninstall.cleanup_account { "Started custom package uninstall with confirmed account cleanup; recipes will be preserved" } else { "Started package-only uninstall; recipes and account data will be preserved" },
        ));
    }

    pub(super) fn poll_uninstall(&mut self) {
        if let Some(result) = received(&self.uninstall.review_receiver) {
            self.uninstall.review_receiver = None;
            match result {
                Ok(plan) => {
                    self.uninstall.cleanup_account = plan.account_cleanup().is_some();
                    self.uninstall.plan = Some(plan);
                }
                Err(error) => {
                    self.log
                        .push(LogEntry::error(format!("Uninstall review: {error}")));
                    self.uninstall.error = Some(error);
                }
            }
        }
        if let Some(result) = received(&self.uninstall.remove_receiver) {
            self.uninstall.remove_receiver = None;
            self.uninstall.plan = None;
            self.packages_changed = true;
            self.catalog_force_rebuild = true;
            self.invalidate_results();
            match result {
                Ok(report) => {
                    self.log.push(LogEntry::info(format!(
                        "Removed {} custom packages; recovery backup: {}",
                        report.removed_files.len(),
                        report.backup_directory.display()
                    )));
                    self.uninstall.report = Some(report);
                }
                Err(error) => {
                    self.log
                        .push(LogEntry::error(format!("Uninstall stopped: {error}")));
                    self.uninstall.error = Some(error);
                }
            }
        }
    }

    pub(super) fn draw_uninstall_window(&mut self, ctx: &egui::Context) {
        let mut close = false;
        let mut remove = false;
        let mut retry = false;
        let mut recover = false;
        egui::Window::new("Uninstall Custom Packages").collapsible(false)
            .default_width(580.0).default_height(430.0).resizable(true).show(ctx, |ui| {
                workbench_style(ui);
                egui::ScrollArea::vertical().max_height((ui.available_height() - 65.0).max(100.0)).show(ui, |ui| {
                    if self.uninstall.busy() {
                        ui.spinner();
                        ui.label(if self.uninstall.remove_receiver.is_some() { "Backing Up and Uninstalling…" } else { "Checking Package Set…" });
                    } else if let Some(report) = &self.uninstall.report {
                        ui.heading("Custom Packages Uninstalled");
                        ui.label(format!("Removed {} Parhelion package files. Stock packages and recipes were kept.", report.removed_files.len()));
                        if let Some(path) = &report.cleaned_account {
                            ui.label(format!("Removed the reviewed custom items, custom plug references and collection flags from {}. The original account is backed up with the package set; unrelated progress was kept.", path.display()));
                        } else { ui.label("Saved account data was kept unchanged."); }
                        ui.label("Runtime caches were invalidated where present. They will be rebuilt on the next launch.");
                        ui.label(format!("Recovery backup: {}", report.backup_directory.display()));
                        ui.label("This uninstall backup is not removed by automatic package-backup retention.");
                        if ui.button("Open Backup Folder").clicked() && let Err(error) = open_directory(&report.backup_directory) { self.log.push(LogEntry::error(error)); }
                    } else if let Some(error) = &self.uninstall.error {
                        ui.colored_label(ui.visuals().error_fg_color, error);
                        retry = ui.button("Review Again").clicked();
                        ui.label("If a previous package operation was interrupted, close Destiny before recovering it. Recovery restores the pre-operation package set.");
                        recover = ui.button("Recover Interrupted Operation").clicked();
                    } else if let Some(plan) = &self.uninstall.plan {
                        if plan.artifacts().is_empty() { ui.label("No custom Parhelion packages are installed."); }
                        else {
                            ui.label(format!("Remove all {} recognized Parhelion packages from:", plan.artifacts().len()));
                            ui.label(plan.target().display().to_string());
                            ui.collapsing("Package Files", |ui| { for artifact in plan.artifacts() { ui.label(&artifact.file_name); } });
                            ui.label("The complete set is backed up before removal. Stock packages and recipes are kept; relevant runtime caches are refreshed.");
                            if let Some(cleanup) = plan.account_cleanup() {
                                if ui.checkbox(&mut self.uninstall.cleanup_account, "Remove Custom Items and Progression").changed() { self.uninstall.acknowledged = false; }
                                ui.label(format!("Selected account: {}", cleanup.settings_path.display()));
                                ui.label(format!("{} saved item instances, {} custom plug references, {} collection unlocks. Includes equipped weapons and all characters in this account; affected equipment slots will be empty.", cleanup.removed_items.values().sum::<usize>(), cleanup.cleared_plugs, cleanup.cleared_unlocks));
                                ui.collapsing("Affected Items", |ui| { for (hash, count) in &cleanup.removed_items {
                                    let name = self.donor_summaries.iter().find(|item| item.hash == *hash).map(|item| item.name.as_str()).unwrap_or("Custom Item");
                                    ui.label(format!("{name} · 0x{hash:08X} · {count} instance(s)"));
                                } });
                                if cleanup.removed_reward_rules > 0 { ui.label(format!("{} dismantle reward rules referencing custom items will also be removed.", cleanup.removed_reward_rules)); }
                                ui.label("Backs up settings with the packages. Only this set’s exact added item and unlock definitions are cleaned up. Unrelated Collections, XP and other progression stay unchanged. Other saved accounts and backups are not edited.");
                            } else if let Some(error) = plan.account_cleanup_error() {
                                ui.colored_label(ui.visuals().warn_fg_color, format!("Automatic account cleanup unavailable: {error}"));
                            }
                            let confirmation = if self.uninstall.cleanup_account {
                                "Destiny and other account editors are closed. I confirm removal of the listed custom items and progression."
                            } else {
                                "Destiny is closed. I have removed custom items and their unlock data from my saved accounts myself."
                            };
                            ui.checkbox(&mut self.uninstall.acknowledged, confirmation);
                        }
                    }
                });
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    if self.uninstall.plan.as_ref().is_some_and(|plan| !plan.artifacts().is_empty()) {
                        remove = ui.add_enabled(self.uninstall.acknowledged && !self.uninstall.busy(), egui::Button::new("Back Up & Uninstall")).clicked();
                    }
                    close = ui.add_enabled(!self.uninstall.busy(), egui::Button::new(if self.uninstall.report.is_some() { "Done" } else { "Cancel" })).clicked();
                });
            });
        if close {
            self.close_uninstall_review();
        }
        if retry || recover {
            self.start_uninstall_review(recover);
        }
        if remove {
            self.start_uninstall();
        }
    }

    fn close_uninstall_review(&mut self) {
        self.uninstall.open = false;
        // Reviewing alone does not release or change the catalog. Recovery and
        // removal set this flag even on failure, when the disk state is uncertain.
        if self.catalog_force_rebuild {
            self.reset_catalog_load();
        }
    }
}

fn received<T>(receiver: &Option<Receiver<Result<T, String>>>) -> Option<Result<T, String>> {
    match receiver.as_ref()?.try_recv() {
        Ok(result) => Some(result),
        Err(TryRecvError::Empty) => None,
        Err(TryRecvError::Disconnected) => Some(Err("The package operation worker stopped without a result. Review or recover the operation before retrying.".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closing_a_read_only_review_does_not_reload_but_mutations_do() {
        for mutated in [false, true] {
            let mut app = PackageAuthoringApp::default();
            app.uninstall.open = true;
            app.catalog_load_requested = true;
            app.catalog_force_rebuild = mutated;
            let recipe = app.recipe.clone();
            app.close_uninstall_review();
            assert!(!app.uninstall.open);
            assert_eq!(app.catalog_load_requested, !mutated);
            assert_eq!(app.recipe, recipe);
            assert!(!app.uninstall.busy());
        }
    }

    fn find(shape: &egui::Shape, label: &str) -> Option<egui::Rect> {
        match shape {
            egui::Shape::Text(text) if text.galley.job.text == label => {
                Some(text.galley.rect.translate(text.pos.to_vec2()))
            }
            egui::Shape::Vec(shapes) => shapes.iter().find_map(|shape| find(shape, label)),
            _ => None,
        }
    }

    #[test]
    fn uninstall_review_keeps_confirmation_visible_and_does_not_change_the_recipe() {
        for size in [egui::vec2(640.0, 480.0), egui::vec2(900.0, 640.0)] {
            let mut app = PackageAuthoringApp::default();
            app.uninstall.open = true;
            app.uninstall.plan = Some(UninstallPlan::preview_fixture());
            let before = app.recipe.clone();
            app.start_uninstall(); // Cannot start without the explicit acknowledgement.
            assert!(!app.uninstall.busy());
            let ctx = egui::Context::default();
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
            let mut output = egui::FullOutput::default();
            for _ in 0..3 {
                output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(screen),
                        ..Default::default()
                    },
                    |ctx| app.draw_uninstall_window(ctx),
                );
            }
            for label in ["Back Up & Uninstall", "Cancel"] {
                let bounds = output
                    .shapes
                    .iter()
                    .find_map(|shape| find(&shape.shape, label))
                    .expect("footer visible");
                assert!(
                    screen.contains_rect(bounds),
                    "{label} outside {size:?}: {bounds:?}"
                );
            }
            assert_eq!(app.recipe, before);
            assert!(!app.uninstall.busy());
        }
    }

    #[test]
    fn disconnected_uninstall_worker_is_a_visible_error_not_success() {
        let mut app = PackageAuthoringApp::default();
        let (sender, receiver) = mpsc::channel();
        app.uninstall.remove_receiver = Some(receiver);
        drop(sender);
        app.poll_uninstall();
        assert!(app.uninstall.error.is_some());
        assert!(app.uninstall.report.is_none());
        assert!(!app.has_background_work());
    }

    #[test]
    fn account_cleanup_is_optional_and_review_requires_confirmation() {
        let plan = UninstallPlan::preview_fixture();
        assert!(plan.account_cleanup().is_some());
        assert!(
            plan.clone()
                .without_account_cleanup()
                .account_cleanup()
                .is_none()
        );
        let mut app = PackageAuthoringApp::default();
        let (sender, receiver) = mpsc::channel();
        app.uninstall.review_receiver = Some(receiver);
        sender.send(Ok(plan)).unwrap();
        app.poll_uninstall();
        assert!(app.uninstall.cleanup_account);
        assert!(!app.uninstall.acknowledged);
        app.start_uninstall();
        assert!(!app.uninstall.busy());
    }
}

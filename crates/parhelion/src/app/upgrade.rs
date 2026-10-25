//! Review the packages an earlier layout left in stock `packages` once Sunrise loads mods. Those
//! cannot load on the mod loader, so the review asks for a build made for the mod folder, whose
//! install retires them after backing them up.
use super::*;
mod view;
use crate::install::{
    RecoveryRequest, UPGRADE_REVIEW_STEPS, UpgradeReview, preview_upgrade_with_progress,
    recover_interrupted_install,
};
use std::sync::atomic::{AtomicUsize, Ordering};

/// The steps a recovery runs before the review's own, as Parhelion shows them.
const RECOVERY_STEPS: [&str; 2] = [
    "Waiting for model previews to finish reading packages…",
    "Recovering the interrupted package operation…",
];

#[derive(Default)]
pub(super) struct UpgradeUi {
    pub open: bool,
    pub requested: bool,
    automatic: bool,
    recovering: bool,
    review: Option<Receiver<Result<Option<UpgradeReview>, String>>>,
    /// The step the worker has reached, counting recovery's steps before the review's.
    step: Arc<AtomicUsize>,
    plan: Option<UpgradeReview>,
    error: Option<String>,
    /// Build & Stage chosen in the review, held until the catalog the build reads has loaded.
    build: bool,
}

impl UpgradeUi {
    pub(super) fn busy(&self) -> bool {
        self.review.is_some()
    }
    pub(super) fn request(&mut self) {
        self.requested = true;
        self.automatic = true;
    }

    /// What the worker is doing, then how many of this check's steps it has finished and how
    /// many there are.
    pub(super) fn progress(&self) -> (&'static str, usize, usize) {
        let first = if self.recovering {
            0
        } else {
            RECOVERY_STEPS.len()
        };
        let last = RECOVERY_STEPS.len() + UPGRADE_REVIEW_STEPS.len() - 1;
        let step = self.step.load(Ordering::Relaxed).clamp(first, last);
        let message = RECOVERY_STEPS
            .get(step)
            .or_else(|| UPGRADE_REVIEW_STEPS.get(step - RECOVERY_STEPS.len()))
            .copied()
            .unwrap_or_default();
        (message, step - first, last + 1 - first)
    }
}

impl PackageAuthoringApp {
    pub(super) fn start_upgrade_review(&mut self, recover: bool) {
        self.perk_workbench.stop_optional_reads();
        if self.has_background_work()
            || self.perk_workbench.editing()
            || self.presentation_editor.editing()
            || self.icon_editor.is_some()
        {
            return;
        }
        let automatic = self.upgrade.automatic && !recover;
        let packages = self.packages.clone();
        let backup_root = PathBuf::from(self.backup_root.trim());
        if recover {
            self.reset_catalog_load();
        }
        let (sender, receiver) = mpsc::channel();
        let first = if recover { 0 } else { RECOVERY_STEPS.len() };
        let step = Arc::new(AtomicUsize::new(first));
        self.upgrade = UpgradeUi {
            open: !automatic,
            automatic,
            recovering: recover,
            review: Some(receiver),
            step: Arc::clone(&step),
            ..Default::default()
        };
        thread::spawn(move || {
            let result = (|| {
                if recover {
                    if !sundial::ui::model_preview::wait_for_package_reads(
                        super::jobs::PREVIEW_READ_WAIT,
                    ) {
                        return Err(super::jobs::PREVIEW_READ_BUSY.into());
                    }
                    step.store(1, Ordering::Relaxed);
                    recover_interrupted_install(&RecoveryRequest {
                        target_packages_directory: packages.clone(),
                        backup_root,
                        game_running_check: sundial::package_authoring::destiny_is_running,
                        runtime_snapshot_check: sundial::package_authoring::installed_runtime,
                    })
                    .map_err(|e| e.to_string())?;
                }
                preview_upgrade_with_progress(&packages, &mut |index| {
                    step.store(RECOVERY_STEPS.len() + index, Ordering::Relaxed);
                })
                .map_err(|e| e.to_string())
            })();
            let _ = sender.send(result);
        });
        if recover {
            self.packages_changed = true;
            self.catalog_force_rebuild = true;
        }
    }

    pub(super) fn poll_upgrade(&mut self) {
        if self.upgrade.requested {
            self.start_upgrade_review(false);
        }
        if let Some(result) = received(&self.upgrade.review) {
            self.upgrade.review = None;
            match result {
                Ok(plan) => {
                    self.upgrade.open |= plan.is_some();
                    self.upgrade.plan = plan;
                }
                Err(error) => {
                    self.upgrade.open = true;
                    self.upgrade.error = Some(error);
                }
            }
            self.upgrade.automatic = false;
        }
    }

    pub(super) fn draw_upgrade_window(&mut self, ctx: &egui::Context) {
        match view::show(ctx, &self.upgrade, &self.backup_root) {
            view::Action::None => {}
            view::Action::Build => {
                // The open review holds back the catalog a build reads, so on a fresh open the
                // build waits for it to load rather than starting now and stopping without it.
                self.upgrade.open = false;
                self.upgrade.build = true;
                if self.build_ready().is_err() {
                    self.log.push(LogEntry::info(
                        "Build & Stage starts once the weapon catalog loads",
                    ));
                }
            }
            view::Action::Restore => self.start_restore(),
            view::Action::Review => self.start_upgrade_review(false),
            view::Action::Recover => self.start_upgrade_review(true),
            view::Action::Close => self.upgrade.open = false,
        }
    }

    /// Starts the Build & Stage chosen in the review once the catalog has finished loading, or
    /// blocks it with the reason when the load ended without a catalog.
    pub(super) fn start_upgrade_build(&mut self, ctx: &egui::Context) {
        if !std::mem::take(&mut self.upgrade.build) {
            return;
        }
        if self.catalog.is_some() {
            self.start_build_checked(ctx);
        } else {
            self.block_build("The weapon catalog did not load. Reload it, then build again".into());
        }
    }
}

impl PackageAuthoringApp {
    /// Opens the installation review for the build kept for this runtime, in place of a new
    /// build. The install it leads to is an ordinary one.
    fn start_restore(&mut self) {
        if self.build_receiver.is_some() || self.install_receiver.is_some() {
            self.log.push(LogEntry::error(
                "Wait for the build or install to finish before restoring",
            ));
            return;
        }
        let Some(kept) = self.upgrade.plan.as_ref().and_then(|plan| plan.kept()) else {
            return;
        };
        match BuildReport::kept(&kept.run_directory) {
            Ok(report) => {
                self.upgrade.open = false;
                self.restore = Some(report);
                self.build_status_open = true;
                self.build_dialog_step = BuildDialogStep::ReviewInstall;
                self.start_replacement_review();
            }
            Err(error) => {
                self.upgrade.error = Some(format!(
                    "The kept build can't be read: {error}. Build & Stage instead."
                ));
            }
        }
    }
}

fn received<T>(receiver: &Option<Receiver<Result<T, String>>>) -> Option<Result<T, String>> {
    match receiver.as_ref()?.try_recv() {
        Ok(result) => Some(result),
        Err(TryRecvError::Empty) => None,
        Err(TryRecvError::Disconnected) => {
            Some(Err("The upgrade worker stopped without a result".into()))
        }
    }
}

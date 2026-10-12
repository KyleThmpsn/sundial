//! Review the version-gated layout upgrade before publishing the managed mod.
use super::*;
mod view;
use crate::install::{
    RecoveryRequest, UpgradeReport, UpgradeReview, preview_upgrade, recover_interrupted_install,
    upgrade_installation,
};

#[derive(Default)]
pub(super) struct UpgradeUi {
    pub open: bool,
    pub requested: bool,
    automatic: bool,
    recovering: bool,
    review: Option<Receiver<Result<Option<UpgradeReview>, String>>>,
    worker: Option<Receiver<Result<UpgradeReport, String>>>,
    plan: Option<UpgradeReview>,
    report: Option<UpgradeReport>,
    error: Option<String>,
}

impl UpgradeUi {
    pub(super) fn busy(&self) -> bool {
        self.review.is_some() || self.worker.is_some()
    }
    pub(super) fn request(&mut self) {
        self.requested = true;
        self.automatic = true;
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
        self.upgrade = UpgradeUi {
            open: !automatic,
            automatic,
            recovering: recover,
            review: Some(receiver),
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
                    recover_interrupted_install(&RecoveryRequest {
                        target_packages_directory: packages.clone(),
                        backup_root,
                        game_running_check: sundial::package_authoring::destiny_is_running,
                        runtime_snapshot_check: sundial::package_authoring::installed_runtime,
                    })
                    .map_err(|e| e.to_string())?;
                }
                preview_upgrade(&packages).map_err(|e| e.to_string())
            })();
            let _ = sender.send(result);
        });
        if recover {
            self.packages_changed = true;
            self.catalog_force_rebuild = true;
        }
    }

    fn start_upgrade(&mut self) {
        self.perk_workbench.stop_optional_reads();
        if self.has_background_work() {
            return;
        }
        let Some(plan) = self.upgrade.plan.clone() else {
            return;
        };
        self.drop_loaded_catalog();
        self.catalog_load_requested = true;
        let backups = PathBuf::from(self.backup_root.trim());
        let (sender, receiver) = mpsc::channel();
        self.upgrade.worker = Some(receiver);
        thread::spawn(move || {
            if !sundial::ui::model_preview::wait_for_package_reads(super::jobs::PREVIEW_READ_WAIT) {
                let _ = sender.send(Err(super::jobs::PREVIEW_READ_BUSY.into()));
                return;
            }
            let _ = sender.send(
                upgrade_installation(
                    &plan,
                    &backups,
                    sundial::package_authoring::destiny_is_running,
                )
                .map_err(|e| e.to_string()),
            );
        });
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
        if let Some(result) = received(&self.upgrade.worker) {
            self.upgrade.worker = None;
            self.upgrade.plan = None;
            self.packages_changed = true;
            self.catalog_force_rebuild = true;
            self.catalog_load_requested = false;
            self.installed.request();
            self.invalidate_results();
            match result {
                Ok(report) => {
                    self.log.push(LogEntry::info(format!(
                        "Moved {} packages into {}. Recovery backup: {}",
                        report.package_count,
                        report.mod_directory.display(),
                        report.backup_directory.display()
                    )));
                    self.upgrade.report = Some(report);
                }
                Err(error) => {
                    self.log
                        .push(LogEntry::error(format!("Upgrade stopped: {error}")));
                    self.upgrade.error = Some(error);
                }
            }
        }
    }

    pub(super) fn draw_upgrade_window(&mut self, ctx: &egui::Context) {
        match view::show(ctx, &self.upgrade, &self.backup_root) {
            view::Action::None => {}
            view::Action::Upgrade => self.start_upgrade(),
            view::Action::Review => self.start_upgrade_review(false),
            view::Action::Recover => self.start_upgrade_review(true),
            view::Action::Close => self.upgrade.open = false,
            view::Action::OpenBackup => {
                if let Some(report) = &self.upgrade.report
                    && let Err(error) = open_directory(&report.backup_directory)
                {
                    self.log.push(LogEntry::error(error));
                }
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

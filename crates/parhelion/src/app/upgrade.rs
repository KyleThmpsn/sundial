//! Review the packages an earlier layout left in stock `packages` once Sunrise loads mods. Those
//! cannot load on the mod loader, so the review asks for a build made for the mod folder, whose
//! install retires them after backing them up.
use super::*;
mod view;
use crate::install::{
    RecoveryRequest, UpgradeReview, preview_upgrade, recover_interrupted_install,
};

#[derive(Default)]
pub(super) struct UpgradeUi {
    pub open: bool,
    pub requested: bool,
    automatic: bool,
    recovering: bool,
    review: Option<Receiver<Result<Option<UpgradeReview>, String>>>,
    plan: Option<UpgradeReview>,
    error: Option<String>,
}

impl UpgradeUi {
    pub(super) fn busy(&self) -> bool {
        self.review.is_some()
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
                self.upgrade.open = false;
                self.start_build_checked(ctx);
            }
            view::Action::Review => self.start_upgrade_review(false),
            view::Action::Recover => self.start_upgrade_review(true),
            view::Action::Close => self.upgrade.open = false,
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

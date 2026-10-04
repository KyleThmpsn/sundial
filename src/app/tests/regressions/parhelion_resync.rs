use super::*;
use crate::package_authoring::{
    PackageAuthoringPreferences, PackageAuthoringUpdate, PackageAuthoringUtility,
};

struct Authoring(PackageAuthoringUpdate);

impl PackageAuthoringUtility for Authoring {
    fn open(
        &mut self,
        _: &egui::Context,
        _: &std::path::Path,
        _: PackageAuthoringPreferences,
    ) -> Result<(), String> {
        Ok(())
    }

    fn update(&mut self, _: &egui::Context) -> PackageAuthoringUpdate {
        std::mem::take(&mut self.0)
    }
}

#[test]
fn resync_requests_a_host_refresh_without_reloading_packages_or_discarding_edits() {
    let directory = TestDirectory::new("authoring-account-refresh");
    let mut app = app(directory.0.clone());
    app.package_authoring_open = true;
    app.package_authoring = Some(Box::new(Authoring(PackageAuthoringUpdate {
        open: true,
        account_changed: true,
        ..Default::default()
    })));
    app.dirty = true;
    let before = app.document.clone();
    let ctx = egui::Context::default();
    app.update_package_authoring(&ctx);
    assert!(app.workspace_refresh_pending);
    assert!(!app.package_authoring_packages_changed);
    assert!(app.catalog_task.is_none());
    app.refresh_after_focus_if_needed(&ctx, true);
    assert!(app.workspace_refresh_pending);
    assert_eq!(app.document, before);
    assert!(app.dirty);
    app.dirty = false;
    app.package_authoring_busy = true;
    app.refresh_after_focus_if_needed(&ctx, true);
    assert!(
        app.workspace_refresh_pending,
        "a running authoring worker defers refresh"
    );
    assert_eq!(app.document, before);
}

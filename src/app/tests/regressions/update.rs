use super::*;

#[test]
fn restart_waits_for_account_drafts_confirmations_and_parhelion_work() {
    let directory = TestDirectory::new("update-restart-admission");
    let mut app = app(directory.0.clone());
    assert!(app.update_restart_blocker().is_none());
    let original = app.document.clone();
    app.document.json_mut()["future_extension"] = serde_json::json!("unsaved");
    assert!(
        app.update_restart_blocker().is_some(),
        "edits count before dirty tracking runs"
    );
    app.document = original.clone();
    app.json_editor.mark_modified();
    assert!(app.update_restart_blocker().is_some());
    app.json_editor.mark_synced();
    app.package_authoring_busy = true;
    assert!(
        app.update_restart_blocker().is_some(),
        "background work counts even if Parhelion is hidden"
    );
    app.package_authoring_busy = false;
    app.package_authoring_dirty = true;
    assert!(app.update_restart_blocker().is_some());
    app.package_authoring_dirty = false;
    app.confirmation = Some(ConfirmationDialog::Exit);
    assert!(app.update_restart_blocker().is_some());
    app.confirmation = None;
    assert!(app.update_restart_blocker().is_none());
    assert_eq!(app.document, original);
    assert!(!app.exit_confirmed);
    assert!(!app.settings_path.exists());
}

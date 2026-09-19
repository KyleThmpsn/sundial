use super::*;

#[test]
fn activity_log_is_independent_read_only_and_copies_both_severities() {
    let mut app = PackageAuthoringApp {
        activity_log_open: true,
        ..Default::default()
    };
    app.log = ActivityLog::new(LogEntry::info("Build started"));
    app.log.push(LogEntry::error("Example build failure"));
    let before = app.recipe.clone();
    let (output, _) = render(900.0, |ui| {
        app.draw_preferences_window(ui.ctx());
        app.draw_activity_log_window(ui.ctx());
    });
    let labels = text(&output);
    assert!(labels.contains("Activity Log") && labels.contains("Copy Log"));
    assert!(!labels.contains("Parhelion Preferences"));
    assert!(labels.contains("[Error] Example build failure"));
    assert!(labels.contains("[Info] Build started"));
    let exported = app.activity_log_text();
    assert!(
        exported
            .lines()
            .next()
            .unwrap()
            .ends_with("[Error] Example build failure")
    );
    assert!(
        exported
            .lines()
            .nth(1)
            .unwrap()
            .ends_with("[Info] Build started")
    );
    assert!(labels.contains("Open Log Folder"));
    assert_eq!(app.recipe, before);
    assert_eq!(app.log.len(), 2);
}

#[test]
fn build_preferences_stay_locked_during_installation_review() {
    let mut app = PackageAuthoringApp {
        preferences_open: true,
        preferences_page: preferences_view::PreferencesPage::BuildsBackups,
        build_status_open: true,
        build_dialog_step: BuildDialogStep::ReviewInstall,
        ..Default::default()
    };
    let (output, _) = render(900.0, |ui| {
        ui.ctx().enable_accesskit();
        app.draw_preferences_window(ui.ctx());
    });
    assert!(text(&output).contains("locked during a package operation"));
    let tree = output.platform_output.accesskit_update.unwrap();
    for label in [
        "Keep Last",
        "Include Recipe Snapshots in Package Backups",
        "Build from a Temporary Stock Package View",
    ] {
        let node = tree
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some(label))
            .expect(label);
        assert!(node.1.is_disabled(), "{label} must be locked");
    }
    assert!(
        app.build_status_open,
        "reading preferences must not invalidate the staged review"
    );
}

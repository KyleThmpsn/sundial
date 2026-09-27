use super::*;

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
    assert!(text(&output).contains("Locked during a build"));
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

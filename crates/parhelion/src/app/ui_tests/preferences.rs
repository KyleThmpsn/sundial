use super::*;

#[test]
fn failed_preferences_persistence_is_visible_while_session_choices_remain_available() {
    let mut app = PackageAuthoringApp {
        preferences_open: true,
        preferences_error: Some(
            "Unsupported Parhelion preferences schema 2. The saved file was preserved.".into(),
        ),
        show_technical_build: true,
        ..Default::default()
    };
    let ctx = egui::Context::default();
    let mut output = egui::FullOutput::default();
    for _ in 0..3 {
        output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1100.0, 850.0),
                )),
                ..Default::default()
            },
            |ctx| app.draw_preferences_window(ctx),
        );
    }
    assert!(text(&output).contains("Unsupported Parhelion preferences schema 2"));
    assert!(text(&output).contains("Show Technical Build"));
    assert!(app.show_technical_build);
    crate::app::custom_perks::workbench::tests::capture::write(
        &ctx,
        &output,
        "preferences-save-error",
    );
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

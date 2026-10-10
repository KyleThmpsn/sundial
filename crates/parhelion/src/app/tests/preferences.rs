use super::*;

#[test]
fn preview_preferences_are_clickable_without_editing_the_recipe() {
    let mut app = PackageAuthoringApp::default();
    let before = app.recipe.clone();
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut draw = |events| {
        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(700.0, 300.0),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    app.draw_preview_preferences(ui);
                });
            },
        );
        crate::app::custom_perks::workbench::tests::capture::record(&output);
        output
    };
    for label in ["Show Preview FPS", "Play Preview Animations by Default"] {
        let output = draw(Vec::new());
        let at = crate::test_support::driver::accessible(&output, label)
            .unwrap()
            .center();
        for events in crate::test_support::driver::tap(at) {
            draw(events);
        }
    }
    let output = draw(Vec::new());
    crate::app::custom_perks::workbench::tests::capture::write(
        &ctx,
        &output,
        "preview-preferences",
    );
    assert!(app.show_preview_fps);
    assert!(!app.play_preview_animations);
    assert_eq!(app.recipe, before);
}

#[test]
fn failed_preferences_persistence_is_visible_while_session_choices_remain_available() {
    let mut app = PackageAuthoringApp {
        preferences_open: true,
        preferences_error: Some(
            "Unsupported Parhelion preferences schema 2. The saved file was preserved.".into(),
        ),
        show_experimental_options: true,
        ..Default::default()
    };
    let ctx = egui::Context::default();
    let mut output = egui::FullOutput::default();
    for _ in 0..3 {
        output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1100.0, 850.0),
                )),
                ..Default::default()
            },
            |ui| app.draw_preferences_window(ui),
        );
    }
    assert!(text(&output).contains("Unsupported Parhelion preferences schema 2"));
    assert!(text(&output).contains("Enable Experimental Features"));
    assert!(app.show_experimental_options);
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

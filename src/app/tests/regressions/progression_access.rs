use super::*;

#[test]
fn progression_browsing_remains_available_when_editing_is_disabled_or_reset() {
    let directory = TestDirectory::new("progression-access");
    let mut app = app(directory.0.clone());
    let before = app.document.clone();
    let ctx = egui::Context::default();
    app.select_view(ViewMode::Progression);
    assert!(app.view_mode == ViewMode::Progression);

    for editing in [false, true, false] {
        app.preferences.experimental_progression = editing;
        for section in [
            ProgressionSection::Collections,
            ProgressionSection::Unlocks,
            ProgressionSection::Investment,
        ] {
            app.progression_section = section;
            let output = ctx.run(Default::default(), |ctx| {
                app.draw_app_chrome(ctx, None);
                app.draw_active_view(ctx);
            });
            assert!(!output.shapes.is_empty());
            assert_eq!(app.progression_ui.read_only, !editing);
            assert_eq!(app.collections_ui.read_only, !editing);
            assert_eq!(app.document, before);
            assert!(!app.dirty);
        }
    }
    app.reset_preferences_to_defaults(&ctx);
    assert!(app.view_mode == ViewMode::Progression);
    assert!(!app.preferences.experimental_progression);
}

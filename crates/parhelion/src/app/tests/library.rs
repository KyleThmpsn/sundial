use super::*;

#[test]
fn library_restore_confirmation_is_read_only_and_refresh_preserves_open_edits() {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let entries = library.scan().unwrap().entries;
    let path = entries[0].path.clone();
    let original = std::fs::read(&path).unwrap();
    let mut app = PackageAuthoringApp {
        recipe_entries: entries,
        library_open: true,
        recipe_library: Some(library.clone()),
        ..Default::default()
    };
    let (output, _) = render(720.0, |ui| app.draw_library_windows(ui.ctx()));
    assert!(text(&output).contains("Refresh"));
    assert!(text(&output).contains("Restore Default Recipes…"));
    app.restore_defaults_preview = Some(library.prepare_restore_defaults().unwrap());
    let (output, _) = render(720.0, |ui| app.draw_library_windows(ui.ctx()));
    assert!(text(&output).contains("Restore Default Recipes?"));
    assert!(text(&output).contains("Cancel"));
    assert_eq!(std::fs::read(&path).unwrap(), original);
    app.recipe.name = "Unsaved edit".into();
    app.recipe_dirty = true;
    let before = app.recipe.clone();
    app.refresh_recipe_library();
    assert_eq!(app.recipe, before);
    assert!(app.recipe_dirty);
}

#[test]
fn clicking_library_name_opens_the_recipe_instead_of_selecting_text() {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let entries = library.scan().unwrap().entries;
    let path = entries[0].path.clone();
    let name = entries[0].name.clone();
    let mut app = PackageAuthoringApp {
        recipe_entries: entries,
        library_open: true,
        ..Default::default()
    };
    let ctx = egui::Context::default();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(900.0, 1000.0),
        )),
        ..Default::default()
    };
    let mut output = egui::FullOutput::default();
    for _ in 0..3 {
        output = ctx.run_ui(input.clone(), |ui| app.draw_library_windows(ui));
    }
    let pos = text_origin(&output, &name) + egui::vec2(5.0, 5.0);
    for pressed in [true, false] {
        let mut click = input.clone();
        click.events = vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            },
        ];
        let _ = ctx.run_ui(click, |ui| app.draw_library_windows(ui));
    }
    assert_eq!(app.recipe_path.as_ref(), Some(&path));
    assert!(!app.library_open);
    assert!(app.enabled_recipe_paths.is_empty());
    assert!(!app.recipe_dirty);
}

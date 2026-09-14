use super::*;

#[test]
fn library_rows_show_authored_metadata_and_search_it_without_changing_selection() {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let mut app = PackageAuthoringApp {
        recipe_entries: library.scan().unwrap().entries,
        library_open: true,
        ..Default::default()
    };
    let entry = &mut app.recipe_entries[0];
    entry.name = "Library test weapon".into();
    entry.type_name = Some("Micro-Missile Shotgun".into());
    entry.ammo_type = Some(RecipeAmmoType::Special);
    entry.damage_type = Some(crate::recipe::RecipeDamageType::Arc);
    entry.rarity = Some(RecipeRarity::Legendary);
    app.recipe_path = Some(entry.path.clone());
    let before = app.recipe.clone();
    app.library_query = "  test ARC   special micro-missile  ".into();
    let (output, _) = render(900.0, |ui| app.draw_library_windows(ui.ctx()));
    let labels = text(&output);
    assert!(labels.contains("Library test weapon"));
    assert!(labels.contains("Open"));
    assert!(labels.contains("Built-in"));
    assert!(labels.contains("Micro-Missile Shotgun"));
    assert!(labels.contains("Arc · Special"));
    assert!(
        (text_origin(&output, "Library test weapon").x
            - text_origin(&output, "Micro-Missile Shotgun · Legendary · Arc · Special").x)
            .abs()
            < 1.0
    );
    assert!(labels.lines().any(|line| line == "1 recipe"));
    assert_eq!(app.recipe, before);
    assert!(app.enabled_recipe_paths.is_empty());
}

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
        output = ctx.run(input.clone(), |ctx| app.draw_library_windows(ctx));
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
        let _ = ctx.run(click, |ctx| app.draw_library_windows(ctx));
    }
    assert_eq!(app.recipe_path.as_ref(), Some(&path));
    assert!(!app.library_open);
    assert!(app.enabled_recipe_paths.is_empty());
    assert!(!app.recipe_dirty);
}

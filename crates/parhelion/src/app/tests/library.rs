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

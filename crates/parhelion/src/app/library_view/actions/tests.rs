use super::*;

#[test]
fn reopening_the_current_recipe_loads_external_changes_and_guards_unsaved_edits() {
    for dirty in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
        let path = library.root().join("every-end.parhelion.json");
        let mut app = PackageAuthoringApp {
            recipe_library: Some(library.clone()),
            ..Default::default()
        };
        assert!(app.open_recipe_path(&path));
        let mut external = app.recipe.clone();
        external.flavor = "Changed outside Parhelion".into();
        library.save_existing(&path, &external).unwrap();
        if dirty {
            app.recipe.flavor = "Keep this draft until confirmed".into();
            app.synchronize_recipe_dirty();
        }
        let draft = app.recipe.clone();
        app.apply_library_action(
            &egui::Context::default(),
            LibraryAction::Entry(path.clone(), EntryAction::Open),
        );
        if dirty {
            assert_eq!(app.recipe, draft);
            assert_eq!(
                app.pending_recipe_action,
                Some(PendingRecipeAction::Open(path))
            );
            let action = app.pending_recipe_action.take().unwrap();
            assert!(app.execute_recipe_action(action));
        }
        assert_eq!(app.recipe, external);
        assert_eq!(app.recipe_baseline, external);
        assert!(!app.recipe_dirty);
        assert_eq!(
            WeaponRecipe::load_json(app.recipe_path.as_ref().unwrap()).unwrap(),
            external
        );
    }
}

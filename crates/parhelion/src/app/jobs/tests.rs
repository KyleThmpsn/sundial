use super::*;

#[test]
fn build_saves_current_edits_before_snapshot_without_changing_selection() {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let path = library.root().join("every-end.parhelion.json");
    let mut app = PackageAuthoringApp {
        recipe_library: Some(library),
        ..Default::default()
    };
    assert!(app.open_recipe_path(&path));
    app.enabled_recipe_paths = BTreeSet::from([path.clone()]);
    app.recipe.flavor = "Edits saved by Build & Stage".into();
    app.recipe.overrides.socket_columns = vec![Some(WeaponSocketColumnRecipe {
        socket_type: Some(700),
        choices: vec![0x1234.into()],
        ..Default::default()
    })];
    // Do not rely on the previous frame's dirty flag.
    assert!(!app.recipe_dirty);
    app.save_edits_for_build().unwrap();
    assert_eq!(WeaponRecipe::load_json(&path).unwrap(), app.recipe);
    assert_eq!(app.recipe_baseline, app.recipe);
    assert_eq!(app.observed_recipe, app.recipe);
    assert!(!app.recipe_dirty);
    assert_eq!(app.enabled_recipe_paths, BTreeSet::from([path]));
    let request = app.batch_request().unwrap();
    assert_eq!(request.recipes, vec![app.recipe.clone()]);
}

#[test]
fn build_save_failure_preserves_draft_and_does_not_start_worker() {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let path = library.root().join("every-end.parhelion.json");
    let mut app = PackageAuthoringApp {
        recipe_library: Some(library.clone()),
        ..Default::default()
    };
    assert!(app.open_recipe_path(&path));
    let baseline = app.recipe_baseline.clone();
    let mut external = app.recipe.clone();
    external.flavor = "Changed by another editor".into();
    library.save_existing(&path, &external).unwrap();
    app.recipe.flavor = "Keep my draft".into();
    app.synchronize_recipe_dirty();
    let draft = app.recipe.clone();
    app.start_build();
    assert!(app.build_receiver.is_none());
    assert!(
        app.latest_build
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap_err()
            .contains("changed on disk")
    );
    assert_eq!(WeaponRecipe::load_json(&path).unwrap(), external);
    assert_eq!(app.recipe, draft);
    assert_eq!(app.recipe_baseline, baseline);
    assert!(app.recipe_dirty);
}

use super::*;

#[test]
fn saving_custom_perk_stats_does_not_create_a_false_disk_conflict() {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let mut app = PackageAuthoringApp::default();
    let path = library.root().join("every-end.parhelion.json");
    assert!(app.open_recipe_path(&path));
    app.recipe_library = Some(library.clone());
    app.enabled_recipe_paths.insert(path.clone());
    let mut perk = crate::perk::PerkRecipe::new();
    perk.stats = [29, 3, 21, 13]
        .into_iter()
        .map(|definition_index| crate::WeaponStatOverride {
            definition_index,
            value: 10,
        })
        .collect();
    app.recipe
        .overrides
        .socket_plug_variants
        .push(perk.at_socket(0, 0));
    app.save_library_recipe();
    assert!(!app.recipe_dirty);
    app.enabled_recipe_paths.insert(path.clone());
    assert!(app.batch_request().is_ok());
    app.recipe.flavor = "Another local edit".into();
    app.save_edits_for_build().unwrap();
    assert!(app.batch_request().is_ok());
    let mut external = WeaponRecipe::load_json(&path).unwrap();
    external
        .overrides
        .socket_plug_variants
        .last_mut()
        .unwrap()
        .investment_stats[0]
        .value += 1;
    library.save_existing(&path, &external).unwrap();
    assert!(app.batch_request().unwrap_err().contains("changed on disk"));
    assert!(
        library
            .save_existing_if_unchanged(&path, &app.recipe_baseline, &app.recipe)
            .unwrap_err()
            .contains("changed on disk")
    );
}

//! Real library workflows with competing writers and failures at the commit boundary.
use super::*;
use crate::test_support::artifact;

#[test]
fn long_named_recipes_can_be_copied_repeatedly_and_reopened() {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let mut receipts = Vec::new();
    for (index, name) in [
        "A".repeat(54),
        format!("{} Éclipse", "B".repeat(46)),
        "C".repeat(140),
    ]
    .into_iter()
    .enumerate()
    {
        let mut recipe = WeaponRecipe::new_weapon(format!("parhelion.long-copy-{index}")).unwrap();
        recipe.name = name;
        recipe.flavor = "The copy keeps its story.".into();
        recipe.overrides.ammo_type = Some(crate::RecipeAmmoType::Heavy);
        let original_path = library.save_new(&recipe).unwrap();
        let first_path = library.duplicate(&recipe).unwrap();
        let second_path = library.duplicate(&recipe).unwrap();
        let reopened = RecipeLibrary::open(library.root().to_owned()).unwrap();
        assert!(reopened.scan().unwrap().errors.is_empty());
        let first = WeaponRecipe::load_json(&first_path).unwrap();
        let second = WeaponRecipe::load_json(&second_path).unwrap();
        assert_ne!(first.identity, recipe.identity);
        assert_ne!(first.identity, second.identity);
        assert!(first.name.ends_with(" Copy"));
        assert!(second.name.ends_with(" Copy 2"));
        for copy in [&first, &second] {
            copy.validate().unwrap();
            assert_eq!(copy.overrides, recipe.overrides);
            assert_eq!(copy.flavor, recipe.flavor);
            assert_eq!(copy.donor, recipe.donor);
            assert!(copy.identity_is_name_derived());
        }
        assert_eq!(WeaponRecipe::load_json(&original_path).unwrap(), recipe);
        receipts.push(serde_json::json!({"original": recipe, "first": first, "second": second}));
    }
    artifact("long-recipe-copies.json", &serde_json::json!(receipts));
}

#[test]
fn competing_saves_publish_only_one_identity_and_remain_usable() {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let recipe = WeaponRecipe::new_weapon("parhelion.competing-writers").unwrap();
    let gate = std::sync::Barrier::new(2);
    let results = std::thread::scope(|scope| {
        let save = || {
            gate.wait();
            library.save_new(&recipe)
        };
        let first = scope.spawn(save);
        let second = scope.spawn(save);
        [first.join().unwrap(), second.join().unwrap()]
    });
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    let reopened = RecipeLibrary::open(library.root.clone()).unwrap();
    let scan = reopened.scan().unwrap();
    assert!(scan.errors.is_empty());
    assert_eq!(
        scan.entries
            .iter()
            .filter(|entry| entry.namespace == recipe.namespace)
            .count(),
        1
    );
    let copy = reopened.duplicate(&recipe).unwrap();
    let duplicate = WeaponRecipe::load_json(&copy).unwrap();
    assert_ne!(duplicate.identity, recipe.identity);
    artifact(
        "competing-library-writers.json",
        &serde_json::json!({
            "successful_saves": 1,
            "rejected_save": results.into_iter().find_map(Result::err).unwrap(),
            "original": recipe,
            "independent_copy": duplicate,
        }),
    );
}

#[test]
fn a_busy_library_refuses_mutations_and_releases_the_lease_afterward() {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let entries = library.scan().unwrap().entries;
    let enabled = library.enabled_paths(&entries).unwrap();
    let path = library.root.join(BUNDLED_RECIPES[0].0);
    let original = fs::read(&path).unwrap();
    let recipe = WeaponRecipe::load_json(&path).unwrap();
    let delete = library.prepare_delete_recipe(&path).unwrap();
    let restore = library.prepare_restore_defaults().unwrap();
    let single = library.prepare_restore_recipe(&path).unwrap();
    let state = fs::read(library.state_path().unwrap()).unwrap();
    let guard = library.lock().unwrap();
    assert!(
        library
            .save_new(&WeaponRecipe::new_weapon("parhelion.blocked-save").unwrap())
            .is_err()
    );
    assert!(library.duplicate(&recipe).is_err());
    assert!(library.save_existing(&path, &recipe).is_err());
    assert!(
        library
            .save_enabled_paths(&BTreeSet::new(), &entries)
            .is_err()
    );
    assert!(library.delete_recipe(&delete).is_err());
    assert!(library.restore_defaults(&restore).is_err());
    assert!(library.restore_recipe(&single).is_err());
    assert_eq!(fs::read(&path).unwrap(), original);
    assert_eq!(fs::read(library.state_path().unwrap()).unwrap(), state);
    drop(guard);
    library
        .save_enabled_paths(&BTreeSet::new(), &entries)
        .unwrap();
    assert!(library.enabled_paths(&entries).unwrap().is_empty());
    library.save_enabled_paths(&enabled, &entries).unwrap();
    assert_eq!(library.enabled_paths(&entries).unwrap(), enabled);
}

#[test]
fn deletion_with_unreadable_metadata_keeps_the_recipe_and_all_records() {
    for corrupt in ["{broken", "{\"schema\":2}"] {
        let directory = tempfile::tempdir().unwrap();
        let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
        let path = library.root.join(BUNDLED_RECIPES[0].0);
        let preview = library.prepare_delete_recipe(&path).unwrap();
        let original = fs::read(&path).unwrap();
        let state = library.state_path().unwrap();
        fs::write(&state, corrupt).unwrap();
        let error = library.delete_recipe(&preview).unwrap_err();
        assert_eq!(fs::read(&path).unwrap(), original);
        assert_eq!(fs::read_to_string(&state).unwrap(), corrupt);
        assert!(library.removed_bundled().unwrap().is_empty());
        artifact(
            if corrupt.contains('2') {
                "delete-future-state.json"
            } else {
                "delete-invalid-state.json"
            },
            &serde_json::json!({"error": error, "preserved_recipe": WeaponRecipe::load_json(&path).unwrap(), "preserved_state": corrupt}),
        );
    }
}

#[test]
fn failed_final_deletion_restores_metadata_and_preserves_the_backup() {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let entries = library.scan().unwrap().entries;
    let enabled = library.enabled_paths(&entries).unwrap();
    let path = library.root.join(BUNDLED_RECIPES[0].0);
    let preview = library.prepare_delete_recipe(&path).unwrap();
    let original = fs::read(&path).unwrap();
    let state = fs::read(library.state_path().unwrap()).unwrap();
    let error = library
        .delete_recipe_with(&preview, |_| Err("Injected deletion failure".into()))
        .unwrap_err();
    assert!(error.contains("Injected deletion failure"));
    assert_eq!(fs::read(&path).unwrap(), original);
    assert_eq!(fs::read(library.state_path().unwrap()).unwrap(), state);
    assert!(library.removed_bundled().unwrap().is_empty());
    assert_eq!(library.enabled_paths(&entries).unwrap(), enabled);
    let backup = fs::read_dir(directory.path().join("backups/recipes"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert_eq!(
        fs::read(backup.join(BUNDLED_RECIPES[0].0)).unwrap(),
        original
    );
    assert!(error.contains(&fs::canonicalize(&backup).unwrap().display().to_string()));
    artifact(
        "delete-rollback.json",
        &serde_json::json!({"error":error,"state":serde_json::from_slice::<serde_json::Value>(&state).unwrap(),"recipe": WeaponRecipe::load_json(&path).unwrap()}),
    );
    library.delete_recipe(&preview).unwrap();
    assert!(!path.exists());
    assert!(
        !RecipeLibrary::open(library.root.clone())
            .unwrap()
            .root
            .join(BUNDLED_RECIPES[0].0)
            .exists()
    );
}

#[test]
fn deletion_recovery_preserves_an_external_metadata_edit() {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    library
        .enabled_paths(&library.scan().unwrap().entries)
        .unwrap();
    let path = library.root.join(BUNDLED_RECIPES[0].0);
    let preview = library.prepare_delete_recipe(&path).unwrap();
    let external = b"{\"schema\":1,\"enabled_recipes\":[],\"external\":true}";
    let error = library
        .delete_recipe_with(&preview, |_| {
            fs::write(library.state_path().unwrap(), external).unwrap();
            Err("Injected deletion failure after external edit".into())
        })
        .unwrap_err();
    assert!(error.contains("Recovery"));
    assert_eq!(fs::read(library.state_path().unwrap()).unwrap(), external);
    assert!(path.is_file());
    assert!(library.removed_bundled().unwrap().is_empty());
    artifact(
        "delete-external-edit.json",
        &serde_json::json!({"error":error,"preserved_external":serde_json::from_slice::<serde_json::Value>(external).unwrap()}),
    );
}

#[test]
fn selection_and_deletion_keep_unknown_supported_metadata() {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let entries = library.scan().unwrap().entries;
    let mut selected = library.enabled_paths(&entries).unwrap();
    let path = library.state_path().unwrap();
    let mut state: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    state["future_view"] = serde_json::json!({"order":[3,1,2],"nested":{"enabled":true}});
    fs::write(&path, serde_json::to_vec(&state).unwrap()).unwrap();
    selected.remove(&entries[0].path);
    library.save_enabled_paths(&selected, &entries).unwrap();
    library
        .delete_recipe(&library.prepare_delete_recipe(&entries[1].path).unwrap())
        .unwrap();
    let saved: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved["future_view"], state["future_view"]);
    assert!(
        !library
            .enabled_paths(&library.scan().unwrap().entries)
            .unwrap()
            .contains(&entries[0].path)
    );
    artifact("library-unknown-fields.json", &saved);
}

#[cfg(unix)]
#[test]
fn a_redirected_library_cannot_create_recipes_or_update_selection() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("recipes");
    let library = RecipeLibrary::open(root.clone()).unwrap();
    let entries = library.scan().unwrap().entries;
    let state = library.enabled_paths(&entries).unwrap();
    let moved = directory.path().join("moved");
    fs::rename(&root, &moved).unwrap();
    let other = directory.path().join("outside");
    fs::create_dir(&other).unwrap();
    std::os::unix::fs::symlink(&other, &root).unwrap();
    assert!(
        library
            .save_new(&WeaponRecipe::new_weapon("parhelion.redirected").unwrap())
            .is_err()
    );
    assert!(library.save_enabled_paths(&state, &entries).is_err());
    assert!(fs::read_dir(other).unwrap().next().is_none());
}

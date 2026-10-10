use super::*;

#[cfg(unix)]
#[test]
fn exporting_through_a_link_and_parent_cannot_overwrite_a_library_recipe() {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("library")).unwrap();
    let original = library.scan().unwrap().entries[0].path.clone();
    let bytes = fs::read(&original).unwrap();
    let child = library.root().join("child");
    fs::create_dir(&child).unwrap();
    let alias = directory.path().join("alias");
    std::os::unix::fs::symlink(child, &alias).unwrap();
    let target = alias.join("..").join(original.file_name().unwrap());
    let mut edited = WeaponRecipe::load_json(&original).unwrap();
    edited.flavor = "Must not replace the saved recipe".into();
    assert!(
        library
            .export(&edited, &target)
            .unwrap_err()
            .contains("outside")
    );
    assert_eq!(fs::read(&original).unwrap(), bytes);
    let outside = directory.path().join("export.parhelion.json");
    library.export(&edited, &outside).unwrap();
    assert_eq!(WeaponRecipe::load_json(&outside).unwrap(), edited);
}

#[test]
fn selection_edits_preserve_membership_of_unreadable_recipes() {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let scan = library.scan().unwrap();
    let enabled = library.enabled_paths(&scan.entries).unwrap();
    let broken = enabled.iter().next().unwrap();
    let original = fs::read(broken).unwrap();
    fs::write(broken, b"{").unwrap();
    // Deselect everything visible, without touching the hidden broken recipe.
    let visible = library.scan().unwrap().entries;
    library
        .save_enabled_paths(&BTreeSet::new(), &visible)
        .unwrap();
    fs::write(broken, original).unwrap();
    let scan = library.scan().unwrap();
    assert_eq!(
        library.enabled_paths(&scan.entries).unwrap(),
        BTreeSet::from([broken.clone()])
    );
}

#[test]
fn discovery_errors_are_not_reported_as_an_empty_library() {
    let error =
        collect_recipe_paths([Err(std::io::Error::other("directory read failed"))]).unwrap_err();
    assert!(error.contains("directory read failed"));
    let directory = tempfile::tempdir().unwrap();
    let missing = directory.path().join("missing.parhelion.json");
    let error = collect_recipe_paths([Ok(missing.clone())]).unwrap_err();
    assert!(error.contains(&missing.display().to_string()));
}

#[test]
fn unreadable_selection_state_is_not_replaced_with_defaults() {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let state_path = library.state_path().unwrap();
    fs::create_dir(&state_path).unwrap();
    assert!(library.read_state().is_err());
    assert!(
        library
            .enabled_paths(&library.scan().unwrap().entries)
            .is_err()
    );
    assert!(state_path.is_dir());
}

#[test]
fn library_preview_tracks_authored_icon_and_its_inheritance() {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let path = library.root().join(EVERY_END_FILE_NAME);
    let mut recipe = WeaponRecipe::load_json(&path).unwrap();
    for source in [0, 1, 2] {
        recipe.presentation_donor = (source > 0).then(|| crate::WeaponDonorReference {
            item_hash: 0x1234ABCD.into(),
            expected_name: None,
        });
        recipe.icon_donor = (source > 1).then(|| crate::WeaponDonorReference {
            item_hash: 0x5678ABCD.into(),
            expected_name: None,
        });
        recipe.overrides.icon_edit.hue_shift_degrees = 45;
        library.save_existing(&path, &recipe).unwrap();
        let scan = library.scan().unwrap();
        let entry = scan
            .entries
            .iter()
            .find(|entry| entry.path == path)
            .unwrap();
        assert_eq!(
            entry.icon_hash,
            match source {
                0 => recipe.donor.item_hash.parse_u32().unwrap(),
                1 => 0x1234ABCD,
                _ => 0x5678ABCD,
            }
        );
        assert_eq!(entry.icon_edit, recipe.overrides.icon_edit);
    }
}

#[test]
fn bundled_recipes_validate_with_unique_identities() {
    let mut namespaces = BTreeSet::new();
    let mut hashes = BTreeSet::new();
    for (file_name, json) in BUNDLED_RECIPES {
        let recipe = WeaponRecipe::from_json_str(json).unwrap();
        let spec = recipe.to_spec().unwrap();
        match file_name {
            EVERY_END_FILE_NAME => {
                assert_eq!(recipe.namespace, "parhelion.every-end");
                assert!(!recipe.identity_is_name_derived());
                assert_eq!(spec.identity.item_hash, 0x5355_4E44);
            }
            SECOND_SUN_FILE_NAME => {
                assert!(recipe.identity_is_name_derived());
                assert_eq!(
                    (
                        spec.identity.item_hash,
                        spec.identity.collectible_hash,
                        spec.identity.unlock_hash
                    ),
                    (0x757D_33F5, 0xD347_B59A, 0xF137_B0F4),
                );
            }
            _ => {}
        }
        assert!(namespaces.insert(recipe.namespace.clone()));
        for hash in recipe.identity.parsed_hashes(&recipe.namespace).unwrap() {
            assert!(
                hashes.insert(hash),
                "duplicate identity in {file_name}: {hash:08X}"
            );
        }
    }
}

#[test]
fn new_bundles_are_enabled_without_reenabling_disabled_old_bundles() {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let state = RecipeLibraryState {
        schema: LIBRARY_STATE_SCHEMA,
        known_bundled_recipes: BTreeSet::from([
            EVERY_END_FILE_NAME.into(),
            SECOND_SUN_FILE_NAME.into(),
        ]),
        enabled_recipes: BTreeSet::from([EVERY_END_FILE_NAME.into()]),
        ..RecipeLibraryState::default()
    };
    library.write_state(&state).unwrap();
    let scan = library.scan().unwrap();
    let enabled = library.enabled_paths(&scan.entries).unwrap();
    assert_eq!(enabled.len(), BUNDLED_RECIPES.len() - 1);
    assert!(!enabled.contains(&library.root().join(SECOND_SUN_FILE_NAME)));
    assert_eq!(library.enabled_paths(&scan.entries).unwrap(), enabled);
}

#[test]
fn reopening_keeps_user_edits_while_the_bundled_recipe_is_unchanged() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("recipes");
    let library = RecipeLibrary::open(root.clone()).unwrap();
    assert!(library.defaults_refresh().is_none());
    let path = library.root().join(EVERY_END_FILE_NAME);
    fs::write(&path, "user-owned edit").unwrap();

    let library = RecipeLibrary::open(root).unwrap();

    assert!(library.defaults_refresh().is_none());
    assert_eq!(fs::read_to_string(path).unwrap(), "user-owned edit");
}

/// Simulates a copy left by an earlier release by dropping its applied version.
fn forget_applied_version(library: &RecipeLibrary, file_name: &str) {
    let path = library.versions_path().unwrap();
    let mut record: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    record["applied"].as_object_mut().unwrap().remove(file_name);
    fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
}

#[test]
fn a_changed_bundled_recipe_replaces_the_stale_copy_and_keeps_edits_as_a_duplicate() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("recipes");
    let library = RecipeLibrary::open(root.clone()).unwrap();
    let path = library.root().join(EVERY_END_FILE_NAME);
    let mut edited = WeaponRecipe::load_json(&path).unwrap();
    edited.flavor = "My own story.".into();
    library.save_existing(&path, &edited).unwrap();
    let edited_bytes = fs::read(&path).unwrap();
    let custom = WeaponRecipe::new_weapon("parhelion.untouched").unwrap();
    let custom_path = library.save_new(&custom).unwrap();
    forget_applied_version(&library, EVERY_END_FILE_NAME);

    let library = RecipeLibrary::open(root.clone()).unwrap();

    let refresh = library.defaults_refresh().unwrap();
    assert_eq!(refresh.names, vec![edited.name.clone()]);
    assert_eq!(refresh.copies, vec![format!("{} Copy", edited.name)]);
    assert_eq!(fs::read(&path).unwrap(), EVERY_END_TEMPLATE.as_bytes());
    assert_eq!(
        fs::read(refresh.backup.join(EVERY_END_FILE_NAME)).unwrap(),
        edited_bytes
    );
    assert_eq!(WeaponRecipe::load_json(custom_path).unwrap(), custom);
    let scan = library.scan().unwrap();
    let copy = scan
        .entries
        .iter()
        .find(|entry| entry.name == format!("{} Copy", edited.name))
        .expect("the edited copy is kept in the library");
    assert!(!copy.bundled);
    let copy = WeaponRecipe::load_json(&copy.path).unwrap();
    assert_eq!(copy.flavor, edited.flavor);
    assert_ne!(copy.namespace, edited.namespace);

    assert!(
        RecipeLibrary::open(root)
            .unwrap()
            .defaults_refresh()
            .is_none()
    );
}

#[test]
fn an_unavailable_backup_folder_does_not_block_the_recipe_library() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("recipes");
    let library = RecipeLibrary::open(root.clone()).unwrap();
    let path = library.root().join(EVERY_END_FILE_NAME);
    let mut edited = WeaponRecipe::load_json(&path).unwrap();
    edited.flavor = "Keep this edit.".into();
    library.save_existing(&path, &edited).unwrap();
    let original = fs::read(&path).unwrap();
    forget_applied_version(&library, EVERY_END_FILE_NAME);
    fs::write(directory.path().join("backups"), b"not a directory").unwrap();

    let reopened = RecipeLibrary::open(root).unwrap();

    assert!(reopened.defaults_refresh().is_none());
    assert_eq!(fs::read(&path).unwrap(), original);
    assert!(!reopened.scan().unwrap().entries.is_empty());
}

#[test]
fn a_stale_copy_that_is_not_a_recipe_is_only_backed_up() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("recipes");
    let library = RecipeLibrary::open(root.clone()).unwrap();
    let path = library.root().join(EVERY_END_FILE_NAME);
    fs::write(&path, "not a recipe").unwrap();
    forget_applied_version(&library, EVERY_END_FILE_NAME);

    let library = RecipeLibrary::open(root).unwrap();

    let refresh = library.defaults_refresh().unwrap();
    assert!(refresh.copies.is_empty());
    assert_eq!(fs::read(&path).unwrap(), EVERY_END_TEMPLATE.as_bytes());
    assert_eq!(
        fs::read_to_string(refresh.backup.join(EVERY_END_FILE_NAME)).unwrap(),
        "not a recipe"
    );
    assert_eq!(library.scan().unwrap().entries.len(), BUNDLED_RECIPES.len());
}

#[test]
fn a_library_without_applied_versions_refreshes_only_differing_copies() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("recipes");
    let library = RecipeLibrary::open(root.clone()).unwrap();
    let path = library.root().join(SECOND_SUN_FILE_NAME);
    fs::write(&path, "left by an older release").unwrap();
    fs::remove_file(library.versions_path().unwrap()).unwrap();

    let library = RecipeLibrary::open(root).unwrap();

    let refresh = library.defaults_refresh().unwrap();
    assert_eq!(refresh.names.len(), 1);
    assert_eq!(fs::read(&path).unwrap(), SECOND_SUN_TEMPLATE.as_bytes());
    assert!(library.versions_path().unwrap().is_file());
}

#[test]
fn failed_recipe_discovery_preserves_selection_until_the_recipe_is_repaired() {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let scan = library.scan().unwrap();
    let enabled = library.enabled_paths(&scan.entries).unwrap();
    let path = library.root().join(EVERY_END_FILE_NAME);
    let original = fs::read(&path).unwrap();
    let state_before = fs::read(library.state_path().unwrap()).unwrap();

    fs::write(&path, "incomplete edit").unwrap();
    let scan = library.scan().unwrap();
    assert_eq!(scan.errors.len(), 1);
    let available = library.enabled_paths(&scan.entries).unwrap();
    assert!(!available.contains(&path));
    assert_eq!(
        fs::read(library.state_path().unwrap()).unwrap(),
        state_before
    );

    fs::write(&path, original).unwrap();
    let scan = library.scan().unwrap();
    assert!(scan.errors.is_empty());
    assert_eq!(library.enabled_paths(&scan.entries).unwrap(), enabled);
}

#[test]
fn save_new_and_import_never_overwrite_or_duplicate_an_identity() {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let recipe = WeaponRecipe::new_weapon("parhelion.first").unwrap();
    let imported_recipe = WeaponRecipe::new_weapon("parhelion.imported").unwrap();
    let external = directory.path().join("shared.json");
    imported_recipe.save_json(&external).unwrap();

    let first = library.save_new(&recipe).unwrap();
    let (second, imported) = library.import(&external).unwrap();

    assert_ne!(first, second);
    assert_eq!(imported, imported_recipe);
    assert!(first.is_file());
    assert!(second.is_file());
    assert!(library.import(&external).is_err());
    assert_eq!(WeaponRecipe::load_json(&second).unwrap(), imported_recipe);
}

#[test]
fn duplicate_allocates_a_fresh_copy_and_preserves_recipe_content() {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let mut recipe = WeaponRecipe::new_named_weapon_for_donor(
        "Library Copy Fixture",
        0x1234_5678,
        "Fixture Donor",
    )
    .unwrap();
    recipe.flavor = "A copied story.".into();
    recipe.overrides.ammo_type = Some(crate::RecipeAmmoType::Heavy);
    let original_path = library.save_new(&recipe).unwrap();

    let collision = WeaponRecipe::new_named_weapon_for_donor(
        "Library Copy Fixture Copy",
        0x1234_5678,
        "Fixture Donor",
    )
    .unwrap();
    library.save_new(&collision).unwrap();

    let copy_path = library.duplicate(&recipe).unwrap();
    let copy = WeaponRecipe::load_json(&copy_path).unwrap();
    assert_eq!(copy.name, "Library Copy Fixture Copy 2");
    assert_ne!(copy.namespace, recipe.namespace);
    assert_eq!(copy.donor, recipe.donor);
    assert_eq!(copy.overrides, recipe.overrides);
    assert_eq!(copy.flavor, recipe.flavor);
    assert_eq!(WeaponRecipe::load_json(original_path).unwrap(), recipe);
}

#[test]
fn save_existing_is_confined_to_library_root() {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let recipe = WeaponRecipe::new_weapon("parhelion.confined").unwrap();

    assert!(
        library
            .save_existing(&directory.path().join("outside.json"), &recipe)
            .is_err()
    );

    let every_end = library.root().join(EVERY_END_FILE_NAME);
    let traversal = library
        .root()
        .join("..")
        .join("recipes")
        .join(EVERY_END_FILE_NAME);
    assert!(library.save_existing(&traversal, &recipe).is_err());
    assert_eq!(
        WeaponRecipe::load_json(every_end).unwrap(),
        WeaponRecipe::every_end()
    );
}

#[cfg(unix)]
#[test]
fn save_existing_rejects_a_symlinked_recipe() {
    use std::os::unix::fs::symlink;

    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let outside = directory.path().join("outside.json");
    WeaponRecipe::new_weapon("parhelion.outside")
        .unwrap()
        .save_json(&outside)
        .unwrap();
    let link = library.root().join("linked.parhelion.json");
    symlink(&outside, &link).unwrap();

    assert!(
        library
            .save_existing(
                &link,
                &WeaponRecipe::new_weapon("parhelion.replacement").unwrap()
            )
            .is_err()
    );
    assert_eq!(
        WeaponRecipe::load_json(outside).unwrap().namespace,
        "parhelion.outside"
    );
}

#[test]
fn save_rejects_duplicate_namespace_or_any_identity_hash() {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let first = WeaponRecipe::new_weapon("parhelion.unique").unwrap();
    let first_path = library.save_new(&first).unwrap();

    let duplicate_namespace = WeaponRecipe::new_weapon("parhelion.unique").unwrap();
    assert!(library.save_new(&duplicate_namespace).is_err());

    let mut duplicate_hash = WeaponRecipe::new_weapon("parhelion.other").unwrap();
    duplicate_hash.identity.flavor_hash = first.identity.flavor_hash.clone();
    duplicate_hash.identity.name_hash = first.identity.name_hash.clone();
    duplicate_hash.identity.source_hash = first.identity.source_hash.clone();
    let error = library.save_new(&duplicate_hash).unwrap_err();
    assert!(error.contains("identity hash"), "{error}");

    assert_eq!(WeaponRecipe::load_json(first_path).unwrap(), first);
}

#[test]
fn save_existing_excludes_only_itself_from_collision_checks() {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let mut first = WeaponRecipe::new_weapon("parhelion.first-existing").unwrap();
    let first_path = library.save_new(&first).unwrap();
    let second = WeaponRecipe::new_weapon("parhelion.second-existing").unwrap();
    let second_path = library.save_new(&second).unwrap();

    first.flavor = "A safe in-place edit.".to_owned();
    library.save_existing(&first_path, &first).unwrap();
    assert_eq!(WeaponRecipe::load_json(&first_path).unwrap(), first);

    let before = fs::read(&first_path).unwrap();
    first.namespace = second.namespace.clone();
    assert!(library.save_existing(&first_path, &first).is_err());
    assert_eq!(fs::read(first_path).unwrap(), before);
    assert_eq!(WeaponRecipe::load_json(second_path).unwrap(), second);
}

#[test]
fn generated_recipe_filenames_are_bounded_and_windows_safe() {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let mut reserved = WeaponRecipe::new_weapon("parhelion.reserved-filename").unwrap();
    reserved.name = "CON".to_owned();
    let reserved_path = library.save_new(&reserved).unwrap();
    assert!(
        !reserved_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .split('.')
            .next()
            .unwrap()
            .eq_ignore_ascii_case("CON")
    );
    assert_eq!(WeaponRecipe::load_json(&reserved_path).unwrap(), reserved);

    let mut long = WeaponRecipe::new_weapon("parhelion.long-filename").unwrap();
    long.name = "a".repeat(500);
    let long_path = library.save_new(&long).unwrap();
    let file_name = long_path.file_name().unwrap().to_string_lossy();
    assert!(file_name.len() <= 255);
    assert!(file_name.ends_with(".parhelion.json"));

    let mut same_long_name = WeaponRecipe::new_weapon("parhelion.long-filename-two").unwrap();
    same_long_name.name = "a".repeat(500);
    let suffixed_path = library.save_new(&same_long_name).unwrap();
    let suffixed_name = suffixed_path.file_name().unwrap().to_string_lossy();
    assert!(suffixed_name.len() <= 255);
    assert!(suffixed_name.ends_with(".parhelion.json"));
    assert_ne!(suffixed_path, long_path);
    assert_eq!(WeaponRecipe::load_json(&long_path).unwrap(), long);
    assert_eq!(
        WeaponRecipe::load_json(&suffixed_path).unwrap(),
        same_long_name
    );
}

#[test]
fn disabling_bundled_recipe_survives_library_restart() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("recipes");
    let library = RecipeLibrary::open(root.clone()).unwrap();
    let scan = library.scan().unwrap();
    let mut enabled = library.enabled_paths(&scan.entries).unwrap();
    let every_end = scan
        .entries
        .iter()
        .find(|entry| entry.namespace == "parhelion.every-end")
        .unwrap();
    enabled.remove(&every_end.path);
    library.save_enabled_paths(&enabled, &scan.entries).unwrap();

    let reopened = RecipeLibrary::open(root).unwrap();
    let reopened_scan = reopened.scan().unwrap();
    let reopened_enabled = reopened.enabled_paths(&reopened_scan.entries).unwrap();

    assert_eq!(reopened_enabled, enabled);
    assert!(!reopened_enabled.contains(&every_end.path));
}

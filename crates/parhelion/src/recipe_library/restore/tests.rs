use super::*;

#[test]
fn restore_backs_up_malformed_defaults_and_preserves_selection() {
    let dir = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(dir.path().join("recipes")).unwrap();
    let entries = library.scan().unwrap().entries;
    let enabled = library.enabled_paths(&entries).unwrap();
    let state = fs::read(library.state_path().unwrap()).unwrap();
    let path = library.root.join(BUNDLED_RECIPES[0].0);
    fs::write(&path, b"broken user recipe").unwrap();
    fs::remove_file(library.root.join(BUNDLED_RECIPES[1].0)).unwrap();
    let preview = library.prepare_restore_defaults().unwrap();
    let backup = library.restore_defaults(&preview).unwrap().unwrap();
    assert_eq!(
        backup.parent(),
        Some(
            fs::canonicalize(dir.path().join("backups/recipes"))
                .unwrap()
                .as_path()
        )
    );
    assert_eq!(
        fs::read(backup.join(BUNDLED_RECIPES[0].0)).unwrap(),
        b"broken user recipe"
    );
    assert_eq!(fs::read_to_string(path).unwrap(), BUNDLED_RECIPES[0].1);
    assert_eq!(fs::read(library.state_path().unwrap()).unwrap(), state);
    assert_eq!(
        library
            .enabled_paths(&library.scan().unwrap().entries)
            .unwrap(),
        enabled
    );
    assert!(
        library
            .restore_defaults(&library.prepare_restore_defaults().unwrap())
            .unwrap()
            .is_none()
    );
}

#[test]
fn stale_preview_and_partial_failure_do_not_lose_edits() {
    let dir = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(dir.path().join("recipes")).unwrap();
    let preview = library.prepare_restore_defaults().unwrap();
    let path = library.root.join(BUNDLED_RECIPES[0].0);
    fs::write(&path, b"first edit").unwrap();
    assert!(library.restore_defaults(&preview).is_err());
    fs::write(library.root.join(BUNDLED_RECIPES[1].0), b"second edit").unwrap();
    let preview = library.prepare_restore_defaults().unwrap();
    let mut count = 0;
    let error = library
        .restore_defaults_with(&preview, |path, bytes, _| {
            count += 1;
            if count == 2 {
                return Err("Injected failure".into());
            }
            atomic_write_replace(path, bytes)
        })
        .unwrap_err();
    assert_eq!(fs::read(path).unwrap(), b"first edit");
    let backups = fs::read_dir(dir.path().join("backups/recipes"))
        .unwrap()
        .map(|entry| fs::canonicalize(entry.unwrap().path()).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(backups.len(), 1);
    assert!(
        error.contains(&backups[0].display().to_string()),
        "Restore failure must identify the retained backup at {}: {error}",
        backups[0].display()
    );
    for (index, bytes) in [
        (0, b"first edit".as_slice()),
        (1, b"second edit".as_slice()),
    ] {
        assert_eq!(
            fs::read(backups[0].join(BUNDLED_RECIPES[index].0)).unwrap(),
            bytes
        );
    }
}

#[test]
fn repeated_restores_preserve_previous_backup_files() {
    let dir = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(dir.path().join("recipes")).unwrap();
    let root = dir.path().join("backups/recipes");
    fs::create_dir_all(&root).unwrap();
    let previous = root.join("previous-manual-backup.json");
    fs::write(&previous, b"previous backup").unwrap();
    let target = library.root.join(BUNDLED_RECIPES[0].0);
    let mut backups = Vec::new();
    for bytes in [b"first edit".as_slice(), b"second edit".as_slice()] {
        fs::write(&target, bytes).unwrap();
        let backup = library
            .restore_defaults(&library.prepare_restore_defaults().unwrap())
            .unwrap()
            .unwrap();
        backups.push((backup, bytes));
    }
    assert_ne!(backups[0].0, backups[1].0);
    for (backup, bytes) in backups {
        assert_eq!(fs::read(backup.join(BUNDLED_RECIPES[0].0)).unwrap(), bytes);
    }
    assert_eq!(fs::read(previous).unwrap(), b"previous backup");
}

#[test]
fn backup_directory_file_collisions_preserve_existing_bytes_and_recipes() {
    for name in ["backups", "backups/recipes"] {
        let dir = tempfile::tempdir().unwrap();
        let library = RecipeLibrary::open(dir.path().join("recipes")).unwrap();
        let collision = dir.path().join(name);
        fs::create_dir_all(collision.parent().unwrap()).unwrap();
        fs::write(&collision, b"existing file").unwrap();
        let recipe = library.root.join(BUNDLED_RECIPES[0].0);
        fs::write(&recipe, b"saved edit").unwrap();
        assert!(
            library
                .restore_defaults(&library.prepare_restore_defaults().unwrap())
                .is_err()
        );
        assert_eq!(fs::read(collision).unwrap(), b"existing file");
        assert_eq!(fs::read(recipe).unwrap(), b"saved edit");
    }
}

#[cfg(any(unix, windows))]
#[test]
fn linked_backup_directories_cannot_redirect_recipe_backups() {
    for name in ["backups", "backups/recipes"] {
        let dir = tempfile::tempdir().unwrap();
        let library = RecipeLibrary::open(dir.path().join("recipes")).unwrap();
        let external = dir.path().join("outside");
        fs::create_dir(&external).unwrap();
        let link = dir.path().join(name);
        fs::create_dir_all(link.parent().unwrap()).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&external, &link).unwrap();
        #[cfg(windows)]
        if let Err(error) = std::os::windows::fs::symlink_dir(&external, &link) {
            if error.kind() == std::io::ErrorKind::PermissionDenied {
                return;
            }
            panic!("Could not create backup directory symlink: {error}");
        }
        let recipe = library.root.join(BUNDLED_RECIPES[0].0);
        fs::write(&recipe, b"saved edit").unwrap();
        assert!(
            library
                .restore_defaults(&library.prepare_restore_defaults().unwrap())
                .is_err()
        );
        assert_eq!(fs::read(recipe).unwrap(), b"saved edit");
        assert_eq!(fs::read_dir(external).unwrap().count(), 0);
    }
}

#[test]
fn custom_recipe_collision_blocks_restore_without_writes() {
    let dir = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(dir.path().join("recipes")).unwrap();
    let path = library.root.join("custom.json");
    fs::write(&path, BUNDLED_RECIPES[0].1).unwrap();
    assert!(library.prepare_restore_defaults().is_err());
    assert_eq!(fs::read_to_string(path).unwrap(), BUNDLED_RECIPES[0].1);
}

#[test]
fn custom_recipe_bytes_survive_restore() {
    let dir = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(dir.path().join("recipes")).unwrap();
    let custom = WeaponRecipe::new_weapon_for_donor(
        "parhelion.test-custom",
        0x249F67B4,
        "Falling Guillotine",
    )
    .unwrap();
    let path = library.save_new(&custom).unwrap();
    let bytes = fs::read(&path).unwrap();
    fs::write(library.root.join(BUNDLED_RECIPES[0].0), b"modified").unwrap();
    library
        .restore_defaults(&library.prepare_restore_defaults().unwrap())
        .unwrap();
    assert_eq!(fs::read(path).unwrap(), bytes);
}

#[test]
fn directory_in_place_of_default_is_not_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(dir.path().join("recipes")).unwrap();
    let path = library.root.join(BUNDLED_RECIPES[0].0);
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(library.prepare_restore_defaults().is_err());
    assert!(path.is_dir());
}

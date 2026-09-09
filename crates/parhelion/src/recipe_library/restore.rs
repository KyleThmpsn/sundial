use super::*;

/// A read-only preview. Confirmation is rejected if any target changes meanwhile.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RestoreDefaults {
    root: PathBuf,
    originals: Vec<Option<Vec<u8>>>,
}

impl RecipeLibrary {
    pub(crate) fn prepare_restore_defaults(&self) -> Result<RestoreDefaults, String> {
        if fs::canonicalize(&self.root).map_err(|e| e.to_string())? != self.canonical_root {
            return Err("The recipe library location changed. Reopen the library first.".into());
        }
        let mut namespaces = BTreeSet::new();
        let mut hashes = BTreeSet::new();
        let mut validate = |recipe: WeaponRecipe| -> Result<(), String> {
            recipe.to_spec().map_err(|e| e.to_string())?;
            if !namespaces.insert(recipe.namespace.to_lowercase()) {
                return Err(format!("Duplicate recipe namespace: {}", recipe.namespace));
            }
            for hash in recipe
                .identity
                .parsed_hashes(&recipe.namespace)
                .map_err(|e| e.to_string())?
            {
                if !hashes.insert(hash) {
                    return Err(format!("Duplicate recipe identity: 0x{hash:08X}"));
                }
            }
            Ok(())
        };
        for (_, json) in BUNDLED_RECIPES {
            validate(WeaponRecipe::from_json_str(json).map_err(|e| e.to_string())?)?;
        }
        for path in self.recipe_paths()? {
            if BUNDLED_RECIPES
                .iter()
                .any(|(name, _)| path.file_name().is_some_and(|n| n == *name))
            {
                continue;
            }
            self.confined_existing_path(&path)?;
            validate(
                WeaponRecipe::load_json(&path).map_err(|e| format!("{}: {e}", path.display()))?,
            )?;
        }
        let mut originals = Vec::new();
        for (name, _) in BUNDLED_RECIPES {
            let path = self.root.join(name);
            originals.push(match fs::symlink_metadata(&path) {
                Ok(_) => {
                    self.confined_existing_path(&path)?;
                    Some(fs::read(&path).map_err(|e| e.to_string())?)
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(e) => return Err(e.to_string()),
            });
        }
        Ok(RestoreDefaults {
            root: self.canonical_root.clone(),
            originals,
        })
    }

    pub(crate) fn restore_defaults(
        &self,
        preview: &RestoreDefaults,
    ) -> Result<Option<PathBuf>, String> {
        self.restore_defaults_with(preview, |path, bytes, exists| {
            if exists {
                atomic_write_replace(path, bytes)
            } else {
                atomic_write_create_new(path, bytes).map_err(|e| match e {
                    WriteNewError::AlreadyExists => {
                        format!("{} appeared during restore", path.display())
                    }
                    WriteNewError::Other(e) => e,
                })
            }
        })
    }

    fn restore_defaults_with(
        &self,
        preview: &RestoreDefaults,
        mut write: impl FnMut(&Path, &[u8], bool) -> Result<(), String>,
    ) -> Result<Option<PathBuf>, String> {
        if &self.prepare_restore_defaults()? != preview {
            return Err(
                "A default recipe changed after the preview. Review the restore again.".into(),
            );
        }
        let changed: Vec<_> = BUNDLED_RECIPES
            .iter()
            .enumerate()
            .filter(|(i, (_, json))| preview.originals[*i].as_deref() != Some(json.as_bytes()))
            .collect();
        if changed.is_empty() {
            return Ok(None);
        }
        let backup = self.create_restore_backup()?;
        for (i, (name, _)) in &changed {
            if let Some(bytes) = &preview.originals[*i] {
                atomic_write_create_new(&backup.join(name), bytes).map_err(|error| {
                    let error = match error {
                        WriteNewError::AlreadyExists => "Backup already exists".into(),
                        WriteNewError::Other(error) => error,
                    };
                    format!("Backup failed at {}: {error}", backup.display())
                })?;
            }
        }
        let mut written: Vec<usize> = Vec::new();
        for (i, (name, json)) in changed {
            let path = self.root.join(name);
            let result = self
                .prepare_restore_target(&path, &preview.originals[i])
                .and_then(|()| write(&path, json.as_bytes(), preview.originals[i].is_some()));
            if let Err(error) = result {
                let mut recovery_errors = Vec::new();
                for index in written.into_iter().rev() {
                    let (name, json) = BUNDLED_RECIPES[index];
                    let path = self.root.join(name);
                    let rollback = self
                        .prepare_restore_target(&path, &Some(json.as_bytes().to_vec()))
                        .and_then(|()| match &preview.originals[index] {
                            Some(bytes) => atomic_write_replace(&path, bytes),
                            None => fs::remove_file(&path).map_err(|e| e.to_string()),
                        });
                    if let Err(e) = rollback {
                        recovery_errors.push(e);
                    }
                }
                return Err(format!(
                    "Restore failed: {error}. Original files are backed up at {}. Recovery errors: {:?}",
                    backup.display(),
                    recovery_errors
                ));
            }
            written.push(i);
        }
        Ok(Some(backup))
    }

    fn create_restore_backup(&self) -> Result<PathBuf, String> {
        let parent = self
            .canonical_root
            .parent()
            .ok_or("Recipe library has no parent")?;
        let backups = confined_backup_child(parent, "backups")?;
        let recipes = confined_backup_child(&backups, "recipes")?;
        tempfile::Builder::new()
            .prefix("recipe-backup-")
            .tempdir_in(&recipes)
            .map(tempfile::TempDir::keep)
            .map_err(|error| {
                format!(
                    "Could not create recipe backup in {}: {error}",
                    recipes.display()
                )
            })
    }

    fn prepare_restore_target(
        &self,
        path: &Path,
        expected: &Option<Vec<u8>>,
    ) -> Result<(), String> {
        if fs::canonicalize(&self.root).map_err(|e| e.to_string())? != self.canonical_root {
            return Err("Recipe library location changed".into());
        }
        let actual = match fs::symlink_metadata(path) {
            Ok(_) => {
                self.confined_existing_path(path)?;
                Some(fs::read(path).map_err(|e| e.to_string())?)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.to_string()),
        };
        if &actual != expected {
            return Err(format!("{} changed during restore", path.display()));
        }
        Ok(())
    }
}

fn confined_backup_child(parent: &Path, name: &str) -> Result<PathBuf, String> {
    let path = parent.join(name);
    match fs::create_dir(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => {
            return Err(format!(
                "Could not create recipe backup directory {}: {error}",
                path.display()
            ));
        }
    }
    let metadata = fs::symlink_metadata(&path).map_err(|error| {
        format!(
            "Could not inspect recipe backup directory {}: {error}",
            path.display()
        )
    })?;
    if !metadata.is_dir() || backup_directory_is_link(&metadata) {
        return Err(format!(
            "Refusing a linked or non-directory recipe backup location: {}",
            path.display()
        ));
    }
    let canonical = fs::canonicalize(&path).map_err(|error| {
        format!(
            "Could not resolve recipe backup directory {}: {error}",
            path.display()
        )
    })?;
    if canonical.parent() != Some(parent) {
        return Err(format!(
            "Refusing a recipe backup location outside its parent: {}",
            path.display()
        ));
    }
    Ok(canonical)
}

fn backup_directory_is_link(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // Include junctions and other directory reparse points, not only symbolic links.
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    metadata.file_type().is_symlink()
}

#[cfg(test)]
mod tests {
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
    fn unchanged_defaults_do_not_create_backup_directories() {
        let dir = tempfile::tempdir().unwrap();
        let library = RecipeLibrary::open(dir.path().join("recipes")).unwrap();
        assert!(
            library
                .restore_defaults(&library.prepare_restore_defaults().unwrap())
                .unwrap()
                .is_none()
        );
        assert!(!dir.path().join("backups").exists());
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
}

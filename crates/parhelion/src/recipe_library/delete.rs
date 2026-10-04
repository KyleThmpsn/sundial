//! Deleting a recipe backs it up first, and a deleted bundled recipe stays deleted.
use super::*;

/// A read-only preview. Confirmation is rejected if the recipe changes meanwhile.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DeleteRecipe {
    pub path: PathBuf,
    pub name: String,
    /// A bundled recipe is recreated whenever the library opens, unless it is recorded as
    /// deleted. Restore Default Recipes brings it back.
    pub bundled: bool,
    original: Vec<u8>,
}

impl RecipeLibrary {
    pub(crate) fn prepare_delete_recipe(&self, path: &Path) -> Result<DeleteRecipe, String> {
        let target = self.confined_existing_path(path)?;
        let original = fs::read(&target)
            .map_err(|error| format!("Could not read recipe {}: {error}", path.display()))?;
        let recipe = WeaponRecipe::from_json_str(
            std::str::from_utf8(&original).map_err(|error| error.to_string())?,
        )
        .map_err(|error| format!("{}: {error}", path.display()))?;
        Ok(DeleteRecipe {
            path: path.to_path_buf(),
            name: recipe.name,
            bundled: target
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| BUNDLED_RECIPES.iter().any(|(file, _)| name == *file)),
            original,
        })
    }

    /// Backs the recipe up, then removes it and its place in the build. Returns the backup.
    pub(crate) fn delete_recipe(&self, preview: &DeleteRecipe) -> Result<PathBuf, String> {
        self.delete_recipe_with(preview, |path| mutation::remove(path, &preview.original))
    }

    pub(super) fn delete_recipe_with(
        &self,
        preview: &DeleteRecipe,
        remove: impl FnOnce(&Path) -> Result<(), String>,
    ) -> Result<PathBuf, String> {
        let _lock = self.lock()?;
        if &self.prepare_delete_recipe(&preview.path)? != preview {
            return Err("This recipe changed after the preview. Review the deletion again.".into());
        }
        let file_name = preview
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("Recipe filename is missing")?
            .to_owned();
        let changes = self.deletion_metadata(&file_name, preview.bundled)?;
        let backup = self.create_restore_backup()?;
        atomic_write_create_new(&backup.join(&file_name), &preview.original).map_err(|error| {
            match error {
                WriteNewError::AlreadyExists => "Recipe backup already exists".to_owned(),
                WriteNewError::Other(error) => error,
            }
        })?;
        for change in &changes {
            if let Some(before) = &change.before {
                let name = change
                    .path
                    .file_name()
                    .ok_or("Metadata filename is missing")?;
                atomic_write_create_new(&backup.join(name), before).map_err(
                    |error| match error {
                        WriteNewError::AlreadyExists => "Metadata backup already exists".to_owned(),
                        WriteNewError::Other(error) => error,
                    },
                )?;
            }
        }
        mutation::commit(&changes, || {
            self.prepare_restore_target(&preview.path, &Some(preview.original.clone()))?;
            remove(&preview.path)
        })
        .map_err(|error| {
            format!(
                "Recipe deletion failed: {error}. Original files are backed up at {}",
                backup.display()
            )
        })?;
        Ok(backup)
    }

    fn deletion_metadata(
        &self,
        file_name: &str,
        bundled: bool,
    ) -> Result<Vec<mutation::Change>, String> {
        let (mut state, original) = self.read_state()?;
        let before = state.clone();
        state.enabled_recipes.remove(file_name);
        state.known_bundled_recipes.remove(file_name);
        let mut changes = Vec::new();
        if state != before {
            changes.push(mutation::Change {
                path: self.state_path()?,
                before: original,
                after: encode_state(&state)?,
            });
        }
        if bundled {
            let (mut removed, original) = self.read_removed_bundled()?;
            if removed.insert(file_name.to_owned()) {
                changes.push(mutation::Change {
                    path: self.removed_bundled_path()?,
                    before: original,
                    after: encode_removed(&removed),
                });
            }
        }
        Ok(changes)
    }

    /// Kept beside the library state for compatibility with older Parhelion versions.
    fn removed_bundled_path(&self) -> Result<PathBuf, String> {
        self.state_path()
            .map(|state| state.with_file_name(REMOVED_BUNDLED_FILE_NAME))
    }

    /// The bundled recipes that were deleted, one file name per line.
    pub(super) fn removed_bundled(&self) -> Result<BTreeSet<String>, String> {
        self.read_removed_bundled().map(|(removed, _)| removed)
    }

    fn read_removed_bundled(&self) -> Result<(BTreeSet<String>, Option<Vec<u8>>), String> {
        let path = self.removed_bundled_path()?;
        let original = mutation::read_optional(&path)?;
        let listing = std::str::from_utf8(original.as_deref().unwrap_or_default())
            .map_err(|error| format!("Could not decode {}: {error}", path.display()))?;
        let removed = listing
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .collect();
        Ok((removed, original))
    }

    /// An empty set removes the record, so a library with nothing deleted carries no file.
    pub(super) fn write_removed_bundled(&self, removed: &BTreeSet<String>) -> Result<(), String> {
        let path = self.removed_bundled_path()?;
        if removed.is_empty() {
            return match fs::remove_file(&path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(format!("Could not update {}: {error}", path.display())),
            };
        }
        let original = mutation::read_optional(&path)?;
        mutation::publish(&path, &encode_removed(removed), original.as_deref())
    }
}

fn encode_removed(removed: &BTreeSet<String>) -> Vec<u8> {
    let mut listing = removed.iter().cloned().collect::<Vec<_>>().join("\n");
    listing.push('\n');
    listing.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enabled(library: &RecipeLibrary) -> BTreeSet<PathBuf> {
        library
            .enabled_paths(&library.scan().unwrap().entries)
            .unwrap()
    }

    #[test]
    fn deleting_a_recipe_backs_it_up_and_leaves_the_build() {
        let dir = tempfile::tempdir().unwrap();
        let library = RecipeLibrary::open(dir.path().join("recipes")).unwrap();
        let recipe = WeaponRecipe::new_weapon("parhelion.delete-me").unwrap();
        let path = library.save_new(&recipe).unwrap();
        let mut selected = enabled(&library);
        selected.insert(path.clone());
        library
            .save_enabled_paths(&selected, &library.scan().unwrap().entries)
            .unwrap();
        assert!(enabled(&library).contains(&path));
        let original = fs::read(&path).unwrap();

        let preview = library.prepare_delete_recipe(&path).unwrap();
        assert!(!preview.bundled);
        let backup = library.delete_recipe(&preview).unwrap();

        assert!(!path.exists());
        assert_eq!(
            fs::read(backup.join(path.file_name().unwrap())).unwrap(),
            original
        );
        assert!(!enabled(&library).contains(&path));
        // A user recipe needs no record to stay deleted.
        assert!(!library.removed_bundled_path().unwrap().exists());
        // A new recipe saved under the same name does not inherit the old membership.
        let again = library.save_new(&recipe).unwrap();
        assert_eq!(again, path);
        assert!(!enabled(&library).contains(&again));
    }

    #[test]
    fn a_deleted_bundled_recipe_stays_deleted_until_defaults_are_restored() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("recipes");
        let library = RecipeLibrary::open(root.clone()).unwrap();
        let path = library.root.join(BUNDLED_RECIPES[0].0);
        assert!(enabled(&library).contains(&path));

        let preview = library.prepare_delete_recipe(&path).unwrap();
        assert!(preview.bundled);
        library.delete_recipe(&preview).unwrap();

        // Opening the library again does not recreate it.
        let library = RecipeLibrary::open(root.clone()).unwrap();
        assert!(!path.exists());
        assert!(!enabled(&library).contains(&path));

        let restore = library.prepare_restore_defaults().unwrap();
        library.restore_defaults(&restore).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), BUNDLED_RECIPES[0].1);
        assert!(!library.removed_bundled_path().unwrap().exists());
        // It comes back enabled, the way a fresh library has it.
        let library = RecipeLibrary::open(root).unwrap();
        assert!(enabled(&library).contains(&path));
    }

    #[test]
    fn a_recipe_changed_after_the_preview_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let library = RecipeLibrary::open(dir.path().join("recipes")).unwrap();
        let recipe = WeaponRecipe::new_weapon("parhelion.edited-meanwhile").unwrap();
        let path = library.save_new(&recipe).unwrap();
        let preview = library.prepare_delete_recipe(&path).unwrap();
        let mut edited = recipe.clone();
        edited.name = "Edited Meanwhile".into();
        library.save_existing(&path, &edited).unwrap();

        assert!(library.delete_recipe(&preview).is_err());
        assert!(path.exists());
        assert_eq!(
            WeaponRecipe::load_json(&path).unwrap().name,
            "Edited Meanwhile"
        );
    }
}

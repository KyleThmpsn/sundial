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
        if &self.prepare_delete_recipe(&preview.path)? != preview {
            return Err("This recipe changed after the preview. Review the deletion again.".into());
        }
        let file_name = preview
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("Recipe filename is missing")?
            .to_owned();
        let backup = self.create_restore_backup()?;
        atomic_write_create_new(&backup.join(&file_name), &preview.original).map_err(|error| {
            match error {
                WriteNewError::AlreadyExists => "Recipe backup already exists".to_owned(),
                WriteNewError::Other(error) => error,
            }
        })?;
        // Recorded before the file goes, so a bundled recipe cannot come back half deleted.
        if preview.bundled {
            let mut removed = self.removed_bundled()?;
            if removed.insert(file_name.clone()) {
                self.write_removed_bundled(&removed)?;
            }
        }
        self.prepare_restore_target(&preview.path, &Some(preview.original.clone()))?;
        fs::remove_file(&preview.path)
            .map_err(|error| format!("Could not delete {}: {error}", preview.path.display()))?;
        // The recipe was removed on purpose, so it leaves the build too. A bundled one also
        // forgets it was seen, so Restore Default Recipes brings it back enabled, as it was
        // on a fresh library.
        let mut state = self.load_state()?;
        let before = state.clone();
        state.enabled_recipes.remove(&file_name);
        state.known_bundled_recipes.remove(&file_name);
        if state != before {
            self.write_state(&state)?;
        }
        Ok(backup)
    }

    /// Kept beside the library state rather than inside it. That state rejects unknown
    /// fields, so a new field would stop an older Parhelion from opening the library.
    fn removed_bundled_path(&self) -> Result<PathBuf, String> {
        self.state_path()
            .map(|state| state.with_file_name(REMOVED_BUNDLED_FILE_NAME))
    }

    /// The bundled recipes that were deleted, one file name per line.
    pub(super) fn removed_bundled(&self) -> Result<BTreeSet<String>, String> {
        let path = self.removed_bundled_path()?;
        match fs::read_to_string(&path) {
            Ok(listing) => Ok(listing
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_owned)
                .collect()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(BTreeSet::new()),
            Err(error) => Err(format!("Could not read {}: {error}", path.display())),
        }
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
        let mut listing = removed.iter().cloned().collect::<Vec<_>>().join("\n");
        listing.push('\n');
        atomic_write_replace(&path, listing.as_bytes())
    }
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

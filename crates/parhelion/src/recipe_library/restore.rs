use super::*;

mod recipe;
pub(crate) use recipe::RestoreRecipe;

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
        self.restore_defaults_with(preview, mutation::publish)
    }

    fn restore_defaults_with(
        &self,
        preview: &RestoreDefaults,
        mut write: impl FnMut(&Path, &[u8], Option<&[u8]>) -> Result<(), String>,
    ) -> Result<Option<PathBuf>, String> {
        let _lock = self.lock()?;
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
            let _ = self.write_removed_bundled(&BTreeSet::new());
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
                .and_then(|()| write(&path, json.as_bytes(), preview.originals[i].as_deref()));
            if let Err(error) = result {
                let mut recovery_errors = Vec::new();
                for index in written.into_iter().rev() {
                    let (name, json) = BUNDLED_RECIPES[index];
                    let path = self.root.join(name);
                    let rollback = self
                        .prepare_restore_target(&path, &Some(json.as_bytes().to_vec()))
                        .and_then(|()| match &preview.originals[index] {
                            Some(bytes) => mutation::publish(&path, bytes, Some(json.as_bytes())),
                            None => mutation::remove(&path, json.as_bytes()),
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
        // Keep the deletion record under the same write lease as the restore.
        // A failed cleanup is harmless while the restored files remain present.
        let _ = self.write_removed_bundled(&BTreeSet::new());
        Ok(Some(backup))
    }

    pub(super) fn create_restore_backup(&self) -> Result<PathBuf, String> {
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

    pub(super) fn prepare_restore_target(
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
mod tests;

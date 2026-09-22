use super::*;

/// A read-only snapshot used to reject a restore when a target changes before confirmation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RestoreDefaults {
    root: PathBuf,
    originals: Vec<Option<Vec<u8>>>,
    removed: Option<Vec<u8>>,
}

struct Target {
    path: PathBuf,
    original: Option<Vec<u8>>,
    restored: Option<Vec<u8>>,
}

impl Library {
    pub(crate) fn prepare_restore_defaults(&self) -> Result<RestoreDefaults, String> {
        self.check_root()?;
        let recipes = bundled_recipes()?;
        let mut originals = Vec::with_capacity(recipes.len());
        for (_, recipe) in recipes {
            originals.push(
                self.read_restore_target(&self.root.join(format!("{}.perk.json", recipe.id)))?,
            );
        }
        Ok(RestoreDefaults {
            root: self.root.clone(),
            originals,
            removed: self.read_restore_target(&self.removed_bundled_path())?,
        })
    }

    pub(crate) fn restore_defaults(
        &self,
        preview: &RestoreDefaults,
    ) -> Result<Option<PathBuf>, String> {
        let _lock = self.lock()?;
        if &self.prepare_restore_defaults()? != preview {
            return Err(
                "A default custom perk changed after the preview. Review the restore again.".into(),
            );
        }
        let recipes = bundled_recipes()?;
        let ids = recipes
            .iter()
            .map(|(_, recipe)| recipe.id.clone())
            .collect::<BTreeSet<_>>();
        let mut targets = Vec::new();
        for ((encoded, recipe), original) in recipes.into_iter().zip(&preview.originals) {
            let restored = Some(encoded.as_bytes().to_vec());
            if original != &restored {
                targets.push(Target {
                    path: self.root.join(format!("{}.perk.json", recipe.id)),
                    original: original.clone(),
                    restored,
                });
            }
        }
        let restored_removed = restored_removed_marker(preview.removed.as_deref(), &ids)?;
        if preview.removed != restored_removed {
            targets.push(Target {
                path: self.removed_bundled_path(),
                original: preview.removed.clone(),
                restored: restored_removed,
            });
        }
        if targets.is_empty() {
            return Ok(None);
        }

        let backup = self.create_restore_backup()?;
        for target in &targets {
            if let Some(bytes) = &target.original {
                let name = target
                    .path
                    .file_name()
                    .ok_or("A custom perk restore target has no file name")?;
                sundial::storage::create_file(&backup.join(name), bytes).map_err(|error| {
                    format!("Could not back up {}: {error}", target.path.display())
                })?;
            }
        }

        let mut written: Vec<usize> = Vec::new();
        for (index, target) in targets.iter().enumerate() {
            if let Err(error) =
                self.replace_restore_target(&target.path, &target.original, &target.restored)
            {
                let mut recovery_errors = Vec::new();
                for written_index in written.into_iter().rev() {
                    let written_target = &targets[written_index];
                    if let Err(recovery_error) = self.replace_restore_target(
                        &written_target.path,
                        &written_target.restored,
                        &written_target.original,
                    ) {
                        recovery_errors.push(recovery_error);
                    }
                }
                return Err(format!(
                    "Restore failed: {error}. Original files are backed up at {}. Recovery errors: {:?}",
                    backup.display(),
                    recovery_errors
                ));
            }
            written.push(index);
        }
        Ok(Some(backup))
    }

    pub(crate) fn bundled_ids(&self) -> Result<BTreeSet<String>, String> {
        bundled_recipes().map(|recipes| recipes.into_iter().map(|(_, recipe)| recipe.id).collect())
    }

    fn check_root(&self) -> Result<(), String> {
        if fs::canonicalize(&self.root).map_err(|error| error.to_string())? != self.root {
            return Err("The custom perk library location changed. Reopen it first.".into());
        }
        Ok(())
    }

    pub(super) fn read_restore_target(&self, path: &Path) -> Result<Option<Vec<u8>>, String> {
        self.check_root()?;
        match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                Err(format!(
                    "The custom perk restore target is not a regular file: {}",
                    path.display()
                ))
            }
            Ok(_) => fs::read(path).map(Some).map_err(|error| error.to_string()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    }

    pub(super) fn replace_restore_target(
        &self,
        path: &Path,
        expected: &Option<Vec<u8>>,
        replacement: &Option<Vec<u8>>,
    ) -> Result<(), String> {
        if &self.read_restore_target(path)? != expected {
            return Err(format!("{} changed during restore", path.display()));
        }
        match (expected, replacement) {
            (_, Some(bytes)) if expected.is_some() => {
                sundial::package_authoring::replace_authoring_file(path, bytes)
                    .map_err(|error| error.to_string())
            }
            (None, Some(bytes)) => {
                sundial::storage::create_file(path, bytes).map_err(|error| error.to_string())
            }
            (Some(_), None) => fs::remove_file(path).map_err(|error| error.to_string()),
            (None, None) => Ok(()),
            _ => unreachable!(),
        }
    }

    pub(super) fn create_restore_backup(&self) -> Result<PathBuf, String> {
        let parent = self
            .root
            .parent()
            .ok_or("Custom perk library has no parent")?;
        let backups = confined_backup_child(parent, "backups")?;
        let perks = confined_backup_child(&backups, "perks")?;
        tempfile::Builder::new()
            .prefix("perk-backup-")
            .tempdir_in(&perks)
            .map(tempfile::TempDir::keep)
            .map_err(|error| {
                format!(
                    "Could not create custom perk backup in {}: {error}",
                    perks.display()
                )
            })
    }
}

fn restored_removed_marker(
    current: Option<&[u8]>,
    bundled_ids: &BTreeSet<String>,
) -> Result<Option<Vec<u8>>, String> {
    let Some(current) = current else {
        return Ok(None);
    };
    let listing = std::str::from_utf8(current)
        .map_err(|error| format!("Could not read removed default custom perks: {error}"))?;
    let kept = listing
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !bundled_ids.contains(*line))
        .collect::<Vec<_>>();
    if kept.is_empty() {
        Ok(None)
    } else {
        Ok(Some(format!("{}\n", kept.join("\n")).into_bytes()))
    }
}

fn confined_backup_child(parent: &Path, name: &str) -> Result<PathBuf, String> {
    let parent = fs::canonicalize(parent).map_err(|error| error.to_string())?;
    let child = parent.join(name);
    fs::create_dir_all(&child).map_err(|error| error.to_string())?;
    let resolved = fs::canonicalize(&child).map_err(|error| error.to_string())?;
    if resolved.parent() != Some(parent.as_path()) {
        return Err(format!(
            "The backup folder resolves outside {}: {}",
            parent.display(),
            resolved.display()
        ));
    }
    Ok(resolved)
}

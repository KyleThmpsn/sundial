//! How many recipe backups are kept.
//!
//! A reimport that replaces a recipe's sockets, and a restore of the bundled recipes, first keep
//! the files they replace in a new `backups/recipes/recipe-backup-*` folder. A backup names the
//! conversion its recipe used, so the sweep keeps that conversion while the backup exists. A sweep
//! of every item keeps each recipe's newest backed-up copies, as many as package backup retention
//! keeps install generations, and removes its older copies before it reads what the snapshots
//! name, so their conversions go in the same sweep. A folder left without a recipe goes with them.
//! Turning off the package backup limit keeps every copy.
//!
//! Copies are counted per recipe file name, since one backup usually holds one recipe and a
//! reimport of many items makes a backup for each. Counting folders instead would drop the only
//! copy of most recipes in such a batch.
use super::*;
use std::ffi::OsString;

const PREFIX: &str = "recipe-backup-";

impl RecipeLibrary {
    /// Keeps the `keep` newest backed-up copies of each recipe, by its file name and the time its
    /// backup folder was made, and removes the older copies and the folders they leave empty.
    /// `None` keeps every copy. Returns how many copies it removed. A folder that holds anything
    /// but recipe files is left whole and counts toward no recipe, as is one whose age cannot be
    /// read.
    pub(super) fn prune_recipe_backups(
        &self,
        keep: Option<usize>,
        errors: &mut Vec<String>,
    ) -> usize {
        let Some(keep) = keep else {
            return 0;
        };
        let folder = match self.data_folder() {
            Ok(data) => data.join("backups").join("recipes"),
            Err(error) => {
                errors.push(error);
                return 0;
            }
        };
        let entries = match fs::read_dir(&folder) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return 0,
            Err(error) => {
                errors.push(format!("Could not read {}: {error}", folder.display()));
                return 0;
            }
        };
        let mut copies: BTreeMap<OsString, Vec<(SystemTime, PathBuf)>> = BTreeMap::new();
        let mut backups = Vec::new();
        for entry in entries.filter_map(Result::ok) {
            if !entry.file_name().to_string_lossy().starts_with(PREFIX) {
                continue;
            }
            let path = entry.path();
            let Some((made, recipes)) = recipes(&path) else {
                continue;
            };
            for recipe in recipes {
                copies
                    .entry(recipe.clone())
                    .or_default()
                    .push((made, path.join(recipe)));
            }
            backups.push(path);
        }
        let mut removed = 0;
        for mut copies in copies.into_values() {
            // Newest first, with the folder name settling copies made at the same time.
            copies.sort_by(|left, right| right.cmp(left));
            for (_, path) in copies.into_iter().skip(keep) {
                match fs::remove_file(&path) {
                    Ok(()) => removed += 1,
                    Err(error) => {
                        errors.push(format!("Could not remove {}: {error}", path.display()))
                    }
                }
            }
        }
        for backup in backups {
            if fs::read_dir(&backup).is_ok_and(|mut entries| entries.next().is_none())
                && let Err(error) = fs::remove_dir(&backup)
            {
                errors.push(format!("Could not remove {}: {error}", backup.display()));
            }
        }
        removed
    }
}

/// When the backup folder at `path` was made and the recipe files it holds, or `None` when it
/// holds anything else, is a link or its age cannot be read.
fn recipes(path: &Path) -> Option<(SystemTime, Vec<OsString>)> {
    let metadata = fs::symlink_metadata(path).ok()?;
    if !metadata.is_dir() || super::super::restore::backup_directory_is_link(&metadata) {
        return None;
    }
    let made = metadata.created().or_else(|_| metadata.modified()).ok()?;
    let mut recipes = Vec::new();
    for entry in fs::read_dir(path).ok()? {
        let entry = entry.ok()?;
        let name = entry.file_name();
        if !entry.file_type().ok()?.is_file() || !name.to_string_lossy().ends_with(".json") {
            return None;
        }
        recipes.push(name);
    }
    Some((made, recipes))
}

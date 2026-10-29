//! How long recipe backups are kept.
//!
//! A reimport that replaces a recipe's sockets, and a restore of the bundled recipes, first keep
//! the files they replace in a new `backups/recipes/recipe-backup-*` folder. A backup names the
//! conversion its recipe used, so the sweep keeps that conversion while the backup exists. Each
//! backup stays for `KEEP` after it was made. A sweep of every item removes older ones before it
//! reads what the snapshots name, so their conversions go in the same sweep.
use super::*;
use std::time::Duration;

/// How long every recipe backup is kept.
const KEEP: Duration = Duration::from_secs(14 * 24 * 60 * 60);
const PREFIX: &str = "recipe-backup-";

impl RecipeLibrary {
    /// Removes the recipe backups older than `KEEP` at `now`, and returns how many it removed. A
    /// folder that holds anything but recipe files is kept, as is one whose age cannot be read.
    pub(super) fn prune_recipe_backups(&self, now: SystemTime, errors: &mut Vec<String>) -> usize {
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
        let mut removed = 0;
        for entry in entries.filter_map(Result::ok) {
            if !entry.file_name().to_string_lossy().starts_with(PREFIX) {
                continue;
            }
            let path = entry.path();
            if !expired(&path, now) {
                continue;
            }
            match fs::remove_dir_all(&path) {
                Ok(()) => removed += 1,
                Err(error) => errors.push(format!("Could not remove {}: {error}", path.display())),
            }
        }
        removed
    }
}

/// Whether the backup folder at `path` is older than `KEEP` and holds only recipe files. A link
/// is never removed.
fn expired(path: &Path, now: SystemTime) -> bool {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    if !metadata.is_dir() || super::super::restore::backup_directory_is_link(&metadata) {
        return false;
    }
    let Ok(made) = metadata.created().or_else(|_| metadata.modified()) else {
        return false;
    };
    if !now.duration_since(made).is_ok_and(|age| age >= KEEP) {
        return false;
    }
    fs::read_dir(path).is_ok_and(|entries| {
        entries.into_iter().all(|entry| {
            entry.is_ok_and(|entry| {
                entry.file_type().is_ok_and(|kind| kind.is_file())
                    && entry.file_name().to_string_lossy().ends_with(".json")
            })
        })
    })
}

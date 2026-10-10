use std::{
    fs, io,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{DEFAULT_PACKAGE_BACKUP_RETENTION, MAX_PACKAGE_BACKUP_RETENTION};

const PREFERENCES_SCHEMA: u32 = 1;
const PREFERENCES_FILE_NAME: &str = "preferences.json";

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct ParhelionPreferences {
    pub(crate) schema: u32,
    pub limit_package_backups: bool,
    pub package_backup_retention: usize,
    pub backup_recipe_snapshots: bool,
    pub show_preview_fps: bool,
    pub play_preview_animations: bool,
}

impl Default for ParhelionPreferences {
    fn default() -> Self {
        Self {
            schema: PREFERENCES_SCHEMA,
            limit_package_backups: true,
            package_backup_retention: DEFAULT_PACKAGE_BACKUP_RETENTION,
            backup_recipe_snapshots: true,
            show_preview_fps: false,
            play_preview_animations: true,
        }
    }
}

impl ParhelionPreferences {
    pub(crate) fn load_default() -> Result<Self, String> {
        Self::load_from(&preferences_path()?)
    }

    fn load_from(path: &Path) -> Result<Self, String> {
        let Some(encoded) = read_preferences(path)? else {
            return Ok(Self::default());
        };
        let (_, preferences) = decode_preferences(&encoded, path)?;
        Ok(preferences)
    }

    pub(crate) fn save_default(self) -> Result<PathBuf, String> {
        let path = preferences_path()?;
        self.save_to(&path)?;
        Ok(path)
    }

    fn save_to(self, path: &Path) -> Result<(), String> {
        self.validate()?;
        let parent = path
            .parent()
            .ok_or_else(|| format!("Preferences path has no parent: {}", path.display()))?;
        fs::create_dir_all(parent)
            .map_err(|error| format!("Could not create {}: {error}", parent.display()))?;
        let _lock = sundial::storage::try_lock_file(&path.with_extension("lock"))
            .map_err(|error| format!("Could not acquire the preferences write lock. Retry after other saves finish: {error}"))?;
        let original = read_preferences(path)?;
        // Read and validate before merging. Defaults must never replace an unreadable
        // file or data belonging to a newer schema, even after a failed startup load.
        let mut document = original
            .as_deref()
            .map(|bytes| decode_preferences(bytes, path).map(|(document, _)| document))
            .transpose()?
            .unwrap_or_default();
        let serde_json::Value::Object(known) = serde_json::to_value(self)
            .map_err(|error| format!("Could not encode Parhelion preferences: {error}"))?
        else {
            return Err("Parhelion preferences must be an object".into());
        };
        document.extend(known);
        let encoded = serde_json::to_vec_pretty(&document)
            .map_err(|error| format!("Could not encode Parhelion preferences: {error}"))?;
        let result = match original {
            Some(original) => sundial::package_authoring::replace_authoring_file_if_unchanged(
                path, &encoded, &original,
            ),
            None => sundial::storage::create_file(path, &encoded),
        };
        result.map_err(|error| format!("Could not save {}: {error}", path.display()))
    }

    fn validate(self) -> Result<(), String> {
        if self.schema != PREFERENCES_SCHEMA {
            return Err(format!(
                "Unsupported Parhelion preferences schema {}. Expected {PREFERENCES_SCHEMA}",
                self.schema
            ));
        }
        if !(1..=MAX_PACKAGE_BACKUP_RETENTION).contains(&self.package_backup_retention) {
            return Err(format!(
                "Package backup retention must be between 1 and {MAX_PACKAGE_BACKUP_RETENTION}"
            ));
        }
        Ok(())
    }
}

fn read_preferences(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("Could not read {}: {error}", path.display())),
    }
}

fn decode_preferences(
    bytes: &[u8],
    path: &Path,
) -> Result<
    (
        serde_json::Map<String, serde_json::Value>,
        ParhelionPreferences,
    ),
    String,
> {
    let document: serde_json::Value = sundial::package_authoring::read_json(bytes)
        .map_err(|error| format!("Could not parse {}: {error}", path.display()))?;
    let serde_json::Value::Object(document) = document else {
        return Err(format!("Preferences must be an object: {}", path.display()));
    };
    let preferences: ParhelionPreferences =
        serde_json::from_value(serde_json::Value::Object(document.clone()))
            .map_err(|error| format!("Could not parse {}: {error}", path.display()))?;
    preferences.validate()?;
    Ok((document, preferences))
}

fn preferences_path() -> Result<PathBuf, String> {
    sundial::package_authoring::parhelion_data_directory()
        .map(|directory| directory.join(PREFERENCES_FILE_NAME))
        .ok_or_else(|| "Could not locate Sundial's per-user data directory".to_owned())
}

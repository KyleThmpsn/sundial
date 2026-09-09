use std::{fs, io, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::{DEFAULT_PACKAGE_BACKUP_RETENTION, MAX_PACKAGE_BACKUP_RETENTION};

const PREFERENCES_SCHEMA: u32 = 1;
const PREFERENCES_FILE_NAME: &str = "preferences.json";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct ParhelionPreferences {
    pub(crate) schema: u32,
    pub limit_package_backups: bool,
    pub package_backup_retention: usize,
    pub backup_recipe_snapshots: bool,
}

impl Default for ParhelionPreferences {
    fn default() -> Self {
        Self {
            schema: PREFERENCES_SCHEMA,
            limit_package_backups: true,
            package_backup_retention: DEFAULT_PACKAGE_BACKUP_RETENTION,
            backup_recipe_snapshots: true,
        }
    }
}

impl ParhelionPreferences {
    pub(crate) fn load_default() -> Result<Self, String> {
        let path = preferences_path()?;
        match fs::File::open(&path) {
            Ok(file) => {
                let preferences: Self = sundial::package_authoring::read_json(file)
                    .map_err(|error| format!("Could not parse {}: {error}", path.display()))?;
                preferences.validate()?;
                Ok(preferences)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(format!("Could not open {}: {error}", path.display())),
        }
    }

    pub(crate) fn save_default(self) -> Result<PathBuf, String> {
        self.validate()?;
        let path = preferences_path()?;
        let parent = path
            .parent()
            .ok_or_else(|| format!("Preferences path has no parent: {}", path.display()))?;
        fs::create_dir_all(parent)
            .map_err(|error| format!("Could not create {}: {error}", parent.display()))?;
        let encoded = serde_json::to_vec_pretty(&self)
            .map_err(|error| format!("Could not encode Parhelion preferences: {error}"))?;
        sundial::package_authoring::replace_authoring_file(&path, &encoded)
            .map_err(|error| format!("Could not save {}: {error}", path.display()))?;
        Ok(path)
    }

    fn validate(self) -> Result<(), String> {
        if self.schema != PREFERENCES_SCHEMA {
            return Err(format!(
                "Unsupported Parhelion preferences schema {}; expected {PREFERENCES_SCHEMA}",
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

fn preferences_path() -> Result<PathBuf, String> {
    sundial::package_authoring::parhelion_data_directory()
        .map(|directory| directory.join(PREFERENCES_FILE_NAME))
        .ok_or_else(|| "Could not locate Sundial's per-user data directory".to_owned())
}

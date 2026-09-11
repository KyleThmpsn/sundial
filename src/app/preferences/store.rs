//! Preference loading distinguishes absence from corruption; writes preserve invalid originals.
use super::Preferences;
use crate::{paths, storage};
use std::{
    fs, io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub(in crate::app) struct LoadedPreferences {
    pub(in crate::app) preferences: Preferences,
    pub(in crate::app) warning: Option<String>,
}

pub(in crate::app) fn preferences_path() -> Option<PathBuf> {
    paths::config_dir().map(|path| path.join("preferences.json"))
}

fn legacy_preferences_path() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        paths::data_dir().map(|path| path.join("paths.json"))
    }
    #[cfg(not(windows))]
    {
        None
    }
}

pub(in crate::app) fn load_preferences() -> LoadedPreferences {
    load_from_paths(
        preferences_path().as_deref(),
        legacy_preferences_path().as_deref(),
    )
}

fn read_preferences(path: &Path) -> Result<Option<Preferences>, String> {
    match fs::read(path) {
        Ok(raw) => serde_json::from_slice(&raw)
            .map(Some)
            .map_err(|error| format!("Invalid preferences in {}: {error}", path.display())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!(
            "Could not read preferences at {}: {error}",
            path.display()
        )),
    }
}

fn load_from_paths(current: Option<&Path>, legacy: Option<&Path>) -> LoadedPreferences {
    for path in [current, legacy].into_iter().flatten() {
        match read_preferences(path) {
            Ok(Some(mut preferences)) => {
                let warning = if preferences.plug_defaults_version < 1 {
                    preferences.default_plug_selection_mode =
                        super::PlugSelectionMode::SocketAndGearType;
                    preferences.plug_defaults_version = 1;
                    save_preferences_from(current.unwrap_or(path), &preferences, path).err().map(|error| {
                        format!("The Socket + Gear Type default is active, but its one-time migration could not be saved: {error}")
                    })
                } else {
                    None
                };
                return LoadedPreferences {
                    preferences,
                    warning,
                };
            }
            Ok(None) => {}
            Err(error) => {
                return LoadedPreferences {
                    preferences: Preferences::default(),
                    warning: Some(format!(
                        "{error}. Defaults are in use. Invalid JSON will be preserved beside the preferences file before saving; unreadable files will not be overwritten."
                    )),
                };
            }
        }
    }
    LoadedPreferences {
        preferences: Preferences::default(),
        warning: None,
    }
}

pub(in crate::app) fn save_preferences(
    path: &Path,
    preferences: &Preferences,
) -> Result<(), String> {
    save_preferences_from(path, preferences, path)
}

fn save_preferences_from(
    path: &Path,
    preferences: &Preferences,
    source: &Path,
) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or("Sundial's preferences path has no parent folder")?;
    let mut document = serde_json::to_value(preferences)
        .map_err(|error| format!("Could not encode Sundial's preferences: {error}"))?;
    if source != path {
        let raw = fs::read(source)
            .map_err(|error| format!("Could not read legacy preferences: {error}"))?;
        let mut legacy: serde_json::Value = serde_json::from_slice(&raw)
            .map_err(|error| format!("Could not decode legacy preferences: {error}"))?;
        if let (Some(legacy), Some(updated)) = (legacy.as_object_mut(), document.as_object()) {
            legacy.extend(updated.clone());
        }
        document = legacy;
    }
    // Never silently replace a damaged file with defaults. Keep its exact bytes for recovery.
    match fs::read(path) {
        Ok(raw) if serde_json::from_slice::<Preferences>(&raw).is_err() => {
            preserve_invalid_preferences(path, &raw)?;
        }
        Ok(raw) => {
            let mut existing: serde_json::Value = serde_json::from_slice(&raw)
                .map_err(|error| format!("Could not decode existing preferences: {error}"))?;
            if let (Some(existing), Some(updated)) =
                (existing.as_object_mut(), document.as_object())
            {
                existing.extend(updated.clone());
            }
            document = existing;
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!(
                "Could not read {} before saving; it was not overwritten: {error}",
                path.display()
            ));
        }
    }
    fs::create_dir_all(parent)
        .map_err(|error| format!("Could not create Sundial's preferences folder: {error}"))?;
    let encoded = serde_json::to_vec_pretty(&document)
        .map_err(|error| format!("Could not encode Sundial's preferences: {error}"))?;
    storage::replace_file(path, &encoded)
        .map_err(|error| format!("Could not save Sundial's preferences: {error}"))
}

fn preserve_invalid_preferences(path: &Path, raw: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("Could not timestamp invalid preferences: {error}"))?
        .as_nanos();
    let backup = path.with_file_name(format!("preferences.invalid-{timestamp}.json"));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&backup)
        .map_err(|error| {
            format!(
                "Could not preserve invalid preferences at {}: {error}",
                backup.display()
            )
        })?;
    file.write_all(raw)
        .and_then(|_| file.sync_all())
        .map_err(|error| {
            format!(
                "Could not preserve invalid preferences at {}: {error}",
                backup.display()
            )
        })
}

#[cfg(test)]
mod tests;

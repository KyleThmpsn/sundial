//! Inspect the two supported runtime locations without changing installed content.
mod backup;
mod movement;
mod restore;
mod defaults;
pub(crate) use defaults::SettingsResetPlan;
pub(crate) use backup::archive_other_runtime;
use movement::move_without_replacing;
pub(crate) use restore::{RuntimeRestorePlan, preview_runtime_restore, restore_runtime};

use crate::game_settings::dawn::Runtime as DawnRuntime;
use pelite::resources::Name;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum RuntimeLocation {
    Root,
    BinX64,
}

impl RuntimeLocation {
    pub(crate) const ALL: [Self; 2] = [Self::Root, Self::BinX64];
    pub(crate) fn directory(self, install: &Path) -> PathBuf {
        match self {
            Self::Root => install.to_owned(),
            Self::BinX64 => install.join("bin/x64"),
        }
    }
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Root => "Game Folder",
            Self::BinX64 => "bin/x64 (Standard Location)",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RuntimeCopy {
    pub location: RuntimeLocation,
    pub dll_path: PathBuf,
    pub settings_path: PathBuf,
    pub version: Option<String>,
    pub dll_modified: String,
    pub settings_modified: String,
    pub dll_hash: Option<String>,
    pub settings_hash: Option<String>,
    pub schema: Option<u64>,
    pub bundled_schema: Option<u64>,
    pub dawn: bool,
    pub dawn_runtime: Option<DawnRuntime>,
    pub compatibility: Vec<String>,
    pub selection_problem: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct RuntimeInspection {
    pub copies: Vec<RuntimeCopy>,
}

impl RuntimeInspection {
    pub(crate) fn inspect(install: &Path) -> Self {
        Self { copies: RuntimeLocation::ALL.into_iter().filter_map(|location| {
            let directory = location.directory(install);
            let dll_path = directory.join("steam_api64.dll");
            if !dll_path.exists() { return None; }
            let settings_path = directory.join("Sunrise/settings.json");
            let dll = fs::read(&dll_path);
            let settings = fs::read(&settings_path);
            let version = dll.as_ref().ok().and_then(|b| super::sunrise_module_version(b));
            let defaults = dll.as_ref().ok().and_then(|b| embedded_defaults(b));
            let bundled_schema = defaults.as_ref().and_then(crate::game_settings::schema_version);
            let dawn = dll.as_ref().ok().is_some_and(|b| dawn_signature(b, defaults.as_ref()));
            let dawn_runtime = dawn.then(|| DawnRuntime::inspect(&dll_path));
            let parsed = settings.as_ref().ok().and_then(|b| settings_document(b));
            let schema = parsed.as_ref().and_then(crate::game_settings::schema_version);
            let mut selection_problem = if let Err(error) = &dll {
                Some(format!("Cannot read the runtime DLL: {error}"))
            } else if let Err(error) = &settings {
                Some(format!("Settings file is missing or unreadable: {error}"))
            } else if schema.is_none() {
                Some("Settings must contain valid JSON and a schema version before this copy can be selected.".into())
            } else { None };
            let mut compatibility = Vec::new();
            if let (Some(bundled), Some(current)) = (defaults.as_ref().and_then(crate::game_settings::schema_version), schema)
                && bundled != current {
                compatibility.push(format!("Settings v{current} differs from the DLL's bundled v{bundled}."));
            }
            if version.is_none() {
                compatibility.push("No Sunrise version resource was detected in this DLL.".into());
            }
            if dawn {
                compatibility.push("Recognized Dawn mission runtime.".into());
                if let Some(json) = &parsed {
                    if let Err(error) = dawn_runtime.as_ref().expect("detected Dawn").validate(json) {
                        selection_problem = Some(error);
                    }
                    if crate::game_settings::dawn::executor_enabled(json) {
                        compatibility.push("Omega Lua executor is enabled.".into());
                    }
                }
            }
            let mut copy = RuntimeCopy {
                location, version, schema, bundled_schema, dawn, dawn_runtime, compatibility, selection_problem,
                dll_modified: modified(&dll_path), settings_modified: modified(&settings_path),
                dll_hash: dll.as_ref().ok().map(|b| hash(b)),
                settings_hash: settings.as_ref().ok().map(|b| hash(b)),
                dll_path, settings_path,
            };
            if copy.selection_problem.is_none() && let Some(json) = &parsed {
                copy.selection_problem = copy.persistence_problem(json);
            }
            Some(copy)
        }).collect() }
    }
    pub(crate) fn duplicates(&self) -> bool {
        self.copies.len() == 2
    }

    pub(crate) fn for_settings(&self, settings_path: &Path) -> Option<&RuntimeCopy> {
        self.copies
            .iter()
            .find(|copy| copy.settings_path == settings_path)
    }
}

impl RuntimeCopy {
    pub(crate) fn persistence_problem(&self, json: &Value) -> Option<String> {
        let bundled = self.bundled_schema?;
        let current = crate::game_settings::schema_version(json)?;
        if bundled >= 18 && current < 18 {
            Some(format!(
                "This runtime bundles settings v{bundled} with SQLite account storage, but the open settings are v{current} with JSON accounts. Load matching settings and data/investment.sqlite3 before saving. Changing the version number alone does not migrate an account."
            ))
        } else if bundled < 18 && current >= 18 {
            Some(format!(
                "This runtime bundles settings v{bundled} with JSON account storage, but the open settings are v{current} with SQLite accounts. Restore matching JSON settings before saving."
            ))
        } else {
            None
        }
    }
}

fn embedded_defaults(bytes: &[u8]) -> Option<Value> {
    let image = pelite::PeFile::from_bytes(bytes).ok()?;
    let resources = image.resources().ok()?;
    serde_json::from_slice(
        resources
            .find_resource(&[Name::Id(10), Name::Id(101)])
            .ok()?,
    )
    .ok()
}

fn settings_document(bytes: &[u8]) -> Option<Value> {
    let text = std::str::from_utf8(bytes)
        .ok()?
        .trim_start_matches('\u{feff}');
    crate::strict_json::from_str(text).ok()
}

fn dawn_signature(bytes: &[u8], defaults: Option<&Value>) -> bool {
    const MARKERS: [&[u8]; 4] = [
        b"ev=coo_script mission=omega result=loaded format=lua",
        b"ev=coo_executor mission=omega mode=composition",
        b"Sunrise/scripts/omega.lua",
        b"coo_executor",
    ];
    defaults.is_some_and(|json| {
        crate::game_settings::schema_version(json) == Some(6)
            && json.pointer("/experiments/omega/coo_executor").is_some()
    }) && MARKERS
        .iter()
        .all(|marker| bytes.windows(marker.len()).any(|w| w == *marker))
}

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn modified(path: &Path) -> String {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .map_or_else(|| "Unavailable".into(), format_time)
}

fn format_time(value: SystemTime) -> String {
    time::OffsetDateTime::from(value)
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "Unavailable".into())
}

#[cfg(test)]
mod tests;

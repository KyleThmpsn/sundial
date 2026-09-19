//! Inspect the two supported runtime locations without changing installed content.
mod backup;
mod defaults;
mod movement;
mod restore;
mod workspace;
pub(crate) use backup::archive_other_runtime;
pub(crate) use defaults::SettingsResetPlan;
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
            let dll = fs::read(&dll_path);
            let identity = dll.as_ref().ok().and_then(|b| super::runtime_version(b));
            let defaults = dll.as_ref().ok().and_then(|b| embedded_defaults(b));
            let bundled_schema = defaults.as_ref().and_then(crate::game_settings::schema_version);
            let dawn = identity.as_ref().is_some_and(|(name, _)| *name == "Dawn");
            // A runtime owns the folder named after it, so its settings and durable account are
            // read from there rather than from whatever an earlier runtime left behind.
            let settings_path = directory.join(runtime_folder(dawn)).join("settings.json");
            let settings = fs::read(&settings_path);
            let version = identity.map(|(_, version)| version);
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
                compatibility.push("No recognized Sunrise or Dawn version resource was found in this DLL.".into());
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
}

impl RuntimeCopy {
    pub(crate) fn name(&self) -> &'static str {
        if self.dawn {
            "Dawn"
        } else if self.version.is_some() || self.bundled_schema.is_some() {
            "Sunrise"
        } else {
            "Unrecognized Runtime"
        }
    }

    pub(crate) fn persistence_problem(&self, json: &Value) -> Option<String> {
        // This is Sunrise's own storage migration: it moved accounts out of settings.json and
        // into data/investment.sqlite3 at v18. Dawn keeps its account in player-state.db at every
        // schema, so the rule says nothing about it, and what Dawn does expect of its settings is
        // reported by `DawnRuntime::validate` instead.
        if self.dawn {
            return None;
        }
        let bundled = self.bundled_schema?;
        let current = crate::game_settings::schema_version(json)?;
        let name = self.name();
        if bundled >= 18 && current < 18 {
            Some(format!(
                "{name} requires settings v{bundled} with SQLite account storage. The open settings are v{current}. Open matching settings and data/investment.sqlite3 before saving."
            ))
        } else if bundled < 18 && current >= 18 {
            Some(format!(
                "{name} requires settings v{bundled} with JSON accounts. The open settings are v{current} with SQLite account storage. Restore matching JSON settings or switch back to the runtime for this account before saving."
            ))
        } else {
            None
        }
    }
}

/// The folder a runtime keeps its settings and durable account in, which is named after the
/// runtime itself. Dawn 0.1 owns `Dawn`; Sunrise owns `Sunrise`.
pub(crate) const fn runtime_folder(dawn: bool) -> &'static str {
    if dawn { "Dawn" } else { "Sunrise" }
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

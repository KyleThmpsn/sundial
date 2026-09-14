//! Match writes to the runtime that takes precedence in the installation.
use super::{RuntimeCopy, RuntimeInspection, RuntimeLocation};
use serde_json::Value;
use std::path::Path;

impl RuntimeInspection {
    /// Installation precedence, also used by sunrise_module_path. This does not inspect a process.
    pub(crate) fn launch_copy(&self) -> Option<&RuntimeCopy> {
        RuntimeLocation::ALL
            .into_iter()
            .find_map(|location| self.copies.iter().find(|copy| copy.location == location))
    }

    pub(crate) fn workspace_problem(&self, settings_path: &Path, json: &Value) -> Option<String> {
        let runtime = self.launch_copy()?;
        if runtime.dll_hash.is_none() {
            return Some(format!(
                "Cannot verify the runtime DLL at {}. Recheck Runtime Copies in Installation preferences before applying or saving changes.",
                runtime.dll_path.display()
            ));
        }
        if let Some(problem) = runtime.persistence_problem(json) {
            return Some(problem);
        }
        let inactive_copy = inactive_settings(runtime, settings_path);
        // Dawn reads settings beside its DLL. Sunrise also supports settings.json at the game root.
        if inactive_copy || (runtime.dawn && settings_path != runtime.settings_path) {
            return Some(format!(
                "{} at {} takes precedence. Open its matching settings at {} before saving. Current settings: {}",
                runtime.name(),
                runtime.dll_path.display(),
                runtime.settings_path.display(),
                settings_path.display()
            ));
        }
        None
    }
}

fn inactive_settings(runtime: &RuntimeCopy, settings_path: &Path) -> bool {
    let directory = runtime.dll_path.parent();
    let install = match runtime.location {
        RuntimeLocation::Root => directory,
        RuntimeLocation::BinX64 => directory.and_then(Path::parent).and_then(Path::parent),
    };
    install.is_some_and(|install| {
        RuntimeLocation::ALL.into_iter().any(|location| {
            location != runtime.location
                && crate::paths::paths_equal(
                    &location.directory(install).join("Sunrise/settings.json"),
                    settings_path,
                )
        })
    })
}

//! Settings layout resolution and runtime source validation without app preferences.
use std::path::{Path, PathBuf};
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingsLayout {
    GameRoot,
    Root,
    BinX64,
    DawnRoot,
    DawnBinX64,
}

impl SettingsLayout {
    pub(crate) const ALL: [Self; 5] = [
        Self::GameRoot,
        Self::Root,
        Self::BinX64,
        Self::DawnRoot,
        Self::DawnBinX64,
    ];

    /// A runtime owns the folder named after it, so Dawn keeps its settings in `Dawn` and
    /// Sunrise in `Sunrise`. An installation that has run both holds one of each, which is why
    /// they are separate layouts rather than one folder whose name is guessed.
    pub(crate) fn relative_path(self) -> PathBuf {
        match self {
            Self::GameRoot => PathBuf::from("settings.json"),
            Self::Root => PathBuf::from("Sunrise").join("settings.json"),
            Self::BinX64 => PathBuf::from("bin")
                .join("x64")
                .join("Sunrise")
                .join("settings.json"),
            Self::DawnRoot => PathBuf::from("Dawn").join("settings.json"),
            Self::DawnBinX64 => PathBuf::from("bin")
                .join("x64")
                .join("Dawn")
                .join("settings.json"),
        }
    }
}

pub(crate) enum SettingsPathResolution {
    Found(SettingsLayout, PathBuf),
    Missing,
    Ambiguous,
}

pub(crate) fn settings_path_for_install(install: &Path, layout: SettingsLayout) -> PathBuf {
    install.join(layout.relative_path())
}

/// The settings the installed runtime reads, when one is detected and that file is present.
///
/// A runtime owns the folder named after it, so which one is installed decides where its settings
/// live. This comes before any saved layout: swapping the DLL is how a player changes runtime, and
/// a layout saved under the previous one would otherwise keep Sundial pointed at a file the game
/// no longer reads.
pub(crate) fn runtime_settings_path(install: &Path) -> Option<(SettingsLayout, PathBuf)> {
    let inspection = crate::package_runtime::installation::RuntimeInspection::inspect(install);
    let path = inspection.launch_copy()?.settings_path.clone();
    if !path.is_file() {
        return None;
    }
    let layout = SettingsLayout::ALL.into_iter().find(|layout| {
        crate::paths::paths_equal(&settings_path_for_install(install, *layout), &path)
    })?;
    Some((layout, path))
}

pub(crate) fn resolve_settings_path(
    install: &Path,
    preferred_layout: Option<SettingsLayout>,
) -> SettingsPathResolution {
    if let Some((layout, path)) = runtime_settings_path(install) {
        return SettingsPathResolution::Found(layout, path);
    }
    if let Some(layout) = preferred_layout {
        let path = settings_path_for_install(install, layout);
        if path.is_file() {
            return SettingsPathResolution::Found(layout, path);
        }
    }

    let existing = SettingsLayout::ALL
        .into_iter()
        .filter_map(|layout| {
            let path = settings_path_for_install(install, layout);
            path.is_file().then_some((layout, path))
        })
        .collect::<Vec<_>>();
    match existing.as_slice() {
        [] => SettingsPathResolution::Missing,
        [(layout, path)] => SettingsPathResolution::Found(*layout, path.clone()),
        _ => SettingsPathResolution::Ambiguous,
    }
}

pub(crate) fn missing_settings_message(install: &Path) -> String {
    let game_root = settings_path_for_install(install, SettingsLayout::GameRoot);
    let root = settings_path_for_install(install, SettingsLayout::Root);
    let bin_x64 = settings_path_for_install(install, SettingsLayout::BinX64);
    format!(
        "No Project Sunrise settings.json was found in the selected installation. Checked {}, {}, and {}. Choose your Sunrise install, the directory containing destiny2.exe, and confirm Project Sunrise is installed there",
        game_root.display(),
        root.display(),
        bin_x64.display()
    )
}

pub(crate) fn validate_runtime_document(install: &Path, path: &Path) -> Result<(), String> {
    let inspection = crate::package_runtime::installation::RuntimeInspection::inspect(install);
    let document = crate::persistence::json_document::load_workspace_json(path)?;
    if let Some(problem) = inspection.workspace_problem(path, &document) {
        return Err(problem);
    }
    Ok(())
}

#[cfg(test)]
mod tests;

//! Locating settings and shared application data without creating files.
use crate::app::{SettingsLayout, SettingsPathResolution};
use crate::paths;
use std::path::{Path, PathBuf};

pub(in crate::app) fn backups_path() -> Option<PathBuf> {
    crate::backups::root()
}

pub(in crate::app) fn catalog_path() -> Option<PathBuf> {
    paths::shadowkeep_catalog_path()
}

pub(in crate::app) fn settings_path_for_install(install: &Path, layout: SettingsLayout) -> PathBuf {
    install.join(layout.relative_path())
}

pub(in crate::app) fn resolve_settings_path(
    install: &Path,
    preferred_layout: Option<SettingsLayout>,
) -> SettingsPathResolution {
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

pub(in crate::app) fn missing_settings_message(install: &Path) -> String {
    let game_root = settings_path_for_install(install, SettingsLayout::GameRoot);
    let root = settings_path_for_install(install, SettingsLayout::Root);
    let bin_x64 = settings_path_for_install(install, SettingsLayout::BinX64);
    format!(
        "No Project Sunrise settings.json was found in the selected installation. Checked {}, {}, and {}. Choose the Destiny 2 Shadowkeep folder containing destiny2.exe and confirm Project Sunrise is installed there",
        game_root.display(),
        root.display(),
        bin_x64.display()
    )
}

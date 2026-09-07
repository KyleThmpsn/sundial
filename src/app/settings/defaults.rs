//! Defaults embedded in the installed Sunrise module.
use super::validate_document;
use crate::{
    game_settings,
    package_runtime::{installed_sunrise_module_version, sunrise_module_path},
    persistence::json_account::ensure_schema_v8_preferences,
};
use pelite::resources::Name;
use serde_json::Value;
use std::{fs, path::Path};

pub(in crate::app) fn detect_sunrise_version(install_path: &Path) -> String {
    installed_sunrise_module_version(install_path).unwrap_or_else(|| "Not detected".into())
}

pub(in crate::app) fn load_installed_sunrise_defaults(
    install_path: &Path,
) -> Result<Value, String> {
    let module_path = sunrise_module_path(install_path);
    let bytes = fs::read(&module_path).map_err(|error| {
        format!(
            "Could not read Project Sunrise's bundled defaults from {}: {error}",
            module_path.display()
        )
    })?;
    let image = pelite::PeFile::from_bytes(&bytes).map_err(|error| {
        format!(
            "The installed Project Sunrise module is not a valid PE file ({}): {error}",
            module_path.display()
        )
    })?;
    let resources = image.resources().map_err(|error| {
        format!(
            "Could not read resources from the installed Project Sunrise module ({}): {error}",
            module_path.display()
        )
    })?;
    // Resource 101 is IDR_DEFAULT_SETTINGS in every supported Sunrise release.
    let encoded = resources
        .find_resource(&[Name::Id(10), Name::Id(101)])
        .map_err(|_| {
            format!(
                "The installed Project Sunrise module does not contain its default settings resource: {}",
                module_path.display()
            )
        })?;
    if encoded.is_empty() {
        return Err("The installed Project Sunrise default settings resource is empty".into());
    }
    let mut document: Value = serde_json::from_slice(encoded)
        .map_err(|error| format!("Project Sunrise's bundled defaults are invalid JSON: {error}"))?;
    ensure_schema_v8_preferences(&mut document);
    if game_settings::schema_version(&document).is_none() {
        return Err("Project Sunrise's bundled defaults have no valid schema version".into());
    }
    validate_document(&document).map_err(|error| {
        format!("Project Sunrise's bundled defaults contain an unexpected setting: {error}")
    })?;
    Ok(document)
}

//! Strict settings-document reads without schema migration or UI policy.
pub(crate) mod encoding;
use serde_json::Value;
use std::{fs, io, path::Path};
pub(crate) fn load_workspace_json(path: &Path) -> Result<Value, String> {
    let raw = fs::read_to_string(path).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            format!(
                "No Project Sunrise settings.json was found in the selected installation. Expected: {}. Choose your Sunrise install, the directory containing destiny2.exe, and confirm Project Sunrise is installed there",
                path.display()
            )
        } else {
            format!("Could not read {}: {error}", path.display())
        }
    })?;
    // Sunrise accepts a leading UTF-8 BOM because common Windows editors add one. Match the
    // loader before handing the document to serde_json, which otherwise treats it as a token.
    let raw = raw.strip_prefix('\u{feff}').unwrap_or(&raw);
    crate::strict_json::from_str(raw)
        .map_err(|e| format!("Invalid JSON in {}: {e}", path.display()))
}

pub(crate) fn verify_unchanged(path: &Path, expected: &Value) -> Result<(), String> {
    verify_value_unchanged(&load_workspace_json(path)?, expected)
}
pub(crate) fn verify_value_unchanged(current: &Value, expected: &Value) -> Result<(), String> {
    if current == expected {
        Ok(())
    } else {
        Err("settings.json changed outside Sundial after it was loaded. Reload before saving so newer data is not overwritten".into())
    }
}

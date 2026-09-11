//! Reviewed client-setting changes accompanying an authored package installation.

use super::AuthoredAccountCleanup;
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthoredClientSettings {
    pub settings_path: PathBuf,
    pub original_bytes: Vec<u8>,
    pub updated_bytes: Vec<u8>,
}

impl AuthoredClientSettings {
    /// A JSON account and its client settings must be committed as one file change.
    pub fn merge_account_change(
        &self,
        cleanup: &mut AuthoredAccountCleanup,
    ) -> Result<bool, String> {
        if !crate::paths::paths_equal(&self.settings_path, &cleanup.settings_path) {
            return Ok(false);
        }
        if self.original_bytes != cleanup.original_bytes {
            return Err("Sunrise settings changed while preparing installation".into());
        }
        if let Some(updated) = disable_lore_reveal(&cleanup.cleaned_bytes)? {
            cleanup.cleaned_bytes = updated;
        }
        Ok(true)
    }
}

pub fn preview_authored_client_settings(
    install: &Path,
) -> Result<Option<AuthoredClientSettings>, String> {
    let settings_path = crate::app::authoring_bridge::authored_client_settings_path(install)?;
    let original_bytes = std::fs::read(&settings_path).map_err(|error| error.to_string())?;
    Ok(
        disable_lore_reveal(&original_bytes)?.map(|updated_bytes| AuthoredClientSettings {
            settings_path,
            original_bytes,
            updated_bytes,
        }),
    )
}

fn disable_lore_reveal(bytes: &[u8]) -> Result<Option<Vec<u8>>, String> {
    let mut document: Value =
        crate::package_authoring::read_json(bytes).map_err(|error| error.to_string())?;
    // Shipped v8 has no lore reveal hook. Do not introduce newer settings into that schema.
    let version = crate::game_settings::schema_version(&document)
        .ok_or("Sunrise settings are missing a valid schema version")?;
    if version < 16 {
        return Ok(None);
    }
    if version > 18 {
        return Err(
            "This Sunrise settings schema is newer than the package installer supports".into(),
        );
    }
    let path = "/client/reveal_lore_books";
    if let Some(current) = crate::persistence::json_fields::optional_value(&document, path)? {
        if !current.is_boolean() {
            return Err("Reveal Lore Books must be a boolean".into());
        }
        if current == &Value::Bool(false) {
            // Keep an unchanged proposal so installation still guards and verifies this value.
            return Ok(Some(bytes.to_vec()));
        }
    }
    crate::persistence::json_fields::write_value(&mut document, path, false.into())?;
    serde_json::to_vec_pretty(&document)
        .map(Some)
        .map_err(|error| error.to_string())
}

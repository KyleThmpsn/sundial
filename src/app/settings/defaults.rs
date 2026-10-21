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
    let [encoded] = installed_resources(install_path, [(101, "default settings")])?;
    decode_settings_defaults(&encoded)
}

fn decode_settings_defaults(encoded: &str) -> Result<Value, String> {
    let mut document: Value = serde_json::from_str(encoded)
        .map_err(|error| format!("Project Sunrise's bundled defaults are invalid JSON: {error}"))?;
    let version = game_settings::schema_version(&document)
        .ok_or("Project Sunrise's bundled defaults have no valid schema version")?;
    let validation = if version >= 18 {
        // v18 account defaults live in the database resources, not settings.json.
        game_settings::validate_non_account(&document)
    } else {
        ensure_schema_v8_preferences(&mut document);
        validate_document(&document)
    };
    validation.map_err(|error| {
        format!("Project Sunrise's bundled defaults contain an unexpected setting: {error}")
    })?;
    Ok(document)
}

pub(in crate::app) fn load_installed_account_defaults(
    install_path: &Path,
) -> Result<crate::persistence::sqlite_account::AccountDefaults, String> {
    with_installed_resources(install_path, |read| decode_account_defaults(read))
}

fn decode_account_defaults(
    read: &mut dyn FnMut(u32, &str) -> Result<String, String>,
) -> Result<crate::persistence::sqlite_account::AccountDefaults, String> {
    use crate::persistence::sqlite_account::Schema;
    let schema = read(106, "investment schema")?;
    let version = Schema::from_sql(&schema).map_err(|error| error.to_string())?;
    let rows = read(107, "default investment data")?;
    let settings_schema = read(109, "account settings schema")?;
    let settings_rows = read(110, "default account settings")?;
    let starter = if version == Schema::V3 {
        Some(read(111, "character starter defaults")?)
    } else {
        None
    };
    Ok(crate::persistence::sqlite_account::AccountDefaults {
        schema,
        rows,
        settings_schema,
        settings_rows,
        starter,
    })
}

fn installed_resources<const N: usize>(
    install_path: &Path,
    requested: [(u32, &str); N],
) -> Result<[String; N], String> {
    with_installed_resources(install_path, |read| {
        let mut result = std::array::from_fn(|_| String::new());
        for (text, (id, label)) in result.iter_mut().zip(requested) {
            *text = read(id, label)?;
        }
        Ok(result)
    })
}

fn with_installed_resources<T>(
    install_path: &Path,
    decode: impl FnOnce(&mut dyn FnMut(u32, &str) -> Result<String, String>) -> Result<T, String>,
) -> Result<T, String> {
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
    decode(&mut |id, label| {
        let encoded = resources.find_resource(&[Name::Id(10), Name::Id(id)]).map_err(|_| {
            format!("The installed Project Sunrise module does not contain its {label} resource: {}", module_path.display())
        })?;
        if encoded.is_empty() {
            return Err(format!(
                "The installed Project Sunrise {label} resource is empty"
            ));
        }
        Ok(std::str::from_utf8(encoded)
            .map_err(|error| {
                format!("Project Sunrise's {label} resource is not valid UTF-8: {error}")
            })?
            .to_owned())
    })
}

#[cfg(test)]
mod tests;

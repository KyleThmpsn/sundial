//! Explicit experimental account conversion. Callers stage output and keep the source backup.
mod export;
mod import;
pub(crate) use export::{ItemSupport, to_json};
pub(crate) use import::from_json;

use super::{AccountDefaults, SqliteAccountDocument};
use rusqlite::Connection;
use serde_json::Value;
use std::path::Path;

pub(crate) fn snapshot(source: &Path, destination: &Path) -> Result<(), String> {
    if destination.exists() {
        return Err("The conversion snapshot already exists.".into());
    }
    super::writer::create_integrity_checked_snapshot(source, destination).map_err(err)
}

pub(crate) fn restore(path: &Path, expected: &[u8], backup: &Path) -> Result<(), String> {
    super::snapshot::restore(path, expected, backup)
}

fn number(value: &Value, field: &str) -> Result<u64, String> {
    crate::hash::parse_unsigned_value(value).ok_or_else(|| format!("Invalid {field}."))
}

fn err(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests;

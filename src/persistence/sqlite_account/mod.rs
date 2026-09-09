//! Adapter for the official Sunrise investment database (schema 2).
//!
//! The contract is pinned to one reviewed commit. Sundial never creates or migrates a database,
//! and unknown nested versions are surfaced explicitly so they cannot be mistaken for the pinned
//! layout. Writes require an exact loaded document, a matching in-transaction revision, and a
//! verified SQLite-native backup.

mod contract;
mod document;
mod entitlements;
mod error;
pub(crate) mod package;
mod progression;
mod reader;
mod runtime;
mod settings;
mod validation;
mod writer;

use std::path::Path;

use sundial_account::{AccountSettingsState, CharacterState, InstanceSoid, ProfileState};

pub(crate) use document::{SqliteAccountDocument, SqliteAccountDocumentLoad};
pub(crate) use error::{SqliteAccountError, SqliteAccountIncompatibility};
pub(crate) use writer::{SqliteRestoreReceipt, SqliteSaveReceipt};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SqliteAccountSnapshot {
    primary_soid: InstanceSoid,
    profile: ProfileState,
    characters: CharacterState,
    settings: AccountSettingsState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SqliteAccountLoad {
    Empty,
    Incompatible(SqliteAccountIncompatibility),
    Loaded(SqliteAccountSnapshot),
}

pub(crate) fn load_document(path: &Path) -> Result<SqliteAccountDocumentLoad, SqliteAccountError> {
    document::load(path)
}

pub(crate) fn save_document(
    document: &mut SqliteAccountDocument,
) -> Result<SqliteSaveReceipt, SqliteAccountError> {
    writer::save(document)
}

pub(crate) fn rollback_save(
    path: &Path,
    receipt: &SqliteSaveReceipt,
) -> Result<(), SqliteAccountError> {
    writer::rollback_save(path, receipt)
}

pub(crate) fn validate_backup(backup: &Path) -> Result<(), SqliteAccountError> {
    writer::validate_backup(backup)
}

pub(crate) fn restore_backup_safely(
    path: &Path,
    backup: &Path,
) -> Result<SqliteRestoreReceipt, SqliteAccountError> {
    writer::restore_backup_safely(path, backup)
}

#[cfg(test)]
pub(crate) mod tests;

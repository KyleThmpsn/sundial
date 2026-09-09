//! Adapter for the provisional SQLite contract in Sunrise PR 88.
//!
//! The contract is pinned to one reviewed commit. Sundial never creates or migrates a database,
//! and unknown nested versions are surfaced explicitly so they cannot be mistaken for the pinned
//! layout. Writes require an exact loaded document, a matching in-transaction revision, and a
//! verified SQLite-native backup.

mod contract;
mod document;
mod error;
mod reader;
mod settings;
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

#[cfg(test)]
impl SqliteAccountSnapshot {
    pub(crate) const fn primary_soid(&self) -> InstanceSoid {
        self.primary_soid
    }

    pub(crate) const fn profile(&self) -> &ProfileState {
        &self.profile
    }

    pub(crate) const fn characters(&self) -> &CharacterState {
        &self.characters
    }

    pub(crate) const fn settings(&self) -> &AccountSettingsState {
        &self.settings
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SqliteAccountLoad {
    #[cfg(test)]
    Missing,
    Empty,
    Incompatible(SqliteAccountIncompatibility),
    Loaded(SqliteAccountSnapshot),
}

#[cfg(test)]
pub(crate) fn load(path: &Path) -> Result<SqliteAccountLoad, SqliteAccountError> {
    reader::load(path)
}

pub(crate) fn load_document(path: &Path) -> Result<SqliteAccountDocumentLoad, SqliteAccountError> {
    document::load(path)
}

pub(crate) fn save_document(
    document: &mut SqliteAccountDocument,
) -> Result<SqliteSaveReceipt, SqliteAccountError> {
    writer::save(document)
}

pub(crate) fn restore_backup(path: &Path, backup: &Path) -> Result<(), SqliteAccountError> {
    writer::restore_backup(path, backup)
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

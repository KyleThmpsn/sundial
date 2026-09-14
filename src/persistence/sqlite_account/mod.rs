//! Adapter for the official Sunrise investment database (schema 2).
//!
//! The contract is pinned to one reviewed commit. Sundial never implicitly creates or migrates an active database,
//! and unknown nested versions are surfaced explicitly so they cannot be mistaken for the pinned
//! layout. Writes require an exact loaded document, a matching in-transaction revision, and a
//! verified SQLite-native backup.

mod contract;
pub(crate) mod conversion;
mod defaults;
mod document;
mod entitlements;
mod error;
mod inventory_state;
pub(crate) mod package;
mod positions;
mod progression;
mod reader;
mod runtime;
mod settings;
pub(crate) mod snapshot;
mod validation;
mod writer;

use sundial_account::{AccountSettingsState, CharacterState, InstanceSoid, ProfileState};

pub(crate) use defaults::{AccountDefaults, ResetPlan};
pub(crate) use document::{
    SqliteAccountDocument, SqliteAccountDocumentLoad, load as load_document,
};
pub(crate) use error::{SqliteAccountError, SqliteAccountIncompatibility};
pub(crate) use inventory_state::{CharacterStack, PendingReward};
pub(crate) use writer::{
    SqliteRestoreReceipt, SqliteSaveReceipt, restore_backup_safely, rollback_save,
    save as save_document, validate_backup,
};

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

#[cfg(test)]
pub(crate) mod tests;

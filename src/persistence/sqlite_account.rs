//! Adapter for the official Sunrise investment database (schemas 2 and 3).
//!
//! The database version selects its required tables and columns. Loading never creates or
//! migrates an active database. Writes require the loaded document's revision to match inside
//! the transaction and a verified SQLite backup before mutation.

mod contract;
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

pub(crate) use contract::Schema;
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
    schema: Schema,
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

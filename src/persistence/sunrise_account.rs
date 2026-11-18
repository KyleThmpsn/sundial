//! Adapter for Sunrise's account database, `data/investment.sqlite3` (schemas 2, 3, 5 and 7).
//!
//! Sunrise settings before v18 keep the account in `settings.json`, which `json_account` owns,
//! and Dawn's `player-state.db` is `dawn_account`'s separate contract.
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
    SunriseAccountDocument, SunriseAccountDocumentLoad, load as load_document,
};
pub(crate) use error::{SunriseAccountError, SunriseAccountIncompatibility};
pub(crate) use inventory_state::{CharacterStack, PendingReward};
pub(crate) use writer::{
    SunriseRestoreReceipt, SunriseSaveReceipt, restore_backup_checked, rollback_save,
    save_checked as save_document_checked, validate_backup,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SunriseAccountSnapshot {
    schema: Schema,
    primary_soid: InstanceSoid,
    profile: ProfileState,
    characters: CharacterState,
    settings: AccountSettingsState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SunriseAccountLoad {
    Empty,
    Incompatible(SunriseAccountIncompatibility),
    Loaded(SunriseAccountSnapshot),
}

#[cfg(test)]
pub(crate) mod tests;

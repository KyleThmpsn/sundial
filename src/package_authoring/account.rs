//! App adapters for account changes accompanying Parhelion package operations.
pub use crate::account::{
    AuthoredAccountCleanup, AuthoredCollectionUnlock, AuthoredItemMove, AuthoredMoveOutcome,
    AuthoredSlotChange, AuthoredSlotReplacement, AuthoredSocketChange,
    read_authored_account_source, replace_authored_account_source,
    validate_authored_cleanup_backend,
};
use crate::app::authoring_bridge;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};
mod client_settings;
pub use client_settings::{
    AuthoredClientSettings, preview_authored_client_settings,
    preview_authored_client_settings_for_runtime,
};
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthoredProfileSyncReport {
    pub settings_path: PathBuf,
    pub backup_path: Option<PathBuf>,
    pub newly_set_unlocks: usize,
    pub total_unlocks: usize,
}

pub fn preview_authored_account_cleanup(
    install: &Path,
    item_hashes: &BTreeSet<u32>,
    unlocks: &[AuthoredCollectionUnlock],
) -> Result<AuthoredAccountCleanup, String> {
    authoring_bridge::preview_account_cleanup(install, item_hashes, unlocks)
}

/// Proposes removed references and retained-item socket resizing in one account transaction.
/// The caller must review the proposal, verify its source bytes, and journal the account together
/// with package replacement. Removing sockets truncates only the removed suffix of authored lists.
pub fn preview_authored_account_replacement(
    install: &Path,
    item_hashes: &BTreeSet<u32>,
    unlocks: &[AuthoredCollectionUnlock],
    socket_changes: &[AuthoredSocketChange],
) -> Result<AuthoredAccountCleanup, String> {
    preview_authored_account_replacement_with_slots(
        install,
        item_hashes,
        unlocks,
        socket_changes,
        None,
    )
}

/// Includes native slot changes and verified incoming inventory capacities in the same review.
pub fn preview_authored_account_replacement_with_slots(
    install: &Path,
    item_hashes: &BTreeSet<u32>,
    unlocks: &[AuthoredCollectionUnlock],
    socket_changes: &[AuthoredSocketChange],
    slots: Option<&AuthoredSlotReplacement>,
) -> Result<AuthoredAccountCleanup, String> {
    authoring_bridge::preview_account_replacement(
        install,
        item_hashes,
        unlocks,
        socket_changes,
        slots,
    )
}

/// Uses an already verified runtime snapshot so account review and package mutation share one
/// runtime identity and reject any later DLL change.
pub fn preview_authored_account_replacement_for_runtime(
    install: &Path,
    runtime: &crate::package_authoring::RuntimeSnapshot,
    item_hashes: &BTreeSet<u32>,
    unlocks: &[AuthoredCollectionUnlock],
    socket_changes: &[AuthoredSocketChange],
    slots: Option<&AuthoredSlotReplacement>,
) -> Result<AuthoredAccountCleanup, String> {
    authoring_bridge::preview_account_replacement_with_runtime(
        install,
        runtime,
        item_hashes,
        unlocks,
        socket_changes,
        slots,
    )
}

/// Ensures every authored collection unlock is set in the active account source.
/// The installed runtime chooses that source: a Dawn install keeps its unlocks as durable flags
/// in player-state.db, while Sunrise keeps them in the settings document as compact runs or in
/// the investment database as native sparse unlock banks.
/// Saves preserve existing values and require a verified backup and an unchanged source.
pub fn synchronize_authored_collection_unlocks(
    install: &Path,
    unlocks: &[AuthoredCollectionUnlock],
) -> Result<AuthoredProfileSyncReport, String> {
    let unique = unlocks.iter().copied().collect::<BTreeSet<_>>();
    if unique.len() != unlocks.len() {
        return Err(
            "Authored collection unlocks contain duplicate definition/bank/slot rows".into(),
        );
    }
    let rows = unique
        .iter()
        .map(|unlock| {
            (
                usize::from(unlock.definition_index),
                unlock.bank,
                unlock.slot,
            )
        })
        .collect::<Vec<_>>();
    let (settings_path, backup_path, newly_set_unlocks) =
        authoring_bridge::synchronize_authored_collection_unlocks(install, &rows)?;
    Ok(AuthoredProfileSyncReport {
        settings_path,
        backup_path,
        newly_set_unlocks,
        total_unlocks: rows.len(),
    })
}

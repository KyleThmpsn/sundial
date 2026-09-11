//! Account proposals and synchronization accompanying authored package transactions.
use crate::app::authoring_bridge;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};
pub(crate) mod placement;
pub use placement::{
    AuthoredItemMove, AuthoredMoveOutcome, AuthoredSlotChange, AuthoredSlotReplacement,
};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct AuthoredCollectionUnlock {
    pub definition_index: u16,
    pub bank: u8,
    pub slot: u16,
}

/// Read-only, lossless proposal. The caller must back up and check the original bytes before
/// committing this together with package replacement or removal. Review does not write accounts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthoredAccountCleanup {
    pub settings_path: PathBuf,
    pub original_bytes: Vec<u8>,
    pub cleaned_bytes: Vec<u8>,
    pub removed_items: std::collections::BTreeMap<u32, usize>,
    pub cleared_plugs: usize,
    pub removed_reward_rules: usize,
    pub cleared_unlocks: usize,
    pub resized_items: std::collections::BTreeMap<u32, usize>,
    pub slot_moves: Vec<AuthoredItemMove>,
}

/// Verified native socket layouts for a retained definition in a replacement generation.
/// Account proposals preserve existing selections and use new defaults only for added sockets.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthoredSocketChange {
    pub definition_hash: u32,
    pub previous_socket_count: usize,
    pub default_plugs: Vec<Option<u32>>,
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

/// Checks that package transactions use the active account source.
pub fn validate_authored_cleanup_backend(path: &Path) -> Result<(), String> {
    if is_database(path) {
        let settings = path
            .parent()
            .and_then(Path::parent)
            .map(|directory| directory.join("settings.json"));
        if let Some(settings) = settings.filter(|settings| settings.is_file()) {
            let bytes = std::fs::read(settings).map_err(|e| e.to_string())?;
            let document =
                crate::package_authoring::read_json(&bytes[..]).map_err(|e| e.to_string())?;
            if !crate::game_settings::requires_sqlite_account(&document) {
                return Err("The settings schema uses JSON account data. Reload the package operation before updating it".into());
            }
        }
        return Ok(());
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let document = crate::package_authoring::read_json(&bytes[..]).map_err(|e| e.to_string())?;
    if crate::game_settings::requires_sqlite_account(&document) {
        return Err(
            "The active account uses SQLite. Reload the package operation before updating it"
                .into(),
        );
    }
    Ok(())
}
fn is_database(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|name| name == "investment.sqlite3")
}
/// Returns journal bytes. SQLite uses a complete logical snapshot including uncheckpointed WAL data.
pub fn read_authored_account_source(path: &Path) -> Result<Vec<u8>, String> {
    validate_authored_cleanup_backend(path)?;
    if is_database(path) {
        return crate::persistence::sqlite_account::package::read(path);
    }
    std::fs::read(path).map_err(|e| e.to_string())
}
/// Applies reviewed journal bytes with a concurrent-change check and an atomic backend write.
pub fn replace_authored_account_source(
    path: &Path,
    expected: &[u8],
    updated: &[u8],
) -> Result<(), String> {
    validate_authored_cleanup_backend(path)?;
    if is_database(path) {
        return crate::persistence::sqlite_account::package::replace(path, expected, updated);
    }
    if std::fs::read(path).map_err(|e| e.to_string())? != expected {
        return Err("The account changed after review".into());
    }
    crate::package_authoring::replace_authoring_file(path, updated).map_err(|e| e.to_string())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthoredProfileSyncReport {
    pub settings_path: PathBuf,
    pub backup_path: Option<PathBuf>,
    pub newly_set_unlocks: usize,
    pub total_unlocks: usize,
}

/// Ensures every authored collection unlock is set in the active account source.
/// JSON accounts use compact runs. SQLite accounts use the native sparse unlock banks.
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

//! Account proposals and synchronization accompanying authored package transactions.
use crate::app::authoring_bridge;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct AuthoredCollectionUnlock {
    pub definition_index: u16,
    pub bank: u8,
    pub slot: u16,
}

/// Read-only, lossless proposal. The caller must back up and check the original bytes before
/// committing this together with package removal. No account writes happen during review.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthoredAccountCleanup {
    pub settings_path: PathBuf,
    pub original_bytes: Vec<u8>,
    pub cleaned_bytes: Vec<u8>,
    pub removed_items: std::collections::BTreeMap<u32, usize>,
    pub cleared_plugs: usize,
    pub removed_reward_rules: usize,
    pub cleared_unlocks: usize,
}

pub fn preview_authored_account_cleanup(
    install: &Path,
    item_hashes: &BTreeSet<u32>,
    unlocks: &[AuthoredCollectionUnlock],
) -> Result<AuthoredAccountCleanup, String> {
    authoring_bridge::preview_account_cleanup(install, item_hashes, unlocks)
}

/// Checks the account backend before a JSON cleanup/recovery transaction.
pub fn validate_authored_cleanup_backend(settings_path: &Path) -> Result<(), String> {
    #[cfg(feature = "sqlite-account")]
    if settings_path.with_file_name("state.sqlite3").exists() {
        return Err("Automatic uninstall cleanup is not available for SQLite accounts. Remove custom items in Sundial first, then use package-only uninstall.".into());
    }
    let _ = settings_path;
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthoredProfileSyncReport {
    pub settings_path: PathBuf,
    pub backup_path: Option<PathBuf>,
    pub newly_set_unlocks: usize,
    pub total_unlocks: usize,
}

/// Ensures every authored collection unlock is set in the selected `settings.json` policy.
///
/// Unlock policy remains in `settings.json` even when SQLite supplies account inventory. The
/// active settings layout is resolved through Sundial preferences. Existing flags are kept,
/// missing flags are inserted through Sundial's compact-run encoder. Authored bank-1 rows may use
/// the explicitly bounded padding extension of the Shadowkeep account-flag region. Any changed
/// file is backed up and atomically verified by Sundial's settings persistence layer.
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

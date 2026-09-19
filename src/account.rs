//! Account proposals and synchronization accompanying authored package transactions.
use std::path::{Path, PathBuf};
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

impl AuthoredAccountCleanup {
    /// Whether the proposal touches the account at all.
    pub fn changed_anything(&self) -> bool {
        !self.removed_items.is_empty()
            || self.cleared_plugs != 0
            || self.removed_reward_rules != 0
            || self.cleared_unlocks != 0
            || !self.resized_items.is_empty()
            || !self.slot_moves.is_empty()
    }
}

/// Verified native socket layouts for a retained definition in a replacement generation.
/// Account proposals preserve existing selections and use new defaults only for added sockets.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthoredSocketChange {
    pub definition_hash: u32,
    pub previous_socket_count: usize,
    pub default_plugs: Vec<Option<u32>>,
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
                crate::strict_json::from_reader(&bytes[..]).map_err(|e| e.to_string())?;
            if !crate::game_settings::requires_sqlite_account(&document) {
                return Err("The settings schema uses JSON account data. Reload the package operation before updating it".into());
            }
        }
        return Ok(());
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let document = crate::strict_json::from_reader(&bytes[..]).map_err(|e| e.to_string())?;
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

/// Dawn keeps its whole account here. Its settings.json is the seed it consumed on first boot and
/// never reads again, so nothing about a Dawn account is decided from that file.
fn is_dawn_database(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|name| name == crate::persistence::DAWN_DATABASE_NAME)
}
/// Returns journal bytes. SQLite uses a complete logical snapshot including uncheckpointed WAL data.
pub fn read_authored_account_source(path: &Path) -> Result<Vec<u8>, String> {
    if is_dawn_database(path) {
        return crate::persistence::dawn_account::read_snapshot(path);
    }
    validate_authored_cleanup_backend(path)?;
    if is_database(path) {
        return crate::persistence::sqlite_account::snapshot::read(path);
    }
    std::fs::read(path).map_err(|e| e.to_string())
}
/// Applies reviewed journal bytes with a concurrent-change check and an atomic backend write.
pub fn replace_authored_account_source(
    path: &Path,
    expected: &[u8],
    updated: &[u8],
) -> Result<(), String> {
    if is_dawn_database(path) {
        return crate::persistence::dawn_account::replace(path, expected, updated);
    }
    validate_authored_cleanup_backend(path)?;
    if is_database(path) {
        return crate::persistence::sqlite_account::package::replace(path, expected, updated);
    }
    if std::fs::read(path).map_err(|e| e.to_string())? != expected {
        return Err("The account changed after review".into());
    }
    crate::storage::replace_file(path, updated).map_err(|e| e.to_string())
}

mod proposal;
pub use proposal::preview_replacement;
pub(crate) mod source;
pub(crate) mod unlocks;

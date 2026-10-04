//! Account proposals and synchronization accompanying authored package transactions.
use std::path::{Path, PathBuf};
mod grants;
pub use grants::{
    AuthoredGrantOutcome, AuthoredGrantReport, AuthoredGrantTarget, AuthoredItemGrant,
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
/// Account proposals preserve existing selections and use new defaults only for added sockets,
/// and for lanes whose default changed when a saved selection still holds the old default.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthoredSocketChange {
    pub definition_hash: u32,
    pub previous_socket_count: usize,
    pub default_plugs: Vec<Option<u32>>,
    /// Lanes kept by both generations whose default plug changed, each with the installed
    /// generation's default. A saved selection of that old default follows the definition to
    /// its new one, as when a private plug takes the place of a stock default.
    pub replaced_defaults: Vec<(usize, u32)>,
}

impl AuthoredSocketChange {
    /// The plug a saved selection in `lane` becomes: the new default when it still holds the
    /// lane's replaced default, `None` when it stays as it is. The outer `Some` carries the new
    /// default, which may itself be an empty lane.
    #[must_use]
    pub fn replacement(&self, lane: usize, saved: Option<u32>) -> Option<Option<u32>> {
        let (_, old) = self
            .replaced_defaults
            .iter()
            .find(|(replaced, _)| *replaced == lane)?;
        (saved == Some(*old)).then(|| self.default_plugs.get(lane).copied().flatten())
    }
}

/// Checks a replacement's socket changes before any account backend applies them: each names an
/// item the replacement keeps, once, with no more lanes than an item holds and no default plug
/// that is 0 or the disabled sentinel. A replaced default names a lane both generations have,
/// once, with an old default that is a real plug and differs from the new one.
pub(crate) fn validate_socket_changes(
    removed: &std::collections::BTreeSet<u32>,
    changes: &[AuthoredSocketChange],
) -> Result<(), String> {
    let capacity = crate::account_contract::MAX_ITEM_PLUGS;
    let mut seen = std::collections::BTreeSet::new();
    for change in changes {
        let shared = change.previous_socket_count.min(change.default_plugs.len());
        let mut lanes = std::collections::BTreeSet::new();
        if removed.contains(&change.definition_hash)
            || !seen.insert(change.definition_hash)
            || change.previous_socket_count > capacity
            || change.default_plugs.len() > capacity
            || change
                .default_plugs
                .iter()
                .flatten()
                .any(|hash| *hash == 0 || *hash == u32::MAX)
            || change.replaced_defaults.iter().any(|&(lane, old)| {
                lane >= shared
                    || !lanes.insert(lane)
                    || old == 0
                    || old == u32::MAX
                    || change.default_plugs[lane] == Some(old)
            })
        {
            return Err("The replacement has conflicting or unsupported socket layouts".into());
        }
    }
    Ok(())
}

/// Checks that package transactions use the active account source.
pub fn validate_authored_cleanup_backend(path: &Path) -> Result<(), String> {
    if is_dawn_database(path) {
        return Ok(());
    }
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

/// Dawn keeps its whole account here. It imports the account members from settings.json only when
/// it creates this database, so later account state is never decided from the JSON seed. Dawn still
/// reads unrelated runtime configuration from settings.json on every startup.
fn is_dawn_database(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|name| name == crate::persistence::DAWN_DATABASE_NAME)
}

#[cfg(test)]
mod backend_tests {
    use super::*;

    #[test]
    fn dawn_database_is_not_parsed_as_json_during_backend_validation() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory
            .path()
            .join(crate::persistence::DAWN_DATABASE_NAME);
        std::fs::write(&path, b"SQLite format 3\0not JSON").unwrap();

        validate_authored_cleanup_backend(&path).unwrap();
    }
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

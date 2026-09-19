//! Persistence adapters for storage-neutral account state.

pub(crate) mod json_document;
pub(crate) mod json_fields;

pub(crate) mod dawn_account;
pub(crate) mod json_account;
pub(crate) mod native_account;
pub(crate) mod progression;
pub(crate) mod sqlite_account;

/// Sunrise stores its database beneath the selected artifact directory.
pub(crate) fn investment_path(settings_path: &std::path::Path) -> std::path::PathBuf {
    settings_path
        .with_file_name("data")
        .join("investment.sqlite3")
}

/// The file name Dawn keeps its player state in.
pub(crate) const DAWN_DATABASE_NAME: &str = "player-state.db";

/// Dawn stores its player state beside the settings file rather than under a data directory.
pub(crate) fn dawn_path(settings_path: &std::path::Path) -> std::path::PathBuf {
    settings_path.with_file_name(DAWN_DATABASE_NAME)
}

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

/// Dawn stores its player state beside the settings file rather than under a data directory.
pub(crate) fn dawn_path(settings_path: &std::path::Path) -> std::path::PathBuf {
    settings_path.with_file_name("player-state.db")
}

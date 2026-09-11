//! Persistence adapters for storage-neutral account state.

pub(crate) mod json_fields;

pub(crate) mod json_account;
pub(crate) mod sqlite_account;

/// Sunrise stores its database beneath the selected artifact directory.
pub(crate) fn investment_path(settings_path: &std::path::Path) -> std::path::PathBuf {
    settings_path
        .with_file_name("data")
        .join("investment.sqlite3")
}

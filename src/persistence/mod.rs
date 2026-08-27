//! Persistence adapters for storage-neutral account state.

pub(crate) mod json_account;
#[cfg(feature = "sqlite-account")]
pub(crate) mod sqlite_account;

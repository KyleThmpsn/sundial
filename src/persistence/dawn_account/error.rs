//! Failures a Dawn player-state read can report.

use std::fmt;

/// A layout Sundial recognizes but will not treat as the pinned Dawn contract.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DawnAccountIncompatibility {
    /// `PRAGMA user_version` names a schema this build does not implement.
    SchemaVersion { found: i64 },
    /// The metadata table is not the exact three rows Dawn reads back.
    Metadata { detail: String },
    /// The allocator table is not the exact two rows Dawn reads back.
    Allocators { detail: String },
    /// A durable row breaks a limit Dawn enforces when it loads the database.
    Row { detail: String },
}

impl fmt::Display for DawnAccountIncompatibility {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SchemaVersion { found } => write!(
                formatter,
                "player-state.db reports schema {found} and Sundial supports schema {}",
                super::contract::SCHEMA_VERSION
            ),
            Self::Metadata { detail } => {
                write!(formatter, "player-state.db metadata is unusable: {detail}")
            }
            Self::Allocators { detail } => {
                write!(
                    formatter,
                    "player-state.db allocators are unusable: {detail}"
                )
            }
            Self::Row { detail } => {
                write!(
                    formatter,
                    "player-state.db holds data Dawn rejects: {detail}"
                )
            }
        }
    }
}

#[derive(Debug)]
pub(crate) enum DawnAccountError {
    Sqlite(rusqlite::Error),
    Account(sundial_account::AccountError),
    /// Dawn advanced the account while the editor held it, so the save would overwrite its work.
    Conflict {
        expected: i64,
        found: i64,
    },
    /// The verified copy taken before a write could not be produced.
    Backup(String),
    /// The edited account holds something Dawn's format cannot represent.
    Unwritable(String),
}

impl fmt::Display for DawnAccountError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(error) => write!(formatter, "{error}"),
            Self::Account(error) => write!(formatter, "{error}"),
            Self::Conflict { expected, found } => write!(
                formatter,
                "Dawn changed the account while it was open. Sundial loaded revision {expected} and found {found}. Reload before saving."
            ),
            Self::Backup(detail) => write!(
                formatter,
                "Sundial could not take a verified backup before writing player-state.db: {detail}"
            ),
            Self::Unwritable(detail) => write!(formatter, "{detail}"),
        }
    }
}

impl From<rusqlite::Error> for DawnAccountError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

impl From<sundial_account::AccountError> for DawnAccountError {
    fn from(error: sundial_account::AccountError) -> Self {
        Self::Account(error)
    }
}

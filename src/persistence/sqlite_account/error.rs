use std::{error::Error, fmt, io};

use sundial_account::AccountError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SqliteAccountIncompatibility {
    Schema { found: i64, supported: i64 },
    AccountFormat { found: i64, supported: i64 },
    SettingsPayload { found: u32, supported: u32 },
}

#[derive(Debug)]
pub(crate) enum SqliteAccountError {
    FileSystem(io::Error),
    Sqlite {
        operation: &'static str,
        source: rusqlite::Error,
    },
    InvalidSchema(String),
    InvalidData {
        location: String,
        message: String,
    },
    Domain(AccountError),
    EntityIdentityExhausted,
    SourceChanged,
    Backup(String),
}

impl SqliteAccountError {
    pub(super) fn sqlite(operation: &'static str, source: rusqlite::Error) -> Self {
        Self::Sqlite { operation, source }
    }

    pub(super) fn invalid_data(location: impl Into<String>, message: impl Into<String>) -> Self {
        Self::InvalidData {
            location: location.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for SqliteAccountIncompatibility {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Schema { found, supported } => write!(
                formatter,
                "SQLite schema {found} is newer than supported schema {supported}"
            ),
            Self::AccountFormat { found, supported } => write!(
                formatter,
                "SQLite account format {found} is newer than supported format {supported}"
            ),
            Self::SettingsPayload { found, supported } => write!(
                formatter,
                "SQLite settings payload {found} is newer than supported payload {supported}"
            ),
        }
    }
}

impl fmt::Display for SqliteAccountError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FileSystem(error) => write!(formatter, "could not inspect SQLite state: {error}"),
            Self::Sqlite { operation, source } => {
                write!(
                    formatter,
                    "could not {operation} SQLite account state: {source}"
                )
            }
            Self::InvalidSchema(message) => {
                write!(formatter, "invalid PR-88 SQLite schema: {message}")
            }
            Self::InvalidData { location, message } => {
                write!(
                    formatter,
                    "invalid SQLite account data at {location}: {message}"
                )
            }
            Self::Domain(error) => error.fmt(formatter),
            Self::EntityIdentityExhausted => {
                formatter.write_str("no in-session SQLite account entity IDs remain")
            }
            Self::SourceChanged => formatter.write_str(
                "state.sqlite3 changed outside Sundial after it was loaded; reload before saving",
            ),
            Self::Backup(message) => formatter.write_str(message),
        }
    }
}

impl Error for SqliteAccountError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::FileSystem(error) => Some(error),
            Self::Sqlite { source, .. } => Some(source),
            Self::Domain(error) => Some(error),
            Self::InvalidSchema(_)
            | Self::InvalidData { .. }
            | Self::EntityIdentityExhausted
            | Self::SourceChanged
            | Self::Backup(_) => None,
        }
    }
}

impl From<AccountError> for SqliteAccountError {
    fn from(error: AccountError) -> Self {
        Self::Domain(error)
    }
}

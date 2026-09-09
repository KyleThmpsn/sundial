//! Errors produced while translating Project Sunrise JSON account data.

use std::{error::Error, fmt};

use sundial_account::AccountError;

#[derive(Debug)]
pub(crate) enum JsonAccountError {
    Format { path: String, message: String },
    MissingCharacterInventory { path: String },
    Domain(AccountError),
    EntityIdentityExhausted,
}

impl JsonAccountError {
    pub(crate) fn format(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Format {
            path: path.into(),
            message: message.into(),
        }
    }

    pub(crate) fn missing_character_inventory(path: impl Into<String>) -> Self {
        Self::MissingCharacterInventory { path: path.into() }
    }

    pub(crate) const fn is_missing_character_inventory(&self) -> bool {
        matches!(self, Self::MissingCharacterInventory { .. })
    }

    pub(crate) fn path(&self) -> Option<&str> {
        match self {
            Self::Format { path, .. } | Self::MissingCharacterInventory { path } => Some(path),
            Self::Domain(_) | Self::EntityIdentityExhausted => None,
        }
    }

    pub(crate) fn detail(&self) -> String {
        match self {
            Self::Format { message, .. } => message.clone(),
            Self::MissingCharacterInventory { .. } => "character inventory is missing".into(),
            Self::Domain(error) => error.to_string(),
            Self::EntityIdentityExhausted => "no in-session account entity IDs remain".into(),
        }
    }

    pub(crate) const fn domain_error(&self) -> Option<&AccountError> {
        match self {
            Self::Domain(error) => Some(error),
            Self::Format { .. }
            | Self::MissingCharacterInventory { .. }
            | Self::EntityIdentityExhausted => None,
        }
    }
}

impl fmt::Display for JsonAccountError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Format { path, message } if path.is_empty() => formatter.write_str(message),
            Self::Format { path, message } => write!(formatter, "{path}: {message}"),
            Self::MissingCharacterInventory { path } => {
                write!(formatter, "{path}: character inventory is missing")
            }
            Self::Domain(error) => error.fmt(formatter),
            Self::EntityIdentityExhausted => {
                formatter.write_str("no in-session account entity IDs remain")
            }
        }
    }
}

impl Error for JsonAccountError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Domain(error) => Some(error),
            Self::Format { .. }
            | Self::MissingCharacterInventory { .. }
            | Self::EntityIdentityExhausted => None,
        }
    }
}

impl From<AccountError> for JsonAccountError {
    fn from(error: AccountError) -> Self {
        Self::Domain(error)
    }
}

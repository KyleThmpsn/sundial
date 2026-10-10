//! Recipe validation, serialization and file-operation errors.

use crate::AuthoringError;
use std::{fmt, io, path::PathBuf};

#[derive(Debug)]
pub enum RecipeError {
    Validation(String),
    Authoring(AuthoringError),
    Json(serde_json::Error),
    Io {
        operation: &'static str,
        path: PathBuf,
        source: io::Error,
    },
}

impl fmt::Display for RecipeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Validation(message) => formatter.write_str(message),
            Self::Authoring(error) => error.fmt(formatter),
            Self::Json(error) => write!(formatter, "Invalid weapon recipe JSON: {error}"),
            Self::Io {
                operation,
                path,
                source,
            } => write!(
                formatter,
                "Could not {operation} {}: {source}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for RecipeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Authoring(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::Io { source, .. } => Some(source),
            Self::Validation(_) => None,
        }
    }
}

impl From<AuthoringError> for RecipeError {
    fn from(value: AuthoringError) -> Self {
        Self::Authoring(value)
    }
}

impl From<serde_json::Error> for RecipeError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

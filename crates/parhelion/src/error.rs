use std::{fmt, io, path::PathBuf};

pub type AuthoringResult<T> = Result<T, AuthoringError>;

#[derive(Debug)]
pub enum AuthoringError {
    Context {
        context: String,
        source: Box<AuthoringError>,
    },
    Io {
        operation: &'static str,
        path: PathBuf,
        source: io::Error,
    },
    InvalidInput(String),
    InvalidPackage(String),
    Validation(String),
}

impl AuthoringError {
    pub(crate) fn context(self, context: impl Into<String>) -> Self {
        Self::Context {
            context: context.into(),
            source: Box::new(self),
        }
    }

    pub(crate) fn io(operation: &'static str, path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            operation,
            path: path.into(),
            source,
        }
    }
}

impl fmt::Display for AuthoringError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Context { context, source } => write!(formatter, "{context}: {source}"),
            Self::Io {
                operation,
                path,
                source,
            } => write!(
                formatter,
                "Could not {operation} {}: {source}",
                path.display()
            ),
            Self::InvalidInput(message) => formatter.write_str(message),
            Self::InvalidPackage(message) => formatter.write_str(message),
            Self::Validation(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for AuthoringError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Context { source, .. } => Some(source.as_ref()),
            Self::Io { source, .. } => Some(source),
            Self::InvalidInput(_) | Self::InvalidPackage(_) | Self::Validation(_) => None,
        }
    }
}

pub(crate) fn invalid(message: impl Into<String>) -> AuthoringError {
    AuthoringError::InvalidPackage(message.into())
}

pub(crate) fn input(message: impl Into<String>) -> AuthoringError {
    AuthoringError::InvalidInput(message.into())
}

pub(crate) fn validation(message: impl Into<String>) -> AuthoringError {
    AuthoringError::Validation(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_names_the_weapon_and_preserves_the_source_error() {
        let error = AuthoringError::Validation("socket column is incompatible".to_owned())
            .context("Weapon \"Test Weapon\" (parhelion.test-weapon)");

        assert_eq!(
            error.to_string(),
            "Weapon \"Test Weapon\" (parhelion.test-weapon): socket column is incompatible"
        );
        assert_eq!(
            std::error::Error::source(&error).map(ToString::to_string),
            Some("socket column is incompatible".to_owned())
        );
    }
}

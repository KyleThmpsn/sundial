use std::{fmt, io, path::PathBuf};

pub type AuthoringResult<T> = Result<T, AuthoringError>;

#[derive(Debug)]
pub enum AuthoringError {
    Context {
        context: String,
        source: Box<AuthoringError>,
    },
    /// Context naming the recipe the error arose in, so the app can point at that recipe.
    Recipe {
        namespace: String,
        context: String,
        source: Box<AuthoringError>,
    },
    /// The errors of several recipes that each failed on their own in one pass, so a build that
    /// leaves failing recipes out can leave them all out at once.
    Recipes(Vec<AuthoringError>),
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

    pub(crate) fn in_recipe(
        self,
        namespace: impl Into<String>,
        context: impl Into<String>,
    ) -> Self {
        Self::Recipe {
            namespace: namespace.into(),
            context: context.into(),
            source: Box::new(self),
        }
    }

    /// The errors of several recipes from one pass, or the one error when there is only one.
    pub(crate) fn several(mut errors: Vec<AuthoringError>) -> Self {
        if errors.len() == 1 {
            errors.remove(0)
        } else {
            Self::Recipes(errors)
        }
    }

    /// The namespace of the recipe this error arose in, when one recipe caused it.
    pub(crate) fn recipe_namespace(&self) -> Option<&str> {
        match self {
            Self::Recipe { namespace, .. } => Some(namespace),
            Self::Context { source, .. } => source.recipe_namespace(),
            Self::Recipes(_)
            | Self::Io { .. }
            | Self::InvalidInput(_)
            | Self::InvalidPackage(_)
            | Self::Validation(_) => None,
        }
    }

    /// Each recipe this error names, with the full message of the part that recipe caused. An
    /// error no recipe caused names none.
    pub(crate) fn recipe_failures(&self) -> Vec<(String, String)> {
        match self {
            Self::Recipe { namespace, .. } => vec![(namespace.clone(), format!("{self:#}"))],
            Self::Recipes(errors) => errors.iter().flat_map(Self::recipe_failures).collect(),
            Self::Context { context, source } => {
                let inner = source.recipe_failures();
                if inner.len() > 1 {
                    inner
                        .into_iter()
                        .map(|(namespace, message)| (namespace, format!("{context}\n{message}")))
                        .collect()
                } else {
                    inner
                        .into_iter()
                        .map(|(namespace, _)| (namespace, format!("{self:#}")))
                        .collect()
                }
            }
            Self::Io { .. }
            | Self::InvalidInput(_)
            | Self::InvalidPackage(_)
            | Self::Validation(_) => Vec::new(),
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
            Self::Context { context, source }
            | Self::Recipe {
                context, source, ..
            } if formatter.alternate() => {
                write!(formatter, "{context}\n{source:#}")
            }
            Self::Context { context, source }
            | Self::Recipe {
                context, source, ..
            } => {
                write!(formatter, "{context}: {source}")
            }
            Self::Recipes(errors) => {
                for (index, error) in errors.iter().enumerate() {
                    if index > 0 {
                        formatter.write_str(if formatter.alternate() { "\n\n" } else { "\n" })?;
                    }
                    if formatter.alternate() {
                        write!(formatter, "{error:#}")?;
                    } else {
                        write!(formatter, "{error}")?;
                    }
                }
                Ok(())
            }
            Self::Io {
                operation,
                path,
                source,
            } => write!(
                formatter,
                "Could not {operation} {}: {source}",
                path.display()
            ),
            Self::InvalidInput(message)
            | Self::InvalidPackage(message)
            | Self::Validation(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for AuthoringError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Context { source, .. } | Self::Recipe { source, .. } => Some(source.as_ref()),
            Self::Io { source, .. } => Some(source),
            Self::Recipes(errors) => errors
                .first()
                .map(|error| error as &(dyn std::error::Error + 'static)),
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

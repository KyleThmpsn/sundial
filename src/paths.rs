use std::{
    fs, io,
    path::{Component, Path, PathBuf},
};

#[cfg(windows)]
fn windows_data_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .map(|path| path.join("Sundial"))
}

pub(crate) fn config_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        windows_data_dir()
    }
    #[cfg(not(windows))]
    {
        directories::ProjectDirs::from("", "", "Sundial")
            .map(|directories| directories.config_dir().to_path_buf())
    }
}

pub(crate) fn data_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        windows_data_dir()
    }
    #[cfg(not(windows))]
    {
        directories::ProjectDirs::from("", "", "Sundial")
            .map(|directories| directories.data_local_dir().to_path_buf())
    }
}

pub(crate) fn cache_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        windows_data_dir()
    }
    #[cfg(not(windows))]
    {
        directories::ProjectDirs::from("", "", "Sundial")
            .map(|directories| directories.cache_dir().to_path_buf())
    }
}

/// Shared installed-investment catalog cache used by Sundial and companion utilities.
pub(crate) fn shadowkeep_catalog_path() -> Option<PathBuf> {
    cache_dir().map(|path| path.join("catalog").join("d2sk-86657.json"))
}

/// Resolves an existing path, or its closest existing ancestor, for security-sensitive
/// comparisons that may involve an output path which has not been created yet.
pub fn resolve_path_for_comparison(path: &Path) -> io::Result<PathBuf> {
    let absolute = normalize_absolute(path)?;
    if absolute.exists() {
        return fs::canonicalize(absolute);
    }

    let mut missing = Vec::new();
    let mut ancestor = absolute.as_path();
    while !ancestor.exists() {
        let name = ancestor.file_name().ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "path has no existing ancestor")
        })?;
        missing.push(name.to_owned());
        ancestor = ancestor.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "path has no existing ancestor")
        })?;
    }
    let mut resolved = fs::canonicalize(ancestor)?;
    for component in missing.into_iter().rev() {
        resolved.push(component);
    }
    Ok(resolved)
}

fn normalize_absolute(path: &Path) -> io::Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    Ok(normalized)
}

#[cfg(windows)]
fn folded_components(path: &Path) -> Vec<String> {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy().to_lowercase())
        .collect()
}

/// Compares already-resolved paths using the host platform's path semantics.
#[cfg(windows)]
pub fn path_is_within(candidate: &Path, parent: &Path) -> bool {
    folded_components(candidate).starts_with(&folded_components(parent))
}

/// Compares already-resolved paths using the host platform's path semantics.
#[cfg(not(windows))]
pub fn path_is_within(candidate: &Path, parent: &Path) -> bool {
    candidate.starts_with(parent)
}

/// Tests path equality using the host platform's case rules.
#[cfg(windows)]
pub fn paths_equal(left: &Path, right: &Path) -> bool {
    folded_components(left) == folded_components(right)
}

/// Tests path equality using the host platform's case rules.
#[cfg(not(windows))]
pub fn paths_equal(left: &Path, right: &Path) -> bool {
    left == right
}

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

#[cfg(windows)]
fn windows_cache_dir() -> Option<PathBuf> {
    windows_data_dir().map(|path| path.join("cache"))
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
        windows_cache_dir()
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
    use std::path::Prefix;

    path.components()
        .map(|component| match component {
            Component::Prefix(prefix) => match prefix.kind() {
                Prefix::Disk(disk) | Prefix::VerbatimDisk(disk) => {
                    format!("disk:{}", char::from(disk).to_ascii_lowercase())
                }
                Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => format!(
                    "unc:{}:{}",
                    server.to_string_lossy().to_lowercase(),
                    share.to_string_lossy().to_lowercase()
                ),
                Prefix::DeviceNS(device) => {
                    format!("device:{}", device.to_string_lossy().to_lowercase())
                }
                Prefix::Verbatim(value) => {
                    format!("verbatim:{}", value.to_string_lossy().to_lowercase())
                }
            },
            Component::RootDir => "root".to_owned(),
            Component::CurDir => "current".to_owned(),
            Component::ParentDir => "parent".to_owned(),
            Component::Normal(value) => value.to_string_lossy().to_lowercase(),
        })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolved_and_canonical_windows_paths_compare_equally() {
        let directory = tempfile::tempdir().unwrap();
        let canonical = fs::canonicalize(directory.path()).unwrap();
        // The temp directory can sit behind an 8.3 short name, which canonicalize expands and
        // a lexical fold cannot, so the plain form is the canonical path minus its prefix.
        let plain =
            std::path::PathBuf::from(canonical.to_string_lossy().trim_start_matches(r"\\?\"));
        assert!(
            cfg!(not(windows)) || plain != canonical,
            "the prefix was not stripped"
        );
        assert!(paths_equal(&plain, &canonical));
        assert!(path_is_within(&canonical.join("child"), &plain));
    }
}

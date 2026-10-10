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
    // Preserve the platform's traversal rules. On Unix, a link followed by `..`
    // must be resolved by the filesystem before any missing suffix is normalized.
    let mut ancestor = std::path::absolute(path)?;
    let mut missing = Vec::new();
    let mut resolved = loop {
        if let Some(resolved) = canonical_existing_path(&ancestor)? {
            break resolved;
        }
        let component = ancestor.components().next_back().ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "path has no existing ancestor")
        })?;
        match component {
            Component::Normal(_) | Component::ParentDir | Component::CurDir => {
                missing.push(component.as_os_str().to_owned());
            }
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "path has no existing ancestor",
                ));
            }
        }
        ancestor.pop();
    };
    if !missing.is_empty() && !fs::metadata(&resolved)?.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotADirectory,
            "output ancestor is not a directory",
        ));
    }
    let mut traversed_parent = false;
    for component in missing.into_iter().rev() {
        if component == ".." {
            resolved.pop();
            traversed_parent = true;
        } else if component != "." {
            resolved.push(component);
        }
    }
    if traversed_parent {
        // Removing a missing directory can expose a different existing link,
        // as in `missing/../alias/output`. Resolve that ancestry as well. The
        // normalized suffix has no parent components, so this cannot repeat.
        resolve_path_for_comparison(&resolved)
    } else {
        Ok(resolved)
    }
}

fn canonical_existing_path(path: &Path) -> io::Result<Option<PathBuf>> {
    match fs::canonicalize(path) {
        Ok(resolved) => Ok(Some(resolved)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            // A dangling link exists but cannot be resolved. Do not treat it
            // as an ordinary output directory that can safely be created.
            match fs::symlink_metadata(path) {
                Ok(_) => Err(error),
                Err(metadata_error) if metadata_error.kind() == io::ErrorKind::NotFound => Ok(None),
                Err(metadata_error) => Err(metadata_error),
            }
        }
        Err(error) => Err(error),
    }
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
    fn new_outputs_resolve_but_files_cannot_be_used_as_directories() {
        let directory = tempfile::tempdir().unwrap();
        let expected = fs::canonicalize(directory.path())
            .unwrap()
            .join("new/output.json");
        assert_eq!(
            resolve_path_for_comparison(&directory.path().join("new/output.json")).unwrap(),
            expected
        );
        let file = directory.path().join("file");
        fs::write(&file, b"keep").unwrap();
        assert!(resolve_path_for_comparison(&file.join("output.json")).is_err());
        assert_eq!(fs::read(file).unwrap(), b"keep");
    }

    #[cfg(unix)]
    #[test]
    fn parent_traversal_resolves_links_before_missing_outputs_and_rejects_dangling_links() {
        let directory = tempfile::tempdir().unwrap();
        let protected = directory.path().join("protected");
        fs::create_dir_all(protected.join("child")).unwrap();
        let alias = directory.path().join("alias");
        std::os::unix::fs::symlink(protected.join("child"), &alias).unwrap();
        let target = alias.join("../output.json");
        let expected = fs::canonicalize(&protected).unwrap().join("output.json");
        assert_eq!(resolve_path_for_comparison(&target).unwrap(), expected);
        fs::write(&target, b"same destination").unwrap();
        assert_eq!(
            resolve_path_for_comparison(&target).unwrap(),
            fs::canonicalize(&target).unwrap()
        );
        let dangling = directory.path().join("dangling");
        std::os::unix::fs::symlink(directory.path().join("absent"), &dangling).unwrap();
        assert!(resolve_path_for_comparison(&dangling.join("output.json")).is_err());
    }

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

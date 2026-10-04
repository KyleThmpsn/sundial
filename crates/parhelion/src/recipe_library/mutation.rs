//! Checked metadata publication and recovery while the library write lease is held.
use super::{Path, PathBuf, fs};

pub(super) fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {}
        Ok(_) => {
            return Err(format!(
                "Expected a regular library file: {}",
                path.display()
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("Could not inspect {}: {error}", path.display())),
    }
    fs::read(path)
        .map(Some)
        .map_err(|error| format!("Could not read {}: {error}", path.display()))
}

pub(super) fn publish(path: &Path, contents: &[u8], original: Option<&[u8]>) -> Result<(), String> {
    let result = match original {
        Some(original) => sundial::package_authoring::replace_authoring_file_if_unchanged(
            path, contents, original,
        ),
        None => sundial::storage::create_file(path, contents),
    };
    result.map_err(|error| format!("Could not publish {}: {error}", path.display()))
}

pub(super) fn remove(path: &Path, expected: &[u8]) -> Result<(), String> {
    if read_optional(path)?.as_deref() != Some(expected) {
        return Err(format!("{} changed outside this operation", path.display()));
    }
    fs::remove_file(path).map_err(|error| format!("Could not remove {}: {error}", path.display()))
}

pub(super) struct Change {
    pub path: PathBuf,
    pub before: Option<Vec<u8>>,
    pub after: Vec<u8>,
}

impl Change {
    fn apply(&self) -> Result<(), String> {
        publish(&self.path, &self.after, self.before.as_deref())
    }

    fn rollback(&self) -> Result<(), String> {
        if let Some(original) = &self.before {
            return publish(&self.path, original, Some(&self.after));
        }
        remove(&self.path, &self.after)
    }
}

/// Metadata is committed first. A failed final file removal rolls it back while
/// protecting bytes subsequently written by someone else.
pub(super) fn commit(
    changes: &[Change],
    finish: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    let mut committed = 0;
    let result = (|| {
        for change in changes {
            change.apply()?;
            committed += 1;
        }
        finish()
    })();
    let Err(error) = result else { return Ok(()) };
    let recovery = changes[..committed]
        .iter()
        .rev()
        .filter_map(|change| change.rollback().err())
        .collect::<Vec<_>>();
    if recovery.is_empty() {
        Err(format!("{error}. Recovery restored the prior metadata"))
    } else {
        Err(format!("{error}. Recovery errors: {}", recovery.join(". ")))
    }
}

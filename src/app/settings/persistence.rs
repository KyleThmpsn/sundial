//! Verified settings saves, source-change checks, and recovery backups.
use super::{backups_path, prepare_settings};
use crate::{game_settings, persistence::json_account::ensure_schema_v8_preferences, storage};
use serde_json::Value;
use std::{
    fs, io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub(in crate::app) fn load_workspace_json(path: &Path) -> Result<Value, String> {
    let raw = fs::read_to_string(path).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            format!(
                "No Project Sunrise settings.json was found in the selected installation. Expected: {}. Choose the Destiny 2 Shadowkeep folder containing destiny2.exe and the bin folder, and confirm Project Sunrise is installed there",
                path.display()
            )
        } else {
            format!("Could not read {}: {error}", path.display())
        }
    })?;
    // Sunrise accepts a leading UTF-8 BOM because common Windows editors add one. Match the
    // loader before handing the document to serde_json, which otherwise treats it as a token.
    let raw = raw.strip_prefix('\u{feff}').unwrap_or(&raw);
    crate::strict_json::from_str(raw)
        .map_err(|e| format!("Invalid JSON in {}: {e}", path.display()))
}

#[cfg(test)]
pub(in crate::app) fn load_json(path: &Path) -> Result<Value, String> {
    let mut document = load_workspace_json(path)?;
    ensure_schema_v8_preferences(&mut document);
    Ok(document)
}

#[cfg(test)]
pub(in crate::app) fn verify_source_unchanged(path: &Path, expected: &Value) -> Result<(), String> {
    verify_workspace_source_unchanged(path, expected, true)
}

pub(in crate::app) fn verify_workspace_source_unchanged(
    path: &Path,
    expected: &Value,
    normalize_json_account: bool,
) -> Result<(), String> {
    let mut current = load_workspace_json(path)?;
    if normalize_json_account {
        ensure_schema_v8_preferences(&mut current);
    }
    if current == *expected {
        Ok(())
    } else {
        Err("settings.json changed outside Sundial after it was loaded. Reload before saving so newer data is not overwritten".into())
    }
}

pub(in crate::app) struct SaveJsonResult {
    pub(in crate::app) backup: PathBuf,
    pub(in crate::app) encoded_bytes: usize,
    pub(in crate::app) size_limit_bytes: usize,
    pub(in crate::app) compacted: bool,
    pub(in crate::app) durability_warning: Option<String>,
}

#[derive(Debug)]
pub(in crate::app) struct SaveJsonError {
    pub message: String,
    pub may_have_committed: bool,
}

impl From<String> for SaveJsonError {
    fn from(message: String) -> Self {
        Self {
            message,
            may_have_committed: false,
        }
    }
}

impl From<&str> for SaveJsonError {
    fn from(message: &str) -> Self {
        message.to_owned().into()
    }
}

impl From<SaveJsonError> for String {
    fn from(error: SaveJsonError) -> Self {
        error.message
    }
}

impl std::fmt::Display for SaveJsonError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.message.fmt(formatter)
    }
}

pub(in crate::app) fn save_json(
    path: &Path,
    document: &Value,
    expected: &Value,
    normalize_json_account: bool,
) -> Result<SaveJsonResult, SaveJsonError> {
    let backup_root = backups_path().ok_or("Could not locate the local backup folder")?;
    save_json_checked_with_backup_root(
        path,
        document,
        expected,
        normalize_json_account,
        &backup_root,
    )
}

#[cfg(test)]
pub(in crate::app) fn save_json_with_backup_root(
    path: &Path,
    document: &Value,
    backup_root: &Path,
) -> Result<SaveJsonResult, String> {
    let expected = load_workspace_json(path)?;
    save_test_json_checked(path, document, &expected, false, backup_root).map_err(Into::into)
}

// Disposable-file callers inject a closed game without altering production process checks.
#[cfg(test)]
pub(in crate::app) fn save_test_json_checked(
    path: &Path,
    document: &Value,
    expected: &Value,
    normalize_json_account: bool,
    backup_root: &Path,
) -> Result<SaveJsonResult, SaveJsonError> {
    save_json_with_writer(
        path,
        document,
        expected,
        normalize_json_account,
        backup_root,
        storage::replace_file_if_unchanged,
        || Ok(()),
    )
}

pub(in crate::app) fn save_json_checked_with_backup_root(
    path: &Path,
    document: &Value,
    expected: &Value,
    normalize_json_account: bool,
    backup_root: &Path,
) -> Result<SaveJsonResult, SaveJsonError> {
    save_json_with_writer(
        path,
        document,
        expected,
        normalize_json_account,
        backup_root,
        storage::replace_file_if_unchanged,
        || require_game_closed(super::super::platform::destiny_is_running()),
    )
}

fn save_json_with_writer(
    path: &Path,
    document: &Value,
    expected: &Value,
    normalize_json_account: bool,
    backup_root: &Path,
    replace: impl FnOnce(&Path, &[u8], &[u8]) -> io::Result<()>,
    check_game_closed: impl Fn() -> Result<(), String>,
) -> Result<SaveJsonResult, SaveJsonError> {
    // Canonicalization makes symlink and relative-path aliases share a lock.
    let path = fs::canonicalize(path)
        .map_err(|error| format!("Could not resolve settings.json: {error}"))?;
    let path = path.as_path();
    let _lock = lock_settings(path)?;
    check_game_closed()?;
    let original =
        fs::read(path).map_err(|error| format!("Could not read settings.json: {error}"))?;
    // Compare the captured bytes, not a separate read that could go stale.
    let raw = std::str::from_utf8(&original).map_err(|error| error.to_string())?;
    let mut original_document: Value =
        crate::strict_json::from_str(raw.strip_prefix('\u{feff}').unwrap_or(raw))
            .map_err(|error| error.to_string())?;
    if normalize_json_account {
        ensure_schema_v8_preferences(&mut original_document);
    }
    if original_document != *expected {
        return Err("settings.json changed outside Sundial; reload before saving".into());
    }
    let prepared = prepare_settings(document)?;

    let backup_root = crate::backups::create_source_directory(backup_root, path)?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| format!("Could not create backup timestamp: {e}"))?
        .as_nanos();
    let schema = backup_schema_label(path);
    let backup = backup_root.join(format!("settings-{schema}-{timestamp}.json"));
    create_backup(path, &backup)?;
    if fs::read(&backup).map_err(|error| error.to_string())? != original {
        return Err(format!(
            "settings.json changed while it was being backed up; no replacement was attempted. Backup: {}",
            backup.display()
        ).into());
    }
    check_game_closed()?;
    let write_result = replace(path, prepared.encoded.as_bytes(), &original);
    // Rename can succeed before directory syncing reports an error. Reconcile
    // actual bytes so the coordinator does not roll back SQLite after a JSON commit.
    // Never restore blindly: unexpected bytes may belong to another writer.
    let saved = fs::read(path).map_err(|error| SaveJsonError {
        message: format!("Could not verify settings.json ({error}); its state is uncertain and was left untouched. Reload before continuing. Backup: {}", backup.display()),
        may_have_committed: true,
    })?;
    if saved != prepared.encoded.as_bytes() {
        return Err(SaveJsonError {
            message: format!(
                "Settings save did not verify ({}); current contents were preserved. Reload before saving. Backup: {}",
                write_result.err().map_or_else(
                    || "the file changed after replacement".to_owned(),
                    |error| error.to_string()
                ),
                backup.display()
            ),
            may_have_committed: saved != original,
        });
    }
    let durability_warning = write_result.err().map(|error| format!(
        "Settings were written and verified, but disk durability could not be confirmed: {error}. Keep the backup at {}", backup.display()
    ));
    Ok(SaveJsonResult {
        backup,
        encoded_bytes: prepared.encoded_bytes,
        size_limit_bytes: prepared.size_limit_bytes,
        compacted: prepared.compacted,
        durability_warning,
    })
}

pub(in crate::app) fn require_game_closed(running: Result<bool, String>) -> Result<(), String> {
    if running? {
        return Err(
            "Close Destiny 2 before saving settings or account data, then try again".into(),
        );
    }
    Ok(())
}

fn lock_settings(path: &Path) -> Result<fs::File, String> {
    let name = path
        .file_name()
        .ok_or("Settings path has no file name")?
        .to_string_lossy();
    let lock_path = path.with_file_name(format!(".{name}.sundial.lock"));
    if fs::symlink_metadata(&lock_path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err("The settings write lock must not be a symbolic link".into());
    }
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|error| format!("Could not open settings write lock: {error}"))?;
    fs2::FileExt::try_lock_exclusive(&file).map_err(|error| format!(
        "Another Sundial operation may be saving this account; retry after it finishes ({error})"
    ))?;
    // Keep the lock file: unlinking it would let another writer lock a new inode.
    Ok(file)
}

#[cfg(test)]
mod tests;

fn backup_schema_label(source: &Path) -> String {
    fs::read(source)
        .ok()
        .and_then(|contents| {
            let contents = contents
                .strip_prefix(&[0xEF, 0xBB, 0xBF])
                .unwrap_or(&contents);
            serde_json::from_slice::<Value>(contents).ok()
        })
        .and_then(|document| game_settings::schema_version(&document))
        .map_or_else(|| "v0".to_owned(), |schema| format!("v{schema}"))
}

fn create_backup(source: &Path, destination: &Path) -> Result<(), String> {
    let mut source_file = fs::File::open(source)
        .map_err(|e| format!("Could not open {} for backup: {e}", source.display()))?;
    let mut backup_file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|e| format!("Could not create {}: {e}", destination.display()))?;
    if let Err(error) =
        io::copy(&mut source_file, &mut backup_file).and_then(|_| backup_file.sync_all())
    {
        drop(backup_file);
        let failure = format!("Could not create {}: {error}", destination.display());
        return match storage::remove_file_if_present(destination) {
            Ok(()) => Err(failure),
            Err(cleanup_error) => Err(format!(
                "{failure}; the incomplete backup could not be removed: {cleanup_error}"
            )),
        };
    }
    Ok(())
}

pub(in crate::app) fn create_adjacent_backup(source: &Path) -> Result<PathBuf, String> {
    let file_name = source
        .file_name()
        .ok_or_else(|| format!("{} has no file name", source.display()))?
        .to_string_lossy();
    let destination = source.with_file_name(format!("{file_name}.bak"));
    let source_contents = fs::read(source)
        .map_err(|e| format!("Could not read {} for backup: {e}", source.display()))?;

    if destination.exists() {
        let existing = fs::read(&destination)
            .map_err(|e| format!("Could not read {}: {e}", destination.display()))?;
        if existing == source_contents {
            return Ok(destination);
        }

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| format!("Could not create backup timestamp: {e}"))?
            .as_nanos();
        let archived = source.with_file_name(format!("{file_name}.bak.previous-{timestamp}"));
        create_backup(&destination, &archived)?;
        storage::replace_file(&destination, &source_contents).map_err(|e| {
            format!(
                "Could not update {} after preserving its previous contents at {}: {e}",
                destination.display(),
                archived.display()
            )
        })?;
    } else {
        create_backup(source, &destination)?;
    }

    let copied = fs::read(&destination)
        .map_err(|e| format!("Could not verify {}: {e}", destination.display()))?;
    if copied != source_contents {
        return Err(format!(
            "The safety copy at {} did not match the source",
            destination.display()
        ));
    }
    Ok(destination)
}

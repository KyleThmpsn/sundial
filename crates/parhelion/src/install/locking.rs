use super::*;

/// An exclusive lock on a file in a directory, removed again when the lock is released.
pub(super) struct LockFile {
    _file: File,
    path: PathBuf,
}

impl Drop for LockFile {
    fn drop(&mut self) {
        // Removed while still held, so a process that opened the file meanwhile locks it only
        // afterwards, finds its name gone or naming another file, and starts again.
        let _ = fs::remove_file(&self.path);
    }
}

/// Locks `name` in `directory`, creating it when absent. Closing the handle releases ownership,
/// including after a crash, and a crash leaves an empty file the next lock reuses.
pub(super) fn lock_directory(directory: &Path, name: &str) -> Result<LockFile, InstallError> {
    let directory = canonical_directory(directory, "transaction directory")?;
    let path = directory.join(name);
    // Another release can remove the file between opening and locking it, and Windows refuses
    // to inspect or open a file whose removal is still pending. Both pass once that release
    // finishes.
    let mut denied = None;
    for _ in 0..20 {
        let opened = match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                return Err(InstallError::validation(format!(
                    "The transaction lock must be a regular file: {}",
                    path.display()
                )));
            }
            Ok(_) => Ok(()),
        }
        .and_then(|()| {
            OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(&path)
        });
        let file = match opened {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
                denied = Some(error);
                std::thread::sleep(std::time::Duration::from_millis(10));
                continue;
            }
            Err(error) => {
                return Err(InstallError::validation(format!(
                    "Could not open transaction lock {}: {error}",
                    path.display()
                )));
            }
        };
        denied = None;
        fs2::FileExt::try_lock_exclusive(&file).map_err(|error| {
            InstallError::validation(format!(
                "Another package operation may be using {}. Could not acquire its transaction lock: {error}",
                directory.display()
            ))
        })?;
        if still_named(&file, &path) {
            return Ok(LockFile { _file: file, path });
        }
    }
    Err(InstallError::validation(match denied {
        Some(error) => format!(
            "Could not open transaction lock {}: {error}",
            path.display()
        ),
        None => format!(
            "Another package operation kept replacing the transaction lock in {}",
            directory.display()
        ),
    }))
}

/// Whether `path` still names the locked `file`, so the lock covers what other processes open.
fn still_named(file: &File, path: &Path) -> bool {
    let Ok(clone) = file.try_clone() else {
        return false;
    };
    match (
        same_file::Handle::from_file(clone),
        same_file::Handle::from_path(path),
    ) {
        (Ok(locked), Ok(named)) => locked == named,
        _ => false,
    }
}

pub(super) struct InstallationLock {
    _current: LockFile,
    legacy: Option<File>,
    target: PathBuf,
}

impl InstallationLock {
    /// Retire the old empty coordination file only after a successful loader operation.
    pub(super) fn finish(
        &self,
        check: GameRunningCheck,
        runtime: RuntimeSnapshotCheck,
    ) -> Result<(), InstallError> {
        if self.legacy.is_none()
            || !runtime(self.target.parent().unwrap())
                .map_err(InstallError::validation)?
                .uses_mod_loader()
                .map_err(InstallError::validation)?
        {
            return Ok(());
        }
        check_game_before_recovery(check)?;
        let path = self.target.join(".parhelion-transaction.lock");
        validate_legacy_lock(&path)?;
        fs::remove_file(path).map_err(|e| InstallError::validation(e.to_string()))
    }
}

fn validate_legacy_lock(path: &Path) -> Result<(), InstallError> {
    reject_symlink(path, "legacy transaction lock")?;
    let metadata = fs::metadata(path).map_err(|e| InstallError::validation(e.to_string()))?;
    if !metadata.is_file() || metadata.len() != 0 {
        return Err(InstallError::validation(
            "The legacy transaction lock contains unexpected content and was preserved",
        ));
    }
    Ok(())
}

pub(super) fn lock_installation(target: &Path) -> Result<InstallationLock, InstallError> {
    let target = canonical_packages_directory(target)?;
    // Both runtime routes use this stable installation-wide lock outside stock packages.
    let root = target
        .parent()
        .ok_or_else(|| InstallError::validation("Missing game root"))?;
    let current = lock_directory(root, ".parhelion-transaction.lock")?;
    let path = target.join(".parhelion-transaction.lock");
    let legacy = match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(InstallError::validation(error.to_string())),
        Ok(_) => {
            validate_legacy_lock(&path)?;
            // Never create or write the old file. Respect an already active older editor.
            let file = File::open(&path).map_err(|e| InstallError::validation(e.to_string()))?;
            fs2::FileExt::try_lock_exclusive(&file).map_err(|e| {
                InstallError::validation(format!(
                    "An older package operation holds the legacy transaction lock: {e}"
                ))
            })?;
            Some(file)
        }
    };
    Ok(InstallationLock {
        _current: current,
        legacy,
        target,
    })
}

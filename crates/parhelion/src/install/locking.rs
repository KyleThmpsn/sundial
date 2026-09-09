use super::*;

/// The file stays in place: unlinking a lock file would let another process lock a
/// different inode. Closing the handle releases ownership, including after a crash.
pub(super) fn lock_directory(directory: &Path, name: &str) -> Result<File, InstallError> {
    let directory = canonical_directory(directory, "transaction directory")?;
    let path = directory.join(name);
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        _ => reject_symlink(&path, "transaction lock")?,
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|error| {
            InstallError::validation(format!("Could not open transaction lock: {error}"))
        })?;
    fs2::FileExt::try_lock_exclusive(&file).map_err(|error| {
        InstallError::validation(format!(
            "Another package operation may be using {}; could not acquire its transaction lock: {error}",
            directory.display()
        ))
    })?;
    Ok(file)
}

pub(super) fn lock_installation(target: &Path) -> Result<File, InstallError> {
    lock_directory(target, ".parhelion-transaction.lock")
}

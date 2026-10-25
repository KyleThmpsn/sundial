//! Durable file publication. Callers own document formats, locking, and transaction policy.
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

/// Hashes file contents with fixed memory. Callers retain ownership and path checks.
pub(crate) fn file_sha256(path: &Path) -> io::Result<String> {
    use sha2::{Digest, Sha256};
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = match file.read(&mut buffer) {
            Ok(count) => count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(hex::encode(hash.finalize()))
}

/// An advisory write lease for cooperating processes. Keep the lock file in place,
/// since unlinking it would let another writer acquire a different file.
pub fn try_lock_file(path: &Path) -> io::Result<File> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_file() || metadata.file_type().is_symlink() => {
            return Err(io::Error::other("The write lock must be a regular file"));
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::other("The write lock must be a regular file"));
    }
    fs2::FileExt::try_lock_exclusive(&file)?;
    Ok(file)
}

/// Publishes a complete new file without replacing an existing destination.
/// The temporary file lives on the destination filesystem and is flushed before publication.
pub fn create_file(path: &Path, contents: &[u8]) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "destination has no parent folder",
        )
    })?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(contents)?;
    publish_new(temporary, path)
}

/// Publishes an already prepared same-filesystem temporary, including SQLite-native backups.
/// On Unix the folder's entry is flushed too, so a new backup is still reachable after a power
/// loss once a caller goes on to replace what it protects.
pub(crate) fn publish_new(temporary: tempfile::NamedTempFile, path: &Path) -> io::Result<()> {
    temporary.as_file().sync_all()?;
    temporary
        .persist_noclobber(path)
        .map(|_| ())
        .map_err(|error| error.error)?;
    if cfg!(not(windows))
        && let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
    {
        File::open(parent)?.sync_all()?;
    }
    Ok(())
}

/// Writes a complete file beside its destination, flushes it to disk, and then
/// replaces the destination in one filesystem operation.
pub(super) fn replace_file(path: &Path, contents: &[u8]) -> io::Result<()> {
    let temporary = replacement_temporary_path(path)?;
    write_replacement(path, &temporary, |file| file.write_all(contents))
}

/// Optimistic guard for external writers. Callers must also serialize cooperating
/// writers: a process ignoring the lock can still race the final read/rename.
pub(crate) fn replace_file_if_unchanged(
    path: &Path,
    contents: &[u8],
    expected: &[u8],
) -> io::Result<()> {
    let temporary = replacement_temporary_path(path)?;
    write_replacement_guarded(
        path,
        &temporary,
        |file| file.write_all(contents),
        || {
            if fs::read(path)? != expected {
                return Err(io::Error::other(
                    "The file changed outside Sundial before replacement. Reload before saving",
                ));
            }
            Ok(())
        },
    )
}

/// Copies a complete source file beside its destination, flushes it, and atomically replaces the
/// destination. The temporary file always lives on the destination filesystem.
pub(crate) fn replace_file_from_path(source: &Path, destination: &Path) -> io::Result<()> {
    let temporary = replacement_temporary_path(destination)?;
    write_replacement(destination, &temporary, |file| {
        let mut source = File::open(source)?;
        io::copy(&mut source, file)?;
        Ok(())
    })
}

/// Like [`replace_file_from_path`], but replaces the destination only while `before_replace`
/// accepts it. The check runs after the copy is flushed, immediately before the rename, so an
/// outside writer's window is the rename alone rather than the whole copy.
pub(crate) fn replace_file_from_path_if(
    source: &Path,
    destination: &Path,
    before_replace: impl FnOnce() -> io::Result<()>,
) -> io::Result<()> {
    let temporary = replacement_temporary_path(destination)?;
    write_replacement_guarded(
        destination,
        &temporary,
        |file| {
            let mut source = File::open(source)?;
            io::copy(&mut source, file)?;
            Ok(())
        },
        before_replace,
    )
}

/// Copies a complete source file to a destination that must not exist yet. A file that appears
/// there meanwhile is kept, and the copy fails with `AlreadyExists`.
pub(crate) fn create_file_from_path(source: &Path, destination: &Path) -> io::Result<()> {
    let parent = destination.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "destination has no parent folder",
        )
    })?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    io::copy(&mut File::open(source)?, temporary.as_file_mut())?;
    publish_new(temporary, destination)
}

/// Cleanup becomes our responsibility only after exclusive creation succeeds.
fn write_replacement(
    destination: &Path,
    temporary: &Path,
    write_contents: impl FnOnce(&mut File) -> io::Result<()>,
) -> io::Result<()> {
    write_replacement_guarded(destination, temporary, write_contents, || Ok(()))
}

fn write_replacement_guarded(
    destination: &Path,
    temporary: &Path,
    write_contents: impl FnOnce(&mut File) -> io::Result<()>,
    before_replace: impl FnOnce() -> io::Result<()>,
) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temporary)?;
    let result = (|| {
        write_contents(&mut file)?;
        file.sync_all()?;
        drop(file);
        before_replace()?;
        replace_path(temporary, destination)
    })();
    finish_file_operation(temporary, result)
}

/// Removes a failed output while treating an already-absent file as clean.
pub(crate) fn remove_file_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn finish_file_operation<T>(incomplete: &Path, result: io::Result<T>) -> io::Result<T> {
    match result {
        Ok(value) => Ok(value),
        Err(operation_error) => match remove_file_if_present(incomplete) {
            Ok(()) => Err(operation_error),
            Err(cleanup_error) => Err(io::Error::new(
                operation_error.kind(),
                format!(
                    "{operation_error}; could not remove incomplete file {}: {cleanup_error}",
                    incomplete.display()
                ),
            )),
        },
    }
}

fn replacement_temporary_path(path: &Path) -> io::Result<PathBuf> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "destination has no parent folder",
        )
    })?;
    let file_name = path.file_name().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "destination has no file name")
    })?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary = parent.join(format!(
        ".{}.{}-{nonce}.tmp",
        file_name.to_string_lossy(),
        std::process::id()
    ));
    Ok(temporary)
}

#[cfg(windows)]
pub(crate) fn replace_path(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    // SAFETY: Both arguments are owned, NUL-terminated UTF-16 buffers that
    // remain alive for the duration of the Windows API call.
    let succeeded = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if succeeded == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
pub(crate) fn replace_path(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)?;
    let parent = destination.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "destination has no parent folder",
        )
    })?;
    File::open(parent)?.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDirectory;

    #[test]
    fn racing_creators_publish_one_complete_file_and_remove_their_temporaries() {
        let directory = TestDirectory::new("storage-create-race");
        let destination = directory.0.join("document.json");
        let barrier = std::sync::Barrier::new(2);
        let results = std::thread::scope(|scope| {
            let spawn = |value| {
                let destination = &destination;
                let barrier = &barrier;
                scope.spawn(move || {
                    barrier.wait();
                    create_file(destination, &[value; 4096])
                })
            };
            let a = spawn(1);
            let b = spawn(2);
            [a.join().unwrap(), b.join().unwrap()]
        });
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results.into_iter().find_map(Result::err).unwrap().kind(),
            io::ErrorKind::AlreadyExists
        );
        let bytes = fs::read(&destination).unwrap();
        assert!(bytes == [1; 4096] || bytes == [2; 4096]);
        assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 1);
    }

    #[test]
    fn failed_creation_preserves_the_destination_and_cleans_its_temporary() {
        let directory = TestDirectory::new("storage-create-existing");
        let destination = directory.0.join("document.json");
        fs::write(&destination, b"existing").unwrap();
        assert_eq!(
            create_file(&destination, b"replacement")
                .unwrap_err()
                .kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(fs::read(&destination).unwrap(), b"existing");
        let occupied = directory.0.join("folder");
        fs::create_dir(&occupied).unwrap();
        assert!(create_file(&occupied, b"replacement").is_err());
        assert!(occupied.is_dir());
        assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 2);
    }

    #[test]
    fn guarded_replacement_preserves_an_external_change_and_cleans_its_temporary() {
        let directory = TestDirectory::new("guarded-replacement");
        let destination = directory.0.join("settings.json");
        fs::write(&destination, b"external").unwrap();
        assert!(replace_file_if_unchanged(&destination, b"authored", b"original").is_err());
        assert_eq!(fs::read(&destination).unwrap(), b"external");
        assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 1);
    }

    #[test]
    fn temporary_collision_does_not_remove_someone_elses_file() {
        let directory = TestDirectory::new("storage-collision");
        let destination = directory.0.join("settings.json");
        let temporary = directory.0.join("occupied.tmp");
        fs::write(&destination, b"original").unwrap();
        fs::write(&temporary, b"another writer").unwrap();
        let error = write_replacement(&destination, &temporary, |_| {
            panic!("must not write after failed creation")
        })
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&destination).unwrap(), b"original");
        assert_eq!(fs::read(&temporary).unwrap(), b"another writer");
    }

    #[test]
    fn failed_partial_write_cleans_only_the_owned_temporary() {
        let directory = TestDirectory::new("storage-partial-write");
        let destination = directory.0.join("settings.json");
        let temporary = directory.0.join("owned.tmp");
        fs::write(&destination, b"original").unwrap();
        let error = write_replacement(&destination, &temporary, |file| {
            file.write_all(b"partial")?;
            Err(io::Error::other("injected failure"))
        })
        .unwrap_err();
        assert!(error.to_string().contains("injected failure"));
        assert_eq!(fs::read(&destination).unwrap(), b"original");
        assert!(!temporary.exists());
    }

    #[test]
    fn replacement_never_leaves_a_partial_or_temporary_file() {
        let directory = TestDirectory::new("storage");
        let destination = directory.0.join("settings.json");
        fs::write(&destination, b"old").unwrap();

        replace_file(&destination, b"complete replacement").unwrap();

        assert_eq!(fs::read(&destination).unwrap(), b"complete replacement");
        assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 1);
    }

    #[test]
    fn replacement_from_path_streams_and_replaces_without_leaving_a_temporary_file() {
        let directory = TestDirectory::new("storage-from-path");
        let source = directory.0.join("source.pkg");
        let destination = directory.0.join("destination.pkg");
        fs::write(&source, b"complete package replacement").unwrap();
        fs::write(&destination, b"old package").unwrap();

        replace_file_from_path(&source, &destination).unwrap();

        assert_eq!(
            fs::read(&destination).unwrap(),
            b"complete package replacement"
        );
        assert_eq!(fs::read(&source).unwrap(), b"complete package replacement");
        assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 2);
    }

    #[test]
    fn replacement_from_missing_source_preserves_destination_and_cleans_temporary_file() {
        let directory = TestDirectory::new("storage-from-missing-path");
        let source = directory.0.join("missing.pkg");
        let destination = directory.0.join("destination.pkg");
        fs::write(&destination, b"original package").unwrap();

        let error = replace_file_from_path(&source, &destination).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert_eq!(fs::read(&destination).unwrap(), b"original package");
        assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 1);
    }

    #[test]
    fn failed_cleanup_preserves_the_primary_error_and_names_the_residue() {
        let directory = TestDirectory::new("storage-cleanup-failure");
        let residue = directory.0.join("incomplete.tmp");
        fs::create_dir(&residue).unwrap();

        let error =
            finish_file_operation::<()>(&residue, Err(io::Error::other("injected write failure")))
                .unwrap_err()
                .to_string();

        assert!(error.contains("injected write failure"));
        assert!(error.contains("could not remove incomplete file"));
        assert!(error.contains(&residue.display().to_string()));
    }
}

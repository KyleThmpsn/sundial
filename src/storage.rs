use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

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
                    "The file changed outside Sundial before replacement; reload before saving",
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

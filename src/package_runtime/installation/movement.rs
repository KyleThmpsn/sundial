//! Move files or directories atomically without clobbering a concurrent destination.
use std::{io, path::Path};

#[cfg(windows)]
pub(super) fn move_without_replacing(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{MOVEFILE_WRITE_THROUGH, MoveFileExW};
    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    // SAFETY: Both paths are owned, NUL-terminated buffers alive for the entire call.
    // Omitting MOVEFILE_REPLACE_EXISTING makes the destination check atomic.
    let result = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(target_os = "linux")]
pub(super) fn move_without_replacing(source: &Path, destination: &Path) -> io::Result<()> {
    use rustix::fs::{CWD, RenameFlags, renameat_with};
    renameat_with(CWD, source, CWD, destination, RenameFlags::NOREPLACE).map_err(io::Error::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn moving_never_replaces_files_or_directories() {
        for directory in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let source = root.path().join("source");
            let destination = root.path().join("destination");
            if directory {
                fs::create_dir(&source).unwrap();
                fs::create_dir(&destination).unwrap();
            } else {
                fs::write(&source, b"archived").unwrap();
                fs::write(&destination, b"existing").unwrap();
            }
            assert!(move_without_replacing(&source, &destination).is_err());
            assert!(source.exists());
            if !directory {
                assert_eq!(fs::read(&destination).unwrap(), b"existing");
            }
        }
    }
}

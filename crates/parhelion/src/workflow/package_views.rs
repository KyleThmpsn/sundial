//! Owned temporary views and conservative recovery of marked, inactive views.

use super::*;

const VIEW_PREFIX: &str = ".parhelion-package-view-";
const LEASE_FILE: &str = ".parhelion-view-lease";
const LEASE_MAGIC: &[u8] = b"Sundial Parhelion package view lease v1\n";
const CLEANUP_LOCK_FILE: &str = ".parhelion-view-cleanup.lock";

pub(super) struct ViewLease {
    file: File,
}

impl ViewLease {
    pub(super) fn create(directory: &Path) -> Result<Self, String> {
        let path = directory.join(LEASE_FILE);
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| {
                format!(
                    "Could not create package-view lease {}: {error}",
                    path.display()
                )
            })?;
        fs2::FileExt::lock_exclusive(&file).map_err(|error| {
            format!(
                "Could not lock package-view lease {}: {error}",
                path.display()
            )
        })?;
        // A racing cleaner sees either no recognized marker or an already-locked marker.
        file.write_all(LEASE_MAGIC)
            .and_then(|()| file.sync_all())
            .map_err(|error| {
                format!(
                    "Could not record package-view lease {}: {error}",
                    path.display()
                )
            })?;
        Ok(Self { file })
    }
}

pub(super) fn finish_with_cleanup<T>(
    result: Result<T, String>,
    cleanup: Result<(), String>,
) -> Result<T, String> {
    match (result, cleanup) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), Ok(())) | (Ok(_), Err(error)) => Err(error),
        (Err(error), Err(cleanup)) => Err(format!("{error}\n{cleanup}")),
    }
}

#[cfg(windows)]
pub(super) fn initialize_source_decoder(source: &Path) -> Result<(), String> {
    let source = source.canonicalize().map_err(|error| {
        format!(
            "Could not resolve package source {}: {error}",
            source.display()
        )
    })?;
    let bin = source
        .parent()
        .ok_or_else(|| "Package source has no install parent".to_owned())?
        .join("bin");
    if !["oo2core_3_win64.dll", "oo2core_9_win64.dll"]
        .iter()
        .any(|name| bin.join("x64").join(name).is_file())
    {
        return Ok(());
    }
    for entry in fs::read_dir(&bin).map_err(|error| {
        format!(
            "Could not inspect decoder directory {}: {error}",
            bin.display()
        )
    })? {
        let entry =
            entry.map_err(|error| format!("Could not inspect decoder directory: {error}"))?;
        if entry
            .file_type()
            .map_err(|error| error.to_string())?
            .is_file()
            && has_pkg_extension(&entry.path())
        {
            return Err(format!(
                "Decoder bootstrap refuses unexpected package data in {}",
                bin.display()
            ));
        }
    }
    // tiger-pkg 0.21 loads ../bin/x64 DLLs into process-global storage BEFORE enumerating the
    // supplied directory. Pointing it at install/bin anchors those DLLs in the stable install
    // without scanning package data, including authored overlays intentionally excluded below.
    // Both dependencies enable ignore_caches. The expected empty census performs no writes.
    match tiger_pkg::PackageManager::new(
        &bin,
        tiger_pkg::GameVersion::Destiny(tiger_pkg::DestinyVersion::Destiny2Shadowkeep),
        None,
    ) {
        Err(error) if error.to_string() == "No packages found" => Ok(()),
        Err(error) => Err(format!(
            "Could not initialize the source package decoder: {error}"
        )),
        Ok(_) => Err("Decoder bootstrap unexpectedly discovered package data".to_owned()),
    }
}

#[cfg(not(windows))]
pub(super) fn initialize_source_decoder(_source: &Path) -> Result<(), String> {
    Ok(())
}

pub(super) fn prune_stale_views(root: &Path) -> Result<usize, String> {
    let root = root.canonicalize().map_err(|error| {
        format!(
            "Could not resolve package-view root {}: {error}",
            root.display()
        )
    })?;
    let _cleanup_lock = lock_view_cleanup(&root)?;
    let mut removed = 0;
    for entry in fs::read_dir(&root).map_err(|error| {
        format!(
            "Could not inspect package-view root {}: {error}",
            root.display()
        )
    })? {
        let entry =
            entry.map_err(|error| format!("Could not inspect package-view entry: {error}"))?;
        if !entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(VIEW_PREFIX))
        {
            continue;
        }
        let path = entry.path();
        if !is_direct_regular_directory(&root, &path)? {
            continue;
        }
        if let Some(lease) = inactive_lease(&path)? {
            remove_owned_view_locked(&root, &path, lease)?;
            removed += 1;
        }
    }
    Ok(removed)
}

fn lock_view_cleanup(root: &Path) -> Result<File, String> {
    let path = root.join(CLEANUP_LOCK_FILE);
    match fs::symlink_metadata(&path) {
        Ok(metadata) => validate_cleanup_lock(&path, &metadata)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!(
                "Could not inspect package-view cleanup lock {}: {error}",
                path.display()
            ));
        }
    }
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;

        // Inspect the opened reparse point itself if the path changes after the first check.
        options.custom_flags(0x0020_0000); // FILE_FLAG_OPEN_REPARSE_POINT
    }
    let file = options.open(&path).map_err(|error| {
        format!(
            "Could not open package-view cleanup lock {}: {error}",
            path.display()
        )
    })?;
    validate_cleanup_lock(&path, &file.metadata().map_err(|error| error.to_string())?)?;
    // Never unlink this file: all cleaners must lock the same file across view deletions.
    // Stale-view leases are tried without waiting, so an owner can retain its lease while
    // waiting here without deadlocking a prune pass that already holds this root lock.
    fs2::FileExt::lock_exclusive(&file).map_err(|error| {
        format!(
            "Could not lock package-view cleanup root {}: {error}",
            root.display()
        )
    })?;
    Ok(file)
}

fn validate_cleanup_lock(path: &Path, metadata: &fs::Metadata) -> Result<(), String> {
    if !metadata.is_file() || is_reparse(metadata) {
        return Err(format!(
            "Refusing a linked or non-file package-view cleanup lock at {}",
            path.display()
        ));
    }
    Ok(())
}

fn is_reparse(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    metadata.file_type().is_symlink()
}

fn is_direct_regular_directory(root: &Path, path: &Path) -> Result<bool, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(format!(
                "Could not inspect package view {}: {error}",
                path.display()
            ));
        }
    };
    if !metadata.is_dir() || is_reparse(&metadata) {
        return Ok(false);
    }
    match path.canonicalize() {
        Ok(path) => Ok(path.parent() == Some(root)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.to_string()),
    }
}

fn inactive_lease(directory: &Path) -> Result<Option<ViewLease>, String> {
    let path = directory.join(LEASE_FILE);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "Could not inspect package-view lease {}: {error}",
                path.display()
            ));
        }
    };
    if !metadata.is_file() || is_reparse(&metadata) || metadata.len() != LEASE_MAGIC.len() as u64 {
        return Ok(None);
    }
    let mut file = match OpenOptions::new().read(true).write(true).open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "Could not open package-view lease {}: {error}",
                path.display()
            ));
        }
    };
    if let Err(error) = fs2::FileExt::try_lock_exclusive(&file) {
        if error.raw_os_error() == fs2::lock_contended_error().raw_os_error() {
            return Ok(None);
        }
        return Err(format!(
            "Could not inspect package-view ownership {}: {error}",
            path.display()
        ));
    }
    let mut magic = vec![0; LEASE_MAGIC.len()];
    if file.read_exact(&mut magic).is_err() || magic != LEASE_MAGIC {
        return Ok(None);
    }
    Ok(Some(ViewLease { file }))
}

fn validate_owned_tree(directory: &Path) -> Result<(), String> {
    let mut pending = vec![directory.to_path_buf()];
    while let Some(parent) = pending.pop() {
        let entries = match fs::read_dir(&parent) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(format!(
                    "Could not inspect owned package view {}: {error}",
                    parent.display()
                ));
            }
        };
        for entry in entries {
            let path = entry.map_err(|error| error.to_string())?.path();
            let metadata = match fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.to_string()),
            };
            let relative = path
                .strip_prefix(directory)
                .map_err(|error| error.to_string())?;
            if is_reparse(&metadata) || !known_view_entry(relative, &metadata) {
                return Err(format!(
                    "Temporary package view contains unrecognized or linked content at {}. It was preserved.",
                    path.display()
                ));
            }
            if metadata.is_dir() {
                pending.push(path);
            }
        }
    }
    Ok(())
}

fn known_view_entry(relative: &Path, metadata: &fs::Metadata) -> bool {
    if metadata.is_dir() {
        return ["packages", "bin", "bin/x64"]
            .iter()
            .any(|known| relative == Path::new(known));
    }
    if !metadata.is_file() {
        return false;
    }
    relative == Path::new(LEASE_FILE)
        || (relative.parent() == Some(Path::new("packages")) && has_pkg_extension(relative))
        || (relative == Path::new("destiny2.exe") && metadata.len() == 0)
        || ["bin/x64/oo2core_3_win64.dll", "bin/x64/oo2core_9_win64.dll"]
            .iter()
            .any(|known| relative == Path::new(known))
}

pub(super) fn remove_owned_view(directory: &Path, lease: ViewLease) -> Result<(), String> {
    let root = directory
        .parent()
        .ok_or_else(|| "Package view has no owning root".to_owned())?
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let _cleanup_lock = lock_view_cleanup(&root)?;
    remove_owned_view_locked(&root, directory, lease)
}

fn remove_owned_view_locked(root: &Path, directory: &Path, lease: ViewLease) -> Result<(), String> {
    if !is_direct_regular_directory(root, directory)? {
        if view_disappeared(directory) {
            return Ok(());
        }
        return Err(format!(
            "Refusing to clean a redirected package view at {}",
            directory.display()
        ));
    }
    if let Err(error) = validate_owned_tree(directory) {
        return if view_disappeared(directory) {
            Ok(())
        } else {
            Err(error)
        };
    }
    // Keep the valid marker and lock while removing payloads. A failure remains discoverable
    // next time, and a competing cleaner cannot mistake this view for an abandoned one.
    for name in ["packages", "bin"] {
        remove_if_present(|| fs::remove_dir_all(directory.join(name)), directory)?;
    }
    remove_if_present(
        || fs::remove_file(directory.join("destiny2.exe")),
        directory,
    )?;
    // The persistent root lock also covers this final handle release. Another cleaner must
    // not reopen the lease before Windows finishes removing its marker and empty directory.
    drop(lease.file);
    remove_if_present(|| fs::remove_file(directory.join(LEASE_FILE)), directory)?;
    remove_if_present(|| fs::remove_dir(directory), directory)
}

fn view_disappeared(directory: &Path) -> bool {
    matches!(fs::symlink_metadata(directory), Err(error) if error.kind() == std::io::ErrorKind::NotFound)
}

fn remove_if_present(
    action: impl FnOnce() -> std::io::Result<()>,
    directory: &Path,
) -> Result<(), String> {
    match action() {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "Could not clean temporary package view {}: {error}. The remaining view was preserved for a later cleanup attempt.",
            directory.display()
        )),
    }
}

#[cfg(test)]
mod tests;

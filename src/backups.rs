//! Readable backups with per-file ownership and legacy history retention.
mod index;
use crate::paths;
pub(crate) use index::create;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

pub(crate) fn root() -> Option<PathBuf> {
    #[cfg(test)]
    {
        thread_local! {
            static DIRECTORY: crate::test_support::TestDirectory = crate::test_support::TestDirectory::new("backup-root");
        }
        Some(DIRECTORY.with(|directory| directory.0.clone()))
    }
    #[cfg(not(test))]
    {
        paths::data_dir().map(|path| path.join("backups"))
    }
}

/// Legacy files directly under the backup root have no reliable source identity and are
/// deliberately never included in automatic retention.
pub(crate) fn source_directory(root: &Path, source: &Path) -> Result<PathBuf, String> {
    let source = paths::resolve_path_for_comparison(source).map_err(|error| {
        format!(
            "Could not resolve backup source {}: {error}",
            source.display()
        )
    })?;
    let identity = source
        .to_str()
        .ok_or("The backup source path is not valid Unicode")?;
    #[cfg(windows)]
    let identity = identity.to_lowercase();
    let digest = Sha256::digest(identity.as_bytes());
    let resolved_root = paths::resolve_path_for_comparison(root).map_err(|error| {
        format!(
            "Could not resolve backup folder {}: {error}",
            root.display()
        )
    })?;
    let directory = resolved_root.join("sources").join(format!("{digest:x}"));
    let resolved = paths::resolve_path_for_comparison(&directory).map_err(|error| {
        format!(
            "Could not resolve backup folder {}: {error}",
            directory.display()
        )
    })?;
    // Do not follow a redirected source directory into another source's history.
    if !paths::paths_equal(&resolved, &directory) {
        return Err(format!(
            "Backup source folder is redirected: {}",
            directory.display()
        ));
    }
    Ok(directory)
}

#[cfg(test)]
pub(crate) fn create_source_directory(root: &Path, source: &Path) -> Result<PathBuf, String> {
    let directory = source_directory(root, source)?;
    fs::create_dir_all(&directory)
        .map_err(|error| format!("Could not create {}: {error}", directory.display()))?;
    source_directory(root, source)
}

#[derive(Debug)]
struct AutomaticBackup {
    modified: SystemTime,
    file_name: String,
    path: PathBuf,
    sha256: Option<String>,
}

pub(crate) fn prune_automatic_backups(
    backup_root: &Path,
    source: &Path,
    keep_per_source: usize,
) -> Result<usize, String> {
    if !backup_root
        .try_exists()
        .map_err(|error| format!("Could not inspect {}: {error}", backup_root.display()))?
    {
        return Ok(0);
    }
    let root = source_directory(backup_root, source)?;
    let mut store = index::Store::open(backup_root)?;
    let identity = index::source_identity(source)?;
    let mut json_backups = Vec::new();
    let mut sqlite_backups = Vec::new();
    for (name, record) in store.records() {
        if !record.automatic || record.source != identity {
            continue;
        }
        let path = index::checked_child(&store.root, name)?;
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.to_string()),
        };
        if !metadata.is_file() {
            continue;
        }
        let destination = if name.ends_with(".json") {
            &mut json_backups
        } else if name.ends_with(".sqlite3") {
            &mut sqlite_backups
        } else {
            continue;
        };
        let bytes = fs::read(&path).map_err(|error| error.to_string())?;
        if format!("{:x}", Sha256::digest(&bytes)) != record.sha256 {
            continue;
        }
        destination.push(AutomaticBackup {
            modified: metadata.modified().map_err(|error| error.to_string())?,
            file_name: name.clone(),
            path,
            sha256: Some(record.sha256.clone()),
        });
    }
    if root.try_exists().map_err(|error| error.to_string())? {
        for entry in fs::read_dir(&root)
            .map_err(|error| format!("Could not read {}: {error}", root.display()))?
        {
            let entry = entry.map_err(|error| {
                format!("Could not read an entry in {}: {error}", root.display())
            })?;
            if !entry
                .file_type()
                .map_err(|error| format!("Could not inspect {}: {error}", entry.path().display()))?
                .is_file()
            {
                continue;
            }
            let file_name = entry.file_name().to_string_lossy().into_owned();
            let destination = if is_automatic_json_backup(&file_name) {
                &mut json_backups
            } else if is_automatic_sqlite_backup(&file_name) {
                &mut sqlite_backups
            } else {
                continue;
            };
            let path = entry.path();
            let modified = entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .map_err(|error| format!("Could not inspect {}: {error}", path.display()))?;
            destination.push(AutomaticBackup {
                modified,
                file_name,
                path,
                sha256: None,
            });
        }
    }
    let json_removed = prune_automatic_backup_family(json_backups, keep_per_source, &mut store)?;
    let sqlite_removed =
        prune_automatic_backup_family(sqlite_backups, keep_per_source, &mut store)?;
    if json_removed + sqlite_removed > 0 {
        store.save()?;
    }
    Ok(json_removed + sqlite_removed)
}

fn is_automatic_json_backup(file_name: &str) -> bool {
    let Some(name) = file_name
        .strip_prefix("settings-v")
        .and_then(|name| name.strip_suffix(".json"))
    else {
        return false;
    };
    let Some((schema, timestamp)) = name.split_once('-') else {
        return false;
    };
    !schema.is_empty()
        && schema.bytes().all(|byte| byte.is_ascii_digit())
        && !timestamp.is_empty()
        && timestamp.bytes().all(|byte| byte.is_ascii_digit())
}

fn is_automatic_sqlite_backup(file_name: &str) -> bool {
    let Some(name) = file_name
        .strip_prefix("state-sqlite-v1-")
        .and_then(|name| name.strip_suffix(".sqlite3"))
    else {
        return false;
    };
    name.split('-')
        .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

fn prune_automatic_backup_family(
    mut backups: Vec<AutomaticBackup>,
    keep: usize,
    store: &mut index::Store,
) -> Result<usize, String> {
    backups.sort_by(|left, right| {
        right
            .modified
            .cmp(&left.modified)
            .then_with(|| right.file_name.cmp(&left.file_name))
    });
    let mut removed = 0;
    for backup in backups.into_iter().skip(keep) {
        if let Some(expected) = &backup.sha256 {
            index::checked_child(&store.root, &backup.file_name)?;
            let bytes = fs::read(&backup.path).map_err(|error| error.to_string())?;
            if format!("{:x}", Sha256::digest(&bytes)) != *expected {
                return Err("A backup changed during cleanup. It was left untouched".into());
            }
        }
        fs::remove_file(&backup.path)
            .map_err(|error| format!("Could not remove {}: {error}", backup.path.display()))?;
        if backup.sha256.is_some() {
            store.remove(&backup.file_name);
        }
        removed += 1;
    }
    Ok(removed)
}

#[cfg(test)]
mod tests;

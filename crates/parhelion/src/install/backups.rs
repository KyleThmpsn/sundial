use super::*;

const COMPLETED_BACKUP_RECORD: &str = "install-complete.json";

/// Absence (including legacy backups) means retention must leave this generation alone.
pub(super) fn mark_backup_complete(transaction: &InstallTransactionRecord) -> Result<(), String> {
    if transaction.state == InstallTransactionState::Pending {
        return Err("A pending recovery backup cannot be released for cleanup".to_owned());
    }
    write_install_transaction(
        &transaction.backup_directory.join(COMPLETED_BACKUP_RECORD),
        transaction,
    )
    .map_err(|error| error.message)
}

fn completed_backup_owner(path: &Path) -> Option<PathBuf> {
    let marker = path.join(COMPLETED_BACKUP_RECORD);
    let metadata = fs::symlink_metadata(&marker).ok()?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return None;
    }
    let record = read_install_transaction(&marker).ok()?;
    // Account-removing installs are recovery sets, not disposable automatic snapshots.
    if record.account_cleanup.is_some() {
        return None;
    }
    if record.state == InstallTransactionState::Pending
        || !paths_equal(&record.backup_directory, path)
        || validate_install_transaction(&record, &record.target_packages_directory).is_err()
    {
        return None;
    }
    Some(record.target_packages_directory)
}

pub(super) fn backup_originals(
    validated: &ValidatedRun,
    backup_directory: &Path,
) -> Result<Vec<OriginalArtifact>, String> {
    let mut originals = Vec::with_capacity(validated.artifacts.len());
    for artifact in validated
        .artifacts
        .iter()
        .chain(&validated.obsolete_artifacts)
    {
        let target_path = validated
            .target_packages_directory
            .join(&artifact.file_name);
        let original = backup_one_original(&artifact.file_name, &target_path, backup_directory)?;
        originals.push(original);
    }
    Ok(originals)
}

pub(super) fn backup_one_original(
    file_name: &str,
    target_path: &Path,
    backup_directory: &Path,
) -> Result<OriginalArtifact, String> {
    match fs::symlink_metadata(target_path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(OriginalArtifact {
            file_name: file_name.to_owned(),
            target_path: target_path.to_path_buf(),
            backup_path: None,
            digest: None,
        }),
        Err(error) => Err(format!(
            "Could not inspect target package {}: {error}",
            target_path.display()
        )),
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => Err(format!(
            "Target package is not a regular file: {}",
            target_path.display()
        )),
        Ok(_) => {
            let backup_path = backup_directory.join(file_name);
            let copied = copy_file_create_new(target_path, &backup_path).map_err(|error| {
                format!(
                    "Could not back up target package {}: {error}",
                    target_path.display()
                )
            })?;
            let current = digest_file(target_path).map_err(|error| {
                format!(
                    "Could not verify target package {} after backup: {error}",
                    target_path.display()
                )
            })?;
            if copied != current {
                return Err(format!(
                    "Target package {} changed while it was being backed up",
                    target_path.display()
                ));
            }
            Ok(OriginalArtifact {
                file_name: file_name.to_owned(),
                target_path: target_path.to_path_buf(),
                backup_path: Some(backup_path),
                digest: Some(current),
            })
        }
    }
}

pub(super) fn backup_recipe_snapshots(
    validated: &ValidatedRun,
    backup_directory: &Path,
) -> Result<Option<PathBuf>, String> {
    if !validated.backup_recipe_snapshots || validated.selected_recipe_files.is_empty() {
        return Ok(None);
    }
    let recipe_backup_directory = backup_directory.join(RECIPE_BACKUP_DIRECTORY);
    fs::create_dir(&recipe_backup_directory).map_err(|error| {
        format!(
            "Could not create recipe backup directory {}: {error}",
            recipe_backup_directory.display()
        )
    })?;
    for relative_path in &validated.selected_recipe_files {
        let source = validated.staged_run_directory.join(relative_path);
        reject_symlink(&source, "staged recipe backup source").map_err(|error| error.message)?;
        let file_name = Path::new(relative_path)
            .file_name()
            .ok_or_else(|| format!("Staged recipe path has no filename: {relative_path}"))?;
        let target = recipe_backup_directory.join(file_name);
        let copied = copy_file_create_new(&source, &target).map_err(|error| {
            format!(
                "Could not back up staged recipe {}: {error}",
                source.display()
            )
        })?;
        let current = digest_file(&source).map_err(|error| {
            format!(
                "Could not verify staged recipe {} after backup: {error}",
                source.display()
            )
        })?;
        if copied != current {
            return Err(format!(
                "Staged recipe {} changed while it was being backed up",
                source.display()
            ));
        }
    }
    fs::canonicalize(&recipe_backup_directory)
        .map(Some)
        .map_err(|error| {
            format!(
                "Could not resolve recipe backup directory {}: {error}",
                recipe_backup_directory.display()
            )
        })
}

pub(super) fn create_backup_directory(backup_root: &Path) -> io::Result<PathBuf> {
    fs::create_dir_all(backup_root)?;
    let canonical_root = fs::canonicalize(backup_root)?;
    for attempt in 0..128u64 {
        let token = unique_token()
            .split('-')
            .map(|part| {
                let value = part.trim_start_matches('0');
                if value.is_empty() { "0" } else { value }
            })
            .collect::<Vec<_>>()
            .join("-");
        let candidate = canonical_root.join(format!("parhelion-backup-v2-{token}-{attempt}"));
        match fs::create_dir(&candidate) {
            Ok(()) => return fs::canonicalize(candidate),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate a unique backup directory",
    ))
}

/// Removes old installer-generated backup generations while preserving unrelated files and
/// manually named diagnostic snapshots in the same root. Only completed, recorded
/// generations are eligible; retention is counted separately for each installation.
pub fn prune_package_backups(
    backup_root: &Path,
    retain: usize,
) -> Result<BackupPruneReport, String> {
    if !(1..=MAX_PACKAGE_BACKUP_RETENTION).contains(&retain) {
        return Err(format!(
            "Package backup retention must be between 1 and {MAX_PACKAGE_BACKUP_RETENTION}"
        ));
    }
    let root_metadata = match fs::symlink_metadata(backup_root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(BackupPruneReport::default());
        }
        Err(error) => {
            return Err(format!(
                "Could not inspect package backup root {}: {error}",
                backup_root.display()
            ));
        }
    };
    if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
        return Err(format!(
            "Package backup root must be a regular directory: {}",
            backup_root.display()
        ));
    }
    let canonical_root = fs::canonicalize(backup_root).map_err(|error| {
        format!(
            "Could not resolve package backup root {}: {error}",
            backup_root.display()
        )
    })?;
    let _lock = lock_directory(&canonical_root, ".parhelion-retention.lock")
        .map_err(|error| error.message)?;
    let mut automatic = Vec::new();
    for entry in fs::read_dir(&canonical_root).map_err(|error| {
        format!(
            "Could not scan package backup root {}: {error}",
            canonical_root.display()
        )
    })? {
        let entry = entry.map_err(|error| {
            format!(
                "Could not inspect a package backup entry in {}: {error}",
                canonical_root.display()
            )
        })?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !is_automatic_backup_name(&name) {
            continue;
        }
        let metadata = fs::symlink_metadata(entry.path()).map_err(|error| {
            format!("Could not inspect automatic package backup {name}: {error}")
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(format!(
                "Automatic package backup is not a regular directory: {}",
                entry.path().display()
            ));
        }
        let path = fs::canonicalize(entry.path()).map_err(|error| {
            format!(
                "Could not resolve automatic package backup {}: {error}",
                entry.path().display()
            )
        })?;
        if path
            .parent()
            .is_none_or(|parent| !paths_equal(parent, &canonical_root))
        {
            return Err(format!(
                "Automatic package backup escaped its configured root: {}",
                path.display()
            ));
        }
        let modified = metadata.modified().map_err(|error| {
            format!(
                "Could not read automatic package backup timestamp {}: {error}",
                path.display()
            )
        })?;
        automatic.push((modified, name, path));
    }
    automatic.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| right.1.cmp(&left.1)));

    let mut report = BackupPruneReport::default();
    let mut owner_counts: Vec<(PathBuf, usize)> = Vec::new();
    for (_, name, path) in automatic {
        let Some(owner) = completed_backup_owner(&path) else {
            report.retained_directories.push(path);
            continue;
        };
        let count = if let Some((_, count)) = owner_counts
            .iter_mut()
            .find(|(target, _)| paths_equal(target, &owner))
        {
            *count += 1;
            *count
        } else {
            owner_counts.push((owner, 1));
            1
        };
        if count <= retain {
            report.retained_directories.push(path);
            continue;
        }
        let metadata = fs::symlink_metadata(&path).map_err(|error| {
            format!(
                "Could not recheck automatic package backup {}: {error}",
                path.display()
            )
        })?;
        if metadata.file_type().is_symlink()
            || !metadata.is_dir()
            || !is_automatic_backup_name(&name)
            || completed_backup_owner(&path).is_none()
            || path
                .parent()
                .is_none_or(|parent| !paths_equal(parent, &canonical_root))
        {
            return Err(format!(
                "Refusing to remove unsafe package backup target {}",
                path.display()
            ));
        }
        fs::remove_dir_all(&path).map_err(|error| {
            format!(
                "Could not remove old automatic package backup {}: {error}",
                path.display()
            )
        })?;
        report.removed_directories.push(path);
    }
    Ok(report)
}

pub(super) fn is_automatic_backup_name(name: &str) -> bool {
    let Some(suffix) = name.strip_prefix(AUTOMATIC_BACKUP_PREFIX) else {
        return false;
    };
    let compact = suffix.strip_prefix("v2-");
    let mut parts = compact.unwrap_or(suffix).split('-');
    let nanos = parts.next().unwrap_or_default();
    let process = parts.next().unwrap_or_default();
    let counter = parts.next().unwrap_or_default();
    let attempt = parts.next().unwrap_or_default();
    parts.next().is_none()
        && if compact.is_some() {
            (1..=32).contains(&nanos.len())
                && (1..=8).contains(&process.len())
                && (1..=16).contains(&counter.len())
        } else {
            nanos.len() == 32 && process.len() == 8 && counter.len() == 16
        }
        && [nanos, process, counter]
            .into_iter()
            .all(|part| part.bytes().all(|byte| byte.is_ascii_hexdigit()))
        && !attempt.is_empty()
        && attempt.bytes().all(|byte| byte.is_ascii_digit())
}

#[test]
fn compact_backup_names_remain_unique_and_preserve_legacy_recognition() {
    let root = tempfile::tempdir().unwrap();
    let first = create_backup_directory(root.path()).unwrap();
    let second = create_backup_directory(root.path()).unwrap();
    assert_ne!(first, second);
    let name = first.file_name().unwrap().to_str().unwrap();
    assert!(name.len() < 54);
    assert!(is_automatic_backup_name(name));
    assert!(is_automatic_backup_name(
        "parhelion-backup-000000000000000018D32BD1B4C12E54-000072DC-0000000000000000-0"
    ));
    for name in [
        "parhelion-backup-manual",
        "parhelion-backup-1-2-3-0",
        "parhelion-backup-v2--1-0-0",
        "parhelion-backup-v2-XX-1-0-0",
        "parhelion-backup-v2-1-2-3-0-extra",
    ] {
        assert!(!is_automatic_backup_name(name));
    }
}

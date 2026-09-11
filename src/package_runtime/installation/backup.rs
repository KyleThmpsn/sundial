//! Transactional, same-volume archival of the deselected runtime.
use super::{RuntimeInspection, RuntimeLocation, move_without_replacing};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Deserialize, Serialize)]
pub(super) struct Receipt {
    pub install: PathBuf,
    pub kept: RuntimeLocation,
    pub state: String,
    pub paths: Vec<PathBuf>,
}

/// The caller supplies its process check so it also runs immediately before each move.
pub(crate) fn archive_other_runtime(
    install: &Path,
    inspected: &RuntimeInspection,
    keep: RuntimeLocation,
    mut require_closed: impl FnMut() -> Result<(), String>,
) -> Result<PathBuf, String> {
    require_closed()?;
    if !inspected.duplicates() || *inspected != RuntimeInspection::inspect(install) {
        return Err(
            "Runtime files changed. Refresh the comparison before selecting a copy.".into(),
        );
    }
    let selected = inspected
        .copies
        .iter()
        .find(|copy| copy.location == keep)
        .ok_or("The selected runtime no longer exists")?;
    if let Some(problem) = &selected.selection_problem {
        return Err(problem.clone());
    }
    let root = fs::canonicalize(install).map_err(|e| e.to_string())?;
    let other = RuntimeLocation::ALL
        .into_iter()
        .find(|location| *location != keep)
        .unwrap();
    let mut paths = Vec::new();
    for name in [
        "steam_api64.dll",
        "Sunrise",
        "steam_api64.pdb",
        "Lua_LICENSE.txt",
    ] {
        let source = other.directory(&root).join(name);
        if source.try_exists().map_err(|e| e.to_string())? {
            checked_tree(&root, &source)?;
            paths.push(
                source
                    .strip_prefix(&root)
                    .map_err(|e| e.to_string())?
                    .to_owned(),
            );
        }
    }
    let backups = root.join(".sunrise/backups");
    checked_directory(&root, &backups)?;
    let stamp = time::OffsetDateTime::now_utc().unix_timestamp_nanos();
    let backup = backups.join(format!("runtime-selection-{stamp}"));
    fs::create_dir(&backup).map_err(|e| format!("Could not create runtime backup: {e}"))?;
    checked_path(&root, &backup)?;
    let mut receipt = Receipt {
        install: root.clone(),
        kept: keep,
        state: "planned".into(),
        paths,
    };
    write_receipt(&backup, &receipt)?;
    let mut moved = Vec::new();
    let result = (|| {
        // Recheck after preparation. A stale popup must never archive a replacement DLL or settings file.
        if *inspected != RuntimeInspection::inspect(install) {
            return Err(
                "Runtime files changed while preparing the backup. Refresh and try again.".into(),
            );
        }
        for relative in &receipt.paths {
            require_closed()?;
            let source = root.join(relative);
            let destination = backup.join(relative);
            checked_tree(&root, &source)?;
            checked_directory(&root, destination.parent().ok_or("Backup has no parent")?)?;
            if destination.exists() {
                return Err("A backup destination already exists".into());
            }
            move_without_replacing(&source, &destination)
                .map_err(|e| format!("Could not archive {}: {e}", source.display()))?;
            moved.push(relative.clone());
        }
        receipt.state = "complete".into();
        write_receipt(&backup, &receipt)
    })();
    if let Err(error) = result {
        let failures = rollback(&root, &backup, &moved);
        receipt.state = if failures.is_empty() {
            "rolled_back"
        } else {
            "recovery_required"
        }
        .into();
        let _ = write_receipt(&backup, &receipt);
        return Err(if failures.is_empty() {
            format!(
                "{error}. Moved files were restored. Backup record: {}",
                backup.display()
            )
        } else {
            format!(
                "{error}. Some files need recovery from {}: {}",
                backup.display(),
                failures.join(", ")
            )
        });
    }
    Ok(backup)
}

pub(super) fn write_receipt(backup: &Path, receipt: &Receipt) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(receipt).map_err(|e| e.to_string())?;
    crate::storage::replace_file(&backup.join("manifest.json"), &bytes).map_err(|e| e.to_string())
}

fn rollback(root: &Path, backup: &Path, moved: &[PathBuf]) -> Vec<String> {
    let mut failures = Vec::new();
    for relative in moved.iter().rev() {
        let result = (|| {
            let source = backup.join(relative);
            let destination = root.join(relative);
            checked_path(root, &source)?;
            checked_path(root, destination.parent().ok_or("Missing restore parent")?)?;
            if destination.exists() {
                return Err("Original location is occupied".into());
            }
            move_without_replacing(&source, &destination).map_err(|e| e.to_string())
        })();
        if let Err(error) = result {
            failures.push(format!("{}: {error}", relative.display()));
        }
    }
    failures
}

pub(super) fn checked_directory(root: &Path, directory: &Path) -> Result<(), String> {
    if directory.exists() {
        return checked_path(root, directory);
    }
    checked_directory(root, directory.parent().ok_or("Backup path has no parent")?)?;
    fs::create_dir(directory).map_err(|e| e.to_string())?;
    checked_path(root, directory)
}

pub(super) fn checked_path(root: &Path, path: &Path) -> Result<(), String> {
    let resolved = fs::canonicalize(path).map_err(|e| e.to_string())?;
    if !resolved.starts_with(root) || resolved == root && path != root {
        return Err(format!(
            "Runtime backup path leaves the installation: {}",
            path.display()
        ));
    }
    for ancestor in path.ancestors().take_while(|ancestor| *ancestor != root) {
        if !ancestor.starts_with(root) {
            return Err("Runtime backup path leaves the installation".into());
        }
        let metadata = fs::symlink_metadata(ancestor).map_err(|e| e.to_string())?;
        if linked(&metadata) {
            return Err(format!(
                "Resolve the linked runtime path before selecting it: {}",
                ancestor.display()
            ));
        }
    }
    Ok(())
}

fn linked(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    metadata.file_type().is_symlink()
}

pub(super) fn checked_tree(root: &Path, path: &Path) -> Result<(), String> {
    checked_path(root, path)?;
    let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if linked(&metadata) {
        return Err(format!(
            "Resolve the linked runtime path before selecting it: {}",
            path.display()
        ));
    }
    if metadata.is_dir() {
        for entry in fs::read_dir(path).map_err(|e| e.to_string())? {
            checked_tree(root, &entry.map_err(|e| e.to_string())?.path())?;
        }
    }
    Ok(())
}

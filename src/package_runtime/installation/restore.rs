//! Native backup restoration shared by Windows and Linux.
use super::backup::{Receipt, checked_directory, checked_path, checked_tree, write_receipt};
use super::{RuntimeInspection, RuntimeLocation, move_without_replacing};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone)]
pub(crate) struct RuntimeRestorePlan {
    pub backup: PathBuf,
    pub paths: Vec<PathBuf>,
    install: PathBuf,
    manifest: Vec<u8>,
    inspection: RuntimeInspection,
}

pub(crate) fn preview_runtime_restore(
    install: &Path,
    backup: &Path,
) -> Result<RuntimeRestorePlan, String> {
    let install = fs::canonicalize(install).map_err(|e| e.to_string())?;
    let backup = fs::canonicalize(backup).map_err(|e| e.to_string())?;
    checked_path(&install, &backup)?;
    if !backup.starts_with(install.join(".sunrise/backups")) {
        return Err(
            "Select a runtime backup from this installation's .sunrise/backups folder.".into(),
        );
    }
    let manifest_path = backup.join("manifest.json");
    checked_path(&install, &manifest_path)?;
    let manifest = fs::read(manifest_path).map_err(|e| e.to_string())?;
    let receipt: Receipt = serde_json::from_slice(&manifest)
        .map_err(|e| format!("Invalid runtime backup manifest: {e}"))?;
    let recorded_root = fs::canonicalize(&receipt.install).map_err(|e| e.to_string())?;
    if recorded_root != install {
        return Err("This backup belongs to a different installation.".into());
    }
    let other = RuntimeLocation::ALL
        .into_iter()
        .find(|location| *location != receipt.kept)
        .unwrap();
    let allowed: BTreeSet<_> = [
        "steam_api64.dll",
        "Sunrise",
        "steam_api64.pdb",
        "Lua_LICENSE.txt",
    ]
    .into_iter()
    .map(|name| {
        other
            .directory(&install)
            .join(name)
            .strip_prefix(&install)
            .unwrap()
            .to_owned()
    })
    .collect();
    let mut paths = Vec::new();
    let mut seen = BTreeSet::new();
    for path in receipt.paths {
        if !allowed.contains(&path) || !seen.insert(path.clone()) {
            return Err("The runtime backup contains an unsupported or duplicate path.".into());
        }
        if !backup.join(&path).try_exists().map_err(|e| e.to_string())? {
            continue;
        }
        checked_tree(&install, &backup.join(&path))?;
        if install
            .join(&path)
            .try_exists()
            .map_err(|e| e.to_string())?
        {
            return Err(format!(
                "Restore destination already exists: {}. Nothing was restored.",
                install.join(&path).display()
            ));
        }
        paths.push(path);
    }
    if paths.is_empty() {
        return Err("This backup has no archived runtime files to restore.".into());
    }
    let inspection = RuntimeInspection::inspect(&backup);
    Ok(RuntimeRestorePlan {
        backup,
        paths,
        install,
        manifest,
        inspection,
    })
}

pub(crate) fn restore_runtime(
    plan: &RuntimeRestorePlan,
    mut require_closed: impl FnMut() -> Result<(), String>,
) -> Result<(), String> {
    require_closed()?;
    let fresh = preview_runtime_restore(&plan.install, &plan.backup)?;
    if fresh.manifest != plan.manifest
        || fresh.inspection != plan.inspection
        || fresh.paths != plan.paths
    {
        return Err(
            "The runtime backup changed after review. Select it again before restoring.".into(),
        );
    }
    let mut moved = Vec::new();
    let result = (|| {
        for path in &plan.paths {
            require_closed()?;
            let source = plan.backup.join(path);
            let destination = plan.install.join(path);
            checked_tree(&plan.install, &source)?;
            checked_directory(
                &plan.install,
                destination.parent().ok_or("Missing restore parent")?,
            )?;
            if destination.exists() {
                return Err(format!(
                    "Restore destination already exists: {}",
                    destination.display()
                ));
            }
            move_without_replacing(&source, &destination).map_err(|e| e.to_string())?;
            moved.push(path.clone());
        }
        let mut receipt: Receipt =
            serde_json::from_slice(&plan.manifest).map_err(|e| e.to_string())?;
        receipt.state = "restored".into();
        write_receipt(&plan.backup, &receipt)
    })();
    if let Err(error) = result {
        let failures = undo_restore(plan, &moved);
        return Err(if failures.is_empty() {
            format!(
                "{error}. Files remain in the backup at {}.",
                plan.backup.display()
            )
        } else {
            format!(
                "{error}. Recovery is needed at {}: {}",
                plan.backup.display(),
                failures.join(", ")
            )
        });
    }
    Ok(())
}

fn undo_restore(plan: &RuntimeRestorePlan, moved: &[PathBuf]) -> Vec<String> {
    let mut failures = Vec::new();
    for path in moved.iter().rev() {
        let result = (|| {
            let source = plan.install.join(path);
            let destination = plan.backup.join(path);
            checked_path(&plan.install, &source)?;
            checked_path(
                &plan.install,
                destination.parent().ok_or("Missing backup parent")?,
            )?;
            if destination.exists() {
                return Err("Backup destination is occupied".into());
            }
            move_without_replacing(&source, &destination).map_err(|e| e.to_string())
        })();
        if let Err(error) = result {
            failures.push(format!("{}: {error}", path.display()));
        }
    }
    failures
}

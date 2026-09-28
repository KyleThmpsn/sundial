//! Update workspaces left beside the executable once an update is finished with them.
//!
//! The helper runs from a copy of the old executable inside its own workspace, and a running
//! executable cannot delete the folder it runs from, so every update left one behind. The
//! first launch after an update waits for the helper to exit, which it signals by releasing
//! the update lock, and removes the folder. Any later launch removes what an earlier one
//! could not.
use super::files::{self, PLAN, PREFIX, Plan};
use std::{
    fs,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

/// How long a launch waits for a running helper to release the update lock.
const HELPER_WAIT: Duration = Duration::from_secs(120);
/// How long removal retries a file the helper or a scanner still holds open.
const REMOVAL_WAIT: Duration = Duration::from_secs(5);
/// Written before the executable is replaced. Removed first, so a folder that is only partly
/// removed still reads as finished with.
const REPLACEMENT_STARTED: &str = "replacement-started";
/// Receipts the helper writes once the executable is installed or restored.
const RECEIPTS: [&str; 2] = ["complete", "rolled-back.txt"];

/// Removes finished update workspaces beside the running executable, off the UI thread.
pub(crate) fn sweep_workspaces() {
    let _ = thread::Builder::new()
        .name("update-cleanup".into())
        .spawn(|| {
            if let Ok(target) =
                std::env::current_exe().and_then(|executable| executable.canonicalize())
            {
                let _ = sweep(&target, HELPER_WAIT);
            }
        });
}

/// Removes every finished workspace beside `target` and returns how many went.
///
/// A helper holds the update lock from before replacement until it exits, so holding it here
/// means no update is using any of these folders. The lock is released on return.
pub(super) fn sweep(target: &Path, wait: Duration) -> Result<usize, String> {
    let parent = target
        .parent()
        .ok_or("The executable has no parent folder.")?;
    // Opening the lock creates its file, so a folder that never updated is left untouched.
    if workspaces(parent)?.is_empty() {
        return Ok(0);
    }
    let lock = files::open_lock(target)?;
    let deadline = Instant::now() + wait;
    while fs2::FileExt::try_lock_exclusive(&lock).is_err() {
        if Instant::now() >= deadline {
            return Ok(0);
        }
        thread::sleep(Duration::from_millis(250));
    }
    Ok(workspaces(parent)?
        .into_iter()
        .filter(|directory| disposable(directory, target) && remove(directory))
        .count())
}

/// Workspace folders directly beside the executable. A link is never followed, so removal
/// cannot be led anywhere else.
fn workspaces(parent: &Path) -> Result<Vec<PathBuf>, String> {
    let entries = fs::read_dir(parent).map_err(|error| error.to_string())?;
    Ok(entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(PREFIX))
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path())
        .collect())
}

/// Whether a workspace holds nothing an update still needs.
///
/// Its backup is the only copy of the previous executable while a replacement is under way,
/// or after one failed without restoring it. That is the one workspace kept: replacement
/// began, no receipt was written, and the executable matches neither side of the plan.
fn disposable(directory: &Path, target: &Path) -> bool {
    let plan = Plan::read(directory).ok();
    if plan.as_ref().is_some_and(|plan| plan.target != target) {
        return false;
    }
    if !directory.join(REPLACEMENT_STARTED).exists() {
        // A crashed download may have no plan yet. Remove only its known staging files, not
        // an unrelated folder that happens to share the updater's prefix.
        return plan.is_some() || abandoned_staging(directory);
    }
    let Some(plan) = plan else { return false };
    if RECEIPTS
        .iter()
        .any(|receipt| directory.join(receipt).is_file())
    {
        return true;
    }
    // Receipts are best effort, so a missing one is not proof of failure. An executable that
    // still matches either side of the plan is whole, and the backup beside it is redundant.
    files::digest(target).is_ok_and(|digest| digest == plan.old_digest || digest == plan.new_digest)
}

fn abandoned_staging(directory: &Path) -> bool {
    let Ok(entries) = fs::read_dir(directory) else {
        return false;
    };
    let mut found = false;
    for entry in entries {
        let Ok(entry) = entry else { return false };
        let Ok(kind) = entry.file_type() else {
            return false;
        };
        if !kind.is_file()
            || !["download", files::PAYLOAD, files::RUNNER]
                .iter()
                .any(|name| entry.file_name() == *name)
        {
            return false;
        }
        found = true;
    }
    found
}

/// Removes a workspace, retrying while the helper finishes leaving. The helper can release its
/// lock a moment before its executable image does, and Windows refuses to delete it until then.
fn remove(directory: &Path) -> bool {
    let deadline = Instant::now() + REMOVAL_WAIT;
    loop {
        match remove_ordered(directory) {
            Ok(()) => return true,
            Err(_) if Instant::now() < deadline => thread::sleep(Duration::from_millis(250)),
            Err(_) => return false,
        }
    }
}

/// Everything else goes before the markers, and the replacement marker before the receipts
/// and the plan. A folder left half removed then still judges as finished with, rather than
/// holding a backup it can no longer vouch for.
fn remove_ordered(directory: &Path) -> std::io::Result<()> {
    let markers = [REPLACEMENT_STARTED, RECEIPTS[0], RECEIPTS[1], PLAN];
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if markers.iter().any(|marker| entry.file_name() == *marker) {
            continue;
        }
        if entry.file_type()?.is_dir() {
            absent_ok(fs::remove_dir_all(entry.path()))?;
        } else {
            absent_ok(fs::remove_file(entry.path()))?;
        }
    }
    for marker in markers {
        absent_ok(fs::remove_file(directory.join(marker)))?;
    }
    absent_ok(fs::remove_dir(directory))
}

fn absent_ok(result: std::io::Result<()>) -> std::io::Result<()> {
    match result {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::updates::files::{BACKUP, PAYLOAD, RUNNER};

    struct Folder {
        _root: tempfile::TempDir,
        target: PathBuf,
    }

    impl Folder {
        fn new() -> Self {
            let root = tempfile::tempdir().unwrap();
            let target = root.path().join("sundial.exe");
            fs::write(&target, b"current executable").unwrap();
            let target = target.canonicalize().unwrap();
            Self {
                _root: root,
                target,
            }
        }

        fn parent(&self) -> &Path {
            self.target.parent().unwrap()
        }

        /// A workspace holding the named files, with a plan whose digests are given.
        fn workspace(
            &self,
            name: &str,
            files_in: &[&str],
            digests: Option<(&str, &str)>,
        ) -> PathBuf {
            let directory = self.parent().join(format!("{PREFIX}{name}"));
            fs::create_dir(&directory).unwrap();
            for file in files_in {
                fs::write(directory.join(file), b"1").unwrap();
            }
            if let Some((old, new)) = digests {
                Plan {
                    target: self.target.clone(),
                    version: "0.5.2".into(),
                    old_digest: old.into(),
                    new_digest: new.into(),
                    install: None,
                }
                .write(&directory)
                .unwrap();
            }
            directory
        }
    }

    const OTHER: &str = "0000000000000000000000000000000000000000000000000000000000000000";

    #[test]
    fn finished_workspaces_go_and_an_unrestored_backup_stays() {
        let folder = Folder::new();
        let current = files::digest(&folder.target).unwrap();
        let installed = folder.workspace(
            "installed",
            &[
                RUNNER,
                BACKUP,
                PAYLOAD,
                REPLACEMENT_STARTED,
                "started",
                "complete",
            ],
            Some((OTHER, &current)),
        );
        let restored = folder.workspace(
            "restored",
            &[RUNNER, BACKUP, REPLACEMENT_STARTED, "rolled-back.txt"],
            Some((&current, OTHER)),
        );
        let abandoned = folder.workspace("abandoned", &["download", PAYLOAD], None);
        // The receipt was never written, but the executable is the new version, so the
        // backup beside it is redundant.
        let unreceipted = folder.workspace(
            "unreceipted",
            &[BACKUP, REPLACEMENT_STARTED],
            Some((OTHER, &current)),
        );
        // Replacement began, nothing was restored, and the executable matches neither side:
        // the backup may be the only good copy of the previous version.
        let failed = folder.workspace(
            "failed",
            &[BACKUP, REPLACEMENT_STARTED],
            Some((OTHER, OTHER)),
        );
        let unreadable = folder.workspace("unreadable", &[BACKUP, REPLACEMENT_STARTED], None);

        assert_eq!(sweep(&folder.target, Duration::ZERO).unwrap(), 4);

        for gone in [&installed, &restored, &abandoned, &unreceipted] {
            assert!(!gone.exists(), "{} should be removed", gone.display());
        }
        for kept in [&failed, &unreadable] {
            assert!(
                kept.join(BACKUP).is_file(),
                "{} lost its backup",
                kept.display()
            );
        }
        assert_eq!(fs::read(&folder.target).unwrap(), b"current executable");
    }

    #[test]
    fn nothing_outside_a_workspace_is_touched() {
        let folder = Folder::new();
        let unrelated = folder.parent().join("recipes");
        fs::create_dir(&unrelated).unwrap();
        fs::write(unrelated.join("keep.json"), b"{}").unwrap();
        let file = folder.parent().join(format!("{PREFIX}not-a-folder"));
        fs::write(&file, b"keep").unwrap();
        folder.workspace(
            "installed",
            &[REPLACEMENT_STARTED, "complete"],
            Some((OTHER, OTHER)),
        );
        let similarly_named = folder.workspace("personal", &["keep.txt"], None);

        assert_eq!(sweep(&folder.target, Duration::ZERO).unwrap(), 1);

        assert_eq!(fs::read(unrelated.join("keep.json")).unwrap(), b"{}");
        assert_eq!(fs::read(&file).unwrap(), b"keep");
        assert!(similarly_named.join("keep.txt").exists());
    }

    /// While a helper holds the lock it is still using its workspace, so nothing is removed,
    /// and a launch gives up after its wait rather than blocking.
    #[test]
    fn a_workspace_in_use_waits_for_its_helper() {
        let folder = Folder::new();
        let installed = folder.workspace(
            "installed",
            &[REPLACEMENT_STARTED, "complete"],
            Some((OTHER, OTHER)),
        );
        let helper = files::open_lock(&folder.target).unwrap();
        fs2::FileExt::lock_exclusive(&helper).unwrap();

        assert_eq!(sweep(&folder.target, Duration::ZERO).unwrap(), 0);
        assert!(installed.exists());

        drop(helper);
        assert_eq!(sweep(&folder.target, Duration::ZERO).unwrap(), 1);
        assert!(!installed.exists());
    }

    /// A folder that never updated gains no lock file from a launch.
    #[test]
    fn a_folder_that_never_updated_is_left_alone() {
        let folder = Folder::new();
        assert_eq!(sweep(&folder.target, Duration::ZERO).unwrap(), 0);
        let entries = fs::read_dir(folder.parent()).unwrap().count();
        assert_eq!(entries, 1, "only the executable should be there");
    }
}

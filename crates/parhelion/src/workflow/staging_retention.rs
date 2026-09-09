//! Keep one completed generation while protecting builders and staged-file readers.

use std::{
    fs::{self, File, Metadata, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::{manifest::MANIFEST_FILE_NAME, package_profile::authored_package_for_file_name};

const ROOT_LOCK: &str = ".parhelion-staging-lock";
const LEASE_FILE: &str = ".parhelion-staged-run-lease";
const COMPLETE_FILE: &str = ".parhelion-staged-run-complete.json";
const LEASE_MAGIC: &[u8] = b"Sundial Parhelion staged run lease v1\n";

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Completion {
    schema: u32,
    completed_unix_nanos: u128,
}

pub(crate) struct ReadLease {
    directory: PathBuf,
    file: Option<File>,
}

impl Drop for ReadLease {
    fn drop(&mut self) {
        let Some(root) = self.directory.parent() else {
            return;
        };
        let cleanup = (|| {
            let _root_lock = lock_root(root, true)?;
            drop(self.file.take());
            prune_locked(root, None)
        })();
        if let Err(error) = cleanup {
            eprintln!("Could not prune staged builds after use: {error}");
        }
    }
}

pub(super) struct StagedRun {
    directory: PathBuf,
    lease: Option<File>,
    completed: bool,
}

impl StagedRun {
    pub(super) fn begin(root: &Path, slug: &str) -> Result<Self, String> {
        fs::create_dir_all(root).map_err(|error| {
            format!("Could not create staging root {}: {error}", root.display())
        })?;
        let root = root.canonicalize().map_err(|error| error.to_string())?;
        let _root_lock = lock_root(&root, true)?;
        prune_locked(&root, None)?;
        let directory = super::create_unique_run_directory(&root, slug)?;
        let lease = match create_lease(&directory) {
            Ok(lease) => lease,
            Err(error) => {
                let cleanup = clean_initialization_failure(&root, &directory);
                return super::package_views::finish_with_cleanup(Err(error), cleanup);
            }
        };
        Ok(Self {
            directory,
            lease: Some(lease),
            completed: false,
        })
    }

    pub(super) fn directory(&self) -> &Path {
        &self.directory
    }

    pub(super) fn finish<T>(mut self, result: Result<T, String>) -> Result<T, String> {
        let cleanup = if result.is_ok() {
            self.complete()
        } else {
            self.discard()
        };
        super::package_views::finish_with_cleanup(result, cleanup)
    }

    fn complete(&mut self) -> Result<(), String> {
        let root = self.directory.parent().expect("staged run has parent");
        let _root_lock = lock_root(root, true)?;
        let completion = Completion {
            schema: 1,
            completed_unix_nanos: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|error| error.to_string())?
                .as_nanos(),
        };
        let encoded = serde_json::to_vec(&completion).map_err(|error| error.to_string())?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(self.directory.join(COMPLETE_FILE))
            .map_err(|error| format!("Could not mark completed staged run: {error}"))?;
        file.write_all(&encoded)
            .and_then(|()| file.sync_all())
            .map_err(|error| format!("Could not save staged-run completion: {error}"))?;
        drop(file);
        self.completed = true;
        let cleanup = prune_locked(root, Some(&self.directory)).map_err(|error| {
            format!(
                "Build completed at {}, but older staging cleanup failed: {error}",
                self.directory.display()
            )
        });
        drop(self.lease.take());
        cleanup
    }

    fn discard(&mut self) -> Result<(), String> {
        let Some(lease) = self.lease.take() else {
            return Ok(());
        };
        let root = self.directory.parent().expect("staged run has parent");
        let _root_lock = lock_root(root, true)?;
        remove_owned_run(root, &self.directory, lease)
    }
}

impl Drop for StagedRun {
    fn drop(&mut self) {
        if !self.completed
            && let Err(error) = self.discard()
        {
            eprintln!("Could not clean failed staged build: {error}");
        }
    }
}

/// Unmarked legacy runs remain installable and are never pruned by this module.
pub(crate) fn lease_for_read(directory: &Path) -> Result<Option<ReadLease>, String> {
    let lease_path = directory.join(LEASE_FILE);
    if !lease_path.try_exists().map_err(|error| error.to_string())? {
        return Ok(None);
    }
    let directory = directory
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let root = directory
        .parent()
        .ok_or_else(|| "Staged run has no parent directory".to_owned())?;
    let _root_lock = lock_root(root, false)?;
    let mut file = open_regular(&directory.join(LEASE_FILE), false)?;
    fs2::FileExt::try_lock_shared(&file)
        .map_err(|error| format!("The staged build is still being written or cleaned: {error}"))?;
    if !recognized_lease(&mut file)? || completion_time(&directory).is_none() {
        return Err("The staged build does not have a valid completion marker".to_owned());
    }
    Ok(Some(ReadLease {
        directory,
        file: Some(file),
    }))
}

fn lock_root(root: &Path, exclusive: bool) -> Result<File, String> {
    // Copied completed runs keep their markers but may not have the original root lock.
    let file = open_regular(&root.join(ROOT_LOCK), true)?;
    let result = if exclusive {
        fs2::FileExt::lock_exclusive(&file)
    } else {
        fs2::FileExt::lock_shared(&file)
    };
    result.map_err(|error| format!("Could not coordinate staged builds: {error}"))?;
    Ok(file)
}

fn create_lease(directory: &Path) -> Result<File, String> {
    let mut lease = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(directory.join(LEASE_FILE))
        .map_err(|error| format!("Could not create staged-run lease: {error}"))?;
    fs2::FileExt::lock_exclusive(&lease)
        .map_err(|error| format!("Could not lock staged run: {error}"))?;
    lease
        .write_all(LEASE_MAGIC)
        .and_then(|()| lease.sync_all())
        .map_err(|error| format!("Could not mark staged run: {error}"))?;
    Ok(lease)
}

fn clean_initialization_failure(root: &Path, directory: &Path) -> Result<(), String> {
    if !safe_child_directory(root, directory)? {
        return Err("The new staged run was redirected before initialization finished".to_owned());
    }
    match fs::remove_file(directory.join(LEASE_FILE)) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("Could not clean failed staged-run lease: {error}")),
    }
    fs::remove_dir(directory)
        .map_err(|error| format!("Could not clean new empty staged run: {error}"))
}

fn open_regular(path: &Path, create: bool) -> Result<File, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if redirected(&metadata) || !metadata.is_file() => {
            return Err(format!(
                "Staging lease is not a regular file: {}",
                path.display()
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && create => {}
        Err(error) => return Err(format!("Could not inspect {}: {error}", path.display())),
        Ok(_) => {}
    }
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(create)
        .truncate(false)
        .open(path)
        .map_err(|error| format!("Could not open {}: {error}", path.display()))
}

fn recognized_lease(file: &mut File) -> Result<bool, String> {
    if file.metadata().map_err(|error| error.to_string())?.len() != LEASE_MAGIC.len() as u64 {
        return Ok(false);
    }
    let mut bytes = vec![0; LEASE_MAGIC.len()];
    file.read_exact(&mut bytes)
        .map_err(|error| error.to_string())?;
    Ok(bytes == LEASE_MAGIC)
}

fn completion_time(directory: &Path) -> Option<u128> {
    let path = directory.join(COMPLETE_FILE);
    let metadata = fs::symlink_metadata(&path).ok()?;
    if redirected(&metadata) || !metadata.is_file() || metadata.len() > 256 {
        return None;
    }
    let completion: Completion = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
    (completion.schema == 1).then_some(completion.completed_unix_nanos)
}

struct InactiveRun {
    completed_at: Option<u128>,
    directory: PathBuf,
    lease: File,
}

fn prune_locked(root: &Path, keep: Option<&Path>) -> Result<(), String> {
    let mut candidates = Vec::new();
    for entry in fs::read_dir(root).map_err(|error| error.to_string())? {
        let directory = entry.map_err(|error| error.to_string())?.path();
        if let Some(candidate) = inactive_run(root, directory)? {
            candidates.push(candidate);
        }
    }
    let newest = keep.map(Path::to_owned).or_else(|| {
        candidates
            .iter()
            .filter_map(|run| run.completed_at.map(|time| (time, &run.directory)))
            .max()
            .map(|(_, path)| path.clone())
    });
    for run in candidates {
        if newest.as_ref() != Some(&run.directory) && owned_inventory(&run.directory).is_ok() {
            remove_owned_run(root, &run.directory, run.lease)?;
        }
    }
    Ok(())
}

fn inactive_run(root: &Path, directory: PathBuf) -> Result<Option<InactiveRun>, String> {
    if !safe_child_directory(root, &directory)? {
        return Ok(None);
    }
    let path = directory.join(LEASE_FILE);
    if !path.try_exists().map_err(|error| error.to_string())? {
        return Ok(None);
    }
    let Ok(mut lease) = open_regular(&path, false) else {
        return Ok(None);
    };
    match fs2::FileExt::try_lock_exclusive(&lease) {
        Ok(()) => {}
        Err(error) if error.raw_os_error() == fs2::lock_contended_error().raw_os_error() => {
            return Ok(None);
        }
        Err(error) => return Err(format!("Could not inspect inactive staged run: {error}")),
    }
    if !recognized_lease(&mut lease)? {
        return Ok(None);
    }
    Ok(Some(InactiveRun {
        completed_at: completion_time(&directory),
        directory,
        lease,
    }))
}

fn safe_child_directory(root: &Path, directory: &Path) -> Result<bool, String> {
    let metadata = fs::symlink_metadata(directory).map_err(|error| error.to_string())?;
    if redirected(&metadata) || !metadata.is_dir() {
        return Ok(false);
    }
    let resolved = directory
        .canonicalize()
        .map_err(|error| error.to_string())?;
    Ok(resolved.parent() == Some(root) && resolved != root)
}

fn owned_inventory(directory: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
        let path = entry.map_err(|error| error.to_string())?.path();
        let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if redirected(&metadata) {
            return Err(format!(
                "Staged run contains a redirected path: {}",
                path.display()
            ));
        }
        if name == "recipes" && metadata.is_dir() {
            collect_recipes(&path, &mut files)?;
        } else if metadata.is_file() && owned_file_name(name) {
            if name != LEASE_FILE {
                files.push(path);
            }
        } else {
            return Err(format!(
                "Staged run contains an unrecognized file: {}",
                path.display()
            ));
        }
    }
    Ok(files)
}

fn owned_file_name(name: &str) -> bool {
    matches!(name, LEASE_FILE | COMPLETE_FILE | MANIFEST_FILE_NAME)
        || authored_package_for_file_name(name).is_some()
}

fn collect_recipes(directory: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
        let path = entry.map_err(|error| error.to_string())?.path();
        let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        if redirected(&metadata)
            || !metadata.is_file()
            || path.extension().is_none_or(|extension| extension != "json")
        {
            return Err(format!(
                "Staged recipe is not an owned regular JSON file: {}",
                path.display()
            ));
        }
        files.push(path);
    }
    Ok(())
}

fn remove_owned_run(root: &Path, directory: &Path, lease: File) -> Result<(), String> {
    if !safe_child_directory(root, directory)? {
        return Err(format!(
            "Refusing to clean redirected staging run {}",
            directory.display()
        ));
    }
    let files = owned_inventory(directory)?;
    for path in files {
        fs::remove_file(&path)
            .map_err(|error| format!("Could not remove staged file {}: {error}", path.display()))?;
    }
    let recipes = directory.join("recipes");
    if recipes.try_exists().map_err(|error| error.to_string())? {
        fs::remove_dir(&recipes).map_err(|error| error.to_string())?;
    }
    // The root lock excludes both new readers and other cleaners across this gap.
    drop(lease);
    fs::remove_file(directory.join(LEASE_FILE)).map_err(|error| error.to_string())?;
    fs::remove_dir(directory).map_err(|error| error.to_string())
}

fn redirected(metadata: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

#[cfg(test)]
mod tests;

//! Shared read-only indexes keyed by the installed package snapshot.
use std::{
    path::Path,
    sync::{Arc, Mutex, OnceLock},
};

use serde::{Deserialize, Serialize, de::DeserializeOwned};

use super::snapshot::Snapshot;

pub(crate) type Cache<T> = OnceLock<Mutex<Option<(Snapshot, Arc<T>)>>>;

#[derive(Serialize, Deserialize)]
struct Saved<T> {
    snapshot: Snapshot,
    index: T,
}

/// Reuse existing evidence without starting or waiting for a full discovery scan.
pub(crate) fn cached_only<T: Serialize + DeserializeOwned>(
    packages: &Path,
    directory: &str,
    version: &str,
    memory: &Cache<T>,
) -> Result<Option<Arc<T>>, String> {
    let Ok(mut memory) = memory.get_or_init(|| Mutex::new(None)).try_lock() else {
        return Ok(None);
    };
    let snapshot = Snapshot::read(packages)?;
    if let Some((stored, index)) = memory.as_ref()
        && *stored == snapshot
    {
        return Ok(Some(Arc::clone(index)));
    }
    let Some(root) = crate::paths::cache_dir() else {
        return Ok(None);
    };
    let path = root
        .join(directory)
        .join(format!("{version}-{}.json", snapshot.key()?));
    let saved = std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Saved<T>>(&bytes).ok())
        .filter(|saved| saved.snapshot == snapshot);
    let Some(saved) = saved else {
        return Ok(None);
    };
    if Snapshot::read(packages)? != snapshot {
        return Ok(None);
    }
    let index = Arc::new(saved.index);
    *memory = Some((snapshot, Arc::clone(&index)));
    Ok(Some(index))
}

/// Serialize concurrent readers so opening two editors cannot start duplicate scans.
pub(crate) fn cached<T: Serialize + DeserializeOwned>(
    packages: &Path,
    directory: &str,
    version: &str,
    memory: &Cache<T>,
    build: impl FnOnce() -> Result<T, String>,
    persist: impl FnOnce(&T) -> bool,
) -> Result<Arc<T>, String> {
    let mut memory = memory
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|error| error.to_string())?;
    let snapshot = Snapshot::read(packages)?;
    if let Some((stored, index)) = memory.as_ref()
        && *stored == snapshot
    {
        return Ok(Arc::clone(index));
    }
    let key = snapshot.key()?;
    let path = crate::paths::cache_dir()
        .map(|root| root.join(directory).join(format!("{version}-{key}.json")));
    let saved = path
        .as_deref()
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice::<Saved<T>>(&bytes).ok())
        .filter(|saved| saved.snapshot == snapshot);
    let (index, fresh) = match saved {
        Some(saved) => (saved.index, false),
        None => (build()?, true),
    };
    if Snapshot::read(packages)? != snapshot {
        return Err(
            "Packages changed while reading the native index. Retry after installation finishes."
                .into(),
        );
    }
    if fresh
        && persist(&index)
        && let Some(path) = &path
    {
        // Persistence is best effort. Compilation always resolves live package data.
        let _ = write(path, &snapshot, &index);
    }
    if Snapshot::read(packages)? != snapshot {
        return Err(
            "Packages changed while caching the native index. Retry after installation finishes."
                .into(),
        );
    }
    let index = Arc::new(index);
    *memory = Some((snapshot, Arc::clone(&index)));
    Ok(index)
}

fn write<T: Serialize>(path: &Path, snapshot: &Snapshot, index: &T) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or("The index cache has no parent directory")?;
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).map_err(|error| error.to_string())?;
    serde_json::to_writer(
        temporary.as_file_mut(),
        &Saved {
            snapshot: snapshot.clone(),
            index,
        },
    )
    .map_err(|error| error.to_string())?;
    temporary.persist(path).map_err(|error| error.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct PartialIndex {
        entries: Vec<u32>,
        errors: Vec<String>,
    }

    #[test]
    fn partial_results_survive_restart_and_package_changes_invalidate_them() {
        let packages = tempfile::tempdir().unwrap();
        let storage = tempfile::tempdir().unwrap();
        let directory = storage.path().to_str().unwrap();
        std::fs::write(packages.path().join("test.pkg"), b"original").unwrap();
        let memory = Cache::new();
        let first = cached(
            packages.path(),
            directory,
            "test",
            &memory,
            || {
                Ok(PartialIndex {
                    entries: vec![7],
                    errors: vec!["Unreadable asset".into()],
                })
            },
            |_| true,
        )
        .unwrap();
        let restarted = Cache::new();
        let next = cached(
            packages.path(),
            directory,
            "test",
            &restarted,
            || -> Result<PartialIndex, String> { panic!("must reuse disk cache") },
            |_| true,
        )
        .unwrap();
        assert_eq!(first, next);
        let locked = restarted.get().unwrap().lock().unwrap();
        assert!(
            cached_only(packages.path(), directory, "test", &restarted)
                .unwrap()
                .is_none()
        );
        drop(locked);
        std::fs::write(packages.path().join("test.pkg"), b"updated package").unwrap();
        assert!(
            cached_only(packages.path(), directory, "test", &restarted)
                .unwrap()
                .is_none()
        );
    }
}

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

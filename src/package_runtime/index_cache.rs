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
        .and_then(|bytes| super::cache_file::read::<Saved<T>>(&bytes).ok())
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
        .and_then(|bytes| super::cache_file::read::<Saved<T>>(&bytes).ok())
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
    if let Some(path) = &path {
        prune_family(path);
    }
    let index = Arc::new(index);
    *memory = Some((snapshot, Arc::clone(&index)));
    Ok(index)
}

/// Older snapshots of the same index, and older format versions of it, are dead weight
/// once the current one is on disk. Keep the newest few so switching package directories
/// does not rebuild every time. Runs on load as well as on write, so leftovers from an
/// older build go the first time the current index is used.
///
/// A file of an older format version can never be read again, whatever its snapshot, so it
/// is removed outright rather than held as one of the kept few. Only the current version
/// keeps spares, which is what makes switching between two package directories cheap.
fn prune_family(path: &Path) {
    let Some(parent) = path.parent() else {
        return;
    };
    // `{kind}-v{version}-{key}.json`, and the key carries no dash of its own.
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return;
    };
    let (Some((kind, _)), Some((version, _))) = (name.split_once("-v"), name.rsplit_once('-'))
    else {
        return;
    };
    let (family, version) = (format!("{kind}-v"), format!("{version}-"));
    for entry in std::fs::read_dir(parent).into_iter().flatten().flatten() {
        if entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(&family) && !name.starts_with(&version))
        {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    super::tft::shards::prune_siblings(parent, path, |name| name.starts_with(&version));
    super::tft::shards::sweep_stray_temporaries(parent);
}

fn write<T: Serialize>(path: &Path, snapshot: &Snapshot, index: &T) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or("The index cache has no parent directory")?;
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).map_err(|error| error.to_string())?;
    // Deflated and buffered, so a large index is a few big writes rather than one syscall
    // per token. A plain file an older build wrote is still read, so this costs no rebuild.
    super::cache_file::write(
        temporary.as_file_mut(),
        &Saved {
            snapshot: snapshot.clone(),
            index,
        },
    )?;
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

    /// A file of an older format version can never be read again, so it goes at once rather
    /// than holding one of the slots the current version keeps for switching between two
    /// package directories. A temporary file an interrupted write left behind goes too, but
    /// only once it is old enough that no write can still be holding it.
    #[test]
    fn pruning_drops_dead_versions_and_abandoned_temporaries() {
        let directory = tempfile::tempdir().unwrap();
        for name in [
            "tft-v5-aaaa.json",
            "tft-v5-bbbb.json",
            "tft-v4-cccc.json",
            "other-v5-dddd.json",
            ".tmpfresh",
            ".tmpstale",
        ] {
            std::fs::write(directory.path().join(name), b"{}").unwrap();
        }
        let stale = std::fs::File::options()
            .write(true)
            .open(directory.path().join(".tmpstale"))
            .unwrap();
        stale
            .set_modified(
                std::time::SystemTime::now() - std::time::Duration::from_secs(60 * 60 * 24),
            )
            .unwrap();
        drop(stale);

        prune_family(&directory.path().join("tft-v5-aaaa.json"));

        let present = |name: &str| directory.path().join(name).exists();
        assert!(present("tft-v5-aaaa.json"), "the current index went");
        assert!(present("tft-v5-bbbb.json"), "the spare snapshot went");
        assert!(present("other-v5-dddd.json"), "another index family went");
        assert!(present(".tmpfresh"), "a write in flight was removed");
        assert!(
            !present("tft-v4-cccc.json"),
            "the dead format version stayed"
        );
        assert!(!present(".tmpstale"), "an abandoned temporary stayed");
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

    #[test]
    fn writing_a_new_snapshot_prunes_older_files_of_the_same_index() {
        let packages = tempfile::tempdir().unwrap();
        let storage = tempfile::tempdir().unwrap();
        let directory = storage.path().to_str().unwrap();
        for (round, contents) in [b"one".as_slice(), b"two", b"three"]
            .into_iter()
            .enumerate()
        {
            std::fs::write(packages.path().join("test.pkg"), contents).unwrap();
            let memory = Cache::new();
            cached(
                packages.path(),
                directory,
                "sample-v2",
                &memory,
                || {
                    Ok(PartialIndex {
                        entries: vec![round as u32],
                        errors: Vec::new(),
                    })
                },
                |_| true,
            )
            .unwrap();
            // Make the timestamps strictly increasing on file systems with coarse clocks.
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        std::fs::write(storage.path().join("other-v1-key.json"), b"{}").unwrap();
        // A leftover from an older build goes the next time the current index is loaded.
        std::fs::write(storage.path().join("sample-v1-stale.json"), b"{}").unwrap();
        let memory = Cache::new();
        cached(
            packages.path(),
            directory,
            "sample-v2",
            &memory,
            || -> Result<PartialIndex, String> { panic!("must reuse disk cache") },
            |_| true,
        )
        .unwrap();
        let mut names = std::fs::read_dir(storage.path())
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        names.sort();
        assert_eq!(
            names
                .iter()
                .filter(|name| name.starts_with("sample-v"))
                .count(),
            2,
            "{names:?}"
        );
        assert!(names.contains(&"other-v1-key.json".to_owned()));
    }
}

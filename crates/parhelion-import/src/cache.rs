//! Best-effort disk cache publication and generation retention shared by import workflows.
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use fs2::FileExt;
use serde::Serialize;

const LEGACY_GRACE: Duration = Duration::from_secs(24 * 60 * 60);

/// A shared lease keeps a cache generation alive while another worker prunes its family.
pub struct Generation {
    path: PathBuf,
    _lease: File,
}

fn open_lock(path: &Path) -> std::io::Result<File> {
    OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
}

fn valid_key(key: &str) -> bool {
    let stamp = if let Some((schema, stamp)) = key.split_once('-') {
        if schema.is_empty() || !schema.bytes().all(|byte| byte.is_ascii_digit()) {
            return false;
        }
        stamp
    } else {
        key
    };
    stamp.len() == 64 && stamp.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn ordinary(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return false;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return false;
        }
    }
    true
}

struct Candidate {
    path: PathBuf,
    key: String,
    used: SystemTime,
    leased: bool,
}

fn candidate(entry: fs::DirEntry, directories: bool) -> Option<Candidate> {
    let path = entry.path();
    let metadata = fs::symlink_metadata(&path).ok()?;
    if !ordinary(&metadata)
        || (if directories {
            !metadata.is_dir()
        } else {
            !metadata.is_file()
        })
    {
        return None;
    }
    let name = entry.file_name();
    let name = name.to_str()?;
    let key = if directories {
        name
    } else {
        name.strip_suffix(".json")?
    };
    if !valid_key(key) {
        return None;
    }
    let lease = path.parent()?.join(format!(".{key}.lease"));
    let leased = lease.is_file();
    let used = fs::metadata(lease)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .or_else(|| metadata.modified().ok())?;
    Some(Candidate {
        path,
        key: key.to_owned(),
        used,
        leased,
    })
}

fn remove(root: &Path, entry: Candidate, directories: bool) {
    if !entry.leased
        && SystemTime::now()
            .duration_since(entry.used)
            .unwrap_or_default()
            < LEGACY_GRACE
    {
        return;
    }
    let lease_path = root.join(format!(".{}.lease", entry.key));
    let Ok(lease) = open_lock(&lease_path) else {
        return;
    };
    if FileExt::try_lock_exclusive(&lease).is_err() {
        return;
    }
    // Older contract writers hold this lock without a lease. Respect them too.
    let writer = if directories {
        None
    } else {
        open_lock(&entry.path.with_extension("lock")).ok()
    };
    if !directories
        && writer
            .as_ref()
            .is_none_or(|file| FileExt::try_lock_exclusive(file).is_err())
    {
        return;
    }
    // Recheck after acquiring ownership. Never follow a reparse point during cleanup.
    let removable = fs::symlink_metadata(&entry.path).is_ok_and(|metadata| ordinary(&metadata));
    if removable {
        let removed = if directories {
            fs::remove_dir_all(&entry.path)
        } else {
            fs::remove_file(&entry.path)
        };
        if removed.is_ok() {
            drop(lease);
            let _ = fs::remove_file(lease_path);
        }
    }
}

fn prune(root: &Path, current: &Path, directories: bool) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    let mut entries = entries
        .flatten()
        .filter_map(|entry| candidate(entry, directories))
        .filter(|entry| entry.path != current)
        .collect::<Vec<_>>();
    entries.sort_by(|a, b| b.used.cmp(&a.used).then_with(|| b.key.cmp(&a.key)));
    // Keep the current generation and the most recently used spare. Active older ones
    // survive their exclusive-lock probe and can be reclaimed on a later visit.
    for entry in entries.into_iter().skip(1) {
        remove(root, entry, directories);
    }
}

impl Generation {
    /// Lease a directory generation. Failure disables reuse rather than discovery.
    pub fn directory(root: &Path, key: &str) -> Option<Self> {
        Self::open(root, key, true)
    }

    /// Lease one JSON generation, optionally prefixed with its numeric schema.
    pub fn file(root: &Path, key: &str) -> Option<Self> {
        Self::open(root, key, false)
    }

    fn open(root: &Path, key: &str, directories: bool) -> Option<Self> {
        if !valid_key(key) || crate::cancellation::check().is_err() {
            return None;
        }
        fs::create_dir_all(root).ok()?;
        if !ordinary(&fs::symlink_metadata(root).ok()?) {
            return None;
        }
        let family = open_lock(&root.join(".retention.lock")).ok()?;
        FileExt::try_lock_exclusive(&family).ok()?;
        let path = root.join(if directories {
            key.to_owned()
        } else {
            format!("{key}.json")
        });
        if let Ok(metadata) = fs::symlink_metadata(&path)
            && !ordinary(&metadata)
        {
            return None;
        }
        if directories {
            fs::create_dir_all(&path).ok()?;
        }
        let lease = open_lock(&root.join(format!(".{key}.lease"))).ok()?;
        FileExt::try_lock_shared(&lease).ok()?;
        let _ = lease.set_modified(SystemTime::now());
        prune(root, &path, directories);
        Some(Self {
            path,
            _lease: lease,
        })
    }

    /// The location protected for the lifetime of this lease.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Serialize incremental writers. IO failure disables this cache operation.
    pub(crate) fn writer(&self) -> anyhow::Result<Option<File>> {
        let Ok(file) = open_lock(&self.path.with_extension("lock")) else {
            return Ok(None);
        };
        match crate::cancellation::lock(&file) {
            Ok(()) => Ok(Some(file)),
            Err(error) if crate::cancellation::is_cancelled(&error) => Err(error),
            Err(_) => Ok(None),
        }
    }
}

/// Publish complete JSON atomically. Callers treat ordinary IO failure as a cache miss.
pub fn write_json<T: Serialize>(path: &Path, value: &T) -> anyhow::Result<()> {
    crate::cancellation::check()?;
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Cache has no directory"))?;
    fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    {
        let mut writer = std::io::BufWriter::new(temporary.as_file_mut());
        serde_json::to_writer(&mut writer, value)?;
        writer.flush()?;
    }
    crate::cancellation::check()?;
    temporary.persist(path)?;
    Ok(())
}

#[cfg(test)]
mod tests;

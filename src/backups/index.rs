//! Ownership metadata for readable backup files kept directly in the backup folder.
use super::*;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, io::Write};

#[derive(Serialize, Deserialize)]
pub(super) struct Record {
    pub source: String,
    pub sha256: String,
    pub automatic: bool,
    #[serde(flatten)]
    extra: BTreeMap<String, serde_json::Value>,
}

#[derive(Serialize, Deserialize)]
struct Index {
    version: u32,
    files: BTreeMap<String, Record>,
    #[serde(flatten)]
    extra: BTreeMap<String, serde_json::Value>,
}

pub(super) struct Store {
    pub root: PathBuf,
    path: PathBuf,
    original: Option<Vec<u8>>,
    index: Index,
    _lock: fs::File,
}

impl Store {
    pub fn open(root: &Path) -> Result<Self, String> {
        fs::create_dir_all(root).map_err(|error| error.to_string())?;
        let root = paths::resolve_path_for_comparison(root).map_err(|error| error.to_string())?;
        let lock_path = checked_child(&root, ".backup-index.lock")?;
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path)
            .map_err(|error| format!("Could not open the backup index lock: {error}"))?;
        fs2::FileExt::try_lock_exclusive(&lock)
            .map_err(|error| format!("Another backup operation is using this folder: {error}"))?;
        let path = checked_child(&root, ".backup-index.json")?;
        let original = match fs::read(&path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(format!("Could not read the backup index: {error}")),
        };
        let index = original.as_deref().map_or_else(
            || {
                Ok(Index {
                    version: 1,
                    files: BTreeMap::new(),
                    extra: BTreeMap::new(),
                })
            },
            |bytes| {
                serde_json::from_slice(bytes)
                    .map_err(|error| format!("Could not read the backup index: {error}"))
            },
        )?;
        if index.version != 1 {
            return Err(
                "This backup index was written by an unsupported version of Sundial".into(),
            );
        }
        Ok(Self {
            root,
            path,
            original,
            index,
            _lock: lock,
        })
    }

    pub fn records(&self) -> &BTreeMap<String, Record> {
        &self.index.files
    }

    pub fn remove(&mut self, name: &str) {
        self.index.files.remove(name);
    }

    pub fn save(&mut self) -> Result<(), String> {
        checked_child(&self.root, ".backup-index.json")?;
        let bytes = serde_json::to_vec_pretty(&self.index).map_err(|error| error.to_string())?;
        if let Some(original) = &self.original {
            crate::storage::replace_file_if_unchanged(&self.path, &bytes, original)
                .map_err(|error| format!("Could not update the backup index: {error}"))?;
        } else {
            let mut file = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&self.path)
                .map_err(|error| format!("Could not create the backup index: {error}"))?;
            file.write_all(&bytes)
                .and_then(|()| file.sync_all())
                .map_err(|error| format!("Could not write the backup index: {error}"))?;
        }
        self.original = Some(bytes);
        Ok(())
    }
}

pub(super) fn source_identity(source: &Path) -> Result<String, String> {
    let path = paths::resolve_path_for_comparison(source).map_err(|error| error.to_string())?;
    let identity = path
        .to_str()
        .ok_or("The backup source path is not valid Unicode")?;
    #[cfg(windows)]
    return Ok(identity.to_lowercase());
    #[cfg(not(windows))]
    Ok(identity.to_owned())
}

pub(super) fn checked_child(root: &Path, name: &str) -> Result<PathBuf, String> {
    if name.is_empty() || name.contains(['/', '\\', ':']) || matches!(name, "." | "..") {
        return Err("The backup index contains an invalid filename".into());
    }
    let path = root.join(name);
    if fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(format!("The backup file is redirected: {}", path.display()));
    }
    let resolved = paths::resolve_path_for_comparison(&path).map_err(|error| error.to_string())?;
    if !paths::paths_equal(&path, &resolved) {
        return Err(format!("The backup file is redirected: {}", path.display()));
    }
    Ok(path)
}

pub(super) fn timestamp(now: time::OffsetDateTime) -> String {
    format!(
        "{:04}-{:02}-{:02}_{:02}-{:02}-{:02}Z",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    )
}

pub(crate) fn create(
    root: &Path,
    source: &Path,
    prefix: &str,
    extension: &str,
    automatic: bool,
    write: impl FnOnce(&Path, &mut fs::File) -> Result<(), String>,
) -> Result<PathBuf, String> {
    let mut store = Store::open(root)?;
    let source = source_identity(source)?;
    let stamp = timestamp(time::OffsetDateTime::now_utc());
    let mut destination = None;
    for attempt in 0..1000 {
        let suffix = if attempt == 0 {
            String::new()
        } else {
            format!("-{}", attempt + 1)
        };
        let name = format!("{prefix}-{stamp}{suffix}.{extension}");
        if store.records().contains_key(&name) {
            continue;
        }
        let path = checked_child(&store.root, &name)?;
        match fs::OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&path)
        {
            Ok(file) => {
                destination = Some((name, path, file));
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(format!("Could not create a backup: {error}")),
        }
    }
    let (name, path, mut file) = destination.ok_or("Could not choose a unique backup filename")?;
    let result =
        write(&path, &mut file).and_then(|()| file.sync_all().map_err(|error| error.to_string()));
    drop(file);
    if let Err(error) = result {
        return match crate::storage::remove_file_if_present(&path) {
            Ok(()) => Err(error),
            Err(cleanup) => Err(format!(
                "{error}. The incomplete backup could not be removed: {cleanup}"
            )),
        };
    }
    checked_child(&store.root, &name)?;
    let bytes = fs::read(&path).map_err(|error| error.to_string())?;
    store.index.files.insert(
        name,
        Record {
            source,
            sha256: format!("{:x}", Sha256::digest(&bytes)),
            automatic,
            extra: BTreeMap::new(),
        },
    );
    // A complete but unindexed backup is preserved if metadata writing fails.
    store.save()?;
    Ok(path)
}

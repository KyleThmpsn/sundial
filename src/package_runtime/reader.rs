//! Bounded package readers shared by discovery, inspectors and authoring.
//! The upstream manager retains every package and patch handle it has ever read.
//! Use it only for metadata and keep payload readers in a process-wide bounded pool.
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
use tiger_pkg::{GameVersion, Package, PackagePlatform, TagHash, TagHash64, Version};
mod pool;

static NEXT_READER: AtomicU64 = AtomicU64::new(1);
// Reserve room for GUI, database, cache and metadata-index handles even on low-limit systems.
static READERS: pool::Pool<Mutex<Arc<dyn Package>>> = pool::Pool::new(128);

pub struct PackageManager {
    pub package_dir: PathBuf,
    pub package_paths: HashMap<u16, tiger_pkg::manager::PackagePath>,
    pub lookup: tiger_pkg::manager::TagLookupIndex,
    pub version: GameVersion,
    pub platform: PackagePlatform,
    identity: u64,
}
impl PackageManager {
    pub fn new(
        path: impl AsRef<Path>,
        version: GameVersion,
        platform: Option<PackagePlatform>,
    ) -> Result<Self, String> {
        let metadata = tiger_pkg::PackageManager::new(path, version, platform)
            .map_err(|e| format!("{e:#}"))?;
        Ok(Self {
            package_dir: metadata.package_dir,
            package_paths: metadata.package_paths.into_iter().collect(),
            lookup: metadata.lookup,
            version: metadata.version,
            platform: metadata.platform,
            identity: NEXT_READER.fetch_add(1, Ordering::Relaxed),
        })
    }
    pub fn read_tag(&self, tag: impl Into<TagHash>) -> Result<Vec<u8>, String> {
        let tag = tag.into();
        let path = self
            .package_paths
            .get(&tag.pkg_id())
            .ok_or_else(|| format!("No package path for 0x{:04X}", tag.pkg_id()))?;
        // The latest file plus all earlier patch files that this reader might retain.
        let handles = usize::from(path.patch) + 1;
        let lease = READERS.acquire((self.identity, tag.pkg_id()), handles, || {
            self.version
                .open(&path.path)
                .map(Mutex::new)
                .map_err(|e| format!("Could not open {}: {e:#}", path.filename))
        })?;
        let reader = lease
            .value()
            .lock()
            .map_err(|_| "The package reader stopped unexpectedly".to_owned())?;
        // Upstream seeks and reads through separate lock acquisitions. Serialize each
        // package's payload reads, while different packages still run concurrently.
        reader.read_entry(tag.entry_index() as usize).map_err(|e| {
            format!(
                "Could not read 0x{:08X} from {}: {e:#}",
                tag.0, path.filename
            )
        })
    }
    pub fn read_tag64(&self, hash: impl Into<TagHash64>) -> Result<Vec<u8>, String> {
        let hash = hash.into();
        let tag = self
            .lookup
            .tag64_entries
            .get(&hash.0)
            .ok_or_else(|| format!("Hash 0x{:016X} was not found", hash.0))?
            .hash32;
        self.read_tag(tag)
    }
}
impl Drop for PackageManager {
    fn drop(&mut self) {
        READERS.remove_owner(self.identity);
    }
}

impl PackageManager {
    pub fn get_entry(&self, tag: impl Into<TagHash>) -> Option<tiger_pkg::package::UEntryHeader> {
        let tag = tag.into();
        self.lookup
            .tag32_entries_by_pkg
            .get(&tag.pkg_id())?
            .get(tag.entry_index() as usize)
            .cloned()
    }
    pub fn get_tag_name(&self, tag: impl Into<TagHash>) -> Option<String> {
        let tag = tag.into();
        self.lookup
            .named_tags
            .iter()
            .find(|entry| entry.hash == tag)
            .map(|entry| entry.name.clone())
    }
    pub fn get_all_by_reference(
        &self,
        reference: u32,
    ) -> Vec<(TagHash, tiger_pkg::package::UEntryHeader)> {
        self.matching_entries(|entry| entry.reference == reference)
    }
    pub fn get_all_by_type(
        &self,
        file_type: u8,
        subtype: Option<u8>,
    ) -> Vec<(TagHash, tiger_pkg::package::UEntryHeader)> {
        self.matching_entries(|entry| {
            entry.file_type == file_type
                && subtype.is_none_or(|subtype| entry.file_subtype == subtype)
        })
    }
    fn matching_entries(
        &self,
        matches: impl Fn(&tiger_pkg::package::UEntryHeader) -> bool,
    ) -> Vec<(TagHash, tiger_pkg::package::UEntryHeader)> {
        self.lookup
            .tag32_entries_by_pkg
            .iter()
            .flat_map(|(&package, entries)| {
                entries
                    .iter()
                    .enumerate()
                    .filter(|(_, entry)| matches(entry))
                    .map(move |(index, entry)| (TagHash::new(package, index as u16), entry.clone()))
            })
            .collect()
    }
}

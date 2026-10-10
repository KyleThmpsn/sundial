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
use tiger_pkg::{GameVersion, Package, PackagePlatform, TagHash, Version};
mod pool;
mod trace;

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
    trace: Mutex<Option<trace::Reads>>,
    local: HashMap<u16, Vec<Arc<[u8]>>>,
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
            trace: Mutex::new(None),
            local: HashMap::new(),
        })
    }
    pub fn read_tag(&self, tag: impl Into<TagHash>) -> Result<Vec<u8>, String> {
        let tag = tag.into();
        let result = self.read_payload(tag);
        if let Ok(mut trace) = self.trace.lock()
            && let Some(trace) = trace.as_mut()
        {
            trace.record(tag.0, &result);
        }
        result
    }

    fn read_payload(&self, tag: TagHash) -> Result<Vec<u8>, String> {
        if let Some(entries) = self.local.get(&tag.pkg_id()) {
            return entries
                .get(usize::from(tag.entry_index()))
                .map(|data| data.to_vec())
                .ok_or_else(|| format!("Local preview entry 0x{:08X} is missing", tag.0));
        }
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
                .map_err(|e| describe_open_failure(&path.filename, &format!("{e:#}")))
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
}
impl Drop for PackageManager {
    fn drop(&mut self) {
        READERS.remove_owner(self.identity);
    }
}

impl PackageManager {
    /// Add read-only preview payloads under an unused package identity. Existing package
    /// entries cannot be replaced, and no package files or shared readers are changed.
    pub fn add_local_package(
        &mut self,
        package: u16,
        entries: Vec<(tiger_pkg::package::UEntryHeader, Vec<u8>)>,
    ) -> Result<(), String> {
        if package > 0x3ff
            || entries.is_empty()
            || entries.len() > 8192
            || self.package_paths.contains_key(&package)
            || self.lookup.tag32_entries_by_pkg.contains_key(&package)
        {
            return Err("Local preview package identity is unavailable".into());
        }
        let mut headers = Vec::with_capacity(entries.len());
        let mut payloads = Vec::with_capacity(entries.len());
        for (mut header, payload) in entries {
            header.file_size = u32::try_from(payload.len())
                .map_err(|_| "Local preview payload exceeds the package size limit")?;
            header.starting_block = 0;
            header.starting_block_offset = 0;
            headers.push(header);
            payloads.push(Arc::from(payload));
        }
        self.lookup.tag32_entries_by_pkg.insert(package, headers);
        self.local.insert(package, payloads);
        Ok(())
    }

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

/// Names the open-file limit when that is what stopped a package from opening.
///
/// The pool bounds Sundial's own readers, but the limit is shared with everything else the
/// process has open, so it can still run out. The raw message says only "os error 24", which
/// reads like a corrupt package rather than a setting the reader can change.
fn describe_open_failure(file_name: &str, error: &str) -> String {
    let exhausted =
        error.contains("os error 24") || error.to_ascii_lowercase().contains("too many open files");
    if exhausted {
        return format!(
            "Could not open {file_name}: {error}. The open-file limit is too low for this install. \
             Raise it with `ulimit -n 8192` before starting Sundial."
        );
    }
    format!("Could not open {file_name}: {error}")
}

#[cfg(test)]
mod tests {
    use super::describe_open_failure;

    /// The remedy is only useful where it applies, and misapplied it would send someone chasing
    /// a limit while the real fault is the package.
    #[test]
    fn only_a_descriptor_failure_mentions_the_limit() {
        let exhausted = describe_open_failure(
            "w64_globals_01a3_0.pkg",
            "Too many open files (os error 24)",
        );
        assert!(exhausted.contains("ulimit -n 8192"));
        assert!(exhausted.contains("w64_globals_01a3_0.pkg"));

        let corrupt = describe_open_failure("w64_globals_01a3_0.pkg", "invalid package header");
        assert!(!corrupt.contains("ulimit"));
        assert!(corrupt.contains("invalid package header"));
    }
}

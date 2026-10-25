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
mod decoder;
mod index;
mod pool;
mod trace;
pub use decoder::initialize_source_decoder;

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
    // A provider keeps its own inherited block chain and bounded-reader identity. Routing only
    // changed entries preserves disjoint patches that otherwise share the same package filename.
    routes: HashMap<TagHash, usize>,
    overlays: Vec<Self>,
    /// Entries the loader rewrites as it serves them: rows several mods add, merged, and then
    /// configured values.
    rewritten: HashMap<TagHash, Arc<[u8]>>,
    view: Option<Arc<super::mods::View>>,
}
impl PackageManager {
    pub fn new(
        path: impl AsRef<Path>,
        version: GameVersion,
        platform: Option<PackagePlatform>,
    ) -> Result<Self, String> {
        let tables = index::read(path.as_ref(), version, platform)?;
        Ok(Self {
            package_dir: tables.package_dir,
            package_paths: tables.package_paths,
            lookup: tables.lookup,
            version,
            platform: tables.platform,
            identity: NEXT_READER.fetch_add(1, Ordering::Relaxed),
            trace: Mutex::new(None),
            local: HashMap::new(),
            routes: HashMap::new(),
            overlays: Vec::new(),
            rewritten: HashMap::new(),
            view: None,
        })
    }
    pub(super) fn retain_view(&mut self, view: Arc<super::mods::View>) -> Result<(), String> {
        for layer in view.layers() {
            let overlay = Self::new(&layer.packages, self.version, Some(self.platform))?;
            for &tag in &layer.tags {
                let header = overlay
                    .get_entry(tag)
                    .ok_or_else(|| format!("Mod entry 0x{:08X} has no metadata", tag.0))?;
                let target = self
                    .lookup
                    .tag32_entries_by_pkg
                    .get_mut(&tag.pkg_id())
                    .and_then(|entries| entries.get_mut(usize::from(tag.entry_index())))
                    .ok_or("Mod entry has no stock metadata")?;
                *target = header;
                self.routes.insert(tag, self.overlays.len());
            }
            // Where two mods name a 64-bit tag differently, the layer read last is kept. Which
            // one the client keeps is unverified.
            for (&hash, value) in &overlay.lookup.tag64_entries {
                self.lookup.tag64_entries.insert(hash, value.clone());
            }
            for (&tag, &hash) in &overlay.lookup.tag32_to_tag64 {
                self.lookup.tag32_to_tag64.insert(tag, hash);
            }
            for name in &overlay.lookup.named_tags {
                if !self.lookup.named_tags.iter().any(|n| {
                    n.hash == name.hash && n.class_hash == name.class_hash && n.name == name.name
                }) {
                    self.lookup.named_tags.push(name.clone());
                }
            }
            self.overlays.push(overlay);
        }
        for (tag, bytes) in view.merged() {
            let header = self
                .lookup
                .tag32_entries_by_pkg
                .get_mut(&tag.pkg_id())
                .and_then(|entries| entries.get_mut(usize::from(tag.entry_index())))
                .ok_or_else(|| format!("Merged entry 0x{:08X} has no metadata", tag.0))?;
            header.file_size = u32::try_from(bytes.len())
                .map_err(|_| format!("Merged entry 0x{:08X} is too large", tag.0))?;
            self.rewritten.insert(*tag, Arc::clone(bytes));
        }
        self.view = Some(view);
        Ok(())
    }
    /// Reads `values` in place of the entries mods bind configured values to.
    pub(super) fn configure(&mut self, values: Vec<(TagHash, Vec<u8>)>) {
        self.rewritten.extend(
            values
                .into_iter()
                .map(|(tag, bytes)| (tag, Arc::from(bytes))),
        );
    }
    /// The installation's packages directory this reader was opened for. With a Sunrise mod in
    /// place, `package_dir` is a temporary view of it with the resolved mod content added.
    pub fn source_dir(&self) -> &Path {
        self.view
            .as_ref()
            .map_or(self.package_dir.as_path(), |view| view.source())
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
        if let Some(bytes) = self.rewritten.get(&tag) {
            return Ok(bytes.to_vec());
        }
        if let Some(&index) = self.routes.get(&tag) {
            return self.overlays[index].read_payload(tag);
        }
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

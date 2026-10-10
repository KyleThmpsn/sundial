//! Persistent runtime fragments keyed by inputs, with checked relocation when allocations move.
//! Unsupported relocation and cache failures fall back to authoring without changing live state.
use super::*;
mod diagnostics;
mod fragment;
mod relocate;
#[cfg(all(test, feature = "d2-model-importer"))]
mod tests;
use crate::asset_packages::AssetPackage;
pub(super) use diagnostics::{Reason, Timings};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::cell::RefCell;
use std::{
    fs::File,
    io::{Read, Write},
    sync::OnceLock,
};

pub(super) type AnimationCache = BTreeMap<(u32, [u8; 32]), u32>;
type Hash = [u8; 32];
const LIMIT: u64 = 1024 * 1024 * 1024;
const ENTRY_LIMIT: u64 = 512 * 1024 * 1024;

pub(super) struct Session {
    root: PathBuf,
    base: Sha256,
    _lock: File,
    verified: RefCell<BTreeMap<u32, Hash>>,
    identity: Identity,
}

pub(super) struct Pending<'a> {
    key: Hash,
    state: Hash,
    path: PathBuf,
    allocator: AppendedTagAllocator,
    host_start: usize,
    asset_starts: Vec<usize>,
    group_start: usize,
    placed_start: usize,
    impact_start: usize,
    animation: AnimationCache,
    assignments: Vec<u8>,
    verified: &'a RefCell<BTreeMap<u32, Hash>>,
    identity: Option<Identity>,
    timings: RefCell<Timings>,
    /// Whether the work returns a tag, as an ability's copy does, which the fragment keeps.
    returns: bool,
    #[cfg(feature = "d2-model-importer")]
    import_root: Option<PathBuf>,
}

/// What a replayed fragment restores: the pattern its weapon's assignment row names, and the tag
/// its work returned, such as an ability's copy, where the replay placed it.
pub(super) struct Restored {
    pub pattern: Option<u32>,
    pub result: Option<TagHash>,
}

#[derive(Serialize, Deserialize)]
struct Tag {
    assigned: u32,
    template: u32,
    storage: u8,
    span: [usize; 2],
    /// Every declared tag field. An unsupported layout keeps exact-allocation reuse only.
    fields: Option<Vec<(usize, u32)>>,
}

#[derive(Serialize, Deserialize)]
struct Group {
    reservation: crate::asset_packages::Reservation,
    tags: Vec<Tag>,
    references: Vec<(usize, Option<usize>)>,
}

#[derive(Serialize, Deserialize)]
struct Entry {
    #[serde(default)]
    identity: Option<Identity>,
    key: Hash,
    state: Hash,
    payload_hash: Hash,
    reads: BTreeMap<u32, Hash>,
    assignment: Option<(u32, u32)>,
    host: Vec<Tag>,
    groups: Vec<Group>,
    placed: Vec<u32>,
    animation: Vec<((u32, Hash), u32)>,
    dependencies: Vec<((u32, Hash), u32)>,
    relocatable: bool,
    impacts: Vec<crate::ability::banks::MeleeImpact>,
    /// The tag the work returned, such as an ability's copy.
    #[serde(default)]
    result: Option<u32>,
    #[cfg(feature = "d2-model-importer")]
    inputs: custom_runtime::imports::Fingerprints,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
struct Identity {
    compiler: Hash,
    source: Hash,
}

fn hash(bytes: &[u8]) -> Hash {
    Sha256::digest(bytes).into()
}

fn field(digest: &mut Sha256, bytes: &[u8]) {
    digest.update((bytes.len() as u64).to_le_bytes());
    digest.update(bytes);
}

fn file_hash(path: &Path) -> Option<Hash> {
    let mut file = File::open(path).ok()?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0; 1024 * 1024];
    loop {
        let count = file.read(&mut buffer).ok()?;
        if count == 0 {
            return Some(digest.finalize().into());
        }
        digest.update(&buffer[..count]);
    }
}

impl Session {
    pub(super) fn open(
        manager: &PackageManager,
        stock: &EntitySources<'_>,
    ) -> Result<Self, Reason> {
        if std::env::var_os("PARHELION_DISABLE_RUNTIME_CACHE").is_some() {
            return Err(Reason::Disabled);
        }
        let root = std::env::var_os("PARHELION_RUNTIME_CACHE_DIRECTORY")
            .map(PathBuf::from)
            .or_else(|| {
                sundial::package_authoring::parhelion_data_directory()
                    .map(|p| p.join("cache/runtime-v2"))
            })
            .ok_or(Reason::NoDirectory)?;
        fs::create_dir_all(&root).map_err(|error| Reason::Directory(error.to_string()))?;
        let lock = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join(".lock"))
            .map_err(|error| Reason::Directory(error.to_string()))?;
        // A second concurrent compiler simply works without caching.
        lock.try_lock_exclusive()
            .map_err(|error| Reason::Locked(error.to_string()))?;
        static COMPILER: OnceLock<Option<Hash>> = OnceLock::new();
        let compiler = COMPILER.get_or_init(|| file_hash(&std::env::current_exe().ok()?));
        let compiler = compiler.as_ref().ok_or(Reason::CompilerUnavailable)?;
        let mut base = Sha256::new();
        field(&mut base, b"parhelion-runtime-cache-v2");
        field(&mut base, compiler);
        field(&mut base, stock.sandbox_patterns);
        field(&mut base, stock.entity_assignments);
        // Metadata queries can include class-wide searches, so include the complete index.
        let entries = manager
            .lookup
            .tag32_entries_by_pkg
            .iter()
            .collect::<BTreeMap<_, _>>();
        for (id, entries) in entries {
            field(&mut base, &id.to_le_bytes());
            field(&mut base, format!("{entries:?}").as_bytes());
        }
        let tags64 = manager
            .lookup
            .tag64_entries
            .iter()
            .collect::<BTreeMap<_, _>>();
        for (id, entry) in tags64 {
            field(&mut base, &id.to_le_bytes());
            field(&mut base, &entry.hash32.0.to_le_bytes());
            field(&mut base, &entry.reference.0.to_le_bytes());
        }
        field(
            &mut base,
            format!("{:?}/{:?}", manager.version, manager.platform).as_bytes(),
        );
        let identity = Identity {
            compiler: *compiler,
            source: base.clone().finalize().into(),
        };
        Ok(Self {
            root,
            base,
            _lock: lock,
            verified: RefCell::new(BTreeMap::new()),
            identity,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn prepare(
        &self,
        donor: &resolve::ResolvedWeapon,
        allocator: AppendedTagAllocator,
        host: &[NewTagSpec],
        assets: &RuntimeAssets<'_>,
        assignments: &[u8],
        animation: &AnimationCache,
    ) -> Pending<'_> {
        let mut key = self.base.clone();
        field(&mut key, format!("{:?}", donor.weapon).as_bytes());
        field(
            &mut key,
            format!(
                "{:?}/{:?}/{:?}/{:?}",
                donor.runtime_pattern_source,
                donor.gear_art_pattern_source,
                donor.appearance_rig_donor,
                donor.component_splice_sources
            )
            .as_bytes(),
        );
        for component in &donor.runtime_component_donors {
            field(&mut key, &component.binding_hash.to_le_bytes());
            field(&mut key, &component.pattern_item_hash.to_le_bytes());
        }
        self.pending(
            key.finalize().into(),
            donor.weapon.namespace.as_bytes(),
            (allocator, host),
            assets,
            (assignments, animation),
            #[cfg(feature = "d2-model-importer")]
            donor
                .weapon
                .overrides
                .imported_graph
                .as_ref()
                .map(|graph| graph.directory.clone()),
        )
    }

    /// A pending fragment for the private copy of an ability entity, saved under `slot`, one for
    /// each authored ability, and keyed by `inputs`, everything the copy is made from besides the
    /// native payloads it reads.
    pub(super) fn prepare_copy(
        &self,
        (slot, inputs): (&str, &str),
        (allocator, host): (AppendedTagAllocator, &[NewTagSpec]),
        assets: &RuntimeAssets<'_>,
    ) -> Pending<'_> {
        let mut key = self.base.clone();
        field(&mut key, b"ability-copy");
        field(&mut key, inputs.as_bytes());
        let mut pending = self.pending(
            key.finalize().into(),
            format!("ability-copy/{slot}").as_bytes(),
            (allocator, host),
            assets,
            (&[], &AnimationCache::new()),
            #[cfg(feature = "d2-model-importer")]
            None,
        );
        pending.returns = true;
        pending
    }

    /// A pending fragment saved under `name` for work keyed by `key`, which appends to `host`
    /// from `allocator` and to `assets`, after `assignments` and the shared `animation`.
    fn pending(
        &self,
        key: Hash,
        name: &[u8],
        (allocator, host): (AppendedTagAllocator, &[NewTagSpec]),
        assets: &RuntimeAssets<'_>,
        (assignments, animation): (&[u8], &AnimationCache),
        #[cfg(feature = "d2-model-importer")] import_root: Option<PathBuf>,
    ) -> Pending<'_> {
        let mut state = Sha256::new();
        field(
            &mut state,
            format!("{allocator:?}/{}", host.len()).as_bytes(),
        );
        field(&mut state, assignments);
        for package in &assets.packages.packages {
            field(&mut state, &package.id.to_le_bytes());
            field(&mut state, &(package.tags.len() as u64).to_le_bytes());
            for tag in &package.tags {
                field(&mut state, &(tag.payload.len() as u64).to_le_bytes());
            }
        }
        for ((class, digest), tag) in animation {
            field(&mut state, &class.to_le_bytes());
            field(&mut state, digest);
            field(&mut state, &tag.to_le_bytes());
        }
        Pending {
            key,
            state: state.finalize().into(),
            allocator,
            group_start: assets.packages.reservations.len(),
            assignments: assignments.to_vec(),
            verified: &self.verified,
            identity: Some(self.identity),
            timings: RefCell::new(Timings::default()),
            returns: false,
            #[cfg(feature = "d2-model-importer")]
            import_root,
            path: self
                .root
                .join(format!("{}.runtime", hex::encode(Sha256::digest(name)))),
            host_start: host.len(),
            asset_starts: assets
                .packages
                .packages
                .iter()
                .map(|p| p.tags.len())
                .collect(),
            placed_start: assets.placed.len(),
            impact_start: assets.impacts.len(),
            animation: animation.clone(),
        }
    }

    pub(super) fn prune(&self) {
        let Ok(entries) = fs::read_dir(&self.root) else {
            return;
        };
        let mut entries = entries
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let name = entry.file_name();
                let name = name.to_str()?;
                // The session lock excludes active writers. Remove only our abandoned temps.
                if name.starts_with(".runtime-") && name.ends_with(".tmp") {
                    if entry.file_type().ok()?.is_file() {
                        let _ = fs::remove_file(entry.path());
                    }
                    return None;
                }
                let stem = name.strip_suffix(".runtime")?;
                if stem.len() != 64 || !stem.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return None;
                }
                let metadata = entry.metadata().ok()?;
                if !entry.file_type().ok()?.is_file() {
                    return None;
                }
                Some((metadata.modified().ok()?, metadata.len(), entry.path()))
            })
            .collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.0);
        let mut size: u64 = entries.iter().map(|entry| entry.1).sum();
        for (_, length, path) in entries {
            if size <= LIMIT {
                break;
            }
            if fs::remove_file(path).is_ok() {
                size = size.saturating_sub(length)
            }
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.prune();
    }
}

fn append(bytes: &mut Vec<u8>, data: &[u8]) -> [usize; 2] {
    let span = [bytes.len(), data.len()];
    bytes.extend_from_slice(data);
    span
}

fn slice(bytes: &[u8], span: [usize; 2]) -> Option<&[u8]> {
    bytes.get(span[0]..span[0].checked_add(span[1])?)
}

fn read_member(archive: &mut zip::ZipArchive<File>, name: &str, limit: u64) -> Option<Vec<u8>> {
    let member = archive.by_name(name).ok()?;
    if member.size() > limit {
        return None;
    }
    let mut bytes = Vec::new();
    member.take(limit + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() as u64 <= limit).then_some(bytes)
}

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
};

pub(super) type AnimationCache = BTreeMap<(u32, [u8; 32]), u32>;
type Hash = [u8; 32];
/// A shared animation asset's class and payload digest.
type AnimationKey = (u32, Hash);
/// The size pruning brings the store down to by removing fragments earlier builds left unused.
/// The fragments a build reused or saved stay even past it, so a project larger than the limit
/// still finds all of them in its next build. An imported weapon's fragment can take 40 MB to
/// 96 MB, so one library of 31 recipes filled the earlier 1 GiB alone and every fragment its
/// latest build didn't use was removed. 4 GiB keeps several selections of such a library.
const LIMIT: u64 = 4 * 1024 * 1024 * 1024;
const ENTRY_LIMIT: u64 = 512 * 1024 * 1024;
/// The runtime compiler's revision. Raise it with any change that makes runtime compilation write
/// different bytes, records or diagnostics for the same recipe, native and imported inputs, so
/// every saved fragment compiles again. A rebuild that leaves it alone keeps the fragments.
/// The package version is part of the identity too, so a release never replays a fragment an
/// earlier version saved even if a raise was missed.
const REVISION: u32 = 4;

pub(super) struct Session {
    root: PathBuf,
    base: Sha256,
    /// When the build opened the store. A hit touches its fragment and a save writes one, so a
    /// fragment modified since then belongs to this build.
    opened: std::time::SystemTime,
    _lock: File,
    verified: RefCell<BTreeMap<u32, Hash>>,
    /// Each shared animation asset a replay moved, from the key its fragment stores to the key
    /// its relocated payload has in this build. A fragment names the assets it shares by their
    /// stored keys, which stay the same however often their own fragment is replayed.
    provenance: RefCell<BTreeMap<AnimationKey, AnimationKey>>,
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
    provenance: &'a RefCell<BTreeMap<AnimationKey, AnimationKey>>,
    identity: Option<Identity>,
    timings: RefCell<Timings>,
    /// Whether the work returns a tag, as an ability's copy does, which the fragment keeps.
    returns: bool,
    #[cfg(feature = "d2-model-importer")]
    import_root: Option<PathBuf>,
    /// The item whose imported particle symbols the work records, for the private perks that
    /// name them. An ability copy records none.
    #[cfg(feature = "d2-model-importer")]
    ordinal: Option<usize>,
    /// Where the work's HUD statuses start in the build's list.
    #[cfg(feature = "d2-model-importer")]
    status_start: usize,
    /// Each attachment group a private perk brings, by its directory and prepared fingerprint.
    #[cfg(feature = "d2-model-importer")]
    attachments: Vec<(PathBuf, String)>,
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
    /// Validated loading-index memberships, including native package IDs outside authoring.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    loading: Option<BTreeSet<(u16, u16)>>,
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
    /// The attachment groups private perks bring, each checked against its fingerprint again
    /// before a replay.
    #[cfg(feature = "d2-model-importer")]
    #[serde(default)]
    attachments: Vec<(PathBuf, String)>,
    /// The item's imported particle tags by node symbol, for the private perks that name them.
    #[cfg(feature = "d2-model-importer")]
    #[serde(default)]
    symbols: Option<BTreeMap<String, u32>>,
    /// The HUD statuses the work bound, whose images the attachment groups' stage authors.
    #[cfg(feature = "d2-model-importer")]
    #[serde(default)]
    statuses: Vec<sundial::package_authoring::sandbox_perk::program::HudStatus>,
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

/// The compiler's identity: its revision and the package version, not the executable's bytes, so
/// rebuilding the application for changes outside runtime compilation keeps every fragment.
fn compiler_identity() -> Hash {
    let mut digest = Sha256::new();
    field(&mut digest, b"parhelion-runtime-compiler");
    field(&mut digest, &REVISION.to_le_bytes());
    field(&mut digest, env!("CARGO_PKG_VERSION").as_bytes());
    digest.finalize().into()
}

/// An import file's digest. Each member of a family build checks the same import files in
/// every pass, so the turn keeps what it read.
#[cfg(feature = "d2-model-importer")]
fn file_hash(path: &Path) -> Option<Hash> {
    crate::turn_cache::file_digest(path, read_file_hash)
}

#[cfg(feature = "d2-model-importer")]
fn read_file_hash(path: &Path) -> Option<Hash> {
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
        let configured = std::env::var_os("PARHELION_RUNTIME_CACHE_DIRECTORY");
        let root = configured
            .clone()
            .map(PathBuf::from)
            .or_else(|| {
                sundial::package_authoring::parhelion_data_directory()
                    .map(|p| p.join("cache/runtime"))
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
        if configured.is_none()
            && let Some(cache) = root.parent()
        {
            remove_former_folders(cache);
        }
        let compiler = compiler_identity();
        let mut base = Sha256::new();
        field(&mut base, b"parhelion-runtime-cache-v2");
        field(&mut base, &compiler);
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
            compiler,
            source: base.clone().finalize().into(),
        };
        Ok(Self {
            root,
            base,
            opened: std::time::SystemTime::now(),
            _lock: lock,
            verified: RefCell::new(BTreeMap::new()),
            provenance: RefCell::new(BTreeMap::new()),
            identity,
        })
    }

    /// A pending fragment for the runtime entity of `donor`, the item at `ordinal`.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn prepare(
        &self,
        (donor, ordinal): (&resolve::ResolvedWeapon, usize),
        allocator: AppendedTagAllocator,
        host: &[NewTagSpec],
        assets: &RuntimeAssets<'_>,
        assignments: &[u8],
        animation: &AnimationCache,
    ) -> Pending<'_> {
        #[cfg(not(feature = "d2-model-importer"))]
        let _ = ordinal;
        let mut key = self.base.clone();
        field(&mut key, format!("{:?}", donor.weapon).as_bytes());
        field(
            &mut key,
            format!(
                "{:?}/{:?}/{:?}/{:?}",
                donor.runtime_pattern_source,
                donor.gear_art_pattern_source,
                donor.donors.appearance_rig,
                donor.donors.component_splices
            )
            .as_bytes(),
        );
        for component in &donor.donors.runtime_components {
            field(&mut key, &component.binding_hash.to_le_bytes());
            field(&mut key, &component.pattern_item_hash.to_le_bytes());
        }
        #[allow(unused_mut)]
        let mut pending = self.pending(
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
        );
        #[cfg(feature = "d2-model-importer")]
        {
            pending.ordinal = Some(ordinal);
            // The groups the runtime author loads for the item's private perks. The key holds
            // their fingerprints, and a replay checks each directory still has its own.
            pending.attachments = donor
                .weapon
                .overrides
                .socket_plug_variants
                .iter()
                .filter(|_| donor.runtime_pattern_source.is_some())
                .flat_map(|variant| &variant.sandbox_perks)
                .filter_map(|perk| perk.program.as_ref())
                .flat_map(
                    sundial::package_authoring::sandbox_perk::program::Program::owned_programs,
                )
                .flat_map(|program| &program.imported_assets)
                .map(|asset| (asset.directory.clone(), asset.sha256.clone()))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
        }
        pending
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
            provenance: &self.provenance,
            identity: Some(self.identity),
            timings: RefCell::new(Timings::default()),
            returns: false,
            #[cfg(feature = "d2-model-importer")]
            import_root,
            #[cfg(feature = "d2-model-importer")]
            ordinal: None,
            #[cfg(feature = "d2-model-importer")]
            status_start: assets.hud_statuses.len(),
            #[cfg(feature = "d2-model-importer")]
            attachments: Vec::new(),
            path: self.root.join(format!(
                "{}.runtime",
                hex::encode(Sha256::digest(fragment_name(name)))
            )),
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
}

/// Removes the oldest fragments until the store at `root` is at most [`LIMIT`], keeping every
/// fragment modified since `opened`. The caller holds the store's lock.
fn prune(root: &Path, opened: std::time::SystemTime) {
    let Ok(entries) = fs::read_dir(root) else {
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
    // Oldest first, so the first fragment this build touched ends pruning.
    for (modified, length, path) in entries {
        if size <= LIMIT || modified >= opened {
            break;
        }
        if fs::remove_file(path).is_ok() {
            size = size.saturating_sub(length)
        }
    }
}

/// [`prune`] once no other build holds the store, as the end of a build turn runs it.
fn prune_when_free(root: &Path, opened: std::time::SystemTime) {
    let Ok(lock) = File::options()
        .read(true)
        .write(true)
        .open(root.join(".lock"))
    else {
        return;
    };
    if lock.try_lock_exclusive().is_ok() {
        prune(root, opened);
    }
}

impl Drop for Session {
    /// A build turn opens the store for every compile, once per member and pass of a family
    /// build, so pruning waits for the turn to end and keeps every fragment any of them used.
    /// Pruning at each close kept only that compile's fragments, so once the store passed its
    /// limit an early member's close removed the fragments later members and passes were about
    /// to reuse. Outside a turn the session prunes as it closes.
    fn drop(&mut self) {
        if !crate::turn_cache::defer_prune(&self.root, self.opened, prune_when_free) {
            prune(&self.root, self.opened);
        }
    }
}

/// Removes the folders earlier builds kept fragments in, one per cache format. A fragment
/// records the cache identity, which names its format, so a change of format only misses and
/// replaces fragments in `cache/runtime`. A former folder another build still holds stays.
fn remove_former_folders(cache: &Path) {
    for name in ["runtime-v1", "runtime-v2"] {
        let folder = cache.join(name);
        match File::options()
            .read(true)
            .write(true)
            .open(folder.join(".lock"))
        {
            Ok(lock) => {
                if lock.try_lock_exclusive().is_err() {
                    continue;
                }
                // Windows cannot delete the lock file while it is open.
                drop(lock);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => continue,
        }
        let _ = fs::remove_dir_all(&folder);
    }
}

/// The name a fragment of work saved under `name` is kept by. A family build compiles an
/// importer's recipes in the importer's own mod, or in the main mod while the importer builds
/// with it, at other allocations and beside other shared animations, so each mod keeps its own
/// fragment of the recipe. With one file per recipe, the mod that compiled it last replaced the
/// other's fragment, and a family that builds its importers alone and then with the main mod
/// missed in both passes of every build. The main mod's fragments keep the names they had.
fn fragment_name(name: &[u8]) -> Vec<u8> {
    let mut name = name.to_vec();
    if let Some(member) = crate::package_profile::member() {
        name.extend_from_slice(b"\0mod/");
        name.extend_from_slice(member.source().as_bytes());
    }
    name
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

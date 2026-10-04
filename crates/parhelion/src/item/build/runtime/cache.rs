//! Persistent linked runtime fragments. Reuse requires identical inputs and allocation
//! state, including the shared animation map. Cache failures always fall back to authoring.
use super::*;
use crate::asset_packages::AssetPackage;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
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
}

pub(super) struct Pending {
    key: Hash,
    path: PathBuf,
    host_start: usize,
    asset_starts: Vec<usize>,
    placed_start: usize,
    impact_start: usize,
    animation: AnimationCache,
}

#[derive(Serialize, Deserialize)]
struct Tag {
    template: u32,
    storage: u8,
    span: [usize; 2],
}

#[derive(Serialize, Deserialize)]
struct Asset {
    id: u16,
    start: usize,
    tags: Vec<Tag>,
    /// Entry references of the restored tags: the referring ordinal, and the appended ordinal
    /// it names or `None` for the template's own reference.
    #[serde(default)]
    references: Vec<(usize, Option<usize>)>,
}

#[derive(Serialize, Deserialize)]
struct Entry {
    key: Hash,
    payload_hash: Hash,
    reads: BTreeMap<u32, Hash>,
    pattern: Option<u32>,
    assignments: [usize; 2],
    host: Vec<Tag>,
    assets: Vec<Asset>,
    placed: Vec<u32>,
    animation: Vec<((u32, Hash), u32)>,
    impacts: Vec<crate::ability::banks::MeleeImpact>,
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

// Include every file, not only the graph's listed geometry. Runtime extensions may refer
// to supplemental rig/controller payloads. Symlinks disable reuse instead of escaping.
#[cfg(feature = "d2-model-importer")]
fn graph_hash(digest: &mut Sha256, root: &Path) -> Option<()> {
    let mut entries = fs::read_dir(root)
        .ok()?
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let kind = entry.file_type().ok()?;
        field(digest, entry.file_name().to_str()?.as_bytes());
        if kind.is_file() {
            field(digest, b"file");
            field(digest, &file_hash(&entry.path())?);
        } else if kind.is_dir() {
            field(digest, b"directory");
            graph_hash(digest, &entry.path())?;
        } else {
            return None;
        }
    }
    field(digest, b"end-directory");
    Some(())
}

impl Session {
    pub(super) fn open(manager: &PackageManager, stock: &EntitySources<'_>) -> Option<Self> {
        if std::env::var_os("PARHELION_DISABLE_RUNTIME_CACHE").is_some() {
            return None;
        }
        let root = std::env::var_os("PARHELION_RUNTIME_CACHE_DIRECTORY")
            .map(PathBuf::from)
            .or_else(|| {
                sundial::package_authoring::parhelion_data_directory()
                    .map(|p| p.join("cache/runtime-v1"))
            })?;
        fs::create_dir_all(&root).ok()?;
        let lock = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join(".lock"))
            .ok()?;
        // A second concurrent compiler simply works without caching.
        lock.try_lock_exclusive().ok()?;
        static COMPILER: OnceLock<Option<Hash>> = OnceLock::new();
        let compiler = COMPILER.get_or_init(|| file_hash(&std::env::current_exe().ok()?));
        let mut base = Sha256::new();
        field(&mut base, b"parhelion-runtime-cache-v1");
        field(&mut base, compiler.as_ref()?);
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
        Some(Self {
            root,
            base,
            _lock: lock,
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
    ) -> Option<Pending> {
        let mut key = self.base.clone();
        field(&mut key, format!("{:?}", donor.weapon).as_bytes());
        field(
            &mut key,
            format!(
                "{:?}/{:?}/{:?}",
                donor.runtime_pattern_source,
                donor.gear_art_pattern_source,
                donor.appearance_rig_donor
            )
            .as_bytes(),
        );
        for component in &donor.runtime_component_donors {
            field(&mut key, &component.binding_hash.to_le_bytes());
            field(&mut key, &component.pattern_item_hash.to_le_bytes());
        }
        #[cfg(feature = "d2-model-importer")]
        if let Some(graph) = &donor.weapon.overrides.imported_graph {
            graph_hash(&mut key, &graph.directory)?;
            for attachment in &graph.attachments {
                graph_hash(&mut key, &attachment.directory)?;
            }
        }
        field(&mut key, format!("{allocator:?}/{}", host.len()).as_bytes());
        field(&mut key, assignments);
        for package in &assets.packages.packages {
            field(&mut key, &package.id.to_le_bytes());
            field(&mut key, &(package.tags.len() as u64).to_le_bytes());
            for tag in &package.tags {
                field(&mut key, &(tag.payload.len() as u64).to_le_bytes());
            }
        }
        for ((class, digest), tag) in animation {
            field(&mut key, &class.to_le_bytes());
            field(&mut key, digest);
            field(&mut key, &tag.to_le_bytes());
        }
        Some(Pending {
            key: key.finalize().into(),
            path: self.root.join(format!(
                "{:x}.runtime",
                Sha256::digest(donor.weapon.namespace.as_bytes())
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
        })
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

fn pack(tags: &[NewTagSpec], bytes: &mut Vec<u8>) -> Vec<Tag> {
    tags.iter()
        .map(|tag| Tag {
            template: tag.template_tag.0,
            storage: match tag.storage {
                crate::NewTagStorageMode::InheritTemplate => 0,
                crate::NewTagStorageMode::AudioMedia => 1,
                crate::NewTagStorageMode::AudioBank => 2,
            },
            span: append(bytes, &tag.payload),
        })
        .collect()
}

fn unpack(tags: &[Tag], bytes: &[u8]) -> Option<Vec<NewTagSpec>> {
    tags.iter()
        .map(|tag| {
            Some(NewTagSpec {
                template_tag: TagHash(tag.template),
                storage: match tag.storage {
                    0 => crate::NewTagStorageMode::InheritTemplate,
                    1 => crate::NewTagStorageMode::AudioMedia,
                    2 => crate::NewTagStorageMode::AudioBank,
                    _ => return None,
                },
                payload: slice(bytes, tag.span)?.to_vec(),
            })
        })
        .collect()
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

impl Pending {
    pub(super) fn restore(
        &self,
        manager: &PackageManager,
        host: &mut Vec<NewTagSpec>,
        assets: &mut RuntimeAssets<'_>,
        assignments: &mut Vec<u8>,
        animation: &mut AnimationCache,
    ) -> Option<Option<u32>> {
        let mut archive = zip::ZipArchive::new(File::open(&self.path).ok()?).ok()?;
        let entry: Entry =
            serde_json::from_slice(&read_member(&mut archive, "entry.json", 8 * 1024 * 1024)?)
                .ok()?;
        if entry.key != self.key {
            return None;
        }
        for (tag, expected) in &entry.reads {
            if hash(&manager.read_tag(TagHash(*tag)).ok()?) != *expected {
                return None;
            }
        }
        let bytes = read_member(&mut archive, "payload.bin", ENTRY_LIMIT)?;
        if hash(&bytes) != entry.payload_hash {
            return None;
        }
        // Decode and check the entire fragment before changing any live allocation state.
        let restored_host = unpack(&entry.host, &bytes)?;
        let restored_assignments = slice(&bytes, entry.assignments)?.to_vec();
        let restored_assets = entry
            .assets
            .iter()
            .map(|p| unpack(&p.tags, &bytes))
            .collect::<Option<Vec<_>>>()?;
        if entry.assets.len() < self.asset_starts.len() {
            return None;
        }
        for (index, package) in entry.assets.iter().enumerate() {
            if package.start != self.asset_starts.get(index).copied().unwrap_or(0)
                || usize::from(package.id)
                    != usize::from(crate::package_profile::PARHELION_ASSET_PACKAGE_ID) + index
            {
                return None;
            }
        }
        for (key, value) in &entry.animation {
            if animation.get(key).is_some_and(|previous| previous != value) {
                return None;
            }
        }
        host.extend(restored_host);
        *assignments = restored_assignments;
        for (index, (record, tags)) in entry.assets.iter().zip(restored_assets).enumerate() {
            if index == assets.packages.packages.len() {
                assets.packages.packages.push(AssetPackage {
                    id: record.id,
                    tags: vec![],
                    references: vec![],
                });
            }
            let package = &mut assets.packages.packages[index];
            package.tags.extend(tags);
            package
                .references
                .extend(record.references.iter().map(|(ordinal, target)| {
                    crate::NewTagReferenceOverride {
                        new_tag_ordinal: *ordinal,
                        reference: match target {
                            Some(target) => crate::NewTagReference::Appended(*target),
                            None => crate::NewTagReference::Template,
                        },
                    }
                }));
        }
        assets.placed.extend(entry.placed.into_iter().map(TagHash));
        assets.impacts.extend(entry.impacts);
        animation.extend(entry.animation);
        // Retain frequently reused fragments when the bounded cache needs space.
        if let Ok(file) = File::options().write(true).open(&self.path) {
            let _ = file.set_modified(std::time::SystemTime::now());
        }
        Some(entry.pattern)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn save(
        &self,
        reads: BTreeMap<u32, Hash>,
        pattern: Option<u32>,
        host: &[NewTagSpec],
        assets: &RuntimeAssets<'_>,
        assignments: &[u8],
        animation: &AnimationCache,
    ) -> Option<()> {
        let size = assignments.len()
            + host[self.host_start..]
                .iter()
                .map(|t| t.payload.len())
                .sum::<usize>()
            + assets
                .packages
                .packages
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    p.tags[self.asset_starts.get(i).copied().unwrap_or(0)..]
                        .iter()
                        .map(|t| t.payload.len())
                        .sum::<usize>()
                })
                .sum::<usize>();
        if size as u64 > ENTRY_LIMIT {
            return None;
        }
        let mut bytes = Vec::with_capacity(size);
        let mut entry = Entry {
            key: self.key,
            payload_hash: [0; 32],
            reads,
            pattern,
            impacts: assets.impacts[self.impact_start..].to_vec(),
            assignments: append(&mut bytes, assignments),
            host: pack(&host[self.host_start..], &mut bytes),
            assets: assets
                .packages
                .packages
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let start = self.asset_starts.get(i).copied().unwrap_or(0);
                    Asset {
                        id: p.id,
                        start,
                        tags: pack(&p.tags[start..], &mut bytes),
                        references: p
                            .references
                            .iter()
                            .filter(|r| r.new_tag_ordinal >= start)
                            .map(|r| {
                                (
                                    r.new_tag_ordinal,
                                    match r.reference {
                                        crate::NewTagReference::Appended(target) => Some(target),
                                        crate::NewTagReference::Template => None,
                                    },
                                )
                            })
                            .collect(),
                    }
                })
                .collect(),
            placed: assets.placed[self.placed_start..]
                .iter()
                .map(|tag| tag.0)
                .collect(),
            animation: animation
                .iter()
                .filter(|(key, value)| self.animation.get(key) != Some(value))
                .map(|(key, value)| (*key, *value))
                .collect(),
        };
        entry.payload_hash = hash(&bytes);
        let mut temporary = tempfile::Builder::new()
            .prefix(".runtime-")
            .suffix(".tmp")
            .tempfile_in(self.path.parent()?)
            .ok()?;
        {
            let mut archive = zip::ZipWriter::new(temporary.as_file_mut());
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated)
                .compression_level(Some(1));
            archive.start_file("entry.json", options).ok()?;
            archive.write_all(&serde_json::to_vec(&entry).ok()?).ok()?;
            archive.start_file("payload.bin", options).ok()?;
            archive.write_all(&bytes).ok()?;
            archive.finish().ok()?;
        }
        temporary.flush().ok()?;
        temporary.as_file().sync_all().ok()?;
        temporary.persist(&self.path).ok()?;
        Some(())
    }
}

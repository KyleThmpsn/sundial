//! Immutable area conversions, separate from installed package-family reuse. A map's conversions
//! are its imports, kept as every importer keeps its items' in `imports/halo_reach/<HASH>/map-<n>`.
//! `<HASH>` is the FNV-1 hash of the map's name, which its bubble records also carry, since one
//! Reach level can become several maps. A conversion is written in the map's work folder and moved
//! into a folder of its own once its manifest is complete, so a folder holding none is never read.
//! The import sweep keeps each map's newest finished conversion, ordered by when its manifest last
//! changed, so a conversion read again is touched to stay the newest. A stage reads under a hold
//! on the map's conversions (`service::hold_for_reading`) from before it lists them until its
//! world is linked, and a sweep sets none aside meanwhile, in this process or another. Earlier
//! versions kept generations in `cache/map-conversions/<name>`. A stage of the map moves the
//! current one into place and removes the rest.
use anyhow::{Context, Result, ensure};
use parhelion_import::{
    d2_mot::service,
    halo_reach::level::native::{Converted, Templates},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};
use sundial::package_authoring::{PackageManager, mod_content::Importer};
use tiger_pkg::TagHash;

/// A finished conversion's manifest, written last.
const MANIFEST: &str = "cache.json";
/// The conversion without its payloads and reports: each node's symbol, fixups and references,
/// and the placements. A build that keeps the installed map packages links the map's world
/// against them from it. `mod_family::build::remember_map` writes it beside a published
/// conversion.
pub(super) const LINKS: &str = "links.json";
/// The files of a conversion folder its manifest does not list: the manifest itself, the links
/// written after it, and the mark the import store leaves once it shares the payloads.
const UNLISTED: [&str; 3] = [MANIFEST, LINKS, parhelion_import::store::MARKER];
/// The kind of a map's conversion folders, `map-<n>`.
const KIND: &str = "map";

/// Where an earlier version's generation family pointed to its current generation.
#[derive(Deserialize)]
struct Pointer {
    generation: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    path: PathBuf,
    bytes: u64,
    sha256: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Native {
    tag: u32,
    reference: u32,
    kind: u8,
    subtype: u8,
    sha256: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    key: String,
    files: Vec<File>,
    native: Vec<Native>,
}

pub(super) struct Cache {
    /// The map's item folder, `imports/halo_reach/<HASH>`.
    item: PathBuf,
    key: String,
    /// The conversion read again or published.
    folder: Option<PathBuf>,
    pub converted: Option<Converted>,
    /// Keeps the map's conversions from being set aside while `folder` is read.
    hold: Option<service::Hold>,
}

fn hash(value: &impl Serialize) -> Result<String> {
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(value)?)))
}

fn ordinary(path: &Path) -> Result<fs::Metadata> {
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        !metadata.file_type().is_symlink(),
        "Map cache contains a link"
    );
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        ensure!(
            metadata.file_attributes() & 0x400 == 0,
            "Map cache contains a reparse point"
        );
    }
    Ok(metadata)
}

fn files(root: &Path) -> Result<Vec<File>> {
    let mut pending = vec![root.to_path_buf()];
    let mut paths = Vec::new();
    while let Some(directory) = pending.pop() {
        parhelion_import::cancellation::check()?;
        for entry in fs::read_dir(directory)? {
            let path = entry?.path();
            let metadata = ordinary(&path)?;
            if metadata.is_dir() {
                pending.push(path);
                continue;
            }
            ensure!(metadata.is_file(), "Map cache has a non-file entry");
            let relative = path.strip_prefix(root)?.to_owned();
            if UNLISTED.iter().any(|name| relative == Path::new(name)) {
                continue;
            }
            paths.push((path, relative));
        }
    }
    // A campaign generation holds thousands of payloads and gigabytes, so workers take files in
    // turn and the list is sorted afterwards.
    let workers = std::thread::available_parallelism()
        .map_or(1, std::num::NonZero::get)
        .clamp(1, 8)
        .min(paths.len().max(1));
    let next = std::sync::atomic::AtomicUsize::new(0);
    let digests = std::thread::scope(|scope| {
        let handles = (0..workers)
            .map(|_| {
                scope.spawn(|| -> Result<Vec<File>> {
                    let mut done = Vec::new();
                    loop {
                        let index = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some((path, relative)) = paths.get(index) else {
                            return Ok(done);
                        };
                        parhelion_import::cancellation::check()?;
                        let digest = crate::artifact::digest_file(path)
                            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
                        done.push(File {
                            path: relative.clone(),
                            bytes: digest.byte_length,
                            sha256: digest.sha256,
                        });
                    }
                })
            })
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .map(|handle| {
                handle
                    .join()
                    .map_err(|_| anyhow::anyhow!("A map cache hash worker stopped"))?
            })
            .collect::<Result<Vec<_>>>()
    })?;
    let mut result = digests.into_iter().flatten().collect::<Vec<_>>();
    result.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(result)
}

fn native(root: &Path, files: &[File], manager: &PackageManager) -> Result<Vec<Native>> {
    let mut records = BTreeMap::new();
    for file in files.iter().filter(|file| {
        file.path
            .file_name()
            .is_some_and(|name| name == "source-manifest.json")
    }) {
        let source: Value = serde_json::from_slice(&fs::read(root.join(&file.path))?)?;
        for row in source["tags"]
            .as_object()
            .context("Native read manifest has no tags")?
            .values()
        {
            let tag = u32::try_from(row["tag"].as_u64().context("Native read identity")?)?;
            let path = root
                .join(file.path.parent().context("Native manifest parent")?)
                .join("raw")
                .join(format!("{tag:08X}.bin"));
            let bytes = fs::read(path)?;
            let entry = manager
                .get_entry(TagHash(tag))
                .context("Native dependency disappeared")?;
            ensure!(
                row["reference"].as_u64() == Some(u64::from(entry.reference)),
                "Native dependency reference changed"
            );
            ensure!(
                row["type"].as_u64() == Some(u64::from(entry.file_type))
                    && row["subtype"].as_u64() == Some(u64::from(entry.file_subtype)),
                "Native dependency type changed"
            );
            let current = manager.read_tag(TagHash(tag)).map_err(anyhow::Error::msg)?;
            ensure!(
                bytes == current,
                "Native dependency changed during conversion"
            );
            records.insert(
                tag,
                Native {
                    tag,
                    reference: entry.reference,
                    kind: entry.file_type,
                    subtype: entry.file_subtype,
                    sha256: hex::encode(Sha256::digest(bytes)),
                },
            );
        }
    }
    ensure!(
        !records.is_empty(),
        "Area conversion has no recorded native dependencies"
    );
    Ok(records.into_values().collect())
}

/// Reads the conversion in `folder` when its manifest names `key` and every file and native
/// dependency it recorded is unchanged. A matching conversion is touched first, so later import
/// sweeps keep it as the newest.
fn load(folder: &Path, key: &str, manager: &PackageManager) -> Result<Converted> {
    let manifest_path = folder.join(MANIFEST);
    ensure!(
        ordinary(&manifest_path)?.len() <= 64 * 1024 * 1024,
        "Map cache manifest is too large"
    );
    let manifest: Manifest = serde_json::from_slice(&fs::read(&manifest_path)?)?;
    ensure!(
        manifest.key == key && !manifest.files.is_empty() && !manifest.native.is_empty(),
        "Map cache identity differs"
    );
    fs::OpenOptions::new()
        .write(true)
        .open(&manifest_path)
        .and_then(|file| file.set_modified(SystemTime::now()))
        .context("Could not mark the map conversion used")?;
    // Enumerating the tree rejects missing, extra and redirected payloads as well as changed bytes.
    ensure!(
        serde_json::to_vec(&files(folder)?)? == serde_json::to_vec(&manifest.files)?,
        "Map cache payload changed"
    );
    for row in manifest.native {
        parhelion_import::cancellation::check()?;
        let entry = manager
            .get_entry(TagHash(row.tag))
            .context("Cached native dependency disappeared")?;
        ensure!(
            entry.reference == row.reference
                && entry.file_type == row.kind
                && entry.file_subtype == row.subtype,
            "Cached native entry changed"
        );
        let bytes = manager
            .read_tag(TagHash(row.tag))
            .map_err(anyhow::Error::msg)?;
        ensure!(
            hex::encode(Sha256::digest(bytes)) == row.sha256,
            "Cached native payload changed"
        );
    }
    Ok(serde_json::from_slice(&fs::read(
        folder.join("conversion.json"),
    )?)?)
}

/// The finished conversions of `item`, the newest first.
fn conversions(item: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(item) else {
        return Vec::new();
    };
    let mut found = entries
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .and_then(|name| name.strip_prefix(KIND)?.strip_prefix('-'))
                .is_some_and(|index| !index.is_empty() && index.bytes().all(|b| b.is_ascii_digit()))
        })
        .filter(|entry| ordinary(&entry.path()).is_ok_and(|metadata| metadata.is_dir()))
        .filter_map(|entry| {
            let changed = fs::metadata(entry.path().join(MANIFEST))
                .and_then(|metadata| metadata.modified())
                .ok()?;
            Some((changed, entry.path()))
        })
        .collect::<Vec<_>>();
    found.sort_by(|a, b| b.cmp(a));
    found.into_iter().map(|(_, path)| path).collect()
}

/// The newest finished conversion of map `name` in the data folder `data`, whatever build made
/// it. Reading it this way does not touch it.
pub(crate) fn newest(data: &Path, name: &str) -> Result<Option<PathBuf>> {
    let item = service::item_directory(
        &data.join(service::IMPORTS),
        Importer::HaloReach.source(),
        sundial::package_authoring::fnv1_name_hash(name),
    )?;
    Ok(conversions(&item).into_iter().next())
}

/// Moves the finished conversion `work` into the next free `map-<n>` folder of `item`.
fn place(work: &Path, item: &Path) -> Result<PathBuf> {
    for index in 0..u32::MAX {
        let path = item.join(format!("{KIND}-{index}"));
        if fs::symlink_metadata(&path).is_ok() {
            continue;
        }
        // The import sweep removes an item folder it emptied, which can happen at any time.
        fs::create_dir_all(item)?;
        match fs::rename(work, &path) {
            Ok(()) => return Ok(path),
            Err(_) if fs::symlink_metadata(&path).is_ok() => continue,
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("Could not move the map conversion to {}", path.display())
                });
            }
        }
    }
    anyhow::bail!("No unused map conversion folder remains")
}

/// Moves the current generation an earlier version kept in the family `legacy` into `item`, then
/// removes the family with its spare generations. The caller holds the map's stage lock, so no
/// other stage of the map reads them.
fn migrate(legacy: &Path, item: &Path) {
    if !ordinary(legacy).is_ok_and(|metadata| metadata.is_dir()) {
        return;
    }
    let current = fs::read(legacy.join("current.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Pointer>(&bytes).ok())
        .filter(|pointer| {
            pointer.generation.len() == 64
                && pointer.generation.bytes().all(|b| b.is_ascii_hexdigit())
        });
    if let Some(pointer) = current {
        let generation = legacy.join("generations").join(pointer.generation);
        if ordinary(&generation).is_ok_and(|metadata| metadata.is_dir())
            && generation.join(MANIFEST).is_file()
        {
            let _ = place(&generation, item);
        }
    }
    let _ = fs::remove_dir_all(legacy);
}

/// The digests of the package manager's 64-bit binding index and entry index that every
/// conversion key holds. Hashing both indexes takes a noticeable share of a stage, and every map
/// a build stages reads the same manager, so the digests are kept for the manager they were taken
/// from: its identity, and its package count, which changes when it gains a local package, the one
/// change an open manager's index undergoes.
fn index_digests(manager: &PackageManager) -> Result<(String, String)> {
    type Kept = Option<((u64, usize), (String, String))>;
    static KEPT: std::sync::Mutex<Kept> = std::sync::Mutex::new(None);
    let of = (
        manager.identity(),
        manager.lookup.tag32_entries_by_pkg.len(),
    );
    if let Some((kept, digests)) = &*KEPT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        && *kept == of
    {
        return Ok(digests.clone());
    }
    let bindings = manager
        .lookup
        .tag64_entries
        .iter()
        .map(|(&key, value)| (key, (value.hash32.0, value.reference.0)))
        .collect::<BTreeMap<_, _>>();
    let entries = manager
        .lookup
        .tag32_entries_by_pkg
        .iter()
        .map(|(&package, entries)| {
            (
                package,
                entries
                    .iter()
                    .map(|entry| {
                        (
                            entry.reference,
                            entry.file_type,
                            entry.file_subtype,
                            entry.file_size,
                        )
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let digests = (hash(&bindings)?, hash(&entries)?);
    *KEPT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some((of, digests.clone()));
    Ok(digests)
}

impl Cache {
    /// The conversions of map `name` in the data folder `data`, with the one whose identity
    /// matches this build read again. The caller holds the map's stage lock through linking.
    pub(super) fn open(
        data: &Path,
        name: &str,
        source: &crate::map::MapSource,
        templates: &Templates,
        manager: &PackageManager,
    ) -> Result<Option<Self>> {
        if std::env::var_os("PARHELION_MAP_CACHE").is_some_and(|value| value == "off") {
            return Ok(None);
        }
        let Some(inputs) = crate::mod_family::build::map_source_identity(source) else {
            return Ok(None);
        };
        let Some(compiler) = parhelion_import::tiger::shader::compiler_identity() else {
            return Ok(None);
        };
        let (bindings, entries) = index_digests(manager)?;
        let key = hash(
            &json!({ "format": 2, "converter": parhelion_import::CONVERTER_REVISION,
            "reach": parhelion_import::halo_reach::REVISION, "inputs": inputs, "source": source,
            "templates": templates, "bindings": bindings, "entries": entries,
            "compiler": compiler, "compute": std::env::var("PARHELION_COMPUTE").unwrap_or_else(|_| "auto".into()) }),
        )?;
        let item = service::item_directory(
            &data.join(service::IMPORTS),
            Importer::HaloReach.source(),
            sundial::package_authoring::fnv1_name_hash(name),
        )?;
        // Held before the conversions are listed, so the one chosen stays where it was found.
        let hold = Some(service::hold_for_reading(&item)?);
        migrate(
            &data.join("cache").join("map-conversions").join(name),
            &item,
        );
        for folder in conversions(&item) {
            match load(&folder, &key, manager) {
                Ok(converted) => {
                    return Ok(Some(Self {
                        item,
                        key,
                        folder: Some(folder),
                        converted: Some(converted),
                        hold,
                    }));
                }
                Err(error) if parhelion_import::cancellation::is_cancelled(&error) => {
                    return Err(error);
                }
                Err(_) => {}
            }
        }
        Ok(Some(Self {
            item,
            key,
            folder: None,
            converted: None,
            hold,
        }))
    }

    /// The conversion read again or published, where its payloads are read.
    pub(super) fn folder(&self) -> Option<&Path> {
        self.folder.as_deref()
    }

    /// The hold on the map's conversions, for whoever reads the conversion after this cache.
    pub(super) fn take_hold(&mut self) -> Option<service::Hold> {
        self.hold.take()
    }

    /// Records the conversion written in the work folder `work`, then moves it into the map's
    /// item folder, where it is read from then on.
    pub(super) fn publish(
        &mut self,
        converted: &Converted,
        work: &Path,
        manager: &PackageManager,
    ) -> Result<PathBuf> {
        parhelion_import::cache::write_json(&work.join("conversion.json"), converted)?;
        let files = files(work)?;
        let native = native(work, &files, manager)?;
        parhelion_import::cache::write_json(
            &work.join(MANIFEST),
            &Manifest {
                key: self.key.clone(),
                files,
                native,
            },
        )?;
        let folder = place(work, &self.item)?;
        self.folder = Some(folder.clone());
        Ok(folder)
    }
}

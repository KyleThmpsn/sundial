//! Connect component owners through declared native resource references.
//! Entity-only word indexes cannot see an owner -> resource -> resource -> entity path.
//!
//! The walk reads one payload per resource, tens of thousands of times, so its answers
//! are kept per package and keyed by that package's own files, the way the name shards
//! are. An install that touches a few packages then rescans only the resources in those
//! packages. A cached answer is the resources a payload references, before the current
//! installation's existence filter, which runs on every load so a removed resource cannot
//! linger.
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use tiger_pkg::{PackageManager, TagHash};

use crate::package_runtime::{
    parallel,
    references::{self, schema::Registry},
    snapshot::Snapshot,
    tft::shards::prune_siblings,
};

pub(super) struct Ancestry {
    pub parents: HashMap<u32, Vec<u32>>,
    pub errors: Vec<String>,
}

/// The shard format this build writes. Files of older formats are dead weight.
const SHARD_PREFIX: &str = "ancestry-v1-";

/// Where the walk's answers for one installation are kept.
pub(super) struct Cache {
    directory: PathBuf,
    snapshot: Snapshot,
}

impl Cache {
    /// The cache for the installation at `packages`, or nothing when there is no cache
    /// directory, in which case the walk simply runs in full.
    pub(super) fn open(packages: &Path) -> Option<Self> {
        let directory = crate::paths::cache_dir()?
            .join(crate::sandbox_perk::CACHE_DIRECTORY)
            .join("ancestry");
        let snapshot = Snapshot::read(packages).ok()?;
        Some(Self {
            directory,
            snapshot,
        })
    }

    /// The shard file of one package, named by that package's files alone.
    fn path(&self, package: u16) -> Option<PathBuf> {
        let key = self.snapshot.for_package(package).key().ok()?;
        Some(
            self.directory
                .join(format!("{SHARD_PREFIX}{package:04x}-{key}.json")),
        )
    }
}

#[derive(Serialize, Deserialize)]
struct Saved {
    /// The files of this package alone, so other packages changing cannot invalidate it.
    snapshot: Snapshot,
    package: u16,
    /// The resources each walked resource references, unfiltered.
    children: BTreeMap<u32, Vec<u32>>,
}

/// One package's answers as held during a build.
struct Shard {
    path: Option<PathBuf>,
    saved: Saved,
    /// Whether this build added answers the file on disk does not have.
    changed: bool,
}

/// The saved shard for this exact package snapshot, or an empty one to fill.
fn open_shard(cache: &Cache, package: u16) -> Shard {
    let source = cache.snapshot.for_package(package);
    let path = cache.path(package);
    let saved = path
        .as_deref()
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice::<Saved>(&bytes).ok())
        .filter(|saved| saved.snapshot == source && saved.package == package)
        .unwrap_or_else(|| Saved {
            snapshot: source,
            package,
            children: BTreeMap::new(),
        });
    Shard {
        path,
        saved,
        changed: false,
    }
}

fn persist_shard(path: &Path, saved: &Saved) -> Result<(), String> {
    let parent = path.parent().ok_or("Cache path has no parent")?;
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).map_err(|error| error.to_string())?;
    let mut writer = std::io::BufWriter::new(temporary.as_file_mut());
    serde_json::to_writer(&mut writer, saved).map_err(|error| error.to_string())?;
    std::io::Write::flush(&mut writer).map_err(|error| error.to_string())?;
    drop(writer);
    temporary.persist(path).map_err(|error| error.to_string())?;
    let family = format!("{SHARD_PREFIX}{:04x}-", saved.package);
    prune_siblings(parent, path, |name| name.starts_with(&family));
    Ok(())
}

pub(super) fn read(
    manager: &PackageManager,
    owners: impl IntoIterator<Item = u32>,
    cache: Option<&Cache>,
) -> Result<Ancestry, String> {
    // Fails early on a broken built-in schema, before any package is read.
    Registry::new()?;
    let mut shards = BTreeMap::<u16, Shard>::new();
    let result = collect(owners, |wave| {
        // Each wave of the search runs one package per worker, the way the graph scan
        // does. A worker keeps its own schema registry: the generated records it caches
        // are read from the package it is working in.
        let mut tags = wave.to_vec();
        tags.sort_unstable();
        let jobs = tags
            .chunk_by(|a, b| TagHash(*a).pkg_id() == TagHash(*b).pkg_id())
            .collect::<Vec<_>>();
        if let Some(cache) = cache {
            let unopened = jobs
                .iter()
                .map(|package| TagHash(package[0]).pkg_id())
                .filter(|package| !shards.contains_key(package))
                .collect::<Vec<_>>();
            let opened = parallel::map_jobs(&unopened, |package| open_shard(cache, *package));
            shards.extend(unopened.into_iter().zip(opened));
        }
        let answers = parallel::map_jobs(&jobs, |package| {
            let known = shards
                .get(&TagHash(package[0]).pkg_id())
                .map(|shard| &shard.saved.children);
            let mut registry = None;
            package
                .iter()
                .map(|&tag| {
                    if let Some(children) = known.and_then(|known| known.get(&tag)) {
                        return (tag, Ok(children.clone()), false);
                    }
                    match registry.get_or_insert_with(Registry::new) {
                        Ok(registry) => (tag, walk(manager, registry, tag), true),
                        Err(error) => (tag, Err(error.clone()), false),
                    }
                })
                .collect::<Vec<_>>()
        });
        answers
            .into_iter()
            .flatten()
            .map(|(tag, answer, fresh)| {
                if fresh
                    && let Ok(children) = &answer
                    && let Some(shard) = shards.get_mut(&TagHash(tag).pkg_id())
                {
                    shard.saved.children.insert(tag, children.clone());
                    shard.changed = true;
                }
                (tag, answer.map(|children| live(manager, children)))
            })
            .collect()
    });
    // Persistence is best effort: a shard that cannot be written is rebuilt next time.
    for shard in shards.values() {
        if shard.changed
            && let Some(path) = &shard.path
        {
            let _ = persist_shard(path, &shard.saved);
        }
    }
    Ok(result)
}

/// The resources one resource references, read off its payload and unfiltered.
fn walk(manager: &PackageManager, registry: &mut Registry, tag: u32) -> Result<Vec<u32>, String> {
    let entry = manager
        .get_entry(TagHash(tag))
        .ok_or_else(|| format!("Missing native naming resource 0x{tag:08X}"))?;
    // Entity graphs already have component-owner edges. Raw GPU/audio payloads
    // and generated schema definitions do not provide ownership relationships.
    if !matches!(entry.file_type, 8 | 16)
        || matches!(
            entry.reference,
            0x80800000 | 0x80809BBB | crate::weapon_entity::WEAPON_ENTITY_CLASS
        )
    {
        return Ok(Vec::new());
    }
    let payload = manager
        .read_tag(TagHash(tag))
        .map_err(|error| error.to_string())?;
    let children = references::walk(&payload, entry.reference, |class| {
        registry.record(class, |schema| {
            manager
                .read_tag(TagHash(schema))
                .map_err(|error| error.to_string())
        })
    })
    .map_err(|error| format!("Naming resource 0x{tag:08X}: {error}"))?;
    Ok(children.into_keys().collect())
}

/// The referenced resources that exist in the current installation and can own others.
fn live(manager: &PackageManager, children: Vec<u32>) -> Vec<u32> {
    children
        .into_iter()
        .filter(|child| {
            manager.get_entry(TagHash(*child)).is_some_and(|entry| {
                matches!(entry.file_type, 8 | 16)
                    && !matches!(entry.reference, 0x80800000 | 0x80809BBB)
            })
        })
        .collect()
}

/// A breadth-first search from the owners, one wave at a time. `children` answers a
/// whole wave at once so the caller can spread it over workers. Every resource is asked
/// exactly once, and the result does not depend on the order answers come back.
fn collect(
    owners: impl IntoIterator<Item = u32>,
    mut children: impl FnMut(&[u32]) -> Vec<(u32, Result<Vec<u32>, String>)>,
) -> Ancestry {
    let mut pending = owners.into_iter().collect::<Vec<_>>();
    let mut seen = HashSet::new();
    let mut result = Ancestry {
        parents: HashMap::new(),
        errors: Vec::new(),
    };
    loop {
        let wave = pending
            .drain(..)
            .filter(|tag| seen.insert(*tag))
            .collect::<Vec<_>>();
        if wave.is_empty() {
            break;
        }
        for (parent, answer) in children(&wave) {
            match answer {
                Ok(children) => {
                    for child in children {
                        if child == parent {
                            continue;
                        }
                        result.parents.entry(child).or_default().push(parent);
                        if !seen.contains(&child) {
                            pending.push(child);
                        }
                    }
                }
                Err(error) => result.errors.push(error),
            }
        }
    }
    for parents in result.parents.values_mut() {
        parents.sort_unstable();
        parents.dedup();
    }
    result.errors.sort();
    result.errors.dedup();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nested_resources_keep_every_parent_and_terminate_cycles() {
        let links = HashMap::from([(1, vec![3]), (2, vec![3]), (3, vec![4]), (4, vec![3, 5])]);
        let mut reads = BTreeMap::<u32, usize>::new();
        let mut waves = Vec::new();
        let result = collect([1, 2], |wave| {
            waves.push(wave.to_vec());
            wave.iter()
                .map(|tag| {
                    *reads.entry(*tag).or_default() += 1;
                    (*tag, Ok(links.get(tag).cloned().unwrap_or_default()))
                })
                .collect()
        });
        assert_eq!(result.parents[&3], vec![1, 2, 4]);
        assert_eq!(result.parents[&5], vec![4]);
        assert!(reads.values().all(|count| *count == 1));
        assert!(result.errors.is_empty());
        // Each wave holds the resources the previous one reached, asked once, together.
        assert_eq!(waves, vec![vec![1, 2], vec![3], vec![4], vec![5]]);
    }

    #[test]
    fn a_failed_resource_is_reported_once_and_does_not_stop_the_search() {
        let result = collect([1], |wave| {
            wave.iter()
                .map(|tag| match tag {
                    1 => (1, Ok(vec![2, 3])),
                    2 => (2, Err("resource 2 is unreadable".to_owned())),
                    3 => (3, Ok(vec![4])),
                    _ => (*tag, Ok(Vec::new())),
                })
                .collect()
        });
        assert_eq!(result.parents[&4], vec![3]);
        assert_eq!(result.errors, vec!["resource 2 is unreadable".to_owned()]);
    }

    /// A shard is keyed by its own package's files: it comes back after a restart, and a
    /// change to that package empties it while other packages changing leaves it alone.
    #[test]
    fn a_shard_survives_a_restart_and_only_its_own_package_invalidates_it() {
        let packages = tempfile::tempdir().unwrap();
        std::fs::write(packages.path().join("w64_test_01bb_0.pkg"), b"package").unwrap();
        std::fs::write(packages.path().join("w64_other_03c1_0.pkg"), b"other").unwrap();
        let storage = tempfile::tempdir().unwrap();
        let cache = Cache {
            directory: storage.path().to_path_buf(),
            snapshot: Snapshot::read(packages.path()).unwrap(),
        };
        let mut shard = open_shard(&cache, 0x01bb);
        assert!(shard.saved.children.is_empty());
        shard
            .saved
            .children
            .insert(0x81BB_0001, vec![0x81BB_0002, 0x83C1_0004]);
        persist_shard(shard.path.as_deref().unwrap(), &shard.saved).unwrap();
        let reopened = open_shard(&cache, 0x01bb);
        assert_eq!(
            reopened.saved.children[&0x81BB_0001],
            vec![0x81BB_0002, 0x83C1_0004]
        );
        // Another package changing keeps the shard.
        std::fs::write(packages.path().join("w64_other_03c1_0.pkg"), b"changed").unwrap();
        let cache = Cache {
            directory: storage.path().to_path_buf(),
            snapshot: Snapshot::read(packages.path()).unwrap(),
        };
        assert_eq!(open_shard(&cache, 0x01bb).saved.children.len(), 1);
        // The package itself changing starts it over, and the older file is pruned once
        // the new one is written. The new content has a different length: a snapshot is
        // name, size and modified time, and two writes can share a timestamp.
        std::fs::write(
            packages.path().join("w64_test_01bb_0.pkg"),
            b"patched package",
        )
        .unwrap();
        let cache = Cache {
            directory: storage.path().to_path_buf(),
            snapshot: Snapshot::read(packages.path()).unwrap(),
        };
        let fresh = open_shard(&cache, 0x01bb);
        assert!(fresh.saved.children.is_empty());
        persist_shard(fresh.path.as_deref().unwrap(), &fresh.saved).unwrap();
        let files = std::fs::read_dir(storage.path()).unwrap().count();
        assert!(files <= 2, "{files} shard files for one package");
    }
}

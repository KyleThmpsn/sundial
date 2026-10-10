//! Answers for the reference walk, kept per package.
//!
//! A resource's references come from its own payload, so the answers are kept per package
//! and keyed by that package's own files, the way the ancestry and name shards are, though
//! not by the directory, because every build reads a freshly linked package view. Stock
//! packages do not change, so after the first build every stock answer is read from here.
//! A cached answer is unfiltered: the walk still asks for every child it reaches, and a
//! child the current installation lacks is still an error.
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use tiger_pkg::TagHash;

use super::{Registry, children, is_reference, read_resource, resource_limit};
use crate::package_runtime::{
    cache_file, parallel, reader::PackageManager, snapshot::Snapshot, tft::shards::prune_siblings,
};

/// The shard format this build writes. Files of older formats are dead weight.
const SHARD_PREFIX: &str = "references-v1-";

type Answer = Result<BTreeMap<u32, usize>, String>;

#[derive(Serialize, Deserialize)]
struct Saved {
    /// The files of this package alone, so other packages changing cannot invalidate it.
    key: String,
    package: u16,
    /// Each walked resource's references and the offsets they are declared at.
    children: BTreeMap<u32, Vec<(u32, usize)>>,
}

/// One package's answers as held during a walk.
struct Shard {
    path: PathBuf,
    saved: Saved,
    /// Whether this walk added answers the file on disk does not have.
    changed: bool,
}

/// The saved shard for this exact package snapshot, or an empty one to fill.
fn open_shard(directory: &Path, snapshot: &Snapshot, package: u16) -> Option<Shard> {
    let key = snapshot.for_package(package).files_key().ok()?;
    let path = directory.join(format!("{SHARD_PREFIX}{package:04x}-{key}.json"));
    let saved = std::fs::read(&path)
        .ok()
        .and_then(|bytes| cache_file::read::<Saved>(&bytes).ok())
        .filter(|saved| saved.key == key && saved.package == package)
        .unwrap_or_else(|| Saved {
            key,
            package,
            children: BTreeMap::new(),
        });
    Some(Shard {
        path,
        saved,
        changed: false,
    })
}

fn persist(shard: &Shard) -> Result<(), String> {
    let parent = shard.path.parent().ok_or("Cache path has no parent")?;
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).map_err(|error| error.to_string())?;
    cache_file::write(temporary.as_file_mut(), &shard.saved)?;
    temporary
        .persist(&shard.path)
        .map_err(|error| error.to_string())?;
    let family = format!("{SHARD_PREFIX}{:04x}-", shard.saved.package);
    prune_siblings(parent, &shard.path, |name| name.starts_with(&family));
    Ok(())
}

/// Answers for the resources a walk from `roots` reaches, from the shards where they are
/// known and otherwise read one package per worker, a wave at a time. Each answer depends
/// only on its own resource, so the order they come back cannot change them.
pub(super) fn prefetch(manager: &PackageManager, roots: &BTreeSet<u32>) -> HashMap<u32, Answer> {
    let directory = crate::system::paths::cache_dir().map(|root| root.join("references"));
    let snapshot = directory
        .as_ref()
        .and_then(|_| Snapshot::read(&manager.package_dir).ok());
    let mut shards = BTreeMap::<u16, Option<Shard>>::new();
    let mut answers = HashMap::new();
    let mut wave = roots.iter().copied().collect::<Vec<_>>();
    // The walk stops at the same limit, so reading beyond it would be wasted.
    let limit = resource_limit(roots.len());
    while !wave.is_empty() && answers.len() <= limit {
        wave.sort_unstable();
        wave.dedup();
        let jobs = wave
            .chunk_by(|a, b| TagHash(*a).pkg_id() == TagHash(*b).pkg_id())
            .collect::<Vec<_>>();
        if let (Some(directory), Some(snapshot)) = (&directory, &snapshot) {
            let unopened = jobs
                .iter()
                .map(|package| TagHash(package[0]).pkg_id())
                .filter(|package| !shards.contains_key(package))
                .collect::<Vec<_>>();
            let opened = parallel::map_jobs(&unopened, |package| {
                open_shard(directory, snapshot, *package)
            });
            shards.extend(unopened.into_iter().zip(opened));
        }
        let results = parallel::map_jobs(&jobs, |package| {
            let known = shards
                .get(&TagHash(package[0]).pkg_id())
                .and_then(Option::as_ref)
                .map(|shard| &shard.saved.children);
            // A worker keeps its own schema registry for the package it is working in.
            let mut registry = None;
            package
                .iter()
                .map(|&tag| {
                    if let Some(children) = known.and_then(|known| known.get(&tag)) {
                        return (tag, Ok(children.iter().copied().collect()), false);
                    }
                    let registry = match registry.get_or_insert_with(Registry::new) {
                        Ok(registry) => registry,
                        Err(error) => return (tag, Err(error.clone()), false),
                    };
                    let resource = match read_resource(manager, tag) {
                        Ok(resource) => resource,
                        Err(error) => return (tag, Err(error), false),
                    };
                    // Only a payload walk is worth keeping. Other kinds are answered from
                    // the package directory alone.
                    let walked = matches!(resource.kind, 8 | 16);
                    let answer = children(tag, &resource, registry, &mut |schema| {
                        read_resource(manager, schema)
                    });
                    (tag, answer, walked)
                })
                .collect::<Vec<_>>()
        });
        let mut next = Vec::new();
        for (tag, answer, fresh) in results.into_iter().flatten() {
            if let Ok(children) = &answer {
                next.extend(
                    children
                        .keys()
                        .copied()
                        .filter(|child| is_reference(*child) && !answers.contains_key(child)),
                );
                if fresh && let Some(Some(shard)) = shards.get_mut(&TagHash(tag).pkg_id()) {
                    shard.saved.children.insert(
                        tag,
                        children.iter().map(|(&child, &at)| (child, at)).collect(),
                    );
                    shard.changed = true;
                }
            }
            answers.insert(tag, answer);
        }
        next.retain(|tag| !answers.contains_key(tag));
        wave = next;
    }
    // Persistence is best effort: a shard that cannot be written is walked again next time.
    for shard in shards.values().flatten() {
        if shard.changed {
            let _ = persist(shard);
        }
    }
    answers
}

//! Every resource that references a resource, over the whole installation.
//!
//! The reference walk answers the forward question, what one resource declares, for the
//! resources a closure reaches. This answers the reverse one for any tag. It reads every
//! resource of every runtime package once, keeps each package's declared references in a
//! shard keyed by that package's own files, and inverts them in memory. A shard holds a
//! whole package, so it is complete or absent, never partial, and a package it points into
//! changing cannot stale it. Only declared references count: an aligned word that looks like
//! a tag is not one.
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

use serde::{Deserialize, Serialize};
use tiger_pkg::TagHash;

use super::{Registry, children, is_reference, read_resource};
use crate::package_runtime::{
    cache_file, is_valid_package_tag, parallel,
    reader::PackageManager,
    snapshot::Snapshot,
    tft::shards::{prune_siblings, sweep_stray_temporaries},
};

#[cfg(test)]
mod tests;

/// The shard format this build writes. Files of older formats are dead weight.
const SHARD_PREFIX: &str = "referrers-v1-";

pub const CANCELLED: &str = "The reference index read was cancelled";

/// One package's resources with the tags each declares, before the filter for what the
/// current installation holds, and the resources that could not be walked.
#[derive(Serialize, Deserialize)]
struct Saved {
    /// The files of this package alone, so other packages changing cannot invalidate it.
    key: String,
    package: u16,
    children: Vec<(u32, Vec<u32>)>,
    failed: Vec<(u32, String)>,
}

struct Plan {
    package: u16,
    /// The resources a walk can read, in entry order.
    tags: Vec<u32>,
    key: String,
    path: Option<PathBuf>,
}

/// How much of the index came from the disk, and how much it holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    pub reused_packages: usize,
    pub scanned_packages: usize,
    pub resources: usize,
    pub references: usize,
}

/// Which resources reference each resource.
#[derive(Debug, Default)]
pub struct Referrers {
    parents: HashMap<u32, Vec<u32>>,
    /// The resources that could not be walked, with why. They reference nothing here.
    pub errors: Vec<String>,
    pub usage: Usage,
}

impl Referrers {
    /// The resources that declare a reference to `tag`, sorted.
    #[must_use]
    pub fn of(&self, tag: u32) -> &[u32] {
        self.parents.get(&tag).map_or(&[], Vec::as_slice)
    }

    /// How many resources something references.
    #[must_use]
    pub fn referenced(&self) -> usize {
        self.parents.len()
    }
}

fn directory() -> Option<PathBuf> {
    crate::system::paths::cache_dir().map(|root| root.join("references"))
}

/// Every runtime package with the resources a walk can read, in package order.
fn plans(manager: &PackageManager, snapshot: &Snapshot) -> Result<Vec<Plan>, String> {
    let directory = directory();
    let mut packages = manager
        .lookup
        .tag32_entries_by_pkg
        .iter()
        .collect::<Vec<_>>();
    packages.sort_by_key(|(package, _)| **package);
    packages
        .into_iter()
        .filter_map(|(&package, entries)| {
            if !is_valid_package_tag(TagHash::new(package, 0)) {
                return None;
            }
            let tags = entries
                .iter()
                .enumerate()
                .filter(|(_, entry)| matches!(entry.file_type, 8 | 16 | 32..=34))
                .map(|(index, _)| TagHash::new(package, index as u16).0)
                .collect::<Vec<_>>();
            if tags.is_empty() {
                return None;
            }
            Some(snapshot.for_package(package).files_key().map(|key| Plan {
                path: directory.as_ref().map(|directory| {
                    directory.join(format!("{SHARD_PREFIX}{package:04x}-{key}.json"))
                }),
                package,
                tags,
                key,
            }))
        })
        .collect()
}

/// The saved shard for this exact package, when it covers every resource the plan lists.
fn load(plan: &Plan) -> Option<Saved> {
    let bytes = std::fs::read(plan.path.as_ref()?).ok()?;
    let saved: Saved = cache_file::read(&bytes).ok()?;
    if saved.key != plan.key || saved.package != plan.package {
        return None;
    }
    let mut covered = saved
        .children
        .iter()
        .map(|(tag, _)| *tag)
        .chain(saved.failed.iter().map(|(tag, _)| *tag))
        .collect::<Vec<_>>();
    covered.sort_unstable();
    (covered == plan.tags).then_some(saved)
}

/// One package read in full, or nothing when the read was cancelled part way.
fn scan(
    manager: &PackageManager,
    plan: &Plan,
    cancel: &AtomicBool,
) -> Result<Option<Saved>, String> {
    let mut registry = Registry::new()?;
    let mut rows = Vec::with_capacity(plan.tags.len());
    let mut failed = Vec::new();
    for &tag in &plan.tags {
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let declared = read_resource(manager, tag).and_then(|resource| {
            children(tag, &resource, &mut registry, &mut |schema| {
                read_resource(manager, schema)
            })
        });
        match declared {
            Ok(found) => rows.push((
                tag,
                found
                    .into_keys()
                    .filter(|&child| child != tag && is_reference(child))
                    .collect(),
            )),
            Err(error) => failed.push((tag, error)),
        }
    }
    Ok(Some(Saved {
        key: plan.key.clone(),
        package: plan.package,
        children: rows,
        failed,
    }))
}

fn persist(path: &Path, saved: &Saved) -> Result<(), String> {
    let parent = path.parent().ok_or("Cache path has no parent")?;
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).map_err(|error| error.to_string())?;
    cache_file::write(temporary.as_file_mut(), saved)?;
    temporary.persist(path).map_err(|error| error.to_string())?;
    let family = format!("{SHARD_PREFIX}{:04x}-", saved.package);
    prune_siblings(parent, path, |name| name.starts_with(&family));
    Ok(())
}

/// The index over every runtime package, from the shards where they cover the package and
/// otherwise read one package per worker. `progress` is called with packages done out of
/// the total. Setting `cancel` stops the read within a few resources; the packages that
/// finished are kept for the next read.
pub fn read(
    manager: &PackageManager,
    cancel: &AtomicBool,
    progress: impl Fn(usize, usize) + Sync,
) -> Result<Referrers, String> {
    let packages = &manager.package_dir;
    let snapshot = Snapshot::read(packages)?;
    let plans = plans(manager, &snapshot)?;
    let total = plans.len();
    let mut shards = parallel::map_jobs(&plans, load);
    let reused = shards.iter().flatten().count();
    progress(reused, total);
    let pending = shards
        .iter()
        .enumerate()
        .filter_map(|(index, shard)| shard.is_none().then_some(index))
        .collect::<Vec<_>>();
    let done = AtomicUsize::new(reused);
    let scanned = parallel::map_jobs(&pending, |&index| {
        let result = scan(manager, &plans[index], cancel);
        progress(done.fetch_add(1, Ordering::Relaxed) + 1, total);
        result
    });
    let mut fresh = Vec::new();
    for (index, result) in pending.into_iter().zip(scanned) {
        if let Some(saved) = result? {
            shards[index] = Some(saved);
            fresh.push(index);
        }
    }
    if Snapshot::read(packages)? != snapshot {
        return Err(
            "Packages changed while reading references. Retry after installation finishes.".into(),
        );
    }
    // Complete packages of a cancelled read are kept, so the next read resumes after them.
    for &index in &fresh {
        if let Some(path) = &plans[index].path {
            // Best effort, as every cache write here is: the index is right without it.
            let _ = persist(
                path,
                shards[index].as_ref().expect("scanned package has rows"),
            );
        }
    }
    if let Some(directory) = directory().filter(|directory| directory.is_dir()) {
        sweep_stray_temporaries(&directory);
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(CANCELLED.to_owned());
    }
    Ok(invert(
        shards.into_iter().flatten(),
        Usage {
            reused_packages: reused,
            scanned_packages: fresh.len(),
            ..Usage::default()
        },
    ))
}

fn invert(shards: impl Iterator<Item = Saved>, mut usage: Usage) -> Referrers {
    let mut parents: HashMap<u32, Vec<u32>> = HashMap::new();
    let mut errors = Vec::new();
    for saved in shards {
        for (source, targets) in saved.children {
            usage.resources += 1;
            for target in targets {
                usage.references += 1;
                parents.entry(target).or_default().push(source);
            }
        }
        for (tag, error) in saved.failed {
            errors.push(format!("0x{tag:08X}: {error}"));
        }
    }
    for sources in parents.values_mut() {
        sources.sort_unstable();
        sources.dedup();
    }
    Referrers {
        parents,
        errors,
        usage,
    }
}

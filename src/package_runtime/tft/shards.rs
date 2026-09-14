//! Reuse source scans independently, then resolve their tag lanes against the
//! current installation. A changed target package cannot leave stale identities.
//!
//! A shard is keyed by its own package's files only. An install that adds authored graphs
//! changes the set of entity targets, but an unchanged package cannot reference a graph
//! that did not exist when it was written, so its shard stays valid: references to targets
//! that no longer exist are dropped when the shard is loaded. Packages without a usable
//! shard are scanned in parallel, one package per worker, since each package has its own
//! reader.
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
};

use super::*;
use crate::package_runtime::snapshot::Snapshot;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Candidate {
    source: u32,
    source_class: u32,
    offset: usize,
    lane: u64,
    path: String,
}

#[derive(Default, Serialize, Deserialize)]
struct Shard {
    paths: Vec<ContentPath>,
    candidates: Vec<Candidate>,
    #[serde(default)]
    entity_references: Vec<EntityReference>,
    scanned_resources: usize,
    errors: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct Saved {
    /// The files of this package alone, so other packages changing cannot invalidate it.
    snapshot: Snapshot,
    package: u16,
    /// Digest of the entity graph set the shard was scanned against, kept for the record.
    /// Loading filters the references against the current set instead of rescanning.
    #[serde(default)]
    entity_key: u64,
    shard: Shard,
}

/// How many cache files of one kind stay on disk. Two, so switching between two package
/// directories does not rebuild on every switch.
const KEPT_FILES: usize = 2;

/// The shard format this build writes. Files of older formats are dead weight.
const SHARD_PREFIX: &str = "tft-source-v3-";

fn candidates(source: u32, source_class: u32, payload: &[u8], targets: &EntityTargets) -> Shard {
    let mut shard = Shard::default();
    shard
        .entity_references
        .extend(
            entity_words(payload, source, targets)
                .into_iter()
                .map(|target| EntityReference {
                    source,
                    source_class,
                    target,
                }),
        );
    let paths = content_paths(payload);
    if paths.is_empty() {
        return shard;
    }
    // Retain the original lane, including tag64 values. Resolve it later, using
    // the live lookup, even when this source package came from the disk cache.
    for offset in (8..payload.len().saturating_sub(7)).step_by(4) {
        let pointer = offset - 8;
        let Some(path) = i64_at(payload, pointer)
            .ok()
            .and_then(|relative| relative_offset(pointer, 0, relative).ok())
            .and_then(|start| paths.get(&start))
        else {
            continue;
        };
        if let Ok(lane) = u64_at(payload, offset) {
            shard.candidates.push(Candidate {
                source,
                source_class,
                offset,
                lane,
                path: path.clone(),
            });
        }
    }
    shard.paths = paths
        .into_iter()
        .map(|(offset, path)| ContentPath {
            source,
            offset,
            path,
        })
        .collect();
    shard
}

fn resolve(shard: &Shard, mut lookup: impl FnMut(u64) -> Option<(u32, u32)>) -> Vec<Reference> {
    shard
        .candidates
        .iter()
        .filter_map(|candidate| {
            let (target, target_class) = lookup(candidate.lane)?;
            Some(Reference {
                source: candidate.source,
                source_class: candidate.source_class,
                offset: candidate.offset,
                target,
                target_class,
                path: candidate.path.clone(),
            })
        })
        .collect()
}

/// The disk location of one package's shard, keyed by that package's own snapshot.
fn shard_path(
    directory: Option<&Path>,
    package: u16,
    source: &Snapshot,
) -> Result<Option<PathBuf>, String> {
    directory
        .map(|root| {
            source
                .key()
                .map(|key| root.join(format!("{SHARD_PREFIX}{package:04x}-{key}.json")))
        })
        .transpose()
}

/// A saved shard, when one exists for this exact package snapshot. References to entity
/// graphs that no longer exist are dropped, so an uninstalled authored graph cannot linger.
fn load_shard(
    path: Option<&Path>,
    package: u16,
    source: &Snapshot,
    targets: &EntityTargets,
) -> Option<Shard> {
    let bytes = std::fs::read(path?).ok()?;
    let saved = serde_json::from_slice::<Saved>(&bytes).ok()?;
    if saved.snapshot != *source || saved.package != package {
        return None;
    }
    let mut shard = saved.shard;
    shard
        .entity_references
        .retain(|reference| targets.tags.contains(&reference.target));
    Some(shard)
}

/// Progress from one scanning worker: resources read so far in one package, or the
/// finished shard.
enum Message {
    Progress(usize, usize),
    Done(usize, Shard),
}

/// Scans the packages that have no usable shard, one package per worker, and reports
/// progress on the calling thread. Results come back in the order of `jobs`.
fn scan_packages(
    manager: &PackageManager,
    jobs: &[(u16, &[tiger_pkg::package::UEntryHeader])],
    targets: &EntityTargets,
    already: usize,
    total: usize,
    progress: &mut impl FnMut(usize, usize),
) -> Vec<Shard> {
    let mut results = jobs.iter().map(|_| None).collect::<Vec<Option<Shard>>>();
    if jobs.is_empty() {
        return Vec::new();
    }
    let workers = crate::package_runtime::parallel::worker_count(jobs.len());
    let next = AtomicUsize::new(0);
    let (sender, receiver) = mpsc::channel();
    std::thread::scope(|scope| {
        for _ in 0..workers {
            let sender = sender.clone();
            let next = &next;
            scope.spawn(move || {
                loop {
                    let job = next.fetch_add(1, Ordering::Relaxed);
                    let Some(&(package, entries)) = jobs.get(job) else {
                        break;
                    };
                    let shard = scan_package(manager, package, entries, targets, |count| {
                        let _ = sender.send(Message::Progress(job, count));
                    });
                    let _ = sender.send(Message::Done(job, shard));
                }
            });
        }
        drop(sender);
        let mut finished = 0;
        let mut partial = BTreeMap::new();
        for message in receiver {
            match message {
                Message::Progress(job, count) => {
                    partial.insert(job, count);
                }
                Message::Done(job, shard) => {
                    partial.remove(&job);
                    finished += shard.scanned_resources;
                    results[job] = Some(shard);
                }
            }
            progress(already + finished + partial.values().sum::<usize>(), total);
        }
    });
    results.into_iter().map(Option::unwrap_or_default).collect()
}

/// Removes shards of older formats and all but the newest few of each package, so the
/// directory holds one working set and a spare per package rather than every install ever
/// seen. Runs after every assembly, so leftovers go even for packages that never change.
/// Best effort, like the writes it follows.
fn sweep_shards(directory: &Path) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    let mut by_package = BTreeMap::<String, Vec<(std::time::SystemTime, PathBuf)>>::new();
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name();
        let Some(name) = name
            .to_str()
            .filter(|name| name.starts_with("tft-source-v"))
        else {
            continue;
        };
        let Some(package) = name
            .strip_prefix(SHARD_PREFIX)
            .and_then(|rest| rest.get(..4))
        else {
            let _ = std::fs::remove_file(entry.path());
            continue;
        };
        let Some(modified) = entry
            .metadata()
            .ok()
            .and_then(|metadata| metadata.modified().ok())
        else {
            continue;
        };
        by_package
            .entry(package.to_owned())
            .or_default()
            .push((modified, entry.path()));
    }
    for mut files in by_package.into_values() {
        files.sort();
        let stale = files.len().saturating_sub(KEPT_FILES);
        for (_, path) in files.into_iter().take(stale) {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Deletes all but the newest `KEPT_FILES` files in `directory` that `matches` selects,
/// never the file at `keep`. Best effort, like the writes it follows.
pub(crate) fn prune_siblings(directory: &Path, keep: &Path, matches: impl Fn(&str) -> bool) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    let mut candidates = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.path() != keep)
        .filter(|entry| entry.file_name().to_str().is_some_and(&matches))
        .filter_map(|entry| {
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((modified, entry.path()))
        })
        .collect::<Vec<_>>();
    candidates.sort();
    let stale = candidates
        .len()
        .saturating_sub(KEPT_FILES.saturating_sub(1));
    for (_, path) in candidates.into_iter().take(stale) {
        let _ = std::fs::remove_file(path);
    }
}

/// Reads every structured resource of one package.
fn scan_package(
    manager: &PackageManager,
    package: u16,
    entries: &[tiger_pkg::package::UEntryHeader],
    targets: &EntityTargets,
    mut progress: impl FnMut(usize),
) -> Shard {
    let mut shard = Shard::default();
    for (ordinal, entry) in entries.iter().enumerate() {
        if entry.file_type != 8 {
            continue;
        }
        shard.scanned_resources += 1;
        if shard.scanned_resources % 10_000 == 0 {
            progress(shard.scanned_resources);
        }
        let tag = TagHash::new(package, ordinal as u16);
        match manager.read_tag(tag) {
            Ok(payload) if payload.len() == entry.file_size as usize => {
                let found = candidates(tag.0, entry.reference, &payload, targets);
                shard.paths.extend(found.paths);
                shard.candidates.extend(found.candidates);
                shard.entity_references.extend(found.entity_references);
            }
            Ok(_) => shard.errors.push(format!(
                "{tag}: resource size does not match its package entry"
            )),
            Err(error) => shard.errors.push(format!("{tag}: {error}")),
        }
    }
    shard
}

/// Writes a shard beside the others. Best effort, like the assembled index: a failed cache
/// write cannot make the current effect unusable, and cannot replace an existing valid shard.
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
    Ok(())
}

pub(super) fn inspect(
    packages: &Path,
    manager: &PackageManager,
    mut progress: impl FnMut(usize, usize),
) -> Result<Index, String> {
    let snapshot = Snapshot::read(packages)?;
    let directory = crate::paths::cache_dir().map(|root| root.join("native-names/source-packages"));
    let targets = EntityTargets::new(manager);
    let entity_key = targets.key();
    let total = manager
        .lookup
        .tag32_entries_by_pkg
        .values()
        .map(|entries| entries.iter().filter(|entry| entry.file_type == 8).count())
        .sum();
    let mut index = Index::default();
    let mut pending = Vec::new();
    // Packages in one order, so the assembled index and its errors read the same each time.
    let mut shards = BTreeMap::new();
    let mut jobs = Vec::new();
    for (&package, entries) in &manager.lookup.tag32_entries_by_pkg {
        let source = snapshot.for_package(package);
        let path = shard_path(directory.as_deref(), package, &source)?;
        match load_shard(path.as_deref(), package, &source, &targets) {
            Some(shard) => {
                shards.insert(package, (shard, None));
            }
            None => jobs.push((package, entries.as_slice(), path, source)),
        }
    }
    let reused = shards
        .values()
        .map(|(shard, _)| shard.scanned_resources)
        .sum::<usize>();
    progress(reused, total);
    let scanned = scan_packages(
        manager,
        &jobs
            .iter()
            .map(|(package, entries, _, _)| (*package, *entries))
            .collect::<Vec<_>>(),
        &targets,
        reused,
        total,
        &mut progress,
    );
    for ((package, _, path, source), shard) in jobs.into_iter().zip(scanned) {
        shards.insert(package, (shard, path.map(|path| (path, source))));
    }
    for (package, (shard, fresh)) in shards {
        index.references.extend(resolve(&shard, |lane| {
            let target = if let Ok(raw) = u32::try_from(lane) {
                TagHash(raw)
            } else {
                manager.lookup.tag64_entries.get(&lane)?.hash32
            };
            Some((target.0, manager.get_entry(target)?.reference))
        }));
        index.scanned_resources += shard.scanned_resources;
        index.paths.extend(shard.paths.iter().cloned());
        index
            .entity_references
            .extend(shard.entity_references.iter().cloned());
        index.errors.extend(shard.errors.iter().cloned());
        if let Some((path, source)) = fresh {
            pending.push((
                path,
                Saved {
                    snapshot: source,
                    package,
                    entity_key,
                    shard,
                },
            ));
        }
    }
    progress(index.scanned_resources, total);
    if Snapshot::read(packages)? != snapshot {
        return Err(
            "Packages changed while reading asset names. Retry after installation finishes.".into(),
        );
    }
    for (path, saved) in pending {
        let _ = persist_shard(&path, &saved);
    }
    if let Some(directory) = &directory {
        sweep_shards(directory);
    }
    index.paths.sort_by_key(|path| (path.source, path.offset));
    index
        .references
        .sort_by_key(|reference| (reference.source, reference.offset, reference.target));
    index
        .entity_references
        .sort_by_key(|reference| (reference.source, reference.target));
    Ok(index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_sources_resolve_against_current_targets() {
        let bytes = super::super::tests::fixture();
        let targets = EntityTargets {
            tags: HashSet::from([0x8152_82E1]),
            lanes: HashMap::new(),
        };
        let shard = candidates(7, 8, &bytes, &targets);
        let encoded = serde_json::to_vec(&shard).unwrap();
        let cached: Shard = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(cached.paths.len(), 1);
        assert_eq!(cached.candidates.len(), 2);
        assert_eq!(
            cached.entity_references,
            vec![EntityReference {
                source: 7,
                source_class: 8,
                target: 0x8152_82E1,
            }]
        );
        let original = resolve(&cached, |_| Some((10, 11)));
        let changed = resolve(&cached, |_| Some((20, 21)));
        assert!(
            original
                .iter()
                .all(|r| r.target == 10 && r.target_class == 11)
        );
        assert!(
            changed
                .iter()
                .all(|r| r.target == 20 && r.target_class == 21)
        );
        assert!(resolve(&cached, |_| None).is_empty());
        let expected = super::super::references(&bytes, &content_paths(&bytes), |_| Some((10, 11)));
        assert_eq!(
            original
                .iter()
                .map(|r| (r.offset, r.target, r.target_class, r.path.clone()))
                .collect::<Vec<_>>(),
            expected
        );
    }

    #[test]
    fn a_saved_shard_survives_a_changed_entity_set_and_drops_vanished_targets() {
        let packages = tempfile::tempdir().unwrap();
        std::fs::write(packages.path().join("w64_test_01bb_0.pkg"), b"package").unwrap();
        let snapshot = Snapshot::read(packages.path()).unwrap().for_package(0x01bb);
        let cache = tempfile::tempdir().unwrap();
        let path = shard_path(Some(cache.path()), 0x01bb, &snapshot)
            .unwrap()
            .unwrap();
        let shard = Shard {
            entity_references: vec![
                EntityReference {
                    source: 1,
                    source_class: 2,
                    target: 0x8152_82E1,
                },
                EntityReference {
                    source: 1,
                    source_class: 2,
                    target: 0x80B7_795A,
                },
            ],
            scanned_resources: 3,
            ..Shard::default()
        };
        persist_shard(
            &path,
            &Saved {
                snapshot: snapshot.clone(),
                package: 0x01bb,
                entity_key: 1,
                shard,
            },
        )
        .unwrap();
        // A different entity set no longer forces a rescan. The authored graph that was
        // uninstalled since is dropped, the stock one stays.
        let targets = EntityTargets {
            tags: HashSet::from([0x8152_82E1, 0x8161_F4DE]),
            lanes: HashMap::new(),
        };
        let loaded = load_shard(Some(&path), 0x01bb, &snapshot, &targets).unwrap();
        assert_eq!(loaded.scanned_resources, 3);
        assert_eq!(
            loaded
                .entity_references
                .iter()
                .map(|reference| reference.target)
                .collect::<Vec<_>>(),
            vec![0x8152_82E1]
        );
        // A changed package still needs a fresh scan.
        std::fs::write(packages.path().join("w64_test_01bb_0.pkg"), b"changed").unwrap();
        let changed = Snapshot::read(packages.path()).unwrap().for_package(0x01bb);
        assert!(load_shard(Some(&path), 0x01bb, &changed, &targets).is_none());
    }

    #[test]
    fn sweeping_keeps_the_newest_shards_of_each_package_and_drops_older_formats() {
        let cache = tempfile::tempdir().unwrap();
        let names = [
            "tft-source-v3-01bb-aaaa.json",
            "tft-source-v3-01bb-bbbb.json",
            "tft-source-v3-01bb-cccc.json",
            "tft-source-v3-03c1-dddd.json",
            "tft-source-v2-03c1-eeee.json",
            "unrelated.json",
        ];
        for (index, name) in names.iter().enumerate() {
            std::fs::write(cache.path().join(name), b"{}").unwrap();
            let modified = std::time::SystemTime::UNIX_EPOCH
                + std::time::Duration::from_secs(1_700_000_000 + index as u64 * 60);
            std::fs::OpenOptions::new()
                .write(true)
                .open(cache.path().join(name))
                .unwrap()
                .set_modified(modified)
                .unwrap();
        }
        sweep_shards(cache.path());
        let mut remaining = std::fs::read_dir(cache.path())
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        remaining.sort();
        assert_eq!(
            remaining,
            vec![
                "tft-source-v3-01bb-bbbb.json",
                "tft-source-v3-01bb-cccc.json",
                "tft-source-v3-03c1-dddd.json",
                "unrelated.json",
            ]
        );
    }
}

//! Reuse source scans independently, then resolve their tag lanes against the
//! current installation. A changed target package cannot leave stale identities.
//!
//! A shard is keyed by its own package's files only and holds nothing that depends on any
//! other package: the raw candidate lanes beside content paths, and the raw entity
//! evidence, the tag-shaped words and known lanes of each resource. Which graphs those
//! point at is decided on every assembly against the live entity set, through the same
//! resolution a fresh scan uses, so a warm load and a cold scan produce the same
//! references from the same evidence. That matters for installs of packages a stock
//! resource already refers to: a graph that becomes a target after the shard was written is
//! found, a graph that was uninstalled since is dropped, and a lane remapped to another
//! graph follows the remap. The one thing a warm load cannot recover is a 64-bit window
//! that was not a tag lane when the shard was written, which `EntityEvidence` sets out.
//! Packages without a usable shard are scanned in parallel, one package per worker, since
//! each package has its own reader.
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    time::Duration,
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
    /// Unresolved, one row per resource with any. Resolved against the live entity set
    /// by `entity_references` on every assembly, never at scan time.
    #[serde(with = "packed")]
    evidence: Vec<EntityEvidence>,
    vocabulary: Vec<ContentPath>,
    scanned_resources: usize,
    errors: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct Saved {
    /// The files of this package alone, so other packages changing cannot invalidate it.
    snapshot: Snapshot,
    package: u16,
    shard: Shard,
}

/// The on-disk layout of the evidence rows. As plain rows they would outweigh the rest of
/// the shard several times over: an installation holds some ten million tag-shaped words
/// in well over a million resources. Rows are tuples in source order, with the source and
/// each word written as the difference from the previous one and the class as an index
/// into one table, which is about a third of the size. Differences wrap, so any row reads
/// back exactly, and sorted words, the usual case, read back as short numbers.
mod packed {
    use std::collections::BTreeMap;

    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use crate::package_runtime::tft::EntityEvidence;

    #[derive(Serialize, Deserialize)]
    struct Packed {
        classes: Vec<u32>,
        rows: Vec<(u32, usize, Vec<u32>, Vec<u64>)>,
    }

    fn differences(values: &[u32]) -> Vec<u32> {
        let mut previous = 0_u32;
        values
            .iter()
            .map(|value| {
                let difference = value.wrapping_sub(previous);
                previous = *value;
                difference
            })
            .collect()
    }

    fn sums(differences: Vec<u32>) -> Vec<u32> {
        let mut previous = 0_u32;
        differences
            .into_iter()
            .map(|difference| {
                previous = previous.wrapping_add(difference);
                previous
            })
            .collect()
    }

    pub(super) fn serialize<S: Serializer>(
        evidence: &[EntityEvidence],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let mut sorted = evidence.iter().collect::<Vec<_>>();
        sorted.sort_by_key(|row| row.source);
        let mut classes = Vec::new();
        let mut indexes = BTreeMap::new();
        let mut previous = 0_u32;
        let rows = sorted
            .into_iter()
            .map(|row| {
                let class = *indexes.entry(row.source_class).or_insert_with(|| {
                    classes.push(row.source_class);
                    classes.len() - 1
                });
                let source = row.source.wrapping_sub(previous);
                previous = row.source;
                (source, class, differences(&row.words), row.lanes.clone())
            })
            .collect::<Vec<_>>();
        Packed { classes, rows }.serialize(serializer)
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<EntityEvidence>, D::Error> {
        let Packed { classes, rows } = Packed::deserialize(deserializer)?;
        let mut previous = 0_u32;
        rows.into_iter()
            .map(
                |(source, class, words, lanes)| -> Result<EntityEvidence, D::Error> {
                    previous = previous.wrapping_add(source);
                    let source_class = *classes.get(class).ok_or_else(|| {
                        <D::Error as serde::de::Error>::custom(
                            "evidence row names a class outside the table",
                        )
                    })?;
                    Ok(EntityEvidence {
                        source: previous,
                        source_class,
                        words: sums(words),
                        lanes,
                    })
                },
            )
            .collect()
    }
}

/// How many cache files of one kind stay on disk. Two, so switching between two package
/// directories does not rebuild on every switch.
const KEPT_FILES: usize = 2;

/// The shard format this build writes. Files of older formats are dead weight.
const SHARD_PREFIX: &str = "tft-source-v6-";

fn candidates(source: u32, source_class: u32, payload: &[u8], known_lane: KnownLane<'_>) -> Shard {
    let mut shard = Shard::default();
    let evidence = EntityEvidence::gather(payload, source, source_class, known_lane);
    if !evidence.is_empty() {
        shard.evidence.push(evidence);
    }
    shard.vocabulary = vocabulary_strings(payload)
        .into_iter()
        .map(|(offset, path)| ContentPath {
            source,
            offset,
            path,
        })
        .collect();
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

/// A saved shard, when one exists for this exact package snapshot. It carries no resolved
/// references, so nothing in it can be stale with respect to other packages.
fn load_shard(path: Option<&Path>, package: u16, source: &Snapshot) -> Option<Shard> {
    let bytes = std::fs::read(path?).ok()?;
    let saved = crate::package_runtime::cache_file::read::<Saved>(&bytes).ok()?;
    if saved.snapshot != *source || saved.package != package {
        return None;
    }
    Some(saved.shard)
}

/// The entity references of one shard against the live entity set, the same rows a fresh
/// scan of the same package would produce today, whether the shard was just scanned or
/// read from disk.
fn entity_references(shard: &Shard, targets: &EntityTargets) -> Vec<EntityReference> {
    shard
        .evidence
        .iter()
        .flat_map(|evidence| evidence.references(targets))
        .collect()
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
    known_lane: KnownLane<'_>,
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
                    let shard = scan_package(manager, package, entries, known_lane, |count| {
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
    sweep_stray_temporaries(directory);
}

/// Age before a temporary file left in a cache directory is reclaimed. A write in flight is
/// far younger than this, and on Windows its open handle refuses the removal anyway, so only
/// files a process that died mid-write left behind are reached.
const STRAY_TEMPORARY_AGE: Duration = Duration::from_secs(60 * 60);

/// Removes temporary files an interrupted write left behind. `NamedTempFile` deletes itself
/// on drop, so one only survives when the process did not live to drop it, and nothing else
/// reclaims them: every sweep here selects files by a cache prefix, which a temporary name
/// never carries. Best effort, like the writes it follows.
pub(crate) fn sweep_stray_temporaries(directory: &Path) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        if !entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(".tmp"))
        {
            continue;
        }
        let abandoned = entry
            .metadata()
            .ok()
            .and_then(|metadata| metadata.modified().ok())
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age >= STRAY_TEMPORARY_AGE);
        if abandoned {
            let _ = std::fs::remove_file(entry.path());
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
    known_lane: KnownLane<'_>,
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
                let found = candidates(tag.0, entry.reference, &payload, known_lane);
                shard.paths.extend(found.paths);
                shard.candidates.extend(found.candidates);
                shard.evidence.extend(found.evidence);
                shard.vocabulary.extend(found.vocabulary);
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
    crate::package_runtime::cache_file::write(temporary.as_file_mut(), saved)?;
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
    let known_lane = |lane: u64| manager.lookup.tag64_entries.contains_key(&lane);
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
    let mut plans = Vec::new();
    for (&package, entries) in &manager.lookup.tag32_entries_by_pkg {
        let source = snapshot.for_package(package);
        let path = shard_path(directory.as_deref(), package, &source)?;
        plans.push((package, entries.as_slice(), path, source));
    }
    // Five hundred shard files parse in parallel, one per worker, the way they were scanned.
    // Each worker resolves its own shard and drops the evidence it resolved from, so only the
    // workers in flight hold any. Holding all of it at once would be most of the memory this
    // assembly needs: the evidence of an installation is an order of magnitude larger than
    // the references it resolves to. A reused shard needs nothing else from it.
    let loaded =
        crate::package_runtime::parallel::map_jobs(&plans, |(package, _, path, source)| {
            let mut shard = load_shard(path.as_deref(), *package, source)?;
            let references = entity_references(&shard, &targets);
            shard.evidence = Vec::new();
            Some((shard, references))
        });
    for ((package, entries, path, source), loaded) in plans.into_iter().zip(loaded) {
        match loaded {
            Some((shard, references)) => {
                shards.insert(package, (shard, references, None));
            }
            None => jobs.push((package, entries, path, source)),
        }
    }
    let reused = shards
        .values()
        .map(|(shard, ..)| shard.scanned_resources)
        .sum::<usize>();
    progress(reused, total);
    let scanned = scan_packages(
        manager,
        &jobs
            .iter()
            .map(|(package, entries, _, _)| (*package, *entries))
            .collect::<Vec<_>>(),
        &known_lane,
        reused,
        total,
        &mut progress,
    );
    // A freshly scanned shard keeps its evidence until it has been written, which is only
    // the packages an install touched.
    for ((package, _, path, source), shard) in jobs.into_iter().zip(scanned) {
        let references = entity_references(&shard, &targets);
        shards.insert(
            package,
            (shard, references, path.map(|path| (path, source))),
        );
    }
    for (package, (shard, references, fresh)) in shards {
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
        index.vocabulary.extend(shard.vocabulary.iter().cloned());
        index.entity_references.extend(references);
        index.errors.extend(shard.errors.iter().cloned());
        if let Some((path, source)) = fresh {
            pending.push((
                path,
                Saved {
                    snapshot: source,
                    package,
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
        .vocabulary
        .sort_by_key(|path| (path.source, path.offset));
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

    /// The fixture's 64-bit lane, the only value the lane table of these tests knows.
    const FIXTURE_LANE: u64 = 0x1234_5678_9ABC_DEF0;

    fn known_lane(lane: u64) -> bool {
        lane == FIXTURE_LANE
    }

    fn targets(tags: impl IntoIterator<Item = u32>, lanes: &[(u64, u32)]) -> EntityTargets {
        EntityTargets {
            tags: tags.into_iter().collect(),
            lanes: lanes.iter().copied().collect(),
        }
    }

    fn target_tags(shard: &Shard, targets: &EntityTargets) -> Vec<u32> {
        entity_references(shard, targets)
            .into_iter()
            .map(|reference| reference.target)
            .collect()
    }

    /// A one-file package directory and an empty cache directory, with the shard path of
    /// package `0x01bb` in that cache. Both directories live as long as the returned guards.
    fn package_and_cache() -> (tempfile::TempDir, tempfile::TempDir, Snapshot, PathBuf) {
        let packages = tempfile::tempdir().unwrap();
        std::fs::write(packages.path().join("w64_test_01bb_0.pkg"), b"package").unwrap();
        let snapshot = Snapshot::read(packages.path()).unwrap().for_package(0x01bb);
        let cache = tempfile::tempdir().unwrap();
        let path = shard_path(Some(cache.path()), 0x01bb, &snapshot)
            .unwrap()
            .unwrap();
        (packages, cache, snapshot, path)
    }

    fn save(path: &Path, snapshot: &Snapshot, shard: Shard) {
        persist_shard(
            path,
            &Saved {
                snapshot: snapshot.clone(),
                package: 0x01bb,
                shard,
            },
        )
        .unwrap();
    }

    /// `packed_evidence_rows_read_back_exactly_in_source_order` covers the ordinary values a
    /// scan produces. This covers the ends of the ranges, where the wrapping differences the
    /// layout relies on are the only thing keeping a row readable.
    #[test]
    fn packed_evidence_survives_values_that_wrap_the_delta_coding() {
        let rows = vec![
            EntityEvidence {
                source: u32::MAX,
                source_class: 0x8080_0001,
                words: vec![u32::MAX, 1, 0x8000_0000],
                lanes: vec![u64::MAX, 0],
            },
            EntityEvidence {
                source: 1,
                source_class: 0x8080_0001,
                words: Vec::new(),
                lanes: vec![7],
            },
            EntityEvidence {
                source: 0x8000_0000,
                source_class: 0x8080_0002,
                words: vec![5, 4, 3],
                lanes: Vec::new(),
            },
        ];
        let shard = Shard {
            evidence: rows.clone(),
            ..Shard::default()
        };
        let encoded = serde_json::to_vec(&shard).unwrap();
        let Ok(decoded) = serde_json::from_slice::<Shard>(&encoded) else {
            panic!("packed evidence does not parse back");
        };
        let mut expected = rows;
        expected.sort_by_key(|row| row.source);
        assert_eq!(decoded.evidence, expected);
    }

    /// A class index outside the table is a corrupt shard, not a panic.
    /// Assembly resolves a reused shard in the worker that read it and then drops the
    /// evidence, which is what keeps a whole installation's worth of it out of memory at
    /// once. Everything the assembly still wants from that shard afterwards has to survive
    /// it: the content paths, the candidate lanes it resolves separately, the vocabulary,
    /// the resource count and the errors.
    #[test]
    fn a_shard_still_gives_up_everything_else_once_its_evidence_is_dropped() {
        let bytes = super::super::tests::fixture();
        let targets = targets([0x8152_82E1], &[]);
        let mut shard = candidates(7, 8, &bytes, &known_lane);
        let lookup = |lane: u64| Some((lane as u32, 0x8080_9C0F));

        let references = entity_references(&shard, &targets);
        let before = (
            resolve(&shard, lookup),
            shard.paths.clone(),
            shard.vocabulary.clone(),
            shard.scanned_resources,
            shard.errors.clone(),
        );
        assert!(!references.is_empty(), "the fixture resolves a reference");
        assert!(
            !before.0.is_empty(),
            "the fixture resolves a candidate lane"
        );

        shard.evidence = Vec::new();

        assert_eq!(resolve(&shard, lookup), before.0);
        assert_eq!(shard.paths, before.1);
        assert_eq!(shard.vocabulary, before.2);
        assert_eq!(shard.scanned_resources, before.3);
        assert_eq!(shard.errors, before.4);
    }

    #[test]
    fn packed_evidence_rejects_a_class_index_outside_its_table() {
        let encoded = br#"{"paths":[],"candidates":[],"evidence":{"classes":[],"rows":[[1,0,[],[]]]},"vocabulary":[],"scanned_resources":0,"errors":[]}"#;
        let Err(error) = serde_json::from_slice::<Shard>(encoded) else {
            panic!("a class index outside the table was accepted");
        };
        assert!(error.to_string().contains("class"), "{error}");
    }

    #[test]
    fn cached_sources_resolve_against_current_targets() {
        let bytes = super::super::tests::fixture();
        let targets = targets([0x8152_82E1], &[]);
        let shard = candidates(7, 8, &bytes, &known_lane);
        let encoded = serde_json::to_vec(&shard).unwrap();
        let cached: Shard = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(cached.paths.len(), 1);
        assert_eq!(cached.candidates.len(), 2);
        assert_eq!(
            cached.evidence,
            vec![EntityEvidence {
                source: 7,
                source_class: 8,
                words: vec![0x8152_82E1],
                lanes: vec![FIXTURE_LANE],
            }]
        );
        assert_eq!(
            entity_references(&cached, &targets),
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
        let (packages, _cache, snapshot, path) = package_and_cache();
        // Written while both graphs were installed. The shard keeps the words, not the
        // verdict.
        let shard = Shard {
            evidence: vec![EntityEvidence {
                source: 1,
                source_class: 2,
                words: vec![0x80B7_795A, 0x8152_82E1],
                lanes: Vec::new(),
            }],
            scanned_resources: 3,
            ..Shard::default()
        };
        save(&path, &snapshot, shard);
        // A different entity set does not force a rescan. The authored graph that was
        // uninstalled since is dropped, the stock one stays.
        let targets = targets([0x8152_82E1, 0x8161_F4DE], &[]);
        let loaded = load_shard(Some(&path), 0x01bb, &snapshot).unwrap();
        assert_eq!(loaded.scanned_resources, 3);
        assert_eq!(target_tags(&loaded, &targets), vec![0x8152_82E1]);
        // A changed package still needs a fresh scan. The new content has a different
        // length: a snapshot is name, size and modified time, and two writes can share a
        // timestamp.
        std::fs::write(
            packages.path().join("w64_test_01bb_0.pkg"),
            b"changed package",
        )
        .unwrap();
        let changed = Snapshot::read(packages.path()).unwrap().for_package(0x01bb);
        assert!(load_shard(Some(&path), 0x01bb, &changed).is_none());
    }

    #[test]
    fn a_warm_shard_finds_a_graph_installed_after_the_scan_like_a_cold_scan() {
        let (_packages, _cache, snapshot, path) = package_and_cache();
        let bytes = super::super::tests::fixture();
        // The graph the fixture refers to by word is not a target when the shard is written.
        let absent = targets([0x8161_F4DE], &[]);
        let scanned = candidates(7, 8, &bytes, &known_lane);
        assert!(entity_references(&scanned, &absent).is_empty());
        save(&path, &snapshot, scanned);
        // It is installed later. The source package has not changed, so the shard is
        // reused, and it must say what a fresh scan says.
        let restored = targets([0x8161_F4DE, 0x8152_82E1], &[]);
        let warm = load_shard(Some(&path), 0x01bb, &snapshot).unwrap();
        let cold = candidates(7, 8, &bytes, &known_lane);
        let expected = vec![EntityReference {
            source: 7,
            source_class: 8,
            target: 0x8152_82E1,
        }];
        assert_eq!(entity_references(&warm, &restored), expected);
        assert_eq!(entity_references(&cold, &restored), expected);
    }

    #[test]
    fn a_warm_shard_follows_a_lane_remapped_to_another_live_graph() {
        let (_packages, _cache, snapshot, path) = package_and_cache();
        let bytes = super::super::tests::fixture();
        // At scan time the fixture's lane maps to graph A. Both A and B are live.
        let before = targets([0x80BB_0001, 0x80BB_0002], &[(FIXTURE_LANE, 0x80BB_0001)]);
        let scanned = candidates(7, 8, &bytes, &known_lane);
        assert_eq!(target_tags(&scanned, &before), vec![0x80BB_0001]);
        save(&path, &snapshot, scanned);
        // An install remaps the lane to graph B without touching the source package.
        let after = targets([0x80BB_0001, 0x80BB_0002], &[(FIXTURE_LANE, 0x80BB_0002)]);
        let warm = load_shard(Some(&path), 0x01bb, &snapshot).unwrap();
        let cold = candidates(7, 8, &bytes, &known_lane);
        assert_eq!(target_tags(&warm, &after), vec![0x80BB_0002]);
        assert_eq!(
            entity_references(&warm, &after),
            entity_references(&cold, &after)
        );
    }

    #[test]
    fn a_shard_of_the_previous_format_is_neither_loaded_nor_kept() {
        let (_packages, cache, snapshot, path) = package_and_cache();
        let key = snapshot.key().unwrap();
        let old = cache.path().join(format!("tft-source-v4-01bb-{key}.json"));
        // The v4 layout: references resolved at scan time and a digest of the entity set.
        let v4 = serde_json::json!({
            "snapshot": snapshot,
            "package": 0x01bb,
            "entity_key": 1,
            "shard": {
                "paths": [],
                "candidates": [],
                "entity_references": [{"source": 1, "source_class": 2, "target": 0x8152_82E1_u32}],
                "vocabulary": [],
                "scanned_resources": 3,
                "errors": [],
            },
        });
        std::fs::write(&old, v4.to_string()).unwrap();
        assert_ne!(path, old);
        assert!(load_shard(Some(&path), 0x01bb, &snapshot).is_none());
        // Nor would its contents pass as the current layout.
        assert!(load_shard(Some(&old), 0x01bb, &snapshot).is_none());
        sweep_shards(cache.path());
        assert!(!old.exists());
    }

    #[test]
    fn sweeping_keeps_the_newest_shards_of_each_package_and_drops_older_formats() {
        let cache = tempfile::tempdir().unwrap();
        // Named from the constant, so bumping the format does not break a test about
        // sweeping. The two older formats stay literal: what they are is the point.
        let current = |name: &str| format!("{SHARD_PREFIX}{name}.json");
        let names = [
            current("01bb-aaaa"),
            current("01bb-bbbb"),
            current("01bb-cccc"),
            current("03c1-dddd"),
            "tft-source-v4-03c1-eeee.json".to_owned(),
            "tft-source-v2-03c1-ffff.json".to_owned(),
            "unrelated.json".to_owned(),
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
                current("01bb-bbbb"),
                current("01bb-cccc"),
                current("03c1-dddd"),
                "unrelated.json".to_owned(),
            ]
        );
    }

    #[test]
    fn packed_evidence_rows_read_back_exactly_in_source_order() {
        let rows = vec![
            EntityEvidence {
                source: 0x80A0_2005,
                source_class: 0x8080_9C0F,
                words: vec![0x80A0_2001, 0x80A0_2009, 0x8152_82E1],
                lanes: vec![FIXTURE_LANE],
            },
            // Words are sorted by the scan. The layout does not depend on it.
            EntityEvidence {
                source: 0x80A0_2001,
                source_class: 0x8080_1234,
                words: vec![0x8152_82E1, 0x80A0_2001],
                lanes: Vec::new(),
            },
            EntityEvidence {
                source: 0x80A0_2002,
                source_class: 0x8080_9C0F,
                words: Vec::new(),
                lanes: vec![FIXTURE_LANE, 1],
            },
        ];
        let shard = Shard {
            evidence: rows.clone(),
            ..Shard::default()
        };
        let encoded = serde_json::to_string(&shard).unwrap();
        // Each class once, sources as steps from the previous row, words as steps too.
        assert!(encoded.contains(&format!(
            "\"classes\":[{},{}]",
            0x8080_1234_u32, 0x8080_9C0F_u32
        )));
        assert!(encoded.contains(&format!("[{},0,[{},", 0x80A0_2001_u32, 0x8152_82E1_u32)));
        assert!(encoded.contains(&format!("[1,1,[],[{FIXTURE_LANE},1]]")));
        assert!(encoded.contains(&format!("[3,1,[{},8,", 0x80A0_2001_u32)));
        let decoded: Shard = serde_json::from_str(&encoded).unwrap();
        let mut expected = rows;
        expected.sort_by_key(|row| row.source);
        assert_eq!(decoded.evidence, expected);
        // A row naming a class the table does not have is a broken shard, not a panic.
        let broken = encoded.replace("[1,1,[],[", "[1,7,[],[");
        assert!(serde_json::from_str::<Shard>(&broken).is_err());
    }
}

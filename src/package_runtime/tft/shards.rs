//! Reuse source scans independently, then resolve their tag lanes against the
//! current installation. A changed target package cannot leave stale identities.
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
    scanned_resources: usize,
    errors: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct Saved {
    snapshot: Snapshot,
    package: u16,
    shard: Shard,
}

fn candidates(source: u32, source_class: u32, payload: &[u8]) -> Shard {
    let paths = content_paths(payload);
    let mut shard = Shard::default();
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

pub(super) fn inspect(
    packages: &Path,
    manager: &PackageManager,
    mut progress: impl FnMut(usize, usize),
) -> Result<Index, String> {
    let snapshot = Snapshot::read(packages)?;
    let directory = crate::paths::cache_dir().map(|root| root.join("native-names/source-packages"));
    let total = manager
        .lookup
        .tag32_entries_by_pkg
        .values()
        .map(|entries| entries.iter().filter(|entry| entry.file_type == 8).count())
        .sum();
    let mut index = Index::default();
    let mut pending = Vec::new();
    for (&package, entries) in &manager.lookup.tag32_entries_by_pkg {
        let source = snapshot.for_package(package);
        let path = directory
            .as_ref()
            .map(|root| {
                source
                    .key()
                    .map(|key| root.join(format!("tft-source-v1-{package:04x}-{key}.json")))
            })
            .transpose()?;
        let saved = path
            .as_ref()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice::<Saved>(&bytes).ok())
            .filter(|saved| saved.snapshot == source && saved.package == package);
        let (shard, fresh) = if let Some(saved) = saved {
            (saved.shard, false)
        } else {
            let mut shard = Shard::default();
            for (ordinal, entry) in entries.iter().enumerate() {
                if entry.file_type != 8 {
                    continue;
                }
                shard.scanned_resources += 1;
                if shard.scanned_resources % 10_000 == 0 {
                    progress(index.scanned_resources + shard.scanned_resources, total);
                }
                let tag = TagHash::new(package, ordinal as u16);
                match manager.read_tag(tag) {
                    Ok(payload) if payload.len() == entry.file_size as usize => {
                        let found = candidates(tag.0, entry.reference, &payload);
                        shard.paths.extend(found.paths);
                        shard.candidates.extend(found.candidates);
                    }
                    Ok(_) => shard.errors.push(format!(
                        "{tag}: resource size does not match its package entry"
                    )),
                    Err(error) => shard.errors.push(format!("{tag}: {error}")),
                }
            }
            (shard, true)
        };
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
        index.errors.extend(shard.errors.iter().cloned());
        progress(index.scanned_resources, total);
        if fresh && let Some(path) = path {
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
    if Snapshot::read(packages)? != snapshot {
        return Err(
            "Packages changed while reading asset names. Retry after installation finishes.".into(),
        );
    }
    // Best effort, like the assembled index. A failed cache write cannot make
    // the current effect unusable, and cannot replace an existing valid shard.
    for (path, saved) in pending {
        let _ = (|| -> Result<(), String> {
            let parent = path.parent().ok_or("Cache path has no parent")?;
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            let mut temporary =
                tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
            serde_json::to_writer(temporary.as_file_mut(), &saved).map_err(|e| e.to_string())?;
            temporary.persist(&path).map_err(|e| e.to_string())?;
            Ok(())
        })();
    }
    index.paths.sort_by_key(|path| (path.source, path.offset));
    index
        .references
        .sort_by_key(|reference| (reference.source, reference.offset, reference.target));
    Ok(index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_sources_resolve_against_current_targets() {
        let bytes = super::super::tests::fixture();
        let shard = candidates(7, 8, &bytes);
        let encoded = serde_json::to_vec(&shard).unwrap();
        let cached: Shard = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(cached.paths.len(), 1);
        assert_eq!(cached.candidates.len(), 2);
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
}

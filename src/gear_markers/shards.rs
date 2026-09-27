//! Raw marker evidence is cached by the package that stores it. Object component lists and
//! component payloads are separate phases because a component can live in another package.
use super::*;
use crate::package_runtime::{cache_file, snapshot::Snapshot};
use serde::de::DeserializeOwned;
use std::path::PathBuf;

#[derive(Clone, Copy)]
pub(super) enum Kind {
    Objects,
    Components,
}

impl Kind {
    fn family(self) -> &'static str {
        match self {
            Self::Objects => "marker-objects-v",
            Self::Components => "marker-components-v",
        }
    }

    fn prefix(self) -> &'static str {
        match self {
            Self::Objects => "marker-objects-v1",
            Self::Components => "marker-components-v1",
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Saved<T> {
    snapshot: Snapshot,
    package: u16,
    rows: Vec<(u32, Result<T, String>)>,
}

/// Each tag read with its result.
type Rows<T> = Vec<(u32, Result<T, String>)>;

struct Plan<'a> {
    package: u16,
    tags: &'a [u32],
    snapshot: Snapshot,
    path: Option<PathBuf>,
}

fn plans<'a>(
    kind: Kind,
    jobs: &'a [Vec<u32>],
    snapshot: &Snapshot,
) -> Result<Vec<Plan<'a>>, String> {
    let directory = crate::paths::cache_dir().map(|root| {
        root.join(crate::sandbox_perk::CACHE_DIRECTORY)
            .join("marker-shards")
    });
    jobs.iter()
        .filter(|job| !job.is_empty())
        .map(|job| {
            let package = tiger_pkg::TagHash(job[0]).pkg_id();
            let source = snapshot.for_package(package);
            let path = directory
                .as_ref()
                .map(|directory| {
                    source.key().map(|key| {
                        directory.join(format!("{}-{package:04x}-{key}.json", kind.prefix()))
                    })
                })
                .transpose()?;
            Ok(Plan {
                package,
                tags: job,
                snapshot: source,
                path,
            })
        })
        .collect()
}

fn load<T: DeserializeOwned>(plan: &Plan<'_>) -> Option<Vec<(u32, Result<T, String>)>> {
    let bytes = std::fs::read(plan.path.as_ref()?).ok()?;
    let saved: Saved<T> = cache_file::read(&bytes).ok()?;
    if saved.snapshot != plan.snapshot
        || saved.package != plan.package
        || saved.rows.iter().map(|(tag, _)| tag).ne(plan.tags.iter())
    {
        return None;
    }
    Some(saved.rows)
}

fn persist<T: Clone + Serialize>(plan: &Plan<'_>, rows: &[(u32, Result<T, String>)]) {
    let Some(path) = &plan.path else {
        return;
    };
    let Some(parent) = path.parent() else {
        return;
    };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }
    let Ok(mut temporary) = tempfile::NamedTempFile::new_in(parent) else {
        return;
    };
    let saved = Saved {
        snapshot: plan.snapshot.clone(),
        package: plan.package,
        rows: rows.to_vec(),
    };
    if cache_file::write(temporary.as_file_mut(), &saved).is_ok() {
        let _ = temporary.persist(path);
    }
}

/// Keep the current shard and one spare per package without walking the directory once for
/// every newly written package.
fn sweep(kind: Kind, directory: &Path) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    let prefix = format!("{}-", kind.prefix());
    let mut groups: std::collections::BTreeMap<String, Vec<(std::time::SystemTime, PathBuf)>> =
        Default::default();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name.starts_with(kind.family()) && !name.starts_with(&prefix) {
            let _ = std::fs::remove_file(entry.path());
            continue;
        }
        let Some(package) = name.strip_prefix(&prefix).and_then(|rest| rest.get(..4)) else {
            continue;
        };
        if let Ok(modified) = entry.metadata().and_then(|metadata| metadata.modified()) {
            groups
                .entry(package.to_owned())
                .or_default()
                .push((modified, entry.path()));
        }
    }
    for mut files in groups.into_values() {
        files.sort();
        let old = files.len().saturating_sub(2);
        for (_, path) in files.into_iter().take(old) {
            let _ = std::fs::remove_file(path);
        }
    }
    crate::package_runtime::tft::shards::sweep_stray_temporaries(directory);
}

/// Reuse complete package reads. A cancelled scan still saves complete jobs so a later visit
/// resumes with the packages that did not finish.
pub(super) fn read<T: Clone + Send + Serialize + DeserializeOwned>(
    manager: &PackageManager,
    kind: Kind,
    jobs: &[Vec<u32>],
    read_tag: impl Fn(u32) -> Result<T, String> + Sync,
    cancel: &AtomicBool,
    progress: impl Fn(usize) + Sync,
) -> Result<Rows<T>, String> {
    let packages = &manager.package_dir;
    let snapshot = Snapshot::read(packages)?;
    let plans = plans(kind, jobs, &snapshot)?;
    let mut rows = parallel::map_jobs(&plans, load::<T>);
    let reused = rows.iter().flatten().map(Vec::len).sum::<usize>();
    progress(reused);
    let pending = rows
        .iter()
        .enumerate()
        .filter_map(|(index, rows)| rows.is_none().then_some(index))
        .collect::<Vec<_>>();
    let done = AtomicUsize::new(reused);
    let scanned = parallel::map_jobs(&pending, |&index| {
        let plan = &plans[index];
        let found = plan
            .tags
            .iter()
            .take_while(|_| !cancel.load(Ordering::Relaxed))
            .map(|&tag| (tag, read_tag(tag)))
            .collect::<Vec<_>>();
        progress(done.fetch_add(found.len(), Ordering::Relaxed) + found.len());
        found
    });
    let mut complete = Vec::new();
    for (index, found) in pending.into_iter().zip(scanned) {
        if found.len() == plans[index].tags.len() {
            complete.push(index);
        }
        rows[index] = Some(found);
    }
    if Snapshot::read(packages)? != snapshot {
        return Err(
            "Packages changed while reading markers. Retry after installation finishes.".into(),
        );
    }
    for index in complete {
        persist(
            &plans[index],
            rows[index].as_ref().expect("scanned package has rows"),
        );
    }
    if let Some(directory) = plans.iter().find_map(|plan| plan.path.as_ref()?.parent()) {
        sweep(kind, directory);
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(CANCELLED.to_owned());
    }
    Ok(rows.into_iter().flatten().flatten().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_changed_component_package_keeps_the_object_shard() {
        let packages = tempfile::tempdir().unwrap();
        for name in ["w64_test_0123_0.pkg", "w64_test_0456_0.pkg"] {
            std::fs::write(packages.path().join(name), b"original").unwrap();
        }
        let before = Snapshot::read(packages.path()).unwrap();
        let object_job = vec![vec![tiger_pkg::TagHash::new(0x123, 1).0]];
        let component_job = vec![vec![tiger_pkg::TagHash::new(0x456, 2).0]];
        let mut object_before = plans(Kind::Objects, &object_job, &before).unwrap();
        let mut component_before = plans(Kind::Components, &component_job, &before).unwrap();
        let object_cache = packages.path().join("object-shard.json");
        let component_cache = packages.path().join("component-shard.json");
        object_before[0].path = Some(object_cache.clone());
        component_before[0].path = Some(component_cache.clone());
        let object_rows = vec![(object_job[0][0], Ok(vec![component_job[0][0]]))];
        let component_rows = vec![(component_job[0][0], Ok(vec![7_u32]))];
        persist(&object_before[0], &object_rows);
        persist(&component_before[0], &component_rows);
        assert_eq!(
            load::<Vec<u32>>(&object_before[0]),
            Some(object_rows.clone())
        );
        std::fs::write(packages.path().join("w64_test_0456_0.pkg"), b"changed").unwrap();
        let after = Snapshot::read(packages.path()).unwrap();
        let mut object_after = plans(Kind::Objects, &object_job, &after).unwrap();
        let mut component_after = plans(Kind::Components, &component_job, &after).unwrap();
        object_after[0].path = Some(object_cache);
        component_after[0].path = Some(component_cache);
        assert_eq!(object_before[0].snapshot, object_after[0].snapshot);
        assert_ne!(component_before[0].snapshot, component_after[0].snapshot);
        assert_eq!(load::<Vec<u32>>(&object_after[0]), Some(object_rows));
        assert_eq!(load::<Vec<u32>>(&component_after[0]), None);
    }
}

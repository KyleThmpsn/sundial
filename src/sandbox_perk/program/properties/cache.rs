//! Refresh key observations by package, then by action payload. Item names are joined later.
use std::{path::Path, sync::Arc};

use crate::package_runtime::reader::PackageManager;
use sha2::{Digest, Sha256};
use tiger_pkg::TagHash;

use super::*;
use crate::{
    investment_schema::{GLOBALS_FINISHED_SANDBOX_PERK_TABLE_SLOT, investment_globals_table_tag},
    package_runtime::{index_cache, resolve_live_named_tag, snapshot::Snapshot},
    sandbox_perk::{
        FINISHED_SANDBOX_PERK_CATALOG_CLASS, SANDBOX_PERK_RUNTIME_MAP_TAG, action,
        finished_sandbox_perk_at, finished_sandbox_perk_count, sandbox_perk_runtime_assignment,
        validate_sandbox_perk_runtime_map,
    },
};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyIndex {
    pub(super) perks: Vec<(usize, u32)>,
    pub(super) actions: BTreeMap<u32, KeyUsage>,
    pub issues: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct Saved {
    snapshot: Snapshot,
    actions: BTreeMap<u32, CachedAction>,
}

#[derive(Clone, Serialize, Deserialize)]
struct CachedAction {
    digest: [u8; 32],
    usage: Result<KeyUsage, String>,
}

static CACHE: index_cache::Cache<KeyIndex> = index_cache::Cache::new();
const VERSION: &str = "perk-keys-v2";

/// Load an unchanged installation's keys without opening package readers or waiting for
/// discovery. The caller can show the picker while the asset reader starts separately.
pub fn cached_only(packages: &Path) -> Result<Option<Arc<KeyIndex>>, String> {
    index_cache::cached_only(
        packages,
        crate::sandbox_perk::CACHE_DIRECTORY,
        VERSION,
        &CACHE,
    )
}

/// Read only finished-perk assignments and their action resources. Unchanged packages need
/// no action reads. Unchanged actions inside edited packages need no decoding.
pub fn cached(packages: &Path, manager: &PackageManager) -> Result<Arc<KeyIndex>, String> {
    index_cache::cached(
        packages,
        crate::sandbox_perk::CACHE_DIRECTORY,
        VERSION,
        &CACHE,
        || {
            let snapshot = Snapshot::read(packages)?;
            let canonical = packages.canonicalize().map_err(|error| error.to_string())?;
            let identity = serde_json::to_vec(&canonical).map_err(|error| error.to_string())?;
            let path = crate::paths::cache_dir().map(|root| {
                root.join(crate::sandbox_perk::CACHE_DIRECTORY)
                    .join(format!(
                        "perk-key-sources-v1-{:x}.json",
                        Sha256::digest(identity)
                    ))
            });
            let previous = path
                .as_ref()
                .and_then(|path| std::fs::read(path).ok())
                .and_then(|bytes| serde_json::from_slice::<Saved>(&bytes).ok());
            let perks = assignments(manager)?;
            let (index, saved) = refresh(&snapshot, previous.as_ref(), perks, |tag| {
                let entry = manager
                    .get_entry(TagHash(tag))
                    .ok_or_else(|| format!("Action 0x{tag:08X} is missing"))?;
                if entry.file_type != 8 || entry.reference != action::ACTION_ROOT_CLASS {
                    return Err(format!("Action 0x{tag:08X} has the wrong package class"));
                }
                let bytes = read(manager, tag)?;
                if usize::try_from(entry.file_size).ok() != Some(bytes.len()) {
                    return Err(format!("Action 0x{tag:08X} has the wrong payload size"));
                }
                Ok(bytes)
            });
            if Snapshot::read(packages)? != snapshot {
                return Err(
                    "Packages changed while reading perk keys. Retry after installation finishes."
                        .into(),
                );
            }
            if let Some(path) = path {
                let _ = save(&path, &saved);
            }
            Ok(index)
        },
        |_| true,
    )
}

fn read(manager: &PackageManager, tag: u32) -> Result<Vec<u8>, String> {
    manager
        .read_tag(TagHash(tag))
        .map_err(|error| format!("Could not read tag 0x{tag:08X}: {error}"))
}

fn assignments(manager: &PackageManager) -> Result<Vec<(usize, u32)>, String> {
    let globals = read(
        manager,
        resolve_live_named_tag(manager, "investment_globals", None)?.0,
    )?;
    let perks_tag =
        investment_globals_table_tag(&globals, GLOBALS_FINISHED_SANDBOX_PERK_TABLE_SLOT)?;
    if manager
        .get_entry(TagHash(perks_tag))
        .is_none_or(|entry| entry.reference != FINISHED_SANDBOX_PERK_CATALOG_CLASS)
    {
        return Err("The perk key source has the wrong package class".into());
    }
    let perks = read(manager, perks_tag)?;
    let assignments = read(manager, SANDBOX_PERK_RUNTIME_MAP_TAG)?;
    validate_sandbox_perk_runtime_map(&assignments)?;
    let mut result = Vec::new();
    for index in 0..finished_sandbox_perk_count(&perks)? {
        let perk = finished_sandbox_perk_at(&perks, index)?;
        if let Some(assignment) = sandbox_perk_runtime_assignment(&assignments, perk.runtime_key)?
            && assignment.runtime_tag != u32::MAX
        {
            result.push((index, assignment.runtime_tag));
        }
    }
    Ok(result)
}

fn refresh(
    snapshot: &Snapshot,
    previous: Option<&Saved>,
    perks: Vec<(usize, u32)>,
    mut read: impl FnMut(u32) -> Result<Vec<u8>, String>,
) -> (KeyIndex, Saved) {
    let mut index = KeyIndex {
        perks,
        ..KeyIndex::default()
    };
    let mut saved = Saved {
        snapshot: snapshot.clone(),
        actions: BTreeMap::new(),
    };
    let mut unchanged = BTreeMap::new();
    for tag in index
        .perks
        .iter()
        .map(|&(_, tag)| tag)
        .collect::<BTreeSet<_>>()
    {
        let package = TagHash(tag).pkg_id();
        let same_package = *unchanged.entry(package).or_insert_with(|| {
            previous.is_some_and(|saved| {
                saved.snapshot.for_package(package) == snapshot.for_package(package)
            })
        });
        let old = previous.and_then(|saved| saved.actions.get(&tag));
        let observation = if same_package && let Some(old) = old {
            Ok(old.clone())
        } else {
            read(tag).map(|bytes| {
                let digest = Sha256::digest(&bytes).into();
                if let Some(old) = old
                    && old.digest == digest
                {
                    return old.clone();
                }
                CachedAction {
                    digest,
                    usage: action::decode(&bytes).map(|action| KeyUsage::read(&action)),
                }
            })
        };
        match observation {
            Ok(observation) => {
                match &observation.usage {
                    Ok(usage) => {
                        index.actions.insert(tag, usage.clone());
                    }
                    Err(error) => index.issues.push(format!("Action 0x{tag:08X}: {error}")),
                }
                saved.actions.insert(tag, observation);
            }
            Err(error) => index.issues.push(error),
        }
    }
    (index, saved)
}

fn save(path: &Path, saved: &Saved) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or("The perk key cache has no parent directory")?;
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

#[cfg(test)]
mod tests;

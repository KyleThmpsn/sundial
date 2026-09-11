//! Native content-path references, with their exact source fields preserved.
//!
//! A content path is a name for its paired tag. References found in an owning
//! graph provide context only, never a name for every nested projectile.
use std::{collections::BTreeMap, path::Path, sync::Arc};

use serde::{Deserialize, Serialize};
use tiger_pkg::{PackageManager, TagHash};

use super::index_cache;
use crate::package_payload::{i64_at, relative_offset, u64_at};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentPath {
    pub source: u32,
    pub offset: usize,
    pub path: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reference {
    pub source: u32,
    pub source_class: u32,
    /// The tag lane. Its paired path pointer is eight bytes earlier.
    pub offset: usize,
    pub target: u32,
    pub target_class: u32,
    pub path: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Index {
    pub paths: Vec<ContentPath>,
    pub references: Vec<Reference>,
    pub scanned_resources: usize,
    pub errors: Vec<String>,
}

impl Index {
    #[must_use]
    pub fn names(&self) -> BTreeMap<u32, Vec<String>> {
        let mut names = BTreeMap::<u32, Vec<String>>::new();
        for reference in &self.references {
            names
                .entry(reference.target)
                .or_default()
                .push(reference.path.clone());
        }
        for paths in names.values_mut() {
            paths.sort();
            paths.dedup();
        }
        names
    }
}

/// The game's filename, with its spelling, separators and extensions preserved.
#[must_use]
pub fn asset_label(path: &str) -> String {
    path.rsplit(['\\', '/']).next().unwrap_or(path).to_owned()
}

fn content_paths(payload: &[u8]) -> BTreeMap<usize, String> {
    let mut found = BTreeMap::new();
    let mut start = 0;
    for (offset, &byte) in payload.iter().enumerate() {
        if (32..=126).contains(&byte) {
            continue;
        }
        if byte == 0 && offset - start >= 12 && offset - start <= 1024 {
            let bytes = &payload[start..offset];
            if (bytes.starts_with(b"content\\") || bytes.starts_with(b"content/"))
                && bytes.ends_with(b".tft")
                && let Ok(path) = std::str::from_utf8(bytes)
            {
                found.insert(start, path.to_owned());
            }
        }
        start = offset + 1;
    }
    found
}

fn references(
    payload: &[u8],
    paths: &BTreeMap<usize, String>,
    mut resolve: impl FnMut(u64) -> Option<(u32, u32)>,
) -> Vec<(usize, u32, u32, String)> {
    let mut result = Vec::new();
    for offset in (8..payload.len().saturating_sub(7)).step_by(4) {
        let pointer = offset - 8;
        let Ok(relative) = i64_at(payload, pointer) else {
            continue;
        };
        let Ok(start) = relative_offset(pointer, 0, relative) else {
            continue;
        };
        let Some(path) = paths.get(&start) else {
            continue;
        };
        let Ok(lane) = u64_at(payload, offset) else {
            continue;
        };
        if let Some((target, class)) = resolve(lane) {
            result.push((offset, target, class, path.clone()));
        }
    }
    result
}

/// Read every installed structured resource. Unpaired paths are retained too.
pub fn inspect(manager: &PackageManager, mut progress: impl FnMut(usize, usize)) -> Index {
    let mut index = Index::default();
    let total = manager
        .lookup
        .tag32_entries_by_pkg
        .values()
        .map(|entries| entries.iter().filter(|entry| entry.file_type == 8).count())
        .sum();
    for (&package, entries) in &manager.lookup.tag32_entries_by_pkg {
        for (ordinal, entry) in entries.iter().enumerate() {
            if entry.file_type != 8 {
                continue;
            }
            let tag = TagHash::new(package, ordinal as u16);
            index.scanned_resources += 1;
            if index.scanned_resources % 10_000 == 0 {
                progress(index.scanned_resources, total);
            }
            let payload = match manager.read_tag(tag) {
                Ok(bytes) if bytes.len() == entry.file_size as usize => bytes,
                Ok(_) => {
                    index.errors.push(format!(
                        "{tag}: resource size does not match its package entry"
                    ));
                    continue;
                }
                Err(error) => {
                    index.errors.push(format!("{tag}: {error}"));
                    continue;
                }
            };
            let paths = content_paths(&payload);
            if paths.is_empty() {
                continue;
            }
            let resolved = references(&payload, &paths, |lane| {
                let target = if let Ok(raw) = u32::try_from(lane) {
                    TagHash(raw)
                } else {
                    manager.lookup.tag64_entries.get(&lane)?.hash32
                };
                let target_entry = manager.get_entry(target)?;
                Some((target.0, target_entry.reference))
            });
            index.references.extend(resolved.into_iter().map(
                |(offset, target, target_class, path)| Reference {
                    source: tag.0,
                    source_class: entry.reference,
                    offset,
                    target,
                    target_class,
                    path,
                },
            ));
            index
                .paths
                .extend(paths.into_iter().map(|(offset, path)| ContentPath {
                    source: tag.0,
                    offset,
                    path,
                }));
        }
    }
    index.paths.sort_by_key(|path| (path.source, path.offset));
    index
        .references
        .sort_by_key(|reference| (reference.source, reference.offset, reference.target));
    progress(index.scanned_resources, total);
    index
}

static CACHE: index_cache::Cache<Index> = index_cache::Cache::new();

/// Cache only names and evidence. Compilation always resolves live package tags.
pub fn cached(
    packages: &Path,
    manager: &PackageManager,
    progress: impl FnMut(usize, usize),
) -> Result<Arc<Index>, String> {
    index_cache::cached(
        packages,
        "native-names",
        "tft-v1",
        &CACHE,
        || Ok(inspect(manager, progress)),
        |index| index.errors.is_empty(),
    )
}

#[cfg(test)]
mod tests;

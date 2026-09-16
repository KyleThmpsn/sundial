//! Native content-path references, with their exact source fields preserved.
//!
//! A content path is a name for its paired tag. References found in an owning
//! graph provide context only, never a name for every nested projectile.
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::Path,
    sync::Arc,
};

use serde::{Deserialize, Serialize};
use tiger_pkg::{PackageManager, TagHash};

use super::index_cache;
use crate::{
    package_payload::{i64_at, relative_offset, u64_at},
    weapon_entity::WEAPON_ENTITY_CLASS,
};
pub(crate) mod shards;

/// One aligned word inside a structured resource that equals a live weapon entity graph
/// tag. It is a candidate reference: a matching word is evidence that the source refers
/// to the graph, not proof that the native field is a tag.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntityReference {
    pub source: u32,
    pub source_class: u32,
    pub target: u32,
}

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
    /// Which resources refer to which weapon entity graphs, one row per distinct pair.
    /// An older cached index has none, which the cache version keeps from happening.
    #[serde(default)]
    pub entity_references: Vec<EntityReference>,
    /// Engine strings that name things without being `.tft` content paths: wwise event
    /// paths and the client's enum tables. They are never paired with a tag, so they feed
    /// the name-hash vocabulary and never identify a resource on their own.
    #[serde(default)]
    pub vocabulary: Vec<ContentPath>,
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

    /// Every content path found inside each resource, paired or not, by source tag.
    #[must_use]
    pub fn own_paths(&self) -> BTreeMap<u32, Vec<String>> {
        let mut own = BTreeMap::<u32, Vec<String>>::new();
        for path in &self.paths {
            own.entry(path.source).or_default().push(path.path.clone());
        }
        for paths in own.values_mut() {
            paths.sort();
            paths.dedup();
        }
        own
    }
}

/// Every live weapon entity graph tag, the targets the reference scan looks for.
pub(super) fn entity_graphs(manager: &PackageManager) -> HashSet<u32> {
    manager
        .get_all_by_reference(WEAPON_ENTITY_CLASS)
        .into_iter()
        .filter(|(_, entry)| entry.file_type == 8)
        .map(|(tag, _)| tag.0)
        .collect()
}

/// The live entity graph tags and the 64-bit lanes that resolve to them. Pattern graphs
/// refer to the entities they fire through 64-bit lanes, so a 32-bit word scan alone
/// misses most weapon and ability projectiles.
pub(super) struct EntityTargets {
    pub tags: HashSet<u32>,
    pub lanes: HashMap<u64, u32>,
}

impl EntityTargets {
    pub(super) fn new(manager: &PackageManager) -> Self {
        let tags = entity_graphs(manager);
        let lanes = manager
            .lookup
            .tag64_entries
            .iter()
            .filter(|(_, entry)| tags.contains(&entry.hash32.0))
            .map(|(lane, entry)| (*lane, entry.hash32.0))
            .collect();
        Self { tags, lanes }
    }

    /// A stable digest of the target set, so a cached shard scanned against a different
    /// set of graphs or lanes is not reused.
    pub(super) fn key(&self) -> u64 {
        let mut sorted = self.tags.iter().copied().map(u64::from).collect::<Vec<_>>();
        sorted.extend(self.lanes.keys().copied());
        sorted.sort_unstable();
        sorted
            .iter()
            .fold(0xCBF2_9CE4_8422_2325_u64, |hash, value| {
                (hash ^ value).wrapping_mul(0x0100_0000_01B3)
            })
    }
}

/// Distinct entity graph tags found in `payload`, as aligned 32-bit words or as 64-bit
/// lanes, excluding class handles and the resource's own tag.
pub(super) fn entity_words(payload: &[u8], source: u32, targets: &EntityTargets) -> Vec<u32> {
    if targets.tags.is_empty() {
        return Vec::new();
    }
    let mut found = payload
        .chunks_exact(4)
        .map(|word| u32::from_le_bytes(word.try_into().expect("four-byte chunk")))
        .filter(|word| {
            word & 0x8000_0000 != 0
                && word & 0xFFFF_0000 != 0x8080_0000
                && *word != source
                && targets.tags.contains(word)
        })
        .collect::<Vec<_>>();
    if !targets.lanes.is_empty() {
        found.extend(
            (0..payload.len().saturating_sub(7))
                .step_by(4)
                .filter_map(|offset| {
                    let lane = u64::from_le_bytes(payload[offset..offset + 8].try_into().ok()?);
                    targets.lanes.get(&lane).copied()
                })
                .filter(|tag| *tag != source),
        );
    }
    found.sort_unstable();
    found.dedup();
    found
}

/// The game's filename, with its spelling, separators and extensions preserved.
#[must_use]
pub fn asset_label(path: &str) -> String {
    path.rsplit(['\\', '/']).next().unwrap_or(path).to_owned()
}

/// The folder a content path sits in, without the leading `content` segment and with the
/// separators shown as ` / `, so assets from one folder group under one readable heading.
#[must_use]
pub fn asset_folder(path: &str) -> String {
    let mut segments = path
        .split(['\\', '/'])
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    segments.pop();
    if segments
        .first()
        .is_some_and(|first| first.eq_ignore_ascii_case("content"))
    {
        segments.remove(0);
    }
    if segments.is_empty() {
        "content".to_owned()
    } else {
        segments.join(" / ")
    }
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

/// Strings the engine uses as names outside `.tft` content paths. A wwise event path names
/// the ability or event that plays it. A resource whose strings include one of the form
/// `namespace_enums.e_name` is a client enum table, and every identifier in it is an
/// engine name (the ability enum lists `thermal_maul_super`, `glide` and `pulse_void`).
fn vocabulary_strings(payload: &[u8]) -> Vec<(usize, String)> {
    let mut runs = Vec::new();
    let mut start = 0;
    for (offset, &byte) in payload.iter().chain(std::iter::once(&0)).enumerate() {
        if (32..=126).contains(&byte) {
            continue;
        }
        if byte == 0
            && offset - start >= 3
            && offset - start <= 1024
            && let Ok(text) = std::str::from_utf8(&payload[start..offset])
        {
            runs.push((start, text));
        }
        start = offset + 1;
    }
    let table = runs
        .iter()
        .any(|(_, text)| text.contains("_enums.") && !text.contains(['\\', '/']));
    runs.into_iter()
        .filter(|(_, text)| {
            let lower = text.to_ascii_lowercase();
            let wwise = (lower.starts_with("content\\") || lower.starts_with("content/"))
                && lower.ends_with(".wwise_event");
            let identifier = table
                && text
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_');
            wwise || identifier
        })
        .map(|(offset, text)| (offset, text.to_owned()))
        .collect()
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
    let targets = EntityTargets::new(manager);
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
            index.entity_references.extend(
                entity_words(&payload, tag.0, &targets)
                    .into_iter()
                    .map(|target| EntityReference {
                        source: tag.0,
                        source_class: entry.reference,
                        target,
                    }),
            );
            index
                .vocabulary
                .extend(
                    vocabulary_strings(&payload)
                        .into_iter()
                        .map(|(offset, path)| ContentPath {
                            source: tag.0,
                            offset,
                            path,
                        }),
                );
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
        .vocabulary
        .sort_by_key(|path| (path.source, path.offset));
    index
        .references
        .sort_by_key(|reference| (reference.source, reference.offset, reference.target));
    index
        .entity_references
        .sort_by_key(|reference| (reference.source, reference.target));
    progress(index.scanned_resources, total);
    index
}

static CACHE: index_cache::Cache<Index> = index_cache::Cache::new();

/// Opening a single effect must not trigger installation-wide name discovery.
/// A missing full index is reported separately from missing native references.
pub fn cached_only(packages: &Path) -> Result<Option<Arc<Index>>, String> {
    index_cache::cached_only(packages, "native-names", "tft-v4", &CACHE)
}

/// Cache only names and evidence. Compilation always resolves live package tags.
pub fn cached(
    packages: &Path,
    manager: &PackageManager,
    progress: impl FnMut(usize, usize),
) -> Result<Arc<Index>, String> {
    index_cache::cached(
        packages,
        "native-names",
        "tft-v4",
        &CACHE,
        || shards::inspect(packages, manager, progress),
        // Read errors remain visible in the index. They must not force an
        // otherwise identical installation to repeat the entire scan on launch.
        |_| true,
    )
}

#[cfg(test)]
mod tests;

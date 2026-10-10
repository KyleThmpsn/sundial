//! The named points a weapon's gear art carries: where it is held, where it fires, where a
//! case leaves it.
//!
//! A marker set is one component of a gear entity. Each row holds a position and the FNV-1
//! hash of the marker's name. The runtime resolves markers by that name, which is why an
//! imported model that keeps its donor's marker set aims at the donor's sight.
//!
//! The names themselves are not stored. Those below were recovered by generating candidates
//! from a weapon vocabulary and keeping only hashes exactly one candidate reached, the same
//! method and the same caution as the runtime member names. A recovered name identifies a
//! marker consistently. It is not evidence of what it is for.
use crate::{
    package_authoring::resolve_live_named_tag,
    package_payload::*,
    package_runtime::{index_cache, parallel, reader::PackageManager},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    path::Path,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};
mod names;
mod shards;
pub use names::marker_name;

const GEAR_ART_TABLE: u32 = 0x8080_5DF5;
const ARRANGEMENT_ROW: u32 = 0x8080_5DFB;
const ARRANGEMENT_SLOTS: u32 = 0x8080_5DFE;
const ARRANGEMENT_KEYS: u32 = 0x8080_5E01;
const ASSIGNMENT_MAP: u32 = 0x8080_56EA;
const ASSIGNMENT_ROW: u32 = 0x8080_56EC;
const RELATION: u32 = 0x8080_744A;
const ENTITY: u32 = 0x8080_9C0F;
const COMPONENT_ROW: u32 = 0x8080_9C04;
/// The marker-set component, "Query Markers", and the row class of the array inside it.
const MARKER_COMPONENT: u32 = 0x8080_8506;
const MARKER_ROW: u32 = 0x8080_8513;
/// The marker array's descriptor, relative to the component's data struct.
const MARKER_DESCRIPTOR: usize = 0xB0;
const MARKER_STRIDE: usize = 64;
/// A row is a rotation, then a position, then the name. Two rows may share a name and a
/// position and differ only in rotation, which is how a weapon aims one point two ways.
const MARKER_ORIENTATION: usize = 0x10;
const MARKER_POSITION: usize = 0x20;
const MARKER_NAME: usize = 0x30;
/// An appearance draws few objects, and each carries at most a handful of marker sets.
const MAX_ENTITIES: usize = 64;

/// One named point on a piece of gear art, in the model's own space.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Marker {
    pub name: u32,
    pub position: [f32; 3],
    /// The direction the marker faces, as a quaternion in x, y, z, w order. Two markers may
    /// share a name and a position and differ only here.
    pub orientation: [f32; 4],
}

impl Marker {
    /// Whether the marker faces straight ahead, which most do. Only a turned marker is worth
    /// reporting, so the report stays readable.
    #[must_use]
    pub fn is_aligned(&self) -> bool {
        let [x, y, z, w] = self.orientation;
        x.abs() < 1e-4 && y.abs() < 1e-4 && z.abs() < 1e-4 && (w.abs() - 1.0).abs() < 1e-4
    }

    /// The marker's recovered name, or its hash when it has none.
    #[must_use]
    pub fn label(&self) -> String {
        marker_name(self.name).map_or_else(|| format!("0x{:08X}", self.name), ToOwned::to_owned)
    }
}

/// One gear entity's marker set.
#[derive(Clone, Debug, PartialEq)]
pub struct MarkerSet {
    pub entity: u32,
    pub component: u32,
    pub markers: Vec<Marker>,
}

/// How a marker relates to the ones around it: the named marker nearest to it, and how far.
///
/// A hash whose name is unrecovered is still placed somewhere meaningful, and saying it sits
/// eleven millimetres from `primary_fire` tells a reader more than the hash does. This is a
/// statement about geometry, which the data supports, and not a guess at the name.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Neighbour {
    pub name: &'static str,
    /// Distance in the model's own units, which are metres.
    pub distance: f32,
}

impl std::fmt::Display for Neighbour {
    /// Millimetres, because every distance that matters here is a centimetre or two. Markers
    /// that share a point are common and saying "0mm" reads as a missing number, so they say
    /// so instead.
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.distance < 0.000_5 {
            write!(out, "on {}", self.name)
        } else {
            write!(out, "by {} {:.0}mm", self.name, self.distance * 1000.0)
        }
    }
}

/// The named marker nearest to `marker`, across every set of one appearance.
///
/// `None` when the marker is itself named, when nothing named shares the appearance, or when
/// the nearest named marker is too far off to say anything useful about it.
#[must_use]
pub fn nearest_named(sets: &[MarkerSet], marker: &Marker) -> Option<Neighbour> {
    /// Past this, "near" stops meaning anything on a weapon a metre long.
    const REACH: f32 = 0.5;
    if marker_name(marker.name).is_some() {
        return None;
    }
    // A character's marker set puts every row at the origin and lets the skeleton place them
    // at runtime. Everything is then "on" everything, which is true and tells a reader
    // nothing, so a set that collapses to one point is left unplaced.
    let mut positions = sets.iter().flat_map(|set| &set.markers).map(|m| m.position);
    let first = positions.next()?;
    if !positions.any(|position| position != first) {
        return None;
    }
    sets.iter()
        .flat_map(|set| &set.markers)
        .filter_map(|other| Some((marker_name(other.name)?, other.position)))
        .map(|(name, position)| Neighbour {
            name,
            distance: position
                .iter()
                .zip(&marker.position)
                .map(|(a, b)| (a - b) * (a - b))
                .sum::<f32>()
                .sqrt(),
        })
        .filter(|neighbour| neighbour.distance <= REACH)
        .min_by(|a, b| a.distance.total_cmp(&b.distance))
}

/// A marker's name, or its hash with the named marker it sits next to.
#[must_use]
pub fn describe(sets: &[MarkerSet], marker: &Marker) -> String {
    let label = marker.label();
    match nearest_named(sets, marker) {
        Some(near) => format!("{label} {near}"),
        None => label,
    }
}

/// One marker name across the whole game: how widely it is used, where to find it, and what
/// it sits beside.
#[derive(Clone, Debug, PartialEq)]
pub struct MarkerEntry {
    pub hash: u32,
    /// The recovered name, when there is one.
    pub name: Option<&'static str>,
    /// How many objects carry a marker of this name.
    pub objects: usize,
    /// A few of those objects, so one can be opened and looked at.
    pub examples: Vec<u32>,
    /// The named marker that most often sits nearest to this one, with the median distance
    /// across every object that carries both. This is what stands in for an unrecovered name.
    pub neighbour: Option<Neighbour>,
}

impl MarkerEntry {
    /// What to show in a list: the name, or the hash and what it sits beside.
    #[must_use]
    pub fn label(&self) -> String {
        match (self.name, self.neighbour) {
            (Some(name), _) => name.to_owned(),
            (None, Some(near)) => format!("0x{:08X} {near}", self.hash),
            (None, None) => format!("0x{:08X}", self.hash),
        }
    }
}

/// Every marker name in the game, in descending order of how many objects carry it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MarkerIndex {
    pub entries: Vec<MarkerEntry>,
    /// Every object that carries any marker, with the markers it carries, sorted by tag.
    /// Only a few thousand objects in the game have a marker set, so holding them all is what
    /// makes the reverse lookup possible: from an object to the points on it.
    pub objects: Vec<MarkerObject>,
    /// Native names attached directly to object tags, when the package metadata has them.
    pub tag_names: std::collections::BTreeMap<u32, String>,
    /// How many objects were read, most of which carry no marker at all.
    pub scanned: usize,
    pub sets: usize,
}

/// One object and the markers on it.
#[derive(Clone, Debug, PartialEq)]
pub struct MarkerObject {
    pub entity: u32,
    pub markers: Vec<Marker>,
}

impl MarkerIndex {
    #[must_use]
    pub fn named(&self) -> usize {
        self.entries.iter().filter(|e| e.name.is_some()).count()
    }

    /// The markers on one object.
    #[must_use]
    pub fn object(&self, entity: u32) -> Option<&MarkerObject> {
        self.objects
            .binary_search_by_key(&entity, |object| object.entity)
            .ok()
            .map(|index| &self.objects[index])
    }

    /// Where one marker sits on one object, when that object carries it.
    #[must_use]
    pub fn placement(&self, entity: u32, hash: u32) -> Option<&Marker> {
        self.object(entity)?
            .markers
            .iter()
            .find(|marker| marker.name == hash)
    }
}

/// How many objects of each name to keep as examples. Enough to look at, not enough to bloat.
const MAX_EXAMPLES: usize = 12;

/// Objects that carry markers, as read from the packages. The index is derived from this, and
/// this is what the disk keeps. Names are applied when the index is derived, so a release that
/// recovers more of them reads nothing again.
#[derive(Debug, Default, Serialize, Deserialize)]
pub(crate) struct Scan {
    /// How many objects were read, most of which carry no marker at all.
    scanned: usize,
    /// In tag order.
    carriers: Vec<Carrier>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Carrier {
    entity: u32,
    /// How many marker-set components the object holds.
    sets: usize,
    markers: Vec<Marker>,
}

static CACHE: index_cache::Cache<Scan> = index_cache::Cache::new();

/// The error a cancelled read reports.
pub const CANCELLED: &str = "The marker read was cancelled";

/// The whole-game marker index, from the disk when these packages were read before.
///
/// An uncached first read opens every object in the game on a pool of workers, so it belongs on
/// a background thread. Later reads reuse unchanged object and component packages independently.
/// `progress` is called with how far it is, out of a fixed total. Setting `cancel` stops the
/// read. Complete package shards survive a cancelled read.
pub fn cached_index(
    packages: &Path,
    manager: &PackageManager,
    progress: impl Fn(usize, usize) + Sync,
    cancel: &AtomicBool,
) -> Result<MarkerIndex, String> {
    let scan = index_cache::cached(
        packages,
        crate::sandbox_perk::CACHE_DIRECTORY,
        "markers-v1",
        &CACHE,
        || scan(manager, &progress, cancel),
        |_| true,
    )?;
    Ok(index_with_tag_names(index_from(&scan), manager))
}

/// The whole-game marker index, read fresh from the packages.
pub fn build_index(
    manager: &PackageManager,
    progress: impl Fn(usize, usize) + Sync,
) -> MarkerIndex {
    let scan = scan(manager, &progress, &AtomicBool::new(false))
        .expect("a read nobody cancels runs to the end");
    index_with_tag_names(index_from(&scan), manager)
}

fn index_with_tag_names(mut index: MarkerIndex, manager: &PackageManager) -> MarkerIndex {
    let objects: BTreeSet<u32> = index.objects.iter().map(|object| object.entity).collect();
    index.tag_names = manager
        .lookup
        .named_tags
        .iter()
        .filter(|named| objects.contains(&named.hash.0))
        .map(|named| (named.hash.0, named.name.clone()))
        .collect();
    index
}

/// Reads every object's marker sets in two passes, each grouped by package.
///
/// Most of an object's components live in other packages than the object itself, and far
/// more packages hold them than the reader keeps open. Reading object by object therefore
/// reopened packages constantly and read shared components once per object. Reading every
/// object's component list first, then every distinct component once in package order, opens
/// each package about once per pass.
fn scan(
    manager: &PackageManager,
    progress: &(impl Fn(usize, usize) + Sync),
    cancel: &AtomicBool,
) -> Result<Scan, String> {
    use std::collections::BTreeMap;
    let stopped = || cancel.load(Ordering::Relaxed);
    let entities: Vec<u32> = manager
        .get_all_by_reference(ENTITY)
        .into_iter()
        .map(|(tag, _)| tag.0)
        .collect();
    let total = entities.len();
    // The two passes are reported as one figure. Objects are the first part, components
    // the rest, since there are several components to each object.
    let objects_share = PROGRESS_STEPS / 5;
    progress(0, PROGRESS_STEPS);

    let lists = shards::read(
        manager,
        shards::Kind::Objects,
        &package_jobs(&entities),
        |entity| entity_components(manager, entity),
        cancel,
        |read| progress(read * objects_share / total.max(1), PROGRESS_STEPS),
    )?;
    if stopped() {
        return Err(CANCELLED.to_owned());
    }

    let components: Vec<u32> = lists
        .iter()
        .filter_map(|(_, list)| list.as_ref().ok())
        .flatten()
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let distinct = components.len();
    let read = shards::read(
        manager,
        shards::Kind::Components,
        &package_jobs(&components),
        |tag| component_markers(manager, tag),
        cancel,
        |finished| {
            progress(
                objects_share + finished * (PROGRESS_STEPS - objects_share) / distinct.max(1),
                PROGRESS_STEPS,
            )
        },
    )?;
    if stopped() {
        return Err(CANCELLED.to_owned());
    }
    let read: BTreeMap<u32, Result<Option<Vec<Marker>>, String>> = read.into_iter().collect();

    // Each object as `entity_marker_sets` would have read it: its marker sets in its own
    // order, and nothing at all when one of them does not parse.
    let mut carriers = Vec::new();
    for (entity, list) in lists {
        let Ok(list) = list else {
            continue;
        };
        let mut sets = 0;
        let mut markers = Vec::new();
        let mut whole = true;
        for tag in list {
            match read.get(&tag) {
                Some(Ok(Some(found))) => {
                    sets += 1;
                    markers.extend_from_slice(found);
                }
                Some(Ok(None)) => {}
                Some(Err(_)) | None => {
                    whole = false;
                    break;
                }
            }
        }
        if whole && sets > 0 {
            carriers.push(Carrier {
                entity,
                sets,
                markers,
            });
        }
    }
    carriers.sort_by_key(|carrier| carrier.entity);
    progress(PROGRESS_STEPS, PROGRESS_STEPS);
    Ok(Scan {
        scanned: total,
        carriers,
    })
}

/// How finely a read reports its progress.
const PROGRESS_STEPS: usize = 1000;

/// One job per package keeps workers on different reader locks. Splitting each package into
/// consecutive chunks puts the first several workers on the same lock and serializes the scan.
fn package_jobs(tags: &[u32]) -> Vec<Vec<u32>> {
    let mut by_package: std::collections::BTreeMap<u16, Vec<u32>> = Default::default();
    for &tag in tags {
        by_package
            .entry(tiger_pkg::TagHash(tag).pkg_id())
            .or_default()
            .push(tag);
    }
    by_package.into_values().collect()
}

/// Counts, examples and neighbours for every marker name, from the objects that carry them.
fn index_from(scan: &Scan) -> MarkerIndex {
    use std::collections::BTreeMap;
    let mut objects: BTreeMap<u32, usize> = BTreeMap::new();
    let mut examples: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    // Every nearest-named observation, so the reported distance is a median over objects
    // rather than whatever the first object happened to say.
    let mut near: BTreeMap<u32, BTreeMap<&'static str, Vec<f32>>> = BTreeMap::new();
    let mut sets = 0usize;
    let mut carriers = Vec::with_capacity(scan.carriers.len());
    for carrier in &scan.carriers {
        sets += carrier.sets;
        // Neighbours are measured across every set of an object at once, so one set holding
        // all of its markers answers the same.
        let found = [MarkerSet {
            entity: carrier.entity,
            component: 0,
            markers: carrier.markers.clone(),
        }];
        let mut seen = BTreeSet::new();
        for marker in &found[0].markers {
            if !seen.insert(marker.name) {
                continue;
            }
            *objects.entry(marker.name).or_default() += 1;
            let shown = examples.entry(marker.name).or_default();
            if shown.len() < MAX_EXAMPLES {
                shown.push(carrier.entity);
            }
            if let Some(neighbour) = nearest_named(&found, marker) {
                near.entry(marker.name)
                    .or_default()
                    .entry(neighbour.name)
                    .or_default()
                    .push(neighbour.distance);
            }
        }
        let [set] = found;
        carriers.push(MarkerObject {
            entity: carrier.entity,
            markers: set.markers,
        });
    }
    let mut entries: Vec<_> = objects
        .into_iter()
        .map(|(hash, count)| MarkerEntry {
            hash,
            name: marker_name(hash),
            objects: count,
            examples: examples.remove(&hash).unwrap_or_default(),
            neighbour: near.remove(&hash).and_then(|by_name| {
                // The name seen nearest most often wins; ties go to the closer one.
                by_name
                    .into_iter()
                    .map(|(name, mut distances)| {
                        distances.sort_by(f32::total_cmp);
                        let median = distances[distances.len() / 2];
                        (distances.len(), name, median)
                    })
                    .max_by(|a, b| a.0.cmp(&b.0).then(b.2.total_cmp(&a.2)))
                    .map(|(_, name, distance)| Neighbour { name, distance })
            }),
        })
        .collect();
    entries.sort_by(|a, b| b.objects.cmp(&a.objects).then(a.hash.cmp(&b.hash)));
    MarkerIndex {
        entries,
        // Already in tag order, which `object` relies on to find one without a second map.
        objects: carriers,
        tag_names: Default::default(),
        scanned: scan.scanned,
        sets,
    }
}

/// Every marker set reached from one gear-art arrangement, in entity order.
///
/// An appearance whose objects carry no markers yields an empty list rather than an error:
/// most of a weapon's parts are geometry alone.
pub fn read_appearance(packages: &Path, arrangement: u16) -> Result<Vec<MarkerSet>, String> {
    let manager = crate::investment::discovery::open_packages(packages)?;
    read_appearance_with_manager(&manager, arrangement, false)
}

/// As [`read_appearance`], with every alternative of every region: the barrels, sights and
/// magazines a socket can swap in, each carrying its own markers. A weapon aims with its sight
/// part's own set, which the default parts alone often lack.
pub fn read_appearance_parts(packages: &Path, arrangement: u16) -> Result<Vec<MarkerSet>, String> {
    let manager = crate::investment::discovery::open_packages(packages)?;
    read_appearance_with_manager(&manager, arrangement, true)
}

fn read_appearance_with_manager(
    manager: &PackageManager,
    arrangement: u16,
    alternatives: bool,
) -> Result<Vec<MarkerSet>, String> {
    let globals = manager.read_tag(resolve_live_named_tag(manager, "investment_globals", None)?)?;
    let table = checked(manager, u32_at(&globals, 0x430)?, GEAR_ART_TABLE)?;
    let assets = manager.read_tag(resolve_live_named_tag(manager, "investment_assets", None)?)?;
    let map = checked(manager, u32_at(&assets, 0x20)?, ASSIGNMENT_MAP)?;
    let (count, rows) = array(&map, 8, ASSIGNMENT_ROW, 8, 100_000)?;
    let mut entities = BTreeSet::new();
    for key in assignment_keys(&table, arrangement, alternatives)? {
        for index in 0..count {
            let row = rows + index * 8;
            if u32_at(&map, row)? != key {
                continue;
            }
            let relation = checked(manager, u32_at(&map, row + 4)?, RELATION)?;
            // An empty part names no entity and so carries no markers.
            let entity = u32_at(&relation, 0x10)?;
            if entity != u32::MAX {
                entities.insert(entity);
            }
            break;
        }
    }
    if entities.len()
        > if alternatives {
            MAX_ENTITIES * 4
        } else {
            MAX_ENTITIES
        }
    {
        return Err("This appearance draws more objects than markers are read for".into());
    }
    let mut sets = Vec::new();
    for entity in entities {
        sets.extend(entity_marker_sets(manager, entity)?);
    }
    Ok(sets)
}

/// The marker sets held by one gear entity.
fn entity_marker_sets(manager: &PackageManager, entity: u32) -> Result<Vec<MarkerSet>, String> {
    let mut sets = Vec::new();
    for tag in entity_components(manager, entity)? {
        if let Some(markers) = component_markers(manager, tag)? {
            sets.push(MarkerSet {
                entity,
                component: tag,
                markers,
            });
        }
    }
    Ok(sets)
}

/// The components one gear entity lists, in its own order.
fn entity_components(manager: &PackageManager, entity: u32) -> Result<Vec<u32>, String> {
    let payload = checked(manager, entity, ENTITY)?;
    let (count, rows) = array(&payload, 0x10, COMPONENT_ROW, 12, 4096)?;
    (0..count)
        .map(|index| u32_at(&payload, rows + index * 12))
        .collect()
}

/// The markers of one component, `None` when it is not a marker set.
///
/// A component row can point at anything the object holds, so an unreadable one is not a
/// marker set rather than an error. A marker set that does not parse is an error.
fn component_markers(manager: &PackageManager, tag: u32) -> Result<Option<Vec<Marker>>, String> {
    let Ok(component) = manager.read_tag(tag) else {
        return Ok(None);
    };
    match marker_data(&component)? {
        Some(data) => read_component(&component, data).map(Some),
        None => Ok(None),
    }
}

/// The data struct of `component` when it is a marker set, `None` when it is another component.
/// A marker set whose data struct cannot be reached is an error.
fn marker_data(component: &[u8]) -> Result<Option<usize>, String> {
    // The component's class is the word before its header, not before its data struct.
    let Ok(header) = pointer(component, 0x10) else {
        return Ok(None);
    };
    if header < 4 || u32_at(component, header - 4) != Ok(MARKER_COMPONENT) {
        return Ok(None);
    }
    pointer(component, 0x18).map(Some)
}

/// Whether `component`, a component's whole payload, is a marker set.
#[must_use]
pub fn is_marker_set(component: &[u8]) -> bool {
    matches!(marker_data(component), Ok(Some(_)))
}

/// Moves every row of a marker set whose name has an offset by that offset, in the model's own
/// units, which are metres. Rows that share a name move together, so a marker kept in two
/// orientations stays one point. Only positions change: the rows keep their count, names,
/// orientations and binding words, so nothing that references the set moves. Returns how many
/// rows moved.
pub fn offset_markers(component: &mut [u8], offsets: &[(u32, [f32; 3])]) -> Result<usize, String> {
    let data = marker_data(component)?.ok_or("The component is not a marker set")?;
    let descriptor = data
        .checked_add(MARKER_DESCRIPTOR)
        .ok_or("The marker array descriptor is outside the component")?;
    let (count, rows) = array(component, descriptor, MARKER_ROW, MARKER_STRIDE, 4096)?;
    let mut moved = 0;
    for index in 0..count {
        let row = rows + index * MARKER_STRIDE;
        let name = u32_at(component, row + MARKER_NAME)?;
        let Some((_, offset)) = offsets.iter().find(|(marker, _)| *marker == name) else {
            continue;
        };
        for (axis, delta) in offset.iter().enumerate() {
            let at = row + MARKER_POSITION + axis * 4;
            let value = f32::from_bits(u32_at(component, at)?) + delta;
            if !value.is_finite() {
                return Err("A moved marker has an unusable position".into());
            }
            component[at..at + 4].copy_from_slice(&value.to_bits().to_le_bytes());
        }
        moved += 1;
    }
    Ok(moved)
}

/// Moves every row of a marker set by `delta`, in metres, as when the whole model moves with
/// them. Only positions change, as in [`offset_markers`]. Returns how many rows moved.
pub fn shift_markers(component: &mut [u8], delta: [f32; 3]) -> Result<usize, String> {
    let data = marker_data(component)?.ok_or("The component is not a marker set")?;
    let descriptor = data
        .checked_add(MARKER_DESCRIPTOR)
        .ok_or("The marker array descriptor is outside the component")?;
    let (count, rows) = array(component, descriptor, MARKER_ROW, MARKER_STRIDE, 4096)?;
    for index in 0..count {
        let row = rows + index * MARKER_STRIDE;
        for (axis, delta) in delta.iter().enumerate() {
            let at = row + MARKER_POSITION + axis * 4;
            let value = f32::from_bits(u32_at(component, at)?) + delta;
            if !value.is_finite() {
                return Err("A moved marker has an unusable position".into());
            }
            component[at..at + 4].copy_from_slice(&value.to_bits().to_le_bytes());
        }
    }
    Ok(count)
}

/// The markers of one marker-set component, given its data struct's offset.
fn read_component(component: &[u8], data: usize) -> Result<Vec<Marker>, String> {
    let descriptor = data
        .checked_add(MARKER_DESCRIPTOR)
        .ok_or("The marker array descriptor is outside the component")?;
    let (count, rows) = array(component, descriptor, MARKER_ROW, MARKER_STRIDE, 4096)?;
    (0..count)
        .map(|index| {
            let row = rows + index * MARKER_STRIDE;
            let read = |at: usize| -> Result<f32, String> {
                let value = f32::from_bits(u32_at(component, at)?);
                if value.is_finite() {
                    Ok(value)
                } else {
                    Err("A marker has an unusable transform".to_owned())
                }
            };
            let mut position = [0.0; 3];
            for (axis, value) in position.iter_mut().enumerate() {
                *value = read(row + MARKER_POSITION + axis * 4)?;
            }
            let mut orientation = [0.0; 4];
            for (axis, value) in orientation.iter_mut().enumerate() {
                *value = read(row + MARKER_ORIENTATION + axis * 4)?;
            }
            Ok(Marker {
                name: u32_at(component, row + MARKER_NAME)?,
                position,
                orientation,
            })
        })
        .collect()
}

/// The gear-art assignment keys of one arrangement row. A region lists alternatives rather
/// than simultaneous geometry, so only its base attachment is followed, matching the preview.
fn assignment_keys(
    table: &[u8],
    arrangement: u16,
    alternatives: bool,
) -> Result<BTreeSet<u32>, String> {
    let (count, rows) = array(table, 8, ARRANGEMENT_ROW, 0x20, 65536)?;
    if usize::from(arrangement) >= count {
        return Err("Appearance row is outside the installed table".into());
    }
    let row = rows + usize::from(arrangement) * 0x20;
    let mut keys = BTreeSet::new();
    if u64_at(table, row + 0x10)? == 0 {
        keys.insert(u32_at(table, row + 8)?);
        keys.insert(u32_at(table, row + 12)?);
    } else {
        let (count, rows) = array(table, row + 0x10, ARRANGEMENT_SLOTS, 8, 4096)?;
        for index in 0..count {
            let resource = pointer(table, rows + index * 8)?;
            let (count, rows) = array(table, resource + 8, ARRANGEMENT_KEYS, 4, 65536)?;
            let followed = if alternatives { count } else { count.min(1) };
            for key in 0..followed {
                keys.insert(u32_at(table, rows + key * 4)?);
            }
        }
    }
    keys.remove(&0);
    keys.remove(&u32::MAX);
    keys.remove(&crate::hash::FNV1_EMPTY_HASH);
    Ok(keys)
}

fn pointer(bytes: &[u8], offset: usize) -> Result<usize, String> {
    relative_offset(offset, 0, i64_at(bytes, offset)?)
}

fn checked(manager: &PackageManager, tag: u32, class: u32) -> Result<Vec<u8>, String> {
    let entry = manager
        .get_entry(tag)
        .ok_or_else(|| format!("Resource 0x{tag:08X} is missing"))?;
    if entry.reference != class {
        return Err(format!("Resource 0x{tag:08X} is not a 0x{class:08X}"));
    }
    manager.read_tag(tag)
}

fn array(
    bytes: &[u8],
    offset: usize,
    class: u32,
    stride: usize,
    limit: usize,
) -> Result<(usize, usize), String> {
    let (count, _, rows, actual) = native_array_at(bytes, offset)?;
    // A native empty array has a zero pointer and no header, so it has no class to check.
    if count != 0 && actual != class {
        return Err(format!(
            "Array at {offset:#x} has class {actual:08X}, expected {class:08X}"
        ));
    }
    if count > limit
        || count
            .checked_mul(stride)
            .and_then(|size| rows.checked_add(size))
            .is_none_or(|end| end > bytes.len())
    {
        return Err(format!(
            "Array at {offset:#x} holds {count} rows of {stride} bytes, which its payload cannot"
        ));
    }
    Ok((count, rows))
}

#[cfg(test)]
mod tests;

//! A connected native bubble graph, with separately requested geometry and spatial crossings.
mod lighting;
mod tome;

use crate::{
    AuthoringResult, NewTagSpec, NewTagStorageMode,
    error::invalid,
    shared_tag_dependency_index::{
        enroll_dependencies, remove_dependencies, remove_hash_dependencies,
    },
    tag_payload::{
        append_native_array, array_at, read_i64, read_tag, read_u16, read_u32, read_u64,
        synchronize_payload_size, write_relative_pointer, write_u32, write_u64,
    },
};
use parhelion_import::halo_reach::level::streaming::{self as source, Bounds};
use std::collections::{BTreeMap, BTreeSet};
use sundial::package_authoring::PackageManager;
use tiger_pkg::TagHash;

/// Visibility placement, world record, raw tome and cluster metadata per bubble.
pub(super) const ENTRIES: usize = 4;

pub(super) fn identity(name: &str) -> u64 {
    name.bytes().fold(0xCBF2_9CE4_8422_2325u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01B3)
    }) | 1
}

pub(super) struct Layout {
    pub plan: source::Plan,
    pub names: Vec<String>,
    spawn_mask: u64,
    classes: BTreeMap<TagHash, u32>,
    aliases: BTreeMap<u64, TagHash>,
    table: (TagHash, Vec<u8>),
    world: (TagHash, Vec<u8>),
    metadata: (TagHash, Vec<u8>),
    raw: TagHash,
}

/// The clusters of an area's spatial tome: crossing descriptors, 20-byte cluster rows and the
/// regions each cluster holds.
pub(super) struct Cells {
    descriptors: Vec<u8>,
    clusters: Vec<u8>,
    regions: Vec<tome::Region>,
    /// Explicit probe choices for the lighting clusters appended after the crossing clusters.
    lighting_probes: Vec<u16>,
}

impl Cells {
    /// A lighting cell: a cluster of its own with cluster 0's row, no crossing, at its middle. A
    /// lit object finds the cell it stands in through its connected-region query, and the
    /// occlusion record assigns each cluster a probe ([`Cells::assignments`]).
    fn push_lighting(&mut self, lighting::Cell { bounds, probe }: lighting::Cell) {
        let cluster = (self.clusters.len() / 20) as u32;
        let mut row = self.clusters[..20].to_vec();
        for (k, value) in bounds.center().into_iter().enumerate() {
            row[k * 4..k * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        self.clusters.extend(row);
        self.regions.push(tome::Region { bounds, cluster });
        self.lighting_probes.push((probe + 1) as u16);
    }

    /// Each cluster's probe in the occlusion record, whose probes after the first are `probes`.
    /// Cluster 0, the space outside every cell and crossing, takes the record's first probe, the
    /// open-ground fallback every stock record holds. Crossings take the probe nearest their
    /// middle. Lighting cells retain the in-cell probe chosen during adaptive subdivision.
    pub(super) fn assignments(&self, probes: &[[f32; 4]]) -> Vec<u16> {
        let first_lighting = self.clusters.len() / 20 - self.lighting_probes.len();
        self.clusters
            .chunks_exact(20)
            .enumerate()
            .map(|(cluster, row)| {
                if cluster == 0 {
                    return 0;
                }
                if cluster >= first_lighting {
                    return self.lighting_probes[cluster - first_lighting];
                }
                let middle: [f32; 3] = std::array::from_fn(|k| {
                    f32::from_le_bytes(row[k * 4..k * 4 + 4].try_into().unwrap())
                });
                probes
                    .iter()
                    .enumerate()
                    .map(|(index, probe)| {
                        let distance = (0..3).map(|k| (probe[k] - middle[k]).powi(2)).sum::<f32>();
                        (distance, index)
                    })
                    .min_by(|a, b| a.0.total_cmp(&b.0))
                    .map_or(0, |(_, index)| index as u16 + 1)
            })
            .collect()
    }

    fn push(&mut self, bounds: Bounds, phase: u8, descriptor: usize) {
        let cluster = (self.clusters.len() / 20) as u32;
        let mut row = [0u8; 20];
        for (k, value) in bounds.center().into_iter().enumerate() {
            row[k * 4..k * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        row[16] = phase;
        row[17] = descriptor as u8;
        row[18..].fill(0xFF);
        self.clusters.extend(row);
        self.regions.push(tome::Region { bounds, cluster });
    }
}

/// The visibility record's table of static instance identifiers (`+0x18`: a seed, then at `+0x20`
/// 16-byte rows of a u64 key, a u32 value and padding) and its list from each value's index to a
/// tome object (`+0x30`). A static instance's key is its bounds-row identifier with a zero upper
/// half, as in Distant Shore's record, whose 2,084 listed instances all take that form.
const VISIBILITY_SEED: usize = 0x18;
const VISIBILITY_ROWS: usize = 0x20;
const VISIBILITY_OBJECTS: usize = 0x30;
const ROW_CLASS: u32 = 0x8080_733E;
const OBJECT_INDEX_CLASS: u32 = 0x8080_0007;
/// A row's value: bit 30 marks a stored key, bit 31 a row a probe passes on to the next, and the
/// low 30 bits the key's index in the `+0x30` list.
const STORED: u32 = 0x4000_0000;
const PROBED_PAST: u32 = 0x8000_0000;

/// The client's slot hash (`011501A0`, called by the lookup at `01165F00`): Thomas Wang's 64 to
/// 32-bit integer hash over the key, the seed duplicated into both halves first.
fn slot_hash(seed: u32, key: u64) -> u32 {
    let mut h = ((u64::from(seed) << 32) | u64::from(seed)) ^ !key;
    h = h.wrapping_add(key << 18);
    h ^= h >> 31;
    h = h.wrapping_mul(21);
    h ^= h >> 11;
    h = h.wrapping_mul(65);
    h ^= h >> 22;
    h as u32
}

/// The seed, rows and object indices for area `name`'s instances, instance `i` being tome object
/// `i`. The client hashes a key with the seed, masks it with the row count less one and probes
/// forward: a matching row with bit 30 returns its index, and a different row ends the search
/// unless its bit 31 is set. Rows hold at least twice the keys, a power of two, as stock tables do,
/// and inserting a key sets bit 31 on every occupied row it passes.
fn visibility_table(
    name: &str,
    visible: &[parhelion_import::halo_reach::level::native::Visible],
) -> AuthoringResult<(u32, Vec<u8>, Vec<u8>)> {
    let seed = sundial::package_authoring::fnv1_name_hash(&format!("{name}/visibility"));
    if visible.is_empty() {
        return Ok((seed, Vec::new(), Vec::new()));
    }
    let capacity = (visible.len() * 2).next_power_of_two();
    let mask = capacity - 1;
    let mut rows = vec![(0u64, 0u32); capacity];
    for (index, object) in visible.iter().enumerate() {
        let key = u64::from(object.id);
        if key == 0 {
            return Err(invalid("A static instance identifier is zero"));
        }
        let mut slot = slot_hash(seed, key) as usize & mask;
        while rows[slot].0 != 0 {
            if rows[slot].0 == key {
                return Err(invalid("Static instance identifiers repeat"));
            }
            rows[slot].1 |= PROBED_PAST;
            slot = (slot + 1) & mask;
        }
        rows[slot] = (key, STORED | index as u32);
    }
    let mut bytes = Vec::with_capacity(capacity * 16);
    for (key, value) in rows {
        bytes.extend(key.to_le_bytes());
        bytes.extend(value.to_le_bytes());
        bytes.extend(0u32.to_le_bytes());
    }
    let objects = (0..visible.len() as u32)
        .flat_map(u32::to_le_bytes)
        .collect();
    Ok((seed, bytes, objects))
}

fn distance(bounds: Bounds, point: [f32; 3]) -> f32 {
    bounds
        .center()
        .into_iter()
        .zip(point)
        .map(|(a, b)| (a - b).powi(2))
        .sum()
}

impl Layout {
    pub(super) fn new(
        name: &str,
        plan: source::Plan,
        table: (TagHash, Vec<u8>),
        graph: &[u8],
        manager: &PackageManager,
    ) -> AuthoringResult<Self> {
        let (count, _, nodes, _) = array_at(graph, 0x10)?;
        let spawns = (0..count)
            .filter(|&i| graph[nodes + i * 24 + 12..nodes + i * 24 + 14] == 2u16.to_le_bytes())
            .count();
        let first_spawn = plan.bubbles.len() * 2 - 1;
        if spawns == 0 || first_spawn + spawns > 64 {
            return Err(invalid("Invalid streaming layout node count"));
        }
        let spawn_mask = (first_spawn..first_spawn + spawns).fold(0, |mask, i| mask | (1u64 << i));
        let smallest = |class| -> AuthoringResult<(TagHash, Vec<u8>)> {
            let tag = manager
                .lookup
                .tag32_entries_by_pkg
                .iter()
                .flat_map(|(&pid, entries)| {
                    entries
                        .iter()
                        .enumerate()
                        .filter(move |(_, e)| e.reference == class)
                        .map(move |(i, e)| (e.file_size, TagHash::new(pid, i as u16)))
                })
                .min_by_key(|v| v.0)
                .map(|v| v.1)
                .ok_or_else(|| invalid(format!("Missing streaming template class {class:08X}")))?;
            Ok((tag, read_tag(manager, tag, "streaming template")?))
        };
        let (_, _, row, _) = array_at(&table.1, 8)?;
        let resource = (row + 0x78)
            .checked_add_signed(read_i64(&table.1, row + 0x78)? as isize)
            .ok_or_else(|| invalid("Visibility placement pointer overflow"))?;
        let donor = read_tag(
            manager,
            TagHash(read_u32(&table.1, resource + 0x10)?),
            "visibility world",
        )?;
        let raw = TagHash(read_u32(&donor, 8)?);
        if manager.get_entry(raw).is_none_or(|e| e.file_type != 24) {
            return Err(invalid("Visibility template has no raw spatial tome"));
        }
        let classes = manager
            .lookup
            .tag32_entries_by_pkg
            .iter()
            .flat_map(|(&pid, entries)| {
                entries
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| {
                        matches!(
                            e.reference,
                            0x8080_7320
                                | 0x8080_7DAE
                                | 0x8080_8A54
                                | 0x8080_9162
                                | 0x8080_91DE
                                | 0x8080_91E0
                                | 0x8080_9462
                                | 0x8080_964E
                                | 0x8080_9C36
                                | 0x8080_9EF9
                        )
                    })
                    .map(move |(i, e)| (TagHash::new(pid, i as u16), e.reference))
            })
            .collect();
        let names = plan
            .bubbles
            .iter()
            .enumerate()
            .map(|(i, bubble)| {
                if i == 0 {
                    name.to_owned()
                } else {
                    format!("{name}_{}", bubble.name)
                }
            })
            .collect();
        let world = smallest(0x8080_7320)?;
        let metadata = smallest(0x8080_9756)?;
        if world.1.len() < 0xD0 || metadata.1.len() < 0x88 {
            return Err(invalid("Spatial template has an incomplete native header"));
        }
        Ok(Self {
            plan,
            names,
            spawn_mask,
            classes,
            aliases: manager
                .lookup
                .tag64_entries
                .iter()
                .map(|(&hash, entry)| (hash, entry.hash32))
                .collect(),
            table,
            world,
            metadata,
            raw,
        })
    }

    pub(super) fn hash(&self, bubble: usize) -> u32 {
        sundial::package_authoring::fnv1_name_hash(&self.names[bubble])
    }
    pub(super) fn node_id(&self, bubble: usize) -> u64 {
        identity(&format!("{}/bubble", self.names[bubble]))
    }
    fn crossing_id(&self, edge: usize) -> u64 {
        identity(&format!("{}/crossing/{edge}", self.names[0]))
    }
    fn crossing_hash(&self, edge: usize) -> u32 {
        sundial::package_authoring::fnv1_name_hash(&format!("{}/crossing/{edge}", self.names[0]))
    }
    fn neighbors(&self, bubble: usize) -> impl Iterator<Item = usize> {
        bubble
            .checked_sub(1)
            .into_iter()
            .chain((bubble + 1 < self.names.len()).then_some(bubble + 1))
    }

    /// Native retention masks contain the bubble, its transitions and its spawn-set nodes.
    /// The 256-bit fields share this layout even though authored graphs currently fit 64 bits.
    pub(super) fn mask(&self, bubble: usize) -> u64 {
        self.neighbors(bubble)
            .fold(self.spawn_mask | (1u64 << bubble), |mask, other| {
                mask | (1u64 << (self.names.len() + bubble.min(other)))
            })
    }

    pub(super) fn graph(&self, template: &[u8]) -> AuthoringResult<Vec<u8>> {
        let n = self.names.len();
        let (count, _, start, _) = array_at(template, 0x10)?;
        let mut spawns = Vec::new();
        for i in 0..count {
            let row = &template[start + i * 24..start + (i + 1) * 24];
            if u16::from_le_bytes(row[12..14].try_into().unwrap()) == 2 {
                spawns.push(row.to_vec());
            }
        }
        if spawns.is_empty() || n * 2 - 1 + spawns.len() > 64 {
            return Err(invalid("Invalid streaming layout node count"));
        }
        let mut nodes = Vec::new();
        for i in 0..n * 2 - 1 {
            let mut row = [0u8; 24];
            let (id, hash, kind, ordinal) = if i < n {
                (self.node_id(i), self.hash(i), 0u16, i as u32)
            } else {
                (
                    self.crossing_id(i - n),
                    self.crossing_hash(i - n),
                    1,
                    u32::MAX,
                )
            };
            row[..8].copy_from_slice(&id.to_le_bytes());
            row[8..12].copy_from_slice(&hash.to_le_bytes());
            row[12..14].copy_from_slice(&kind.to_le_bytes());
            row[14..16].copy_from_slice(&(i as u16).to_le_bytes());
            row[16..20].copy_from_slice(&ordinal.to_le_bytes());
            nodes.extend(row);
        }
        let spawn_nodes = (n * 2 - 1..n * 2 - 1 + spawns.len())
            .map(|i| i as u16)
            .collect::<Vec<_>>();
        for (index, mut row) in spawn_nodes.iter().zip(spawns) {
            row[14..16].copy_from_slice(&index.to_le_bytes());
            nodes.extend(row);
        }
        let mut payload = template[..0x30].to_vec();
        payload[8..].fill(0);
        append_native_array(&mut payload, 0x10, 0x8080_98C4, nodes.len() / 24, &nodes)?;
        append_native_array(&mut payload, 0x20, 0x8080_98C2, n, &vec![0u8; n * 80])?;
        let rows = array_at(&payload, 0x20)?.2;
        for i in 0..n {
            let row = rows + i * 80;
            write_u32(&mut payload, row, self.hash(i))?;
            write_u32(&mut payload, row + 4, i as u32)?;
            write_u64(&mut payload, row + 0x28, self.mask(i))?;
            write_u32(&mut payload, row + 0x48, i as u32)?;
            let transitions = self
                .neighbors(i)
                .flat_map(|other| ((n + i.min(other)) as u16).to_le_bytes())
                .collect::<Vec<_>>();
            append_native_array(
                &mut payload,
                row + 8,
                0x8080_9B9C,
                transitions.len() / 2,
                &transitions,
            )?;
            let spawns = spawn_nodes
                .iter()
                .flat_map(|index| index.to_le_bytes())
                .collect::<Vec<_>>();
            append_native_array(
                &mut payload,
                row + 0x18,
                0x8080_9B9C,
                spawn_nodes.len(),
                &spawns,
            )?;
        }
        payload.extend_from_slice(&((self.names[0].len() + 1) as u64).to_le_bytes());
        let text = payload.len();
        payload.extend_from_slice(self.names[0].as_bytes());
        payload.push(0);
        write_relative_pointer(&mut payload, 8, text)?;
        synchronize_payload_size(payload)
    }

    /// Replace ownership masks and add incoming crossing spawn names to this bubble's tables.
    pub(super) fn patch_chain(
        &self,
        tags: &mut [NewTagSpec],
        bubble: usize,
    ) -> AuthoringResult<()> {
        tags[2].payload[0x1C..0x3C].fill(0);
        write_u64(&mut tags[2].payload, 0x1C, self.mask(bubble))?;
        write_u32(&mut tags[2].payload, 0x18, self.hash(bubble))?;
        // Spatial owners register on load, independently of their placement rows. Keeping
        // a removed donor owner can occupy the active slot ahead of the authored one.
        let donor_worlds = self
            .classes
            .iter()
            .filter_map(|(&tag, &class)| (class == 0x8080_7320).then_some(tag))
            .collect::<Vec<_>>();
        // Owner hashes are a second loading path. Their replacements are explicitly
        // enrolled by the authored chain, while unrelated retained content keeps its aliases.
        let replaced = tags
            .iter()
            .filter_map(|spec| {
                matches!(
                    self.classes.get(&spec.template_tag),
                    Some(0x8080_7DAE | 0x8080_8A54 | 0x8080_91DE | 0x8080_91E0 | 0x8080_964E)
                )
                .then_some(spec.template_tag)
            })
            .chain(donor_worlds.iter().copied())
            .collect::<BTreeSet<_>>();
        let removed_hashes = self
            .aliases
            .iter()
            .filter_map(|(&hash, tag)| replaced.contains(tag).then_some(hash))
            .collect();
        for spec in tags {
            match self.classes.get(&spec.template_tag).copied() {
                Some(0x8080_8A54) => write_u64(&mut spec.payload, 8, 1u64 << bubble)?,
                Some(0x8080_9EF9) => {
                    spec.payload = remove_dependencies(
                        &spec.payload,
                        TagHash(read_u32(&spec.payload, 8)?),
                        TagHash(read_u32(&spec.payload, 12)?),
                        &donor_worlds,
                    )?;
                    spec.payload = remove_hash_dependencies(
                        &spec.payload,
                        TagHash(read_u32(&spec.payload, 8)?),
                        TagHash(read_u32(&spec.payload, 12)?),
                        &removed_hashes,
                    )?;
                }
                Some(0x8080_9162) => {
                    let (count, _, rows, class) = array_at(&spec.payload, 8)?;
                    let original = spec.payload[rows..rows + count * 48].to_vec();
                    let mut points = original.clone();
                    for other in self.neighbors(bubble) {
                        for row in original.chunks_exact(48) {
                            let mut row = row.to_vec();
                            write_u32(&mut row, 0x20, self.crossing_hash(bubble.min(other)))?;
                            points.extend(row);
                        }
                    }
                    append_native_array(&mut spec.payload, 8, class, points.len() / 48, &points)?;
                    spec.payload = synchronize_payload_size(std::mem::take(&mut spec.payload))?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Bindings identify common keyed definitions without mistaking interface bodies or the
    /// volume definition's distinct prefix for a slot carrying a bubble index.
    fn registry_slots(&self, tags: &[NewTagSpec]) -> AuthoringResult<BTreeSet<(u32, u16, u16)>> {
        let mut slots = BTreeSet::new();
        for spec in tags {
            if self.classes.get(&spec.template_tag) != Some(&0x8080_9462)
                || read_u64(&spec.payload, 0x20)? == 0
            {
                continue;
            }
            let key = read_u32(&spec.payload, 0x0C)?;
            let (count, _, rows, class) = array_at(&spec.payload, 0x20)?;
            if class != 0x8080_9B3A {
                return Err(invalid("Registry object has an invalid slot array class"));
            }
            for index in 0..count {
                let kind = read_u16(&spec.payload, rows + index * 8)?;
                let index = u16::try_from(index)
                    .map_err(|_| invalid("Registry object slot index overflow"))?;
                slots.insert((key, kind, index));
            }
        }
        Ok(slots)
    }

    pub(super) fn patch_registry(
        &self,
        tags: &mut [NewTagSpec],
        first: TagHash,
        bubble: usize,
    ) -> AuthoringResult<()> {
        let slots = self.registry_slots(tags)?;
        let bubble =
            u32::try_from(bubble).map_err(|_| invalid("Registry bubble index overflow"))?;
        for (index, spec) in tags.iter_mut().enumerate() {
            match self.classes.get(&spec.template_tag).copied() {
                Some(0x8080_9462) => {
                    let (count, _, rows, _) = array_at(&spec.payload, 0x38)?;
                    for i in 0..count {
                        let at = rows + i * 24;
                        if read_u32(&spec.payload, at)? != u32::MAX {
                            write_u32(&mut spec.payload, at, bubble)?;
                        }
                    }
                }
                Some(0x8080_9C36) => {
                    let own = TagHash::new(first.pkg_id(), first.entry_index() + index as u16).0;
                    super::placed::set_bubble(&mut spec.payload, own, bubble, &slots)?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn return_gate(&self, bubble: usize, previous_entry: Option<[f32; 3]>) -> Option<Bounds> {
        let previous = self.plan.bubbles.get(bubble.checked_sub(1)?)?;
        let area = &self.plan.bubbles[bubble];
        if let Some(entry) = previous.commit.first() {
            return area
                .preload
                .iter()
                .min_by(|a, b| {
                    distance(**a, entry.center()).total_cmp(&distance(**b, entry.center()))
                })
                .copied();
        }
        // An opening area has no automatic entry volume. A return gate sits outside the
        // current area's commit volume on the side facing that opening area's source spawn.
        // The commit volume itself remains neutral after arrival, preventing an immediate return.
        let point = previous_entry?;
        let mut gate = *area
            .commit
            .iter()
            .min_by(|a, b| distance(**a, point).total_cmp(&distance(**b, point)))?;
        let axis = (0..2)
            .filter(|&k| point[k] < gate.minimum[k] || point[k] > gate.maximum[k])
            .max_by(|&a, &b| {
                let separation =
                    |k: usize| (point[k] - point[k].clamp(gate.minimum[k], gate.maximum[k])).abs();
                separation(a).total_cmp(&separation(b))
            })?;
        if point[axis] < gate.minimum[axis] {
            gate.maximum[axis] = gate.minimum[axis];
            gate.minimum[axis] -= 16.;
        } else {
            gate.minimum[axis] = gate.maximum[axis];
            gate.maximum[axis] += 16.;
        }
        Some(gate)
    }

    /// The clusters of area `bubble`'s spatial tome: its crossings, then lighting cells over the
    /// space its `probes` inside `area` cover. A crossing wins where it reaches a cell, since
    /// the tome takes the first region that holds a point.
    pub(super) fn cells(
        &self,
        bubble: usize,
        previous_entry: Option<[f32; 3]>,
        probes: &[[f32; 4]],
        area: Bounds,
    ) -> AuthoringResult<Cells> {
        let mut clusters = vec![0u8; 20];
        clusters[12..16].copy_from_slice(&(-1f32).to_le_bytes());
        clusters[17..20].fill(0xFF);
        let mut cells = Cells {
            descriptors: Vec::new(),
            clusters,
            regions: Vec::new(),
            lighting_probes: Vec::new(),
        };
        let neighbors = self.neighbors(bubble).collect::<Vec<_>>();
        for &other in &neighbors {
            let edge = bubble.min(other);
            let mut descriptor = [0u8; 40];
            descriptor[..8].copy_from_slice(&((self.names.len() + edge) as u64).to_le_bytes());
            descriptor[8..16].copy_from_slice(&self.crossing_id(edge).to_le_bytes());
            descriptor[16..24].copy_from_slice(&(other as u64).to_le_bytes());
            descriptor[24..32].copy_from_slice(&self.node_id(other).to_le_bytes());
            cells.descriptors.extend(descriptor);
        }
        // Phase 1 keeps the arrival volume owned by its shared transition without requesting
        // another switch. The native object purge retains a player only when its stored
        // location identifies a transition node also present in the destination's node mask.
        for &bounds in &self.plan.bubbles[bubble].commit {
            if bubble > 0 {
                cells.push(bounds, 1, 0);
            } else {
                cells.regions.push(tome::Region { bounds, cluster: 0 });
            }
        }
        // Commit volumes win over preloads where their conservative bounds overlap.
        for (descriptor, &other) in neighbors.iter().enumerate() {
            for &bounds in &self.plan.bubbles[other].commit {
                cells.push(bounds, 2, descriptor);
            }
        }
        if bubble > 0 {
            if let Some(bounds) = self.return_gate(bubble, previous_entry) {
                cells.push(bounds, 2, 0);
            } else if self.plan.bubbles[bubble - 1].commit.is_empty() {
                return Err(invalid(
                    "The opening area's return crossing cannot be located",
                ));
            }
        }
        // The native location query caches a sphere, while the crossing trigger queries
        // the current point. Give every commit a transition-owned approach, including return
        // gates and sources whose preload lies beyond the commit. Explicit commits win.
        let approaches = cells
            .regions
            .iter()
            .filter_map(|region| {
                let row = region.cluster as usize * 20;
                (cells.clusters[row + 16] == 2)
                    .then_some((region.bounds, cells.clusters[row + 17] as usize))
            })
            .collect::<Vec<_>>();
        for (bounds, descriptor) in approaches {
            cells.push(
                Bounds {
                    minimum: bounds.minimum.map(|value| value - 16.),
                    maximum: bounds.maximum.map(|value| value + 16.),
                },
                3,
                descriptor,
            );
        }
        for (descriptor, &other) in neighbors.iter().enumerate() {
            for &bounds in &self.plan.bubbles[other].preload {
                // Phase 3 with zero distance requests preparation without a phase-4 commit.
                cells.push(bounds, 3, descriptor);
            }
        }
        for cell in lighting::cells(area, probes, &cells.regions)? {
            cells.push_lighting(cell);
        }
        Ok(cells)
    }

    /// Install a native point-query and visibility resource in the bubble's own geometry
    /// container. `visible` are the area's static instances, each an Umbra object of its tome.
    pub(super) fn spatial(
        &self,
        chain: &mut [NewTagSpec],
        first: TagHash,
        bubble: usize,
        mut bounds: Bounds,
        cells: Cells,
        visible: &[parhelion_import::halo_reach::level::native::Visible],
    ) -> AuthoringResult<Vec<NewTagSpec>> {
        let chain_count = u16::try_from(chain.len())
            .map_err(|_| invalid("Spatial geometry chain exceeds the package entry range"))?;
        let entry = first
            .entry_index()
            .checked_add(chain_count)
            .ok_or_else(|| invalid("Spatial geometry chain entry overflow"))?;
        let extra = TagHash::new(first.pkg_id(), entry);
        let at = |offset: u16| TagHash::new(extra.pkg_id(), extra.entry_index() + offset);
        let (table_tag, world_tag, raw_tag, metadata_tag) = (at(0), at(1), at(2), at(3));
        let Cells {
            descriptors,
            clusters,
            regions,
            lighting_probes: _,
        } = cells;
        let objects = visible
            .iter()
            .map(|object| Bounds {
                minimum: object.minimum,
                maximum: object.maximum,
            })
            .collect::<Vec<_>>();
        for extent in regions
            .iter()
            .map(|region| region.bounds)
            .chain(objects.iter().copied())
        {
            for k in 0..3 {
                bounds.minimum[k] = bounds.minimum[k].min(extent.minimum[k] - 16.);
                bounds.maximum[k] = bounds.maximum[k].max(extent.maximum[k] + 16.);
            }
        }
        let raw = tome::encode(bounds, &regions, clusters.len() / 20, &objects)?;
        let mut metadata = self.metadata.1[..0x88].to_vec();
        metadata[0x18..0x78].fill(0);
        metadata[0x78..0x88].fill(0xFF);
        write_u64(&mut metadata, 8, bubble as u64)?;
        write_u64(&mut metadata, 0x10, self.node_id(bubble))?;
        append_native_array(
            &mut metadata,
            0x18,
            0x8080_9778,
            clusters.len() / 20,
            &clusters,
        )?;
        // The descriptor class is verified from stock spatial metadata.
        append_native_array(
            &mut metadata,
            0x28,
            0x8080_9777,
            descriptors.len() / 40,
            &descriptors,
        )?;
        metadata = synchronize_payload_size(metadata)?;
        let mut world = self.world.1.clone();
        // Native spatial registration and active-slot selection use the bubble ordinal.
        write_u32(&mut world, 0x50, bubble as u32)?;
        for (field, tag) in [
            (8, raw_tag),
            (12, raw_tag),
            (0xA4, metadata_tag),
            (0xA8, metadata_tag),
        ] {
            write_u32(&mut world, field, tag.0)?;
        }
        for field in [0x10, 0xA0, 0xC4, 0xC8] {
            write_u32(&mut world, field, u32::MAX)?;
        }
        for (base, values) in [
            (0x60, bounds.center()),
            (0x70, bounds.minimum),
            (0x80, bounds.maximum),
        ] {
            for (k, value) in values.into_iter().enumerate() {
                write_u32(&mut world, base + k * 4, value.to_bits())?;
            }
        }
        let (seed, rows, objects) = visibility_table(&self.names[bubble], visible)?;
        write_u32(&mut world, VISIBILITY_SEED, seed)?;
        append_native_array(
            &mut world,
            VISIBILITY_ROWS,
            ROW_CLASS,
            rows.len() / 16,
            &rows,
        )?;
        append_native_array(
            &mut world,
            VISIBILITY_OBJECTS,
            OBJECT_INDEX_CLASS,
            objects.len() / 4,
            &objects,
        )?;
        world = synchronize_payload_size(world)?;
        let mut table = self.table.1.clone();
        let row = array_at(&table, 8)?.2;
        let resource = (row + 0x78)
            .checked_add_signed(read_i64(&table, row + 0x78)? as isize)
            .ok_or_else(|| invalid("Visibility pointer overflow"))?;
        write_u32(&mut table, resource + 0x10, world_tag.0)?;
        let (count, _, rows, class) = array_at(&chain[6].payload, 0x28)?;
        let mut tables = chain[6].payload[rows..rows + count * 4].to_vec();
        tables.extend_from_slice(&table_tag.0.to_le_bytes());
        append_native_array(&mut chain[6].payload, 0x28, class, count + 1, &tables)?;
        chain[6].payload = synchronize_payload_size(std::mem::take(&mut chain[6].payload))?;
        chain[7].payload = enroll_dependencies(
            &chain[7].payload,
            TagHash::new(first.pkg_id(), first.entry_index() + 7),
            TagHash::new(first.pkg_id(), first.entry_index() + 6),
            &[table_tag, world_tag, raw_tag, metadata_tag],
        )?;
        let spec = |template_tag, payload| NewTagSpec {
            template_tag,
            payload,
            storage: NewTagStorageMode::InheritTemplate,
        };
        Ok(vec![
            spec(self.table.0, table),
            spec(self.world.0, world),
            spec(self.raw, raw),
            spec(self.metadata.0, metadata),
        ])
    }
}

//! Legacy spatial tomes for point, connected-region and visibility queries. Neighboring regions
//! have portals in the cluster graph, so native object-location caching observes crossings, and
//! the same faces as tile cell portals, so the portal culler reaches every cell from the
//! camera's. No occluders are authored. Every cell shares the complete static object list so
//! lighting and crossing partitions cannot hide a surface or a shadow caster. Native bounds
//! and frustum tests still select the objects relevant to the query.
//! Shadowkeep's Umbra 3.3 runtime consumes format 0x12.
use super::Bounds;
use crate::{AuthoringResult, error::invalid};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy)]
pub(super) struct Region {
    pub bounds: Bounds,
    pub cluster: u32,
}

/// The most nodes a tome's partition tree may hold. The client's query (`destiny2.exe+0x1A78B60`
/// in the Shadowkeep build) caches the tree's nodes by index in a per-query table of 2,048 entries
/// and returns `0x3FFF` past it, which its caller does not check: a tome of about 3,200 clusters
/// crashed the client at `+0x1A7CFA7` on entering the map.
pub(super) const MAX_TREE_NODES: usize = 2048;

enum Node {
    Leaf(u32),
    Split(usize, f32, Box<Node>, Box<Node>),
}

fn intersects(a: Bounds, b: Bounds) -> bool {
    (0..3).all(|k| a.minimum[k] < b.maximum[k] && b.minimum[k] < a.maximum[k])
}

fn contains(outer: Bounds, inner: Bounds) -> bool {
    (0..3).all(|k| outer.minimum[k] <= inner.minimum[k] && inner.maximum[k] <= outer.maximum[k])
}

fn partition(
    bounds: Bounds,
    regions: &[&Region],
    depth: usize,
    count: &mut usize,
    limit: usize,
) -> AuthoringResult<Node> {
    *count += 1;
    if depth >= 60 || *count > limit {
        return Err(invalid("Spatial transition tree exceeds native limits"));
    }
    let active = regions
        .iter()
        .copied()
        .filter(|r| intersects(bounds, r.bounds))
        .collect::<Vec<_>>();
    let Some(first) = active.first() else {
        return Ok(Node::Leaf(0));
    };
    if contains(first.bounds, bounds) {
        return Ok(Node::Leaf(first.cluster));
    }
    let split = (0..3)
        .flat_map(|axis| {
            [first.bounds.minimum[axis], first.bounds.maximum[axis]]
                .into_iter()
                .filter(move |&value| bounds.minimum[axis] < value && value < bounds.maximum[axis])
                .map(move |value| (axis, value))
        })
        .min_by(|&(a, av), &(b, bv)| {
            let imbalance = |axis: usize, value: f32| {
                ((value - bounds.minimum[axis]) / (bounds.maximum[axis] - bounds.minimum[axis])
                    - 0.5)
                    .abs()
            };
            imbalance(a, av).total_cmp(&imbalance(b, bv))
        })
        .ok_or_else(|| invalid("Spatial transition box cannot be partitioned"))?;
    let (axis, value) = split;
    let mut low = bounds;
    low.maximum[axis] = value;
    let mut high = bounds;
    high.minimum[axis] = value;
    Ok(Node::Split(
        axis,
        value,
        Box::new(partition(low, &active, depth + 1, count, limit)?),
        Box::new(partition(high, &active, depth + 1, count, limit)?),
    ))
}

fn word(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn append(bytes: &mut Vec<u8>, data: &[u8]) -> u32 {
    bytes.resize(bytes.len().next_multiple_of(16), 0);
    let at = bytes.len() as u32;
    bytes.extend_from_slice(data);
    at
}
fn words(values: impl IntoIterator<Item = u32>) -> Vec<u8> {
    values.into_iter().flat_map(u32::to_le_bytes).collect()
}
fn reference(bytes: &mut Vec<u8>, field: usize, data: &[u8]) {
    let at = append(bytes, data);
    word(bytes, field, at);
}
fn extent(bytes: &mut [u8], at: usize, bounds: Bounds) {
    for (index, value) in bounds.minimum.into_iter().chain(bounds.maximum).enumerate() {
        word(bytes, at + index * 4, value.to_bits());
    }
}

/// Breadth-first two-bit split codes followed by cumulative non-leaf ranks at each 16-node
/// boundary. Split coordinates are explicit, so the geometry does not depend on median rounding.
fn tree(root: &Node) -> (Vec<u8>, Vec<u8>, Vec<u32>, usize) {
    let mut queue = vec![root];
    let mut kinds = Vec::new();
    let mut splits = Vec::new();
    let mut leaves = Vec::new();
    let mut at = 0;
    while at < queue.len() {
        match queue[at] {
            Node::Leaf(cluster) => {
                kinds.push(3u32);
                splits.push(0);
                leaves.push(*cluster);
            }
            Node::Split(axis, value, low, high) => {
                kinds.push(*axis as u32);
                splits.push(value.to_bits());
                queue.extend([low.as_ref(), high.as_ref()]);
            }
        }
        at += 1;
    }
    let count = kinds.len();
    let mut packed = vec![0u32; (count * 2).div_ceil(32)];
    for (i, &kind) in kinds.iter().enumerate() {
        packed[i / 16] |= kind << (i % 16 * 2);
    }
    // Node count is bounded below 65536, so no top-rank entries are needed.
    let mids = count >> 8;
    let bottoms = (count >> 4) - mids;
    let mut ranks = vec![0u8; (mids.div_ceil(2) + bottoms.div_ceil(4)) * 4];
    let (mut total, mut middle, mut mid_index, mut bottom_index) = (0u32, 0u32, 0usize, 0usize);
    for (i, &kind) in kinds.iter().enumerate() {
        if i > 0 && i % 16 == 0 {
            if i % 256 == 0 {
                ranks[mid_index * 2..mid_index * 2 + 2]
                    .copy_from_slice(&(total as u16).to_le_bytes());
                mid_index += 1;
                middle = total;
            } else {
                ranks[mids.div_ceil(2) * 4 + bottom_index] = (total - middle) as u8;
                bottom_index += 1;
            }
        }
        total += u32::from(kind != 3);
    }
    let mut data = words(packed);
    data.extend(ranks);
    (data, words(splits), leaves, count)
}

fn leaves(node: &Node, bounds: Bounds, output: &mut Vec<(u32, Bounds)>) {
    match node {
        Node::Leaf(cluster) => output.push((*cluster, bounds)),
        Node::Split(axis, value, low, high) => {
            let mut low_bounds = bounds;
            low_bounds.maximum[*axis] = *value;
            let mut high_bounds = bounds;
            high_bounds.minimum[*axis] = *value;
            leaves(low, low_bounds, output);
            leaves(high, high_bounds, output);
        }
    }
}

fn coordinate(bounds: Bounds, axis: usize, value: f32) -> f64 {
    ((f64::from(value) - f64::from(bounds.minimum[axis]))
        / (f64::from(bounds.maximum[axis]) - f64::from(bounds.minimum[axis]))
        * 65535.)
        .clamp(0., 65535.)
}

fn packed_bounds(world: Bounds, bounds: Bounds) -> [u32; 3] {
    let minimum: [u32; 3] =
        std::array::from_fn(|k| coordinate(world, k, bounds.minimum[k]).floor() as u32);
    let maximum: [u32; 3] =
        std::array::from_fn(|k| coordinate(world, k, bounds.maximum[k]).ceil() as u32);
    [
        (minimum[0] << 16) | minimum[1],
        (minimum[2] << 16) | maximum[0],
        (maximum[1] << 16) | maximum[2],
    ]
}

type Faces = BTreeMap<(usize, u32), [Vec<usize>; 2]>;

fn face_groups(
    leaf_bounds: &[(u32, Bounds)],
    bounds: Bounds,
    clusters: usize,
) -> (Faces, Vec<Bounds>) {
    let mut extents = vec![None::<Bounds>; clusters];
    let mut faces = Faces::new();
    for (index, &(cluster, leaf)) in leaf_bounds.iter().enumerate() {
        let extent = extents[cluster as usize].get_or_insert(leaf);
        for axis in 0..3 {
            extent.minimum[axis] = extent.minimum[axis].min(leaf.minimum[axis]);
            extent.maximum[axis] = extent.maximum[axis].max(leaf.maximum[axis]);
            for (side, value) in [leaf.minimum[axis], leaf.maximum[axis]]
                .into_iter()
                .enumerate()
            {
                let plane = if value == 0. { 0 } else { value.to_bits() };
                faces.entry((axis, plane)).or_default()[side].push(index);
            }
        }
    }
    (
        faces,
        extents
            .into_iter()
            .map(|extent| extent.unwrap_or(bounds))
            .collect(),
    )
}

fn portal(
    bounds: Bounds,
    axis: usize,
    plane: u32,
    a_bounds: Bounds,
    b_bounds: Bounds,
) -> Option<[u32; 3]> {
    let x = (1 << axis) & 3;
    let y = (1 << x) & 3;
    let low: [f32; 3] = std::array::from_fn(|k| a_bounds.minimum[k].max(b_bounds.minimum[k]));
    let high: [f32; 3] = std::array::from_fn(|k| a_bounds.maximum[k].min(b_bounds.maximum[k]));
    if low[x] >= high[x] || low[y] >= high[y] {
        return None;
    }
    let range = |k| {
        ((coordinate(bounds, k, low[k]).floor() as u32) << 16)
            | coordinate(bounds, k, high[k]).ceil() as u32
    };
    let depth = coordinate(bounds, axis, f32::from_bits(plane)).round() as u32;
    Some([depth, range(x), range(y)])
}

/// Portals describe the actual shared faces of partition leaves. A semantic cluster can have
/// several leaves, so its bounds are their union and its graph contains every exposed face.
/// Quantized rectangles conservatively cover their face in the legacy tome's coordinate space.
/// Also returns the same faces as tile cell portals, one cell per cluster: the face in the link's
/// top bits with tile 0 as its payload, so the portal is internal to the tome's only tile, and
/// the target cell above the depth. Each cell's portals follow its cluster's order.
fn connectivity(root: &Node, bounds: Bounds, clusters: usize) -> AuthoringResult<Connectivity> {
    let mut leaf_bounds = Vec::new();
    leaves(root, bounds, &mut leaf_bounds);
    let (faces, extents) = face_groups(&leaf_bounds, bounds, clusters);
    let mut portals = vec![BTreeSet::new(); clusters];
    let mut comparisons = 0usize;
    for ((axis, plane), sides) in faces {
        // Only leaves whose faces can overlap are compared: the far sides' leaves are bucketed by
        // their face's rectangle on a grid in the plane. A sparse lighting grid puts thousands of
        // cells on one plane, and every pair of them was too many.
        let x = (1 << axis) & 3;
        let y = (1 << x) & 3;
        let span = |leaf: Bounds| -> Option<[[i64; 2]; 2]> {
            let cells = [x, y].map(|k| {
                [
                    (leaf.minimum[k] / PORTAL_BUCKET).floor() as i64,
                    (leaf.maximum[k] / PORTAL_BUCKET).floor() as i64,
                ]
            });
            let count = (cells[0][1] - cells[0][0] + 1) * (cells[1][1] - cells[1][0] + 1);
            (count <= LARGE_FACE_BUCKETS).then_some(cells)
        };
        let mut buckets = BTreeMap::<[i64; 2], Vec<usize>>::new();
        let mut large = Vec::new();
        for &right in &sides[0] {
            match span(leaf_bounds[right].1) {
                Some([[x0, x1], [y0, y1]]) => {
                    for bx in x0..=x1 {
                        for by in y0..=y1 {
                            buckets.entry([bx, by]).or_default().push(right);
                        }
                    }
                }
                None => large.push(right),
            }
        }
        for &left in &sides[1] {
            let mut candidates = large.iter().copied().collect::<BTreeSet<_>>();
            match span(leaf_bounds[left].1) {
                Some([[x0, x1], [y0, y1]]) => {
                    for bx in x0..=x1 {
                        for by in y0..=y1 {
                            candidates.extend(buckets.get(&[bx, by]).into_iter().flatten());
                        }
                    }
                }
                None => candidates.extend(sides[0].iter().copied()),
            }
            for right in candidates {
                comparisons += 1;
                if comparisons > 40_000_000 {
                    return Err(invalid(
                        "Spatial connectivity exceeds the authoring work limit",
                    ));
                }
                let (a, a_bounds) = leaf_bounds[left];
                let (b, b_bounds) = leaf_bounds[right];
                if a == b {
                    continue;
                }
                let Some([depth, x, y]) = portal(bounds, axis, plane, a_bounds, b_bounds) else {
                    continue;
                };
                for (owner, target, side) in [(a, b, 1), (b, a, 0)] {
                    let face = (axis as u32 * 2 + side) << 29;
                    portals[owner as usize].insert([face | target, depth, x, y]);
                }
            }
        }
    }
    let mut rows = Vec::new();
    let mut links = Vec::new();
    let mut cell_links = Vec::new();
    let mut spans = Vec::with_capacity(clusters);
    for (index, portals) in portals.into_iter().enumerate() {
        let start = links.len() as u32 / 16;
        rows.extend(words([start, portals.len() as u32]));
        rows.extend(words(packed_bounds(bounds, extents[index])));
        spans.push([start, portals.len() as u32]);
        for [link, depth, x, y] in portals {
            let target = link & PAYLOAD_MASK;
            cell_links.extend(words([link & !PAYLOAD_MASK, (target << 16) | depth, x, y]));
            links.extend(words([link, depth, x, y]));
        }
    }
    Ok(Connectivity {
        rows,
        links,
        cell_links,
        spans,
        extents,
    })
}

/// The cluster graph's rows and portals, the tile's cell portals, each cell's portal slice and
/// each cluster's extent.
struct Connectivity {
    rows: Vec<u8>,
    links: Vec<u8>,
    cell_links: Vec<u8>,
    spans: Vec<[u32; 2]>,
    extents: Vec<Bounds>,
}

/// The grid, in meters, a face plane's leaves are bucketed on to find the pairs that share a face,
/// and the most buckets a leaf's face takes before it is compared with every leaf instead.
const PORTAL_BUCKET: f32 = 32.;
const LARGE_FACE_BUCKETS: i64 = 4096;

/// A portal link's target cluster or tile, below its face and kind bits.
const PAYLOAD_MASK: u32 = 0x01FF_FFFF;

/// The bits that hold every value up to `max`, at least one.
fn width(max: u32) -> u32 {
    (32 - max.leading_zeros()).max(1)
}

/// Each cell's objects, packed as Umbra's range iterator reads them.
struct ObjectLists {
    /// Runs of consecutive object indices, the first index in the low `elem` bits and the run
    /// length above it, entries back to back from bit 0 of each little-endian word.
    words: Vec<u32>,
    entries: usize,
    elem: u32,
    count: u32,
    /// Each cell's first entry and object count.
    cells: Vec<[u32; 2]>,
}

fn object_lists(clusters: usize, objects: &[Bounds]) -> AuthoringResult<ObjectLists> {
    let length =
        u32::try_from(objects.len()).map_err(|_| invalid("Too many visibility objects"))?;
    let elem = width(objects.len().saturating_sub(1) as u32);
    let count = width(length);
    if elem + count > 32 {
        return Err(invalid(
            "Visibility object lists exceed the legacy entry width",
        ));
    }
    // A single compressed range is shared by every cell. Filtering by cell overlap would
    // turn disconnected lighting regions into unintended occluders. In native queries that
    // discarded visible geometry even though these tomes contain no occlusion geometry.
    Ok(ObjectLists {
        words: if length == 0 {
            Vec::new()
        } else {
            vec![length << elem]
        },
        entries: usize::from(length != 0),
        elem,
        count,
        cells: vec![[0, length]; clusters],
    })
}

/// Region order is priority order. Cluster zero covers the remainder of the supplied bounds.
/// `objects` are the Umbra objects' world boxes, whose indices the visibility record names.
/// Whether the partition tree of `regions` in `bounds` stays within `limit` nodes and the
/// native depth.
pub(super) fn fits(bounds: Bounds, regions: &[Region], limit: usize) -> bool {
    partition(
        bounds,
        &regions.iter().collect::<Vec<_>>(),
        0,
        &mut 0,
        limit,
    )
    .is_ok()
}

pub(super) fn encode(
    bounds: Bounds,
    regions: &[Region],
    clusters: usize,
    objects: &[Bounds],
) -> AuthoringResult<Vec<u8>> {
    if clusters == 0
        || clusters > u16::MAX as usize
        || regions.iter().any(|r| r.cluster as usize >= clusters)
        || (0..3).any(|k| {
            !bounds.minimum[k].is_finite()
                || !bounds.maximum[k].is_finite()
                || bounds.minimum[k] >= bounds.maximum[k]
        })
    {
        return Err(invalid("Invalid spatial transition bounds or clusters"));
    }
    if objects.iter().any(|object| {
        (0..3).any(|k| {
            !object.minimum[k].is_finite()
                || !object.maximum[k].is_finite()
                || object.minimum[k] > object.maximum[k]
        })
    }) {
        return Err(invalid("Invalid visibility object bounds"));
    }
    let root = partition(
        bounds,
        &regions.iter().collect::<Vec<_>>(),
        0,
        &mut 0,
        MAX_TREE_NODES,
    )?;
    let Connectivity {
        rows: cluster_rows,
        links: portals,
        cell_links,
        spans,
        extents: cluster_bounds,
    } = connectivity(&root, bounds, clusters)?;
    let lists = object_lists(clusters, objects)?;
    let (nodes, splits, leaves, count) = tree(&root);
    let width = (32 - (clusters as u32 - 1).leading_zeros() + 1).max(1) as usize;
    let mut mapping = vec![0u32; (leaves.len() * width).div_ceil(32)];
    for (i, cluster) in leaves.into_iter().enumerate() {
        let bit = i * width;
        mapping[bit / 32] |= cluster << (bit % 32);
        if bit % 32 + width > 32 {
            mapping[bit / 32 + 1] |= cluster >> (32 - bit % 32);
        }
    }
    let mut tile = vec![0u8; 96];
    extent(&mut tile, 0, bounds);
    word(&mut tile, 24, ((count as u32) << 5) | width as u32);
    reference(&mut tile, 28, &nodes);
    reference(&mut tile, 32, &words(mapping));
    word(&mut tile, 36, count as u32);
    reference(&mut tile, 40, &splits);
    word(&mut tile, 52, clusters as u32 | ((clusters as u32) << 16));
    // Cell nodes: portal slice, object slice, cluster and packed bounds. A leaf tile's cell
    // holds its cluster directly with a cluster count of 0, as the legacy generator asserts.
    let mut cells = vec![0u8; clusters * 36];
    for (i, &cluster_extent) in cluster_bounds.iter().enumerate() {
        word(&mut cells, i * 36, spans[i][0]);
        word(&mut cells, i * 36 + 4, spans[i][1]);
        word(&mut cells, i * 36 + 8, lists.cells[i][0]);
        word(&mut cells, i * 36 + 12, lists.cells[i][1]);
        word(&mut cells, i * 36 + 16, i as u32);
        for (field, value) in packed_bounds(bounds, cluster_extent)
            .into_iter()
            .enumerate()
        {
            word(&mut cells, i * 36 + 24 + field * 4, value);
        }
    }
    reference(&mut tile, 56, &cells);
    if !cell_links.is_empty() {
        reference(&mut tile, 60, &cell_links);
    }
    append(&mut tile, &[]);
    let tile_size = tile.len() as u32;
    word(&mut tile, 44, (tile_size << 8) | 1);

    let mut tome = vec![0u8; 320];
    word(&mut tome, 0, 0xD600_0012);
    word(&mut tome, 12, 16f32.to_bits());
    extent(&mut tome, 20, bounds);
    word(&mut tome, 44, 32);
    reference(&mut tome, 48, &words([3]));
    if !objects.is_empty() {
        let count = objects.len() as u32;
        word(&mut tome, 64, count);
        let mut boxes = Vec::with_capacity(objects.len() * 24);
        for object in objects {
            boxes.extend(words(
                object
                    .minimum
                    .into_iter()
                    .chain(object.maximum)
                    .map(f32::to_bits),
            ));
        }
        reference(&mut tome, 68, &boxes);
        // One user identifier per object, its own index, grouped as stock tomes group them.
        reference(&mut tome, 76, &words(0..=count));
        reference(&mut tome, 80, &words(0..count));
        word(&mut tome, 84, lists.elem | (lists.count << 5));
        reference(&mut tome, 88, &words(lists.words));
        word(&mut tome, 92, lists.entries as u32);
    }
    word(&mut tome, 124, clusters as u32);
    reference(&mut tome, 128, &cluster_rows);
    reference(&mut tome, 132, &portals);
    reference(&mut tome, 136, &words([0, clusters as u32]));
    for at in [140, 144, 148] {
        word(&mut tome, at, 1);
    }
    reference(&mut tome, 152, &words([1]));
    reference(&mut tome, 156, &words([0]));
    let tile_at = append(&mut tome, &tile);
    reference(&mut tome, 160, &words([tile_at]));
    append(&mut tome, &[]);
    let size = tome.len() as u32;
    word(&mut tome, 8, size);
    let mut crc = u32::MAX;
    for byte in &tome[8..] {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xEDB8_8320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    word(&mut tome, 4, !crc);
    Ok(tome)
}

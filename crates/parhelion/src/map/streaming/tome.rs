//! Legacy spatial tomes for point and connected-region queries. Neighboring regions have
//! portals so native object-location caching observes crossings. No occluder or visibility
//! targets are authored here. Shadowkeep's Umbra 3.3 runtime consumes format 0x12.
use super::Bounds;
use crate::{AuthoringResult, error::invalid};
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct Region {
    pub bounds: Bounds,
    pub cluster: u32,
}

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
) -> AuthoringResult<Node> {
    *count += 1;
    if depth >= 60 || *count > 65_535 {
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
        Box::new(partition(low, &active, depth + 1, count)?),
        Box::new(partition(high, &active, depth + 1, count)?),
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
fn connectivity(
    root: &Node,
    bounds: Bounds,
    clusters: usize,
) -> AuthoringResult<(Vec<u8>, Vec<u8>, Vec<Bounds>)> {
    let mut leaf_bounds = Vec::new();
    leaves(root, bounds, &mut leaf_bounds);
    let (faces, extents) = face_groups(&leaf_bounds, bounds, clusters);
    let mut portals = vec![BTreeSet::new(); clusters];
    let mut comparisons = 0usize;
    for ((axis, plane), sides) in faces {
        for &left in &sides[1] {
            for &right in &sides[0] {
                comparisons += 1;
                if comparisons > 4_000_000 {
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
    for (index, portals) in portals.into_iter().enumerate() {
        rows.extend(words([links.len() as u32 / 16, portals.len() as u32]));
        rows.extend(words(packed_bounds(bounds, extents[index])));
        links.extend(words(portals.into_iter().flatten()));
    }
    Ok((rows, links, extents))
}

/// Region order is priority order. Cluster zero covers the remainder of the supplied bounds.
pub(super) fn encode(
    bounds: Bounds,
    regions: &[Region],
    clusters: usize,
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
    let root = partition(bounds, &regions.iter().collect::<Vec<_>>(), 0, &mut 0)?;
    let (cluster_rows, portals, cluster_bounds) = connectivity(&root, bounds, clusters)?;
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
    let mut cells = vec![0u8; clusters * 36];
    for (i, &cluster_extent) in cluster_bounds.iter().enumerate() {
        word(&mut cells, i * 36 + 16, i as u32);
        word(&mut cells, i * 36 + 20, 1);
        for (field, value) in packed_bounds(bounds, cluster_extent)
            .into_iter()
            .enumerate()
        {
            word(&mut cells, i * 36 + 24 + field * 4, value);
        }
    }
    reference(&mut tile, 56, &cells);
    append(&mut tile, &[]);
    let tile_size = tile.len() as u32;
    word(&mut tile, 44, (tile_size << 8) | 1);

    let mut tome = vec![0u8; 320];
    word(&mut tome, 0, 0xD600_0012);
    word(&mut tome, 12, 16f32.to_bits());
    extent(&mut tome, 20, bounds);
    word(&mut tome, 44, 32);
    reference(&mut tome, 48, &words([3]));
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

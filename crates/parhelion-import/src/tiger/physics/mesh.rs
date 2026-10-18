//! Triangle collision as an `hkpBvCompressedMeshShape`, the shape stock map objects and world
//! collision use. Triangles are grouped into sections of at most 120 primitives and 255 vertices.
//! A section quantizes its vertices to 11, 11 and 10 bits across its bounds, which its six codec
//! floats give as offset then scale, and indexes them with four bytes per primitive, a triangle
//! repeating its third index. A tree of four-byte nodes bounds the section's primitives, and a
//! tree of five-byte nodes bounds the sections.
//!
//! A node's three axis bytes shrink its parent's box. The high nibble `a` raises the minimum by
//! `a * a / 226` of the parent's extent and the low nibble `c` lowers the maximum by `c * c / 226`.
//! Both trees are stored in preorder, with the left child next. A section node's last byte is
//! twice its primitive for a leaf and its right child's distance plus one otherwise. A section
//! tree node keeps a 15-bit value in its last two bytes, with bit 15 set for an inner node, whose
//! value is half its right child's distance, and clear for a leaf, whose value is its section.
//! A shape key is the section shifted left by 8, then the primitive shifted left by 1.
//! These encodings were checked against stock Shadowkeep map collision, where every primitive
//! lies inside its leaf's decoded box. A prop's packfile roots the shape in a body, and a world
//! collision's shape packfiles hold the bare shape.
use super::{Template, Writer, put};
use anyhow::{Context, Result, ensure};
use std::collections::HashMap;

const PRIMITIVES_PER_SECTION: usize = 120;
const VERTICES_PER_SECTION: usize = 255;
/// A nibble's square over this is its share of the parent's extent.
const STEPS: f32 = 226.;
/// The object, section and array layouts of the 64-bit 2012.2 classes.
pub(super) const SHAPE_SIZE: usize = 0x110;
const SHAPE_TREE: usize = 0x70;
/// Whether primitives carry filter and user data indices, then the filter, user data and string
/// palettes those index.
const PER_PRIMITIVE_FILTER: usize = 0x35;
const PER_PRIMITIVE_USER_DATA: usize = 0x36;
const FILTER_PALETTE: usize = 0x38;
const USER_DATA_PALETTE: usize = 0x48;
const STRING_PALETTE: usize = 0x58;
/// The filter stock world collision meshes give every primitive: layer 7, the world.
const WORLD_FILTER: u32 = 0x037F_FCC7;
const SECTION_SIZE: usize = 0x60;
const ROOT_SIZE: usize = 0x40;
/// Quantization steps of the section codec on each axis.
const STEPS_PER_AXIS: [f32; 3] = [2047., 2047., 1023.];

pub(crate) type Triangle = [[f32; 3]; 3];

/// A binary tree whose leaves are items.
pub(super) enum Tree {
    Leaf(usize),
    Split(Box<Tree>, Box<Tree>),
}

impl Tree {
    pub(super) fn count(&self) -> usize {
        match self {
            Self::Leaf(_) => 1,
            Self::Split(left, right) => 1 + left.count() + right.count(),
        }
    }
}

pub(super) fn bounds(points: impl Iterator<Item = [f32; 3]>) -> ([f32; 3], [f32; 3]) {
    let mut low = [f32::INFINITY; 3];
    let mut high = [f32::NEG_INFINITY; 3];
    for point in points {
        for k in 0..3 {
            low[k] = low[k].min(point[k]);
            high[k] = high[k].max(point[k]);
        }
    }
    (low, high)
}

/// Splits `items` at the median of their centres' longest axis until each part satisfies `fits`,
/// as a tree whose leaves are the parts. `leaf` turns a part into a leaf.
pub(super) fn split(
    mut items: Vec<usize>,
    centre: &dyn Fn(usize) -> [f32; 3],
    fits: &dyn Fn(&[usize]) -> bool,
    leaf: &mut dyn FnMut(Vec<usize>) -> usize,
) -> Tree {
    if items.len() <= 1 || fits(&items) {
        return Tree::Leaf(leaf(items));
    }
    let (low, high) = bounds(items.iter().map(|&i| centre(i)));
    let axis = (0..3)
        .max_by(|&a, &b| (high[a] - low[a]).total_cmp(&(high[b] - low[b])))
        .unwrap_or(0);
    items.sort_by(|&a, &b| centre(a)[axis].total_cmp(&centre(b)[axis]));
    let upper = items.split_off(items.len() / 2);
    let left = split(items, centre, fits, leaf);
    let right = split(upper, centre, fits, leaf);
    Tree::Split(Box::new(left), Box::new(right))
}

/// The axis bytes that shrink `parent` to a box holding `child`, and the box they decode to.
pub(super) fn shrink(
    parent: ([f32; 3], [f32; 3]),
    child: ([f32; 3], [f32; 3]),
) -> ([u8; 3], ([f32; 3], [f32; 3])) {
    let mut bytes = [0u8; 3];
    let mut low = parent.0;
    let mut high = parent.1;
    for k in 0..3 {
        let extent = parent.1[k] - parent.0[k];
        if extent <= 0. {
            continue;
        }
        // Leave a margin for the client's own rounding.
        let slack = extent * 1e-5;
        let a = (0..16u8)
            .rev()
            .find(|&n| parent.0[k] + f32::from(n * n) * extent / STEPS <= child.0[k] - slack)
            .unwrap_or(0);
        let c = (0..16u8)
            .rev()
            .find(|&n| parent.1[k] - f32::from(n * n) * extent / STEPS >= child.1[k] + slack)
            .unwrap_or(0);
        bytes[k] = (a << 4) | c;
        low[k] = parent.0[k] + f32::from(a * a) * extent / STEPS;
        high[k] = parent.1[k] - f32::from(c * c) * extent / STEPS;
    }
    (bytes, (low, high))
}

/// One section: its quantized vertices and its primitives' local indices.
struct Section {
    domain: ([f32; 3], [f32; 3]),
    scale: [f32; 3],
    packed: Vec<u32>,
    decoded: Vec<[f32; 3]>,
    primitives: Vec<[u8; 3]>,
}

impl Section {
    fn new(triangles: &[Triangle], members: &[usize]) -> Result<Self> {
        let mut index = HashMap::<[u32; 3], u8>::new();
        let mut points = Vec::new();
        let mut corners = Vec::with_capacity(members.len());
        for &member in members {
            let mut local = [0u8; 3];
            for (slot, point) in local.iter_mut().zip(triangles[member]) {
                let key = point.map(f32::to_bits);
                *slot = match index.get(&key) {
                    Some(&at) => at,
                    None => {
                        let at = u8::try_from(points.len())
                            .ok()
                            .filter(|&at| usize::from(at) < VERTICES_PER_SECTION)
                            .context("Collision section exceeds its vertices")?;
                        index.insert(key, at);
                        points.push(point);
                        at
                    }
                };
            }
            corners.push(local);
        }
        let (low, mut high) = bounds(points.iter().copied());
        for k in 0..3 {
            high[k] = high[k].max(low[k] + 1e-3);
        }
        let scale: [f32; 3] = std::array::from_fn(|k| (high[k] - low[k]) / STEPS_PER_AXIS[k]);
        let mut packed = Vec::with_capacity(points.len());
        let mut decoded = Vec::with_capacity(points.len());
        for point in &points {
            let q: [u32; 3] = std::array::from_fn(|k| {
                ((point[k] - low[k]) / scale[k])
                    .round()
                    .clamp(0., STEPS_PER_AXIS[k]) as u32
            });
            packed.push(q[0] | (q[1] << 11) | (q[2] << 22));
            decoded.push(std::array::from_fn(|k| low[k] + q[k] as f32 * scale[k]));
        }
        // Quantization can merge corners. A primitive that loses one is dropped.
        let primitives = corners
            .into_iter()
            .filter(|c| {
                let [a, b, d] = c.map(|i| decoded[usize::from(i)]);
                a != b && b != d && a != d
            })
            .collect::<Vec<_>>();
        let (low, high) = bounds(decoded.iter().copied());
        Ok(Self {
            domain: (low, high),
            scale,
            packed,
            decoded,
            primitives,
        })
    }

    fn primitive_bounds(&self, primitive: usize) -> ([f32; 3], [f32; 3]) {
        bounds(
            self.primitives[primitive]
                .iter()
                .map(|&i| self.decoded[usize::from(i)]),
        )
    }

    /// The section's four-byte nodes in preorder.
    fn nodes(&self) -> Result<Vec<u8>> {
        let centre = |p: usize| {
            let (low, high) = self.primitive_bounds(p);
            std::array::from_fn(|k| (low[k] + high[k]) / 2.)
        };
        let tree = split(
            (0..self.primitives.len()).collect(),
            &centre,
            &|items| items.len() <= 1,
            &mut |items| items[0],
        );
        let mut out = Vec::with_capacity(tree.count() * 4);
        self.encode(&tree, self.domain, &mut out)?;
        Ok(out)
    }

    fn tree_bounds(&self, tree: &Tree) -> ([f32; 3], [f32; 3]) {
        match tree {
            Tree::Leaf(p) => self.primitive_bounds(*p),
            Tree::Split(left, right) => {
                let (a, b) = (self.tree_bounds(left), self.tree_bounds(right));
                bounds([a.0, a.1, b.0, b.1].into_iter())
            }
        }
    }

    fn encode(&self, tree: &Tree, parent: ([f32; 3], [f32; 3]), out: &mut Vec<u8>) -> Result<()> {
        let (bytes, decoded) = shrink(parent, self.tree_bounds(tree));
        out.extend_from_slice(&bytes);
        match tree {
            Tree::Leaf(p) => out.push(u8::try_from(p * 2).context("Section primitive")?),
            Tree::Split(left, right) => {
                out.push(u8::try_from(left.count() + 2).context("Section tree depth")?);
                self.encode(left, decoded, out)?;
                self.encode(right, decoded, out)?;
            }
        }
        Ok(())
    }
}

/// The section tree's five-byte nodes in preorder. `leaves` receives each section's leaf node.
fn encode_sections(
    tree: &Tree,
    sections: &[Section],
    parent: ([f32; 3], [f32; 3]),
    out: &mut Vec<u8>,
    leaves: &mut [u16],
) -> Result<()> {
    fn tree_bounds(tree: &Tree, sections: &[Section]) -> ([f32; 3], [f32; 3]) {
        match tree {
            Tree::Leaf(s) => sections[*s].domain,
            Tree::Split(left, right) => {
                let (a, b) = (tree_bounds(left, sections), tree_bounds(right, sections));
                bounds([a.0, a.1, b.0, b.1].into_iter())
            }
        }
    }
    let (bytes, decoded) = shrink(parent, tree_bounds(tree, sections));
    let node = u16::try_from(out.len() / 5).context("Too many collision sections")?;
    out.extend_from_slice(&bytes);
    let value = match tree {
        Tree::Leaf(s) => {
            leaves[*s] = node;
            u16::try_from(*s)
                .ok()
                .filter(|&v| v < 0x8000)
                .context("Too many collision sections")?
        }
        Tree::Split(left, _) => {
            let distance = left.count() + 1;
            0x8000
                | u16::try_from(distance / 2)
                    .ok()
                    .filter(|&v| v < 0x8000)
                    .context("Collision section tree depth")?
        }
    };
    out.extend_from_slice(&value.to_be_bytes());
    if let Tree::Split(left, right) = tree {
        encode_sections(left, sections, decoded, out, leaves)?;
        encode_sections(right, sections, decoded, out, leaves)?;
    }
    Ok(())
}

pub(super) fn vector(values: [f32; 3], w: f32) -> Vec<u8> {
    values
        .into_iter()
        .chain([w])
        .flat_map(f32::to_le_bytes)
        .collect()
}

/// A packfile whose one body holds `triangles` in one compressed mesh shape. `template` must
/// hold the root and `hkpBvCompressedMeshShape` classes, as a stock map object's collision does.
pub(crate) fn compressed_mesh(template: &[u8], triangles: &[Triangle]) -> Result<Vec<u8>> {
    let template = Template::read(template, "s_physics_component_havok_data")?;
    let root = template.object("s_physics_component_havok_data", ROOT_SIZE)?;
    let shape_bytes = template.object("hkpBvCompressedMeshShape", SHAPE_SIZE)?;
    let mut w = Writer {
        data: Vec::new(),
        local: Vec::new(),
        references: Vec::new(),
        objects: Vec::new(),
    };
    w.object(&root, "s_physics_component_havok_data")?;
    for field in [0, 16, 48] {
        w.array(field, &[], 8)?;
    }
    let slots = w.array(32, &[0; 8], 8)?;
    let (shape, _) = mesh_shape(&mut w, &shape_bytes, triangles)?;
    w.references
        .push((u32::try_from(slots)?, u32::try_from(shape)?));
    w.finish(template)
}

/// A packfile whose root is one compressed mesh shape holding `triangles`, as each shape packfile
/// of a stock world collision is, and the bits the shape's keys use. `template` must be such a
/// packfile. Like a stock one, every primitive takes the world filter and user data 0 from
/// one-entry palettes, which the world collision record maps to a material.
pub(crate) fn bare_mesh(template: &[u8], triangles: &[Triangle]) -> Result<(Vec<u8>, u32)> {
    let template = Template::read(template, "hkpBvCompressedMeshShape")?;
    let shape_bytes = template.object("hkpBvCompressedMeshShape", SHAPE_SIZE)?;
    let mut w = Writer {
        data: Vec::new(),
        local: Vec::new(),
        references: Vec::new(),
        objects: Vec::new(),
    };
    let (_, bits) = write_shape(&mut w, &shape_bytes, triangles, true)?;
    Ok((w.finish(template)?, bits))
}

/// Writes a compressed mesh shape holding `triangles` from the template object `shape_bytes`,
/// returning where it starts and the bits its keys use. Its primitives carry no filter or user
/// data and keep their body's own.
pub(super) fn mesh_shape(
    w: &mut Writer,
    shape_bytes: &[u8],
    triangles: &[Triangle],
) -> Result<(usize, u32)> {
    write_shape(w, shape_bytes, triangles, false)
}

/// `mesh_shape`, and with `world`, every primitive takes the world filter and user data 0 from
/// one-entry palettes instead.
pub(super) fn write_shape(
    w: &mut Writer,
    shape_bytes: &[u8],
    triangles: &[Triangle],
    world: bool,
) -> Result<(usize, u32)> {
    let kept = triangles
        .iter()
        .copied()
        .filter(|t| {
            let [a, b, c] = *t;
            let ab: [f32; 3] = std::array::from_fn(|k| b[k] - a[k]);
            let ac: [f32; 3] = std::array::from_fn(|k| c[k] - a[k]);
            let n = [
                ab[1] * ac[2] - ab[2] * ac[1],
                ab[2] * ac[0] - ab[0] * ac[2],
                ab[0] * ac[1] - ab[1] * ac[0],
            ];
            t.iter().flatten().all(|v| v.is_finite())
                && n.iter().map(|v| v * v).sum::<f32>() > 1e-12
        })
        .collect::<Vec<_>>();
    ensure!(!kept.is_empty(), "Collision mesh has no triangles");
    let centre = |t: usize| -> [f32; 3] {
        std::array::from_fn(|k| (kept[t][0][k] + kept[t][1][k] + kept[t][2][k]) / 3.)
    };
    let fits = |items: &[usize]| {
        items.len() <= PRIMITIVES_PER_SECTION && {
            let mut seen = std::collections::HashSet::new();
            for &t in items {
                for point in kept[t] {
                    seen.insert(point.map(f32::to_bits));
                }
            }
            seen.len() <= VERTICES_PER_SECTION
        }
    };
    let mut sections = Vec::new();
    let mut failure = None;
    let tree = split((0..kept.len()).collect(), &centre, &fits, &mut |items| {
        match Section::new(&kept, &items) {
            Ok(section) => sections.push(section),
            Err(error) => failure = Some(error),
        }
        sections.len().saturating_sub(1)
    });
    if let Some(error) = failure {
        return Err(error);
    }
    let domain = bounds(sections.iter().flat_map(|s| [s.domain.0, s.domain.1]));
    let mut top = Vec::new();
    let mut leaves = vec![0u16; sections.len()];
    encode_sections(&tree, &sections, domain, &mut top, &mut leaves)?;

    let shape = w.object(shape_bytes, "hkpBvCompressedMeshShape")?;
    w.data[shape + PER_PRIMITIVE_FILTER] = u8::from(world);
    w.data[shape + PER_PRIMITIVE_USER_DATA] = u8::from(world);
    let (filters, user_data) = if world {
        (
            WORLD_FILTER.to_le_bytes().to_vec(),
            0u32.to_le_bytes().to_vec(),
        )
    } else {
        (Vec::new(), Vec::new())
    };
    w.array(shape + FILTER_PALETTE, &filters, 4)?;
    w.array(shape + USER_DATA_PALETTE, &user_data, 4)?;
    w.array(shape + STRING_PALETTE, &[], 8)?;
    let tree_at = shape + SHAPE_TREE;
    w.array(tree_at, &top, 5)?;
    put(&mut w.data, tree_at + 0x10, &vector(domain.0, 1.))?;
    put(&mut w.data, tree_at + 0x20, &vector(domain.1, 1.))?;
    let mut keys = 0u32;
    let mut maximum = 0u32;
    for (index, section) in sections.iter().enumerate() {
        keys += u32::try_from(section.primitives.len())?;
        if let Some(last) = section.primitives.len().checked_sub(1) {
            maximum = maximum.max((u32::try_from(index)? << 8) | (u32::try_from(last)? << 1));
        }
    }
    let bits = (32 - maximum.leading_zeros()).max(1);
    put(&mut w.data, tree_at + 0x30, &keys.to_le_bytes())?;
    put(&mut w.data, tree_at + 0x34, &bits.to_le_bytes())?;
    put(&mut w.data, tree_at + 0x38, &maximum.to_le_bytes())?;
    put(&mut w.data, tree_at + 0x3C, &[0; 4])?;

    let mut rows = vec![0u8; sections.len() * SECTION_SIZE];
    let mut primitives = Vec::new();
    let mut packed = Vec::new();
    let mut runs = Vec::new();
    let mut first_primitive = 0usize;
    let mut first_vertex = 0usize;
    for (index, section) in sections.iter().enumerate() {
        let row = &mut rows[index * SECTION_SIZE..(index + 1) * SECTION_SIZE];
        put(row, 0x10, &vector(section.domain.0, 1.))?;
        put(row, 0x20, &vector(section.domain.1, 1.))?;
        for (k, value) in section
            .domain
            .0
            .into_iter()
            .chain(section.scale)
            .enumerate()
        {
            put(row, 0x30 + k * 4, &value.to_le_bytes())?;
        }
        let count = section.primitives.len();
        let vertices = section.packed.len();
        ensure!(
            first_primitive < 1 << 24 && index < 1 << 24,
            "Collision mesh exceeds its ranges"
        );
        put(row, 0x48, &u32::try_from(first_vertex)?.to_le_bytes())?;
        put(row, 0x4C, &u32::try_from(vertices)?.to_le_bytes())?;
        put(
            row,
            0x50,
            &(u32::try_from(first_primitive)? << 8 | u32::try_from(count)?).to_le_bytes(),
        )?;
        put(row, 0x54, &(u32::try_from(index)? << 8 | 1).to_le_bytes())?;
        row[0x58] = u8::try_from(vertices)?;
        row[0x59] = 0;
        put(row, 0x5A, &leaves[index].to_le_bytes())?;
        for primitive in &section.primitives {
            primitives.extend_from_slice(&[primitive[0], primitive[1], primitive[2], primitive[2]]);
        }
        for value in &section.packed {
            packed.extend_from_slice(&value.to_le_bytes());
        }
        // One run per section: every primitive keeps value 0.
        runs.extend_from_slice(&0u32.to_le_bytes());
        runs.extend_from_slice(&[0, u8::try_from(count)?, 0, 0]);
        first_primitive += count;
        first_vertex += vertices;
    }
    let sections_at = w.array(tree_at + 0x40, &rows, SECTION_SIZE)?;
    for (index, section) in sections.iter().enumerate() {
        let nodes = section.nodes()?;
        w.array(sections_at + index * SECTION_SIZE, &nodes, 4)?;
    }
    w.array(tree_at + 0x50, &primitives, 4)?;
    w.array(tree_at + 0x60, &[], 2)?;
    w.array(tree_at + 0x70, &packed, 4)?;
    w.array(tree_at + 0x80, &[], 8)?;
    w.array(tree_at + 0x90, &runs, 8)?;
    Ok((shape, bits))
}

//! A world's static collision as an `hkpStaticCompoundShape`, the root of a stock world collision
//! record's main packfile. Each 80-byte instance keeps a transform, a shape, a filter and the
//! filter mask its children inherit. A stock instance's shape is null in the packfile, and the
//! client binds it from the instance's user data. These instances also carry their own mesh in
//! the same packfile, so they hold a shape whether or not that binding runs. The
//! translation's `w` carries the instance's flags in its low bits (`2` when the instance moves,
//! `4` when it scales) above the bits of `0.5`, and the scale's `w` stays `0.5`.
//!
//! A tree of six-byte nodes bounds the instances. Its first three bytes shrink the parent's box
//! as the compressed mesh trees' do. The fourth byte holds an inner node's flag in its high bit
//! and the high bits of a value whose low 16 bits follow little-endian: half the distance to the
//! right child for an inner node, and the instance for a leaf. Nodes are stored in preorder with
//! the left child next. A preorder walk of Distant Shore's world collision this way reaches all
//! 3,995 of its nodes and each of its 1,998 instances once.
use super::{
    Template, Writer,
    mesh::{SHAPE_SIZE as MESH_SIZE, Tree, Triangle, bounds, shrink, split, vector, write_shape},
    put,
};
use anyhow::{Context, Result, ensure};

/// The object and instance layouts of the 64-bit 2012.2 class.
pub(super) const SHAPE_SIZE: usize = 0xA0;
const KEY_BITS: usize = 0x30;
const INSTANCES: usize = 0x38;
const EXTRA_INFOS: usize = 0x48;
const TREE: usize = 0x70;
const DOMAIN: usize = 0x80;
const INSTANCE_SIZE: usize = 80;
const INSTANCE_SHAPE: usize = 0x30;
const INSTANCE_FILTER: usize = 0x38;
const INSTANCE_USER_DATA: usize = 0x40;
/// `0.5` as bits, the flag of an instance that moves its shape, and the flag of an instance that
/// holds its shape in the packfile, as stock kill volume compounds set it.
const HALF: u32 = 0x3F00_0000;
const MOVES: u32 = 2;
const HOLDS_SHAPE: u32 = 1;
/// The bits a convex child's shape keys take in a kill volume compound.
const CONVEX_KEY_BITS: u8 = 1;
/// The fewest bits a child's shape keys take, as stock world collision sets them.
const MINIMUM_KEY_BITS: u32 = 16;

/// One placed shape: where its shape's origin sits, the box its shape fills there, the user data
/// that names its shape to the client, and the triangles of its own copy of that shape.
pub(crate) struct Instance {
    pub translation: [f32; 3],
    pub bounds: ([f32; 3], [f32; 3]),
    pub user_data: u64,
    pub triangles: Vec<Triangle>,
}

/// Bounds of a subtree's instances.
fn tree_bounds(tree: &Tree, instances: &[Instance]) -> ([f32; 3], [f32; 3]) {
    match tree {
        Tree::Leaf(index) => instances[*index].bounds,
        Tree::Split(left, right) => {
            let (a, b) = (tree_bounds(left, instances), tree_bounds(right, instances));
            bounds([a.0, a.1, b.0, b.1].into_iter())
        }
    }
}

fn encode(
    tree: &Tree,
    instances: &[Instance],
    parent: ([f32; 3], [f32; 3]),
    out: &mut Vec<u8>,
) -> Result<()> {
    let (bytes, decoded) = shrink(parent, tree_bounds(tree, instances));
    out.extend_from_slice(&bytes);
    let (inner, value) = match tree {
        Tree::Leaf(index) => (false, *index),
        Tree::Split(left, _) => (true, left.count().div_ceil(2)),
    };
    ensure!(value < 1 << 23, "Collision instance tree is too large");
    out.push(u8::from(inner) << 7 | u8::try_from(value >> 16)?);
    out.extend_from_slice(&u16::try_from(value & 0xFFFF)?.to_le_bytes());
    if let Tree::Split(left, right) = tree {
        encode(left, instances, decoded, out)?;
        encode(right, instances, decoded, out)?;
    }
    Ok(())
}

/// A packfile whose root is a static compound of `instances`, each holding a compressed mesh of
/// its triangles. `template` must be a stock world collision's main packfile, whose class names
/// include the mesh class, and `mesh_template` one of its bare mesh packfiles.
pub(crate) fn static_compound(
    template: &[u8],
    mesh_template: &[u8],
    instances: &[Instance],
) -> Result<Vec<u8>> {
    ensure!(!instances.is_empty(), "A static compound needs instances");
    let template = Template::read(template, "hkpStaticCompoundShape")?;
    let shape_bytes = template.object("hkpStaticCompoundShape", SHAPE_SIZE)?;
    let mesh_bytes = Template::read(mesh_template, "hkpBvCompressedMeshShape")?
        .object("hkpBvCompressedMeshShape", MESH_SIZE)?;
    ensure!(
        template.names.contains_key("hkpBvCompressedMeshShape"),
        "The world collision template does not name the mesh class"
    );
    let centre = |index: usize| -> [f32; 3] {
        let (low, high) = instances[index].bounds;
        std::array::from_fn(|k| (low[k] + high[k]) / 2.)
    };
    let tree = split(
        (0..instances.len()).collect(),
        &centre,
        &|items| items.len() == 1,
        &mut |items| items[0],
    );
    let domain = bounds(instances.iter().flat_map(|i| [i.bounds.0, i.bounds.1]));
    let mut nodes = Vec::with_capacity(tree.count() * 6);
    encode(&tree, instances, domain, &mut nodes)?;

    let mut rows = vec![0u8; instances.len() * INSTANCE_SIZE];
    for (instance, row) in instances.iter().zip(rows.chunks_exact_mut(INSTANCE_SIZE)) {
        let moves = instance.translation.iter().any(|&v| v != 0.);
        put(row, 0, &vector(instance.translation, 0.))?;
        put(
            row,
            12,
            &(HALF | if moves { MOVES } else { 0 }).to_le_bytes(),
        )?;
        put(row, 16, &vector([0., 0., 0.], 1.))?;
        put(row, 32, &vector([1., 1., 1.], 0.))?;
        put(row, 44, &HALF.to_le_bytes())?;
        put(row, INSTANCE_FILTER, &0u32.to_le_bytes())?;
        put(row, INSTANCE_FILTER + 4, &u32::MAX.to_le_bytes())?;
        put(row, INSTANCE_USER_DATA, &instance.user_data.to_le_bytes())?;
    }

    let mut w = Writer {
        data: Vec::new(),
        local: Vec::new(),
        references: Vec::new(),
        objects: Vec::new(),
    };
    let shape = w.object(&shape_bytes, "hkpStaticCompoundShape")?;
    let instances_at = w.array(shape + INSTANCES, &rows, INSTANCE_SIZE)?;
    w.array(shape + EXTRA_INFOS, &[], 2)?;
    w.array(shape + TREE, &nodes, 6)?;
    put(&mut w.data, shape + DOMAIN, &vector(domain.0, 0.))?;
    put(&mut w.data, shape + DOMAIN + 0x10, &vector(domain.1, 0.))?;
    let mut key_bits = MINIMUM_KEY_BITS;
    for (index, instance) in instances.iter().enumerate() {
        let (mesh, bits) = write_shape(&mut w, &mesh_bytes, &instance.triangles, true)?;
        key_bits = key_bits.max(bits);
        w.references.push((
            u32::try_from(instances_at + index * INSTANCE_SIZE + INSTANCE_SHAPE)?,
            u32::try_from(mesh)?,
        ));
    }
    ensure!(
        key_bits < 32 && (instances.len() as u64) < 1 << (32 - key_bits),
        "Static compound shape keys exceed 32 bits"
    );
    w.data[shape + KEY_BITS] = u8::try_from(key_bits).context("Static compound key bits")?;
    w.finish(template)
}

/// A static compound over convex `shapes` already written to `w`, each at the origin and filling
/// its `bounds`, as a stock kill volume packfile holds the prisms of a volume with more than three.
/// Its instances name their shapes in the packfile, with no filter and every mask bit, and its
/// tree bounds them.
pub(super) fn convex_compound(
    w: &mut Writer,
    shape_bytes: &[u8],
    shapes: &[usize],
    boxes: &[([f32; 3], [f32; 3])],
) -> Result<usize> {
    ensure!(
        !shapes.is_empty() && shapes.len() == boxes.len() && shapes.len() < 1 << 16,
        "Invalid convex compound children"
    );
    let instances = boxes
        .iter()
        .map(|&bounds| Instance {
            translation: [0.; 3],
            bounds,
            user_data: 0,
            triangles: Vec::new(),
        })
        .collect::<Vec<_>>();
    let centre = |index: usize| -> [f32; 3] {
        let (low, high) = instances[index].bounds;
        std::array::from_fn(|k| (low[k] + high[k]) / 2.)
    };
    let tree = split(
        (0..instances.len()).collect(),
        &centre,
        &|items| items.len() == 1,
        &mut |items| items[0],
    );
    let domain = bounds(instances.iter().flat_map(|i| [i.bounds.0, i.bounds.1]));
    let mut nodes = Vec::with_capacity(tree.count() * 6);
    encode(&tree, &instances, domain, &mut nodes)?;
    let mut rows = vec![0u8; instances.len() * INSTANCE_SIZE];
    for row in rows.chunks_exact_mut(INSTANCE_SIZE) {
        put(row, 0, &vector([0., 0., 0.], 0.))?;
        put(row, 12, &(HALF | HOLDS_SHAPE).to_le_bytes())?;
        put(row, 16, &vector([0., 0., 0.], 1.))?;
        put(row, 32, &vector([1., 1., 1.], 0.))?;
        put(row, 44, &HALF.to_le_bytes())?;
        put(row, INSTANCE_FILTER, &0u32.to_le_bytes())?;
        put(row, INSTANCE_FILTER + 4, &u32::MAX.to_le_bytes())?;
        put(row, INSTANCE_USER_DATA, &0u64.to_le_bytes())?;
    }
    let shape = w.object(shape_bytes, "hkpStaticCompoundShape")?;
    let instances_at = w.array(shape + INSTANCES, &rows, INSTANCE_SIZE)?;
    w.array(shape + EXTRA_INFOS, &[], 2)?;
    w.array(shape + TREE, &nodes, 6)?;
    put(&mut w.data, shape + DOMAIN, &vector(domain.0, 0.))?;
    put(&mut w.data, shape + DOMAIN + 0x10, &vector(domain.1, 0.))?;
    for (index, &child) in shapes.iter().enumerate() {
        w.references.push((
            u32::try_from(instances_at + index * INSTANCE_SIZE + INSTANCE_SHAPE)?,
            u32::try_from(child)?,
        ));
    }
    w.data[shape + KEY_BITS] = CONVEX_KEY_BITS;
    Ok(shape)
}

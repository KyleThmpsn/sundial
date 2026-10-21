//! A level's collision surfaces as convex polygons. The structure BSP and each instanced geometry
//! definition keep a collision BSP in the BSP's first structure resource. A structure too large
//! for 16-bit indices keeps a large collision BSP instead, whose rows hold 32-bit indices. Its
//! surfaces are convex polygons whose edges link them, and walking a surface's edges with the
//! surface on their left winds its points so that their normal faces open space.
//!
//! A surface names one of the structure's collision materials. A material with a seam index marks
//! the faces where the structure meets another structure BSP. Where both are loaded, the player
//! walks through those faces into the next: a level of several zone sets kept every seam face of
//! Tip of the Spear solid, an invisible wall where one stretch met the next. So a seam that two
//! loaded BSPs share ([`shared_seams`]) collides with nothing, and one whose neighbour is not
//! loaded still closes the level.
use std::collections::{BTreeMap, BTreeSet};

use super::{Definition, Instance, METRES_PER_UNIT, Space};
use crate::halo_reach::{
    cache::{Cache, Tag},
    resource::Pages,
};
use anyhow::{Context, Result, ensure};

/// The structure resource that keeps collision.
const COLLISION_RESOURCE: usize = 0x518;
/// Blocks of its definition's root: the structure's collision BSP, then its large collision BSP,
/// at most one row each, then the instanced geometry definitions, which follow the render
/// geometry's mesh order.
const ROOT_STRUCTURE: usize = 0x00;
const ROOT_LARGE_STRUCTURE: usize = 0x0C;
const ROOT_DEFINITIONS: usize = 0x18;
const STRUCTURE_SIZE: usize = 0x70;
/// The nine blocks of a large collision BSP row that precede anything this reader leaves unread.
const LARGE_STRUCTURE_SIZE: usize = 0x6C;
const DEFINITION_SIZE: usize = 0x144;
/// A definition's collision BSP.
const DEFINITION_BSP: usize = 0x08;
/// No collision surface in the examined levels has more edges.
const MAX_EDGES: usize = 256;
/// A structure BSP's collision materials, `0x18`-byte rows whose `i16` at `+0x14` is the seam the
/// material marks, -1 for none.
const MATERIALS: (usize, usize) = (0x58, 0x18);
const MATERIAL_SEAM: usize = 0x14;

/// Where a collision BSP keeps its surfaces, edges and vertices: each block's offset in the BSP's
/// row and its row size, the offset of a surface's first edge, and the width of every index.
struct Layout {
    surfaces: usize,
    surface_size: usize,
    surface_first_edge: usize,
    /// The offset of a surface's `i16` collision material index.
    surface_material: usize,
    edges: usize,
    edge_size: usize,
    vertices: usize,
    vertex_size: usize,
    index: usize,
}

/// A collision BSP with 16-bit indices: 12-byte surfaces, edges of six `u16` and 16-byte vertices.
const SMALL: Layout = Layout {
    surfaces: 0x48,
    surface_size: 12,
    surface_first_edge: 2,
    surface_material: 4,
    edges: 0x54,
    edge_size: 12,
    vertices: 0x60,
    vertex_size: 16,
    index: 2,
};

/// A large collision BSP, the same blocks with 32-bit indices: 16-byte surfaces whose first edge is
/// at `+0x04`, edges of six `i32` and 20-byte vertices.
const LARGE: Layout = Layout {
    surfaces: 0x48,
    surface_size: 16,
    surface_first_edge: 4,
    surface_material: 8,
    edges: 0x54,
    edge_size: 24,
    vertices: 0x60,
    vertex_size: 20,
    index: 4,
};

/// A convex collision polygon in meters, wound so that its normal faces open space.
#[derive(Clone, Debug)]
pub struct Polygon {
    pub points: Vec<[f32; 3]>,
}

/// A level's collision: the structure's polygons in world space, and each instanced geometry
/// definition's polygons in its own space.
pub struct Collision {
    pub structure: Vec<Polygon>,
    pub definitions: Vec<Vec<Polygon>>,
    /// The structure's surfaces left out as faces of a seam to another loaded BSP.
    pub open_seam_surfaces: usize,
}

impl Collision {
    /// Instance `instance`'s polygons in world space.
    pub fn placed(&self, instance: &Instance) -> Result<Vec<Polygon>> {
        let polygons = self
            .definitions
            .get(instance.mesh)
            .context("Instance names a definition outside the collision resource")?;
        Ok(polygons
            .iter()
            .map(|polygon| {
                let mut points = polygon
                    .points
                    .iter()
                    .map(|&point| instance.place(point))
                    .collect::<Vec<_>>();
                // A mirroring placement reverses the winding.
                if instance.scale < 0. {
                    points.reverse();
                }
                Polygon { points }
            })
            .collect())
    }
}

/// The polygons of the collision BSP at `bsp` in `definition`, laid out as `layout`, without the
/// surfaces whose collision material index `open` holds, and how many it left out.
fn polygons(
    definition: &Definition,
    cache: &Cache,
    pages: &mut Pages,
    bsp: usize,
    layout: &Layout,
    open: &BTreeSet<usize>,
) -> Result<(Vec<Polygon>, usize)> {
    let (surfaces, surface_rows) =
        definition.data(cache, pages, bsp + layout.surfaces, layout.surface_size)?;
    if surfaces == 0 {
        return Ok((Vec::new(), 0));
    }
    let (edges, edge_rows) = definition.data(cache, pages, bsp + layout.edges, layout.edge_size)?;
    let (vertices, vertex_rows) =
        definition.data(cache, pages, bsp + layout.vertices, layout.vertex_size)?;
    // An index of `layout.index` bytes. A negative 32-bit index reads as a value past every block.
    let index_at = |rows: &[u8], at: usize| -> usize {
        match layout.index {
            2 => usize::from(u16::from_le_bytes([rows[at], rows[at + 1]])),
            _ => usize::try_from(u32::from_le_bytes(rows[at..at + 4].try_into().unwrap()))
                .unwrap_or(usize::MAX),
        }
    };
    let point = |index: usize| -> Result<[f32; 3]> {
        ensure!(index < vertices, "Collision edge names a missing vertex");
        let at = index * layout.vertex_size;
        Ok(std::array::from_fn(|axis| {
            f32::from_le_bytes(
                vertex_rows[at + axis * 4..at + axis * 4 + 4]
                    .try_into()
                    .unwrap(),
            ) * METRES_PER_UNIT
        }))
    };
    let mut out = Vec::with_capacity(surfaces);
    let mut left_out = 0;
    for surface in 0..surfaces {
        let at = surface * layout.surface_size + layout.surface_material;
        let material = i16::from_le_bytes([surface_rows[at], surface_rows[at + 1]]);
        if usize::try_from(material).is_ok_and(|material| open.contains(&material)) {
            left_out += 1;
            continue;
        }
        let first = index_at(
            &surface_rows,
            surface * layout.surface_size + layout.surface_first_edge,
        );
        let mut edge = first;
        let mut points = Vec::new();
        loop {
            ensure!(edge < edges, "Collision surface names a missing edge");
            let at = edge * layout.edge_size;
            let field = |k: usize| index_at(&edge_rows, at + k * layout.index);
            let (start, end, forward, reverse, left, right) =
                (field(0), field(1), field(2), field(3), field(4), field(5));
            if left == surface {
                points.push(point(start)?);
                edge = forward;
            } else {
                ensure!(
                    right == surface,
                    "Collision edge does not border its surface"
                );
                points.push(point(end)?);
                edge = reverse;
            }
            if edge == first {
                break;
            }
            ensure!(points.len() < MAX_EDGES, "Collision surface does not close");
        }
        out.push(Polygon { points });
    }
    Ok((out, left_out))
}

/// Each collision material of structure BSP `tag` with the seam it marks, if any.
fn material_seams(cache: &Cache, tag: &Tag) -> Result<Vec<Option<i16>>> {
    cache
        .block(tag.address()? + MATERIALS.0, MATERIALS.1)?
        .into_iter()
        .map(|row| Ok(Some(cache.i16(row + MATERIAL_SEAM)?).filter(|&seam| seam >= 0)))
        .collect()
}

/// The seams at least two of the structure BSPs `bsps` mark, which join loaded BSPs.
pub fn shared_seams(cache: &Cache, bsps: &[Tag]) -> Result<BTreeSet<i16>> {
    let mut marked = BTreeMap::<i16, usize>::new();
    for bsp in bsps {
        for seam in material_seams(cache, bsp)?
            .into_iter()
            .flatten()
            .collect::<BTreeSet<_>>()
        {
            *marked.entry(seam).or_default() += 1;
        }
    }
    Ok(marked
        .into_iter()
        .filter(|&(_, count)| count >= 2)
        .map(|(seam, _)| seam)
        .collect())
}

/// Reads the collision of `tag`, a structure BSP. The structure's polygons come from its collision
/// BSP or its large collision BSP, whichever the resource holds, and a structure built entirely
/// from instanced geometry would have none. The faces of the `open` seams are left out.
pub fn read(
    cache: &Cache,
    pages: &mut Pages,
    tag: &Tag,
    open: &BTreeSet<i16>,
) -> Result<Collision> {
    let definition = Definition::load(cache, tag, COLLISION_RESOURCE)?;
    let open_materials = material_seams(cache, tag)?
        .into_iter()
        .enumerate()
        .filter(|(_, seam)| seam.is_some_and(|seam| open.contains(&seam)))
        .map(|(material, _)| material)
        .collect::<BTreeSet<_>>();
    let mut structure = Vec::new();
    let mut open_seam_surfaces = 0;
    for (root, size, layout) in [
        (ROOT_STRUCTURE, STRUCTURE_SIZE, &SMALL),
        (ROOT_LARGE_STRUCTURE, LARGE_STRUCTURE_SIZE, &LARGE),
    ] {
        let (count, space, start) = definition.block(definition.root + root, size)?;
        ensure!(
            count == 0 || (count == 1 && space == Space::Definition),
            "The structure's collision BSP block at {root:#x} holds {count} rows"
        );
        if count == 1 {
            let (polygons, left_out) =
                polygons(&definition, cache, pages, start, layout, &open_materials)?;
            structure.extend(polygons);
            open_seam_surfaces += left_out;
        }
    }
    let (count, space, start) =
        definition.block(definition.root + ROOT_DEFINITIONS, DEFINITION_SIZE)?;
    ensure!(
        count == 0 || space == Space::Definition,
        "Instanced geometry definitions are outside the definition"
    );
    let definitions = (0..count)
        .map(|index| {
            polygons(
                &definition,
                cache,
                pages,
                start + index * DEFINITION_SIZE + DEFINITION_BSP,
                &SMALL,
                &BTreeSet::new(),
            )
            .map(|(polygons, _)| polygons)
            .with_context(|| format!("Instanced geometry definition {index}"))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Collision {
        structure,
        definitions,
        open_seam_surfaces,
    })
}

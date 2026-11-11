//! A structure BSP's preplaced decals. Reach projects each decal the scenario places onto the
//! level when it builds the cache, and keeps the projected triangles in the BSP's decal render
//! geometry (`+0x340`), laid out as its other render geometry blocks, with one mesh per cluster
//! and no parts. The BSP's decal rows (`+0x328`) name each decal's decal system by datum, its
//! cluster's mesh at byte `+0x05`, and its property start/count at `+0x18/+0x1A`. A property row
//! (`+0x334`) gives the decal's index start, index count, vertex start and vertex count in that
//! mesh, `i16` each. In Hang 'Em High every mesh's decal ranges end exactly at its vertex
//! buffer's end. A decal system (`decs`) holds its decals at `+0x2C`, `0x98`-byte rows each with
//! an embedded render method at `+0x08`. Each property selects a layer at `+0x08`, with sprite
//! offset and scale at `+0x0C/+0x14`. The original MCC renderer walks all properties.
use super::super::{
    cache::{Cache, Tag},
    material::{self, Material},
    model::{Geometry, Primitive, RawSection, Vertex},
    resource::Pages,
    rig::METRES_PER_UNIT,
};
use anyhow::{Context, Result, ensure};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// A world vertex: position `f32` at `+0x00`, texture coordinates as half floats at `+0x10`, the
/// normal at `+0x14` and the tangent at `+0x1C`, signed normalized 16-bit values.
const WORLD_FORMAT: u8 = 0;
const WORLD_STRIDE: usize = 36;

const GEOMETRY: usize = 0x340;
const ROWS: (usize, usize) = (0x328, 0x1C);
const ROW_MESH: usize = 0x04;
const ROW_PROPERTY: usize = 0x18;
const PROPERTIES: (usize, usize) = (0x334, 0x1C);
const SYSTEM_DECALS: (usize, usize) = (0x2C, 0x98);
const DECAL_RENDER_METHOD: usize = 0x08;
/// How far a decal's triangles stand off the surface they were projected onto, in meters along
/// that surface's normal, so they do not fight its depth. Reach draws them with a depth bias that
/// grows with distance (`c_decal_system::get_depth_bias`), which Shadowkeep's rasterizer states,
/// all of zero bias, cannot express. At 5 mm, Breakneck's scorch marks on its tiled walls showed
/// only on some tiles, with tile-shaped holes where the raised tile faces stood in front of them.
const STANDOFF: f32 = 0.03;
/// The farthest, in meters, a decal triangle's corner lies from the corner of the surface triangle
/// it copies. Both come from the same source positions, the surface's through its compression.
const COPY_TOLERANCE: f32 = 0.002;
/// The cell size, in meters, of the grid surface triangles are binned in by their corners.
const CELL: f32 = 0.25;

/// The decals of the structure BSP `tag` at `at` as primitives, whose materials follow
/// `materials`, to which their decal systems' materials are added, each with its lighting probe.
/// Each is fitted to the surfaces among `receivers`, whose probes are `probes`, that it lies on
/// ([`fit`]). A BSP without decal geometry gives none.
pub(super) fn read(
    cache: &Cache,
    pages: &mut Pages,
    tag: &Tag,
    at: usize,
    materials: &mut Vec<Material>,
    receivers: &[Primitive],
    probes: &[Option<usize>],
) -> Result<Vec<(Primitive, Option<usize>)>> {
    let rows = cache.block(at + ROWS.0, ROWS.1)?;
    if rows.is_empty() || cache.block(at + GEOMETRY + 4, 92)?.is_empty() {
        return Ok(Vec::new());
    }
    let properties = cache.block(at + PROPERTIES.0, PROPERTIES.1)?;
    let geometry = Geometry::read(cache, tag, at + GEOMETRY)
        .with_context(|| format!("Decal geometry of {}", tag.path))?;
    let mut sections = BTreeMap::new();
    let mut systems = BTreeMap::new();
    let mut primitives = Vec::with_capacity(rows.len());
    for (index, &row) in rows.iter().enumerate() {
        let datum = cache.u32(row)?;
        let system = cache
            .tags
            .get((datum & 0xFFFF) as usize)
            .and_then(Option::as_ref)
            .filter(|system| system.datum == datum && system.group == "decs")
            .with_context(|| format!("Decal {index} names no decal system"))?;
        let definitions = cache.block(system.address()? + SYSTEM_DECALS.0, SYSTEM_DECALS.1)?;
        // MCC 0015D75C reads the unsigned mesh byte, then walks the complete property span.
        // Each property selects its own definition and cached sprite transform.
        let mesh = usize::from(cache.u8(row + ROW_MESH + 1)?);
        let first = usize::try_from(cache.i16(row + ROW_PROPERTY)?)?;
        let count = usize::try_from(cache.i16(row + ROW_PROPERTY + 2)?)?;
        let end = first
            .checked_add(count)
            .context("Decal property range overflow")?;
        let properties = properties
            .get(first..end)
            .context("Decal properties outside table")?;
        if let std::collections::btree_map::Entry::Vacant(entry) = sections.entry(mesh) {
            let section = geometry
                .raw_section(cache, pages, mesh)?
                .with_context(|| format!("Decal mesh {mesh} has no vertices"))?;
            ensure!(
                section.format == WORLD_FORMAT
                    && section.vertices.len() == section.count * WORLD_STRIDE,
                "Decal mesh {mesh} holds vertex format {}, not world vertices",
                section.format
            );
            entry.insert(section);
        }
        for (part, &property) in properties.iter().enumerate() {
            let layer = u16::try_from(cache.i16(property + 8)?)?;
            let definition = *definitions
                .get(usize::from(layer))
                .context("Decal layer outside system")?;
            let sprite = cache.floats::<4>(property + 0x0c)?;
            ensure!(
                sprite.iter().all(|value| value.is_finite()),
                "Nonfinite decal sprite"
            );
            let render_pass = cache.u32(definition + 0x90)?;
            ensure!(render_pass <= 1, "Unknown decal render pass {render_pass}");
            let key = (datum, layer, sprite.map(f32::to_bits));
            let material = match systems.get(&key) {
                Some(&material) => material,
                None => {
                    let mut material =
                        material::embedded(cache, system.clone(), definition + DECAL_RENDER_METHOD)
                            .with_context(|| {
                                format!("Decal system {} layer {layer}", system.path)
                            })?;
                    material.decal = Some(material::Decal {
                        layer,
                        sprite,
                        render_pass,
                    });
                    let index = materials.len();
                    materials.push(material);
                    systems.insert(key, index);
                    index
                }
            };
            let primitive = primitive(
                cache,
                &sections[&mesh],
                property,
                mesh,
                first + part,
                material,
            )
            .with_context(|| format!("Decal {index} layer {layer}"))?;
            if !primitive.indices.is_empty() {
                primitives.push(primitive);
            }
        }
    }
    let probes = fit(&mut primitives, receivers, probes)?;
    Ok(primitives.into_iter().zip(probes).collect())
}

/// Fits each of `decals` to the `receivers` it lies on: turns its triangles to face out
/// ([`turn`]), lights it as they are lit ([`light`]) and stands it off them ([`stand_off`]).
/// Returns each decal's lighting probe, a receiver's among `probes` where it takes no lightmap
/// coordinates.
fn fit(
    decals: &mut [Primitive],
    receivers: &[Primitive],
    probes: &[Option<usize>],
) -> Result<Vec<Option<usize>>> {
    let surfaces = Surfaces::new(decals, receivers);
    decals
        .iter_mut()
        .map(|decal| {
            turn(decal, &surfaces);
            let corners = surfaces.light(decal);
            stand_off(decal)?;
            Ok(light(decal, &corners, probes))
        })
        .collect()
}

/// A triangle of a decal's receiving surface: its corners, the lightmap coordinates of each where
/// its receiver has them, and the receiver's index.
struct Surface {
    corners: [[f32; 3]; 3],
    lightmap: Option<[[f32; 2]; 3]>,
    receiver: usize,
}

/// The receivers' triangles binned in [`CELL`] cells by their bounds widened by
/// [`COPY_TOLERANCE`], among the cells a decal corner lies in.
struct Surfaces {
    cells: HashMap<[i32; 3], Vec<Surface>>,
}

/// A decal triangle's corner as lit by its surface: lightmap coordinates, or the receiver it lies
/// on when that receiver has none.
#[derive(Clone, Copy)]
enum Lit {
    Coordinates([f32; 2]),
    Receiver(usize),
    Unknown,
}

fn cell(p: [f32; 3]) -> [i32; 3] {
    p.map(|v| (v / CELL).floor() as i32)
}

impl Surfaces {
    fn new(decals: &[Primitive], receivers: &[Primitive]) -> Self {
        let wanted = decals
            .iter()
            .flat_map(|decal| &decal.vertices)
            .map(|vertex| cell(vertex.position))
            .collect::<HashSet<_>>();
        let mut cells = HashMap::<[i32; 3], Vec<Surface>>::new();
        for (receiver, primitive) in receivers.iter().enumerate() {
            for face in primitive.indices.chunks_exact(3) {
                let vertices = [0, 1, 2].map(|k| &primitive.vertices[face[k] as usize]);
                let corners = vertices.map(|v| v.position);
                let low = cell(std::array::from_fn(|a| {
                    corners.iter().map(|c| c[a]).fold(f32::MAX, f32::min) - COPY_TOLERANCE
                }));
                let high = cell(std::array::from_fn(|a| {
                    corners.iter().map(|c| c[a]).fold(f32::MIN, f32::max) + COPY_TOLERANCE
                }));
                // A face's cells, or the wanted cells inside its bounds where those are fewer, as
                // a backdrop face may span millions of cells.
                let spans = (0..3).map(|a| i64::from(high[a]) - i64::from(low[a]) + 1);
                let inside = |c: &[i32; 3]| (0..3).all(|a| (low[a]..=high[a]).contains(&c[a]));
                let keys = if spans.product::<i64>() <= wanted.len() as i64 {
                    let mut keys = Vec::new();
                    for x in low[0]..=high[0] {
                        for y in low[1]..=high[1] {
                            for z in low[2]..=high[2] {
                                if wanted.contains(&[x, y, z]) {
                                    keys.push([x, y, z]);
                                }
                            }
                        }
                    }
                    keys
                } else {
                    wanted.iter().filter(|&c| inside(c)).copied().collect()
                };
                let lightmap = vertices
                    .iter()
                    .map(|v| v.lightmap)
                    .collect::<Option<Vec<_>>>()
                    .map(|uv| [uv[0], uv[1], uv[2]]);
                for key in keys {
                    cells.entry(key).or_default().push(Surface {
                        corners,
                        lightmap,
                        receiver,
                    });
                }
            }
        }
        Self { cells }
    }

    /// The surface triangle whose corners `corners` copy, each within [`COPY_TOLERANCE`], with the
    /// index of the surface corner each copies.
    fn copied(&self, corners: [[f32; 3]; 3]) -> Option<(&Surface, [usize; 3])> {
        let near = |a: [f32; 3], b: [f32; 3]| {
            (0..3).map(|k| (a[k] - b[k]).powi(2)).sum::<f32>() <= COPY_TOLERANCE * COPY_TOLERANCE
        };
        self.cells
            .get(&cell(corners[0]))?
            .iter()
            .find_map(|surface| {
                let order = corners.map(|c| surface.corners.iter().position(|&s| near(c, s)));
                Some((surface, [order[0]?, order[1]?, order[2]?]))
            })
    }

    /// The surface triangle `point` lies on, within [`COPY_TOLERANCE`] of its plane and inside its
    /// edges, with the point's barycentric weights.
    fn under(&self, point: [f32; 3]) -> Option<(&Surface, [f32; 3])> {
        self.cells.get(&cell(point))?.iter().find_map(|surface| {
            let [a, b, c] = surface.corners;
            let normal = face_normal(surface.corners)?;
            let offset = sub(point, a);
            if dot(offset, normal).abs() > COPY_TOLERANCE {
                return None;
            }
            let area = dot(cross(sub(b, a), sub(c, a)), normal);
            let wb = dot(cross(offset, sub(c, a)), normal) / area;
            let wc = dot(cross(sub(b, a), offset), normal) / area;
            let weights = [1. - wb - wc, wb, wc];
            weights
                .iter()
                .all(|&w| w >= -1e-3)
                .then_some((surface, weights))
        })
    }

    /// How each corner of each triangle of `decal` is lit: by the corresponding corner of the
    /// surface triangle the decal triangle copies, else by the surface each corner lies on.
    fn light(&self, decal: &Primitive) -> Vec<[(u32, Lit); 3]> {
        let lit = |surface: &Surface, weights: [f32; 3]| match surface.lightmap {
            Some(uv) => Lit::Coordinates(std::array::from_fn(|k| {
                (0..3).map(|c| uv[c][k] * weights[c]).sum()
            })),
            None => Lit::Receiver(surface.receiver),
        };
        decal
            .indices
            .chunks_exact(3)
            .map(|face| {
                let face = [face[0], face[1], face[2]];
                let corners = face.map(|v| decal.vertices[v as usize].position);
                match self.copied(corners) {
                    Some((surface, order)) => std::array::from_fn(|k| {
                        let mut weights = [0.; 3];
                        weights[order[k]] = 1.;
                        (face[k], lit(surface, weights))
                    }),
                    None => std::array::from_fn(|k| {
                        let under = self.under(corners[k]);
                        (face[k], under.map_or(Lit::Unknown, |(s, w)| lit(s, w)))
                    }),
                }
            })
            .collect()
    }
}

/// Turns each triangle of `decal` to face out of the surface it lies on: the facing of the surface
/// triangle whose corners it copies, else the facing its stored normals agree on. Reach projects a
/// decal onto whole surface triangles, and the cached triangles keep neither their surfaces'
/// winding nor their normals. In Hang 'Em High 8,564 of 9,281 decal triangles copy one level
/// triangle, 4,604 of them wound against it.
fn turn(decal: &mut Primitive, surfaces: &Surfaces) {
    for face in decal.indices.chunks_exact_mut(3) {
        let corners: [[f32; 3]; 3] =
            std::array::from_fn(|k| decal.vertices[face[k] as usize].position);
        let Some(normal) = face_normal(corners) else {
            continue;
        };
        let outward = surfaces
            .copied(corners)
            .and_then(|(surface, _)| face_normal(surface.corners))
            .unwrap_or_else(|| {
                face.iter().fold([0.; 3], |sum, &v| {
                    add(sum, decal.vertices[v as usize].normal)
                })
            });
        if dot(normal, outward) < 0. {
            face.swap(1, 2);
        }
    }
}

/// Lights `decal` as the surfaces it lies on are lit, by `corners` ([`Surfaces::light`]).
/// Reach draws these decals in its pre-lighting pass, into the receiving surface's G-buffer, so
/// they take its light. Translated decals have no lightmap coordinates of their own, and the
/// ground below a primitive's middle (`native::ground_light`) is no wall's light: on Lone Wolf,
/// every decal on a lit wall drew darker than the wall, some black. Each vertex takes its
/// surface's coordinates, split where the triangles that share it lie on different charts. A corner on no lightmapped
/// surface takes those of the nearest corner on one. A decal on none takes the probe of the
/// receiver lit per vertex that it lies on most (its index among `probes`), else none, and so
/// the ground's light.
fn light(
    decal: &mut Primitive,
    corners: &[[(u32, Lit); 3]],
    probes: &[Option<usize>],
) -> Option<usize> {
    let known = corners
        .iter()
        .flatten()
        .filter_map(|&(v, lit)| match lit {
            Lit::Coordinates(uv) => Some((decal.vertices[v as usize].position, uv)),
            _ => None,
        })
        .collect::<Vec<_>>();
    if known.is_empty() {
        let mut counts = BTreeMap::<usize, usize>::new();
        for &(_, lit) in corners.iter().flatten() {
            if let Lit::Receiver(receiver) = lit
                && let Some(Some(probe)) = probes.get(receiver)
            {
                *counts.entry(*probe).or_default() += 1;
            }
        }
        return counts
            .into_iter()
            .max_by_key(|&(probe, count)| (count, std::cmp::Reverse(probe)))
            .map(|(probe, _)| probe);
    }
    let nearest = |at: [f32; 3]| {
        known
            .iter()
            .min_by(|a, b| distance(a.0, at).total_cmp(&distance(b.0, at)))
            .map_or([0.; 2], |&(_, uv)| uv)
    };
    let mut vertices = Vec::with_capacity(decal.vertices.len());
    let mut split = HashMap::<(u32, [u32; 2]), u32>::new();
    let mut indices = Vec::with_capacity(decal.indices.len());
    for &(v, lit) in corners.iter().flatten() {
        let vertex = &decal.vertices[v as usize];
        let uv = match lit {
            Lit::Coordinates(uv) => uv,
            _ => nearest(vertex.position),
        };
        let index = *split.entry((v, uv.map(f32::to_bits))).or_insert_with(|| {
            let mut vertex = vertex.clone();
            vertex.lightmap = Some(uv);
            vertices.push(vertex);
            (vertices.len() - 1) as u32
        });
        indices.push(index);
    }
    decal.vertices = vertices;
    decal.indices = indices;
    None
}

/// Stands the vertices of `decal`, already turned ([`turn`]), off the surface it lies on. A fifth
/// of the stored normals of Hang 'Em High's decal vertices that copy a surface corner point into
/// that surface: a vertex where a decal crosses a crease keeps one face's normal, and some are
/// zero. Stood off along those normals, such vertices sank behind the wall, and the decal showed
/// on some of its triangles only, in dark jagged patches. Each vertex instead stands off along the
/// mean of its triangles' normals, far enough to clear each of their planes by [`STANDOFF`], and
/// takes that mean as its normal, so the decal is lit as its surface is.
fn stand_off(decal: &mut Primitive) -> Result<()> {
    let faces = decal
        .indices
        .chunks_exact(3)
        .filter_map(|face| {
            let corners: [[f32; 3]; 3] =
                std::array::from_fn(|k| decal.vertices[face[k] as usize].position);
            Some(([face[0], face[1], face[2]], face_normal(corners)?))
        })
        .collect::<Vec<_>>();
    // A vertex whose triangles' normals cancel or have no area takes the decal's mean, else its
    // stored normal, else up.
    let whole = unit(faces.iter().fold([0.; 3], |sum, (_, n)| add(sum, *n)));
    let mut sums = vec![[0f32; 3]; decal.vertices.len()];
    for (face, normal) in &faces {
        for &v in face {
            sums[v as usize] = add(sums[v as usize], *normal);
        }
    }
    let means = sums
        .iter()
        .zip(&decal.vertices)
        .map(|(&sum, vertex)| {
            unit(sum)
                .or(whole)
                .or_else(|| unit(vertex.normal))
                .unwrap_or([0., 0., 1.])
        })
        .collect::<Vec<_>>();
    // The least cosine between a vertex's mean and its triangles' normals, so the stand-off
    // clears every plane by STANDOFF, up to twice that at the sharpest creases.
    let mut least = vec![1f32; decal.vertices.len()];
    for (face, normal) in &faces {
        for &v in face {
            least[v as usize] = least[v as usize].min(dot(means[v as usize], *normal));
        }
    }
    for ((vertex, normal), least) in decal.vertices.iter_mut().zip(means).zip(least) {
        let distance = STANDOFF / least.max(0.5);
        vertex.position = std::array::from_fn(|k| vertex.position[k] + normal[k] * distance);
        let tangent = [vertex.tangent[0], vertex.tangent[1], vertex.tangent[2]];
        let tangent = unit(std::array::from_fn(|k| {
            tangent[k] - normal[k] * dot(tangent, normal)
        }))
        .or_else(|| {
            let axis = if normal[2].abs() < 0.9 {
                [0., 0., 1.]
            } else {
                [1., 0., 0.]
            };
            unit(cross(axis, normal))
        })
        .context("A decal vertex has no tangent")?;
        vertex.normal = normal;
        vertex.tangent = [tangent[0], tangent[1], tangent[2], 1.];
    }
    Ok(())
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|k| a[k] + b[k])
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|k| a[k] - b[k])
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    let d = sub(a, b);
    dot(d, d).sqrt()
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3).map(|k| a[k] * b[k]).sum()
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn unit(v: [f32; 3]) -> Option<[f32; 3]> {
    let length = dot(v, v).sqrt();
    (length > 1e-6 && length.is_finite()).then(|| v.map(|c| c / length))
}

/// The unit normal of the triangle `corners` by its winding, or none when it has no area.
fn face_normal([a, b, c]: [[f32; 3]; 3]) -> Option<[f32; 3]> {
    unit(cross(
        std::array::from_fn(|k| b[k] - a[k]),
        std::array::from_fn(|k| c[k] - a[k]),
    ))
}

fn primitive(
    cache: &Cache,
    section: &RawSection,
    property: usize,
    mesh: usize,
    part: usize,
    material: usize,
) -> Result<Primitive> {
    let read = |offset| -> Result<usize> { Ok(usize::try_from(cache.i16(property + offset)?)?) };
    let [index_start, index_count, vertex_start, vertex_count] = [0, 2, 4, 6].map(read);
    let (index_start, index_count, vertex_start, vertex_count): (usize, usize, usize, usize) =
        (index_start?, index_count?, vertex_start?, vertex_count?);
    ensure!(
        index_start + index_count <= section.indices.len()
            && vertex_start + vertex_count <= section.count,
        "Decal lies outside its mesh's buffers"
    );
    let source = &section.indices[index_start..index_start + index_count];
    // Retain the checked source index domains. The range may index the mesh or its own vertices.
    let offset = if source.iter().all(|&i| {
        (vertex_start..vertex_start + vertex_count).contains(&(i as usize)) || i == section.restart
    }) {
        0
    } else {
        ensure!(
            source
                .iter()
                .all(|&i| (i as usize) < vertex_count || i == section.restart),
            "Decal indices leave its vertex range"
        );
        vertex_start as u32
    };
    let triangles = triangles(source, section.topology, section.restart, offset)?;
    let used = triangles.iter().flatten().copied().collect::<BTreeSet<_>>();
    if used.is_empty() {
        return Ok(Primitive {
            section: mesh,
            part,
            material: Some(material),
            flags: 0,
            analytical_light: None,
            instance: Some(format!("decal{part}")),
            bsps: 0,
            vertices: Vec::new(),
            indices: Vec::new(),
        });
    }
    let remap = used
        .iter()
        .enumerate()
        .map(|(local, &vertex)| (vertex, local as u32))
        .collect::<BTreeMap<_, _>>();
    Ok(Primitive {
        section: mesh,
        part,
        material: Some(material),
        flags: 0,
        analytical_light: None,
        instance: Some(format!("decal{part}")),
        bsps: 0,
        vertices: vertices(section, &used)?,
        indices: triangles.iter().flatten().map(|v| remap[v]).collect(),
    })
}

/// The world vertices `used` of `section`, in meters, with their stored normals and tangents,
/// zero where they hold none. [`stand_off`] replaces both and moves the positions off their
/// surface.
fn vertices(section: &RawSection, used: &BTreeSet<u32>) -> Result<Vec<Vertex>> {
    let field = |vertex: u32, at: usize| -> &[u8] {
        let start = vertex as usize * WORLD_STRIDE;
        &section.vertices[start + at..start + WORLD_STRIDE]
    };
    let f32_at =
        |bytes: &[u8], at: usize| f32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    let snorm = |bytes: &[u8], at: usize| {
        (f32::from(i16::from_le_bytes(bytes[at..at + 2].try_into().unwrap())) / 32767.).max(-1.)
    };
    let stored = |bytes: &[u8], at: usize| {
        unit([snorm(bytes, at), snorm(bytes, at + 2), snorm(bytes, at + 4)]).unwrap_or([0.; 3])
    };
    used.iter()
        .map(|&vertex| {
            let bytes = field(vertex, 0);
            let normal = stored(bytes, 0x14);
            let tangent = stored(bytes, 0x1C);
            let at: [f32; 3] = std::array::from_fn(|a| f32_at(bytes, a * 4) * METRES_PER_UNIT);
            let uv = [0x10, 0x12].map(|at| {
                half::f16::from_bits(u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap()))
                    .to_f32()
            });
            ensure!(
                at.iter().chain(&uv).all(|v| v.is_finite()),
                "A decal vertex is not finite"
            );
            Ok(Vertex {
                water_uv: None,
                position: at,
                normal,
                tangent: [tangent[0], tangent[1], tangent[2], 1.],
                uv,
                joints: [0; 4],
                weights: [1., 0., 0., 0.],
                source: vertex,
                lightmap: None,
                lighting: None,
            })
        })
        .collect()
}

/// The triangles of `source`, a triangle list (topology 3) or strip (5) with `restart`, as vertex
/// indices with `offset` added. Decal meshes declare topology 0, and Hang 'Em High's decal index
/// counts include 149, 256 and 388, which no triangle list has, so topology 0 reads as a strip.
fn triangles(source: &[u32], topology: u8, restart: u32, offset: u32) -> Result<Vec<[u32; 3]>> {
    let mut triangles = Vec::new();
    match topology {
        3 => {
            ensure!(
                source.len().is_multiple_of(3),
                "Incomplete decal triangle list"
            );
            triangles.extend(source.chunks_exact(3).map(|f| [f[0], f[1], f[2]]));
        }
        0 | 5 => {
            let mut previous = Vec::new();
            for &vertex in source {
                if vertex == restart {
                    previous.clear();
                    continue;
                }
                if let [.., a, b] = previous[..] {
                    triangles.push(if previous.len() % 2 == 0 {
                        [a, b, vertex]
                    } else {
                        [b, a, vertex]
                    });
                }
                previous.push(vertex);
            }
        }
        n => anyhow::bail!("Unsupported decal topology {n}"),
    }
    triangles.retain(|f| f[0] != f[1] && f[0] != f[2] && f[1] != f[2]);
    Ok(triangles
        .into_iter()
        .map(|f| f.map(|v| v + offset))
        .collect())
}

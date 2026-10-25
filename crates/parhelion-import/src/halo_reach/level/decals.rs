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
use std::collections::{BTreeMap, BTreeSet};

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
/// their normals, so they do not fight its depth. Reach draws them with a depth bias instead. At
/// 5 mm, Breakneck's scorch marks on its tiled walls showed only on some tiles, with tile-shaped
/// holes where the raised tile faces stood in front of them.
const STANDOFF: f32 = 0.03;

/// The decals of the structure BSP `tag` at `at` as primitives, whose materials follow
/// `materials`, to which their decal systems' materials are added. A BSP without decal geometry
/// gives none.
pub(super) fn read(
    cache: &Cache,
    pages: &mut Pages,
    tag: &Tag,
    at: usize,
    materials: &mut Vec<Material>,
) -> Result<Vec<Primitive>> {
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
    Ok(primitives)
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
        vertices: vertices(section, &used, &triangles)?,
        indices: triangles.iter().flatten().map(|v| remap[v]).collect(),
    })
}

/// The world vertices `used` of `section`, in meters and stood off their surface. A vertex
/// without a normal takes the mean of the decal's other normals, or of its triangles' faces when
/// none has one, and a vertex without a tangent takes one perpendicular to its normal.
fn vertices(
    section: &RawSection,
    used: &BTreeSet<u32>,
    triangles: &[[u32; 3]],
) -> Result<Vec<Vertex>> {
    let field = |vertex: u32, at: usize| -> &[u8] {
        let start = vertex as usize * WORLD_STRIDE;
        &section.vertices[start + at..start + WORLD_STRIDE]
    };
    let f32_at =
        |bytes: &[u8], at: usize| f32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    let snorm = |bytes: &[u8], at: usize| {
        (f32::from(i16::from_le_bytes(bytes[at..at + 2].try_into().unwrap())) / 32767.).max(-1.)
    };
    let unit = |v: [f32; 3]| {
        let length = v.iter().map(|c| c * c).sum::<f32>().sqrt();
        (length > 1e-3 && length.is_finite()).then(|| v.map(|c| c / length))
    };
    let position = |vertex: u32| -> [f32; 3] {
        let bytes = field(vertex, 0);
        std::array::from_fn(|a| f32_at(bytes, a * 4) * METRES_PER_UNIT)
    };
    let stored = |vertex: u32| {
        let bytes = field(vertex, 0x14);
        unit([snorm(bytes, 0), snorm(bytes, 2), snorm(bytes, 4)])
    };
    let mean = |normals: &mut dyn Iterator<Item = [f32; 3]>| {
        let sum = normals.fold([0f32; 3], |sum, n| std::array::from_fn(|k| sum[k] + n[k]));
        unit(sum)
    };
    let fallback = mean(&mut used.iter().filter_map(|&v| stored(v)))
        .or_else(|| {
            mean(&mut triangles.iter().filter_map(|f| {
                let [a, b, c] = f.map(position);
                let (u, w) = (
                    [b[0] - a[0], b[1] - a[1], b[2] - a[2]],
                    [c[0] - a[0], c[1] - a[1], c[2] - a[2]],
                );
                unit([
                    u[1] * w[2] - u[2] * w[1],
                    u[2] * w[0] - u[0] * w[2],
                    u[0] * w[1] - u[1] * w[0],
                ])
            }))
        })
        .context("The decal has neither normals nor area")?;
    used.iter()
        .map(|&vertex| {
            let bytes = field(vertex, 0);
            let normal = stored(vertex).unwrap_or(fallback);
            let tangent = unit([snorm(bytes, 0x1C), snorm(bytes, 0x1E), snorm(bytes, 0x20)])
                .or_else(|| {
                    let axis = if normal[2].abs() < 0.9 {
                        [0., 0., 1.]
                    } else {
                        [1., 0., 0.]
                    };
                    unit([
                        axis[1] * normal[2] - axis[2] * normal[1],
                        axis[2] * normal[0] - axis[0] * normal[2],
                        axis[0] * normal[1] - axis[1] * normal[0],
                    ])
                })
                .context("A decal vertex has no tangent")?;
            let at = position(vertex);
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
                position: std::array::from_fn(|k| at[k] + normal[k] * STANDOFF),
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

//! A structure BSP's decorators: the grass, bushes, vines and pebbles Reach scatters over its
//! surfaces. The BSP names its decorator sets (`dctr`) at `+0x274`, 16-byte tag reference rows,
//! and each cluster holds decorator grids at `+0x50`, `0x3C`-byte rows: an `i16` instance count, an
//! `i8` set index, the grid's first instance as a byte offset into its set's buffer at `+0x04`,
//! and the grid's least corner and extent in world units at `+0x08` and `+0x18`, the box of its
//! instances. The BSP's decorator render geometry (`+0x280`, laid out as its other render
//! geometry blocks) has no meshes and one vertex buffer per set, in palette order, of 16-byte
//! instances (declaration `0x00100014`). Hang 'Em High places 3,795 in 433 grids.
//!
//! Reach's compiled `rasterizer\shaders\decorator_mesh` vertex program reads an instance as
//! follows. Its first word holds x, y and z in 11, 11 and 10 bits, a share of the grid's extent
//! from its corner. Bytes 8 to 11 are a quaternion, signed normalized, with w in byte 8 and z, y
//! and x in bytes 9 to 11, which the program does not normalize, so a vertex turns by it and
//! scales by its squared length. Bytes 12 to 15 are the instance's light, red, green and blue
//! unsigned normalized times `2^(63.75 a - 31.75)` of its alpha `a`, in the units of the level's
//! lightmap, irradiance over pi. Byte 4 is the sun's visibility for the analytical light, which
//! these levels do not use. Byte 7 indexes the set's types. The pixel program multiplies the
//! set's texture (`+0x50`) by that light and discards texels whose alpha is below half.
//!
//! A set names its level of detail models from `+0x00` and its types at `+0x40` by string ID,
//! each the name of an instance of those models. The translation draws each instance with the
//! instance of its type in the set's second model, or its first when it has one alone or the
//! second holds a different number of instances ([`DRAWN_DETAIL`]), from the set's texture, alpha
//! tested, lit by a probe of the instance's light, and from both sides. A type without an instance
//! in that model draws nothing.
use super::{
    super::{
        cache::{Cache, Tag},
        material::{Mapping, Material, Property},
        model::{self, Geometry, Primitive},
        resource::Pages,
        rig::METRES_PER_UNIT,
    },
    CLUSTER_SIZE, CLUSTERS, lightmap,
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashSet};

const PALETTE: usize = 0x274;
const PALETTE_SIZE: usize = 0x10;
const GEOMETRY: usize = 0x280;
const GRIDS: usize = 0x50;
const GRID_SIZE: usize = 0x3C;
const GRID_SET: usize = 0x02;
const GRID_START: usize = 0x04;
const GRID_CORNER: usize = 0x08;
const GRID_EXTENT: usize = 0x18;
const INSTANCE_SIZE: usize = 16;
const INSTANCE_DECLARATION: u32 = 0x0010_0014;
/// A set's four level of detail models, 16-byte tag references, its types and its texture.
const SET_MODELS: usize = 0x00;
const SET_MODEL_SIZE: usize = 0x10;
const SET_TYPES: usize = 0x40;
const SET_TEXTURE: usize = 0x50;
/// The level of detail every instance draws with, when the set has it, and otherwise its first.
/// A set fades its first model out within 2 to 8 world units and its second within 4 to 15, so
/// most decorators in view draw with the second or a later one. Hang 'Em High's first models made
/// its 3,795 decorators 1,341,857 triangles, against about 570,000 for the rest of the level. Its
/// grass's second model is a strip of 32 vertices against the first's 230.
const DRAWN_DETAIL: usize = 1;
/// A render model's instances, each naming itself first.
const MODEL_INSTANCES: usize = 0x20;
const MODEL_INSTANCE_SIZE: usize = 60;

/// One decorator instance's place and light.
struct Instance {
    /// Position in meters.
    position: [f32; 3],
    /// The unit rotation and the scale its squared length gives.
    rotation: [f32; 4],
    scale: f32,
    kind: usize,
    light: [f32; 3],
    sun: f32,
}

impl Instance {
    /// Decodes instance `bytes` in a grid whose least corner and extent are `corner` and `extent`,
    /// in world units.
    fn decode(bytes: &[u8], corner: [f32; 3], extent: [f32; 3]) -> Result<Self> {
        ensure!(bytes.len() == INSTANCE_SIZE, "Short decorator instance");
        let word = u32::from_le_bytes(bytes[0..4].try_into()?);
        let share = [
            (word & 0x7FF) as f32 / 2047.,
            ((word >> 11) & 0x7FF) as f32 / 2047.,
            (word >> 22) as f32 / 1023.,
        ];
        let position =
            std::array::from_fn(|k| (corner[k] + share[k] * extent[k]) * METRES_PER_UNIT);
        let snorm = |byte: u8| (f32::from(byte as i8) / 127.).max(-1.);
        // x, y, z, w.
        let quaternion = [
            snorm(bytes[11]),
            snorm(bytes[10]),
            snorm(bytes[9]),
            snorm(bytes[8]),
        ];
        let squared = quaternion.iter().map(|v| v * v).sum::<f32>();
        ensure!(
            squared.is_finite() && squared > 1e-6,
            "Decorator instance has no rotation"
        );
        let length = squared.sqrt();
        let unorm = |byte: u8| f32::from(byte) / 255.;
        let exposure = (63.75 * unorm(bytes[15]) - 31.75).exp2();
        Ok(Self {
            position,
            rotation: quaternion.map(|v| v / length),
            scale: squared,
            kind: usize::from(bytes[7]),
            light: [bytes[12], bytes[13], bytes[14]].map(|byte| unorm(byte) * exposure),
            sun: unorm(bytes[4]),
        })
    }

    /// `point` turned by the instance's rotation.
    fn turn(&self, point: [f32; 3]) -> [f32; 3] {
        let [x, y, z, w] = self.rotation;
        let u = [x, y, z];
        let cross = |a: [f32; 3], b: [f32; 3]| {
            [
                a[1] * b[2] - a[2] * b[1],
                a[2] * b[0] - a[0] * b[2],
                a[0] * b[1] - a[1] * b[0],
            ]
        };
        let t = cross(u, point);
        let t = std::array::from_fn::<f32, 3, _>(|k| 2. * t[k]);
        let s = cross(u, t);
        std::array::from_fn(|k| point[k] + w * t[k] + s[k])
    }

    /// `primitive`, a primitive of the instance's type, at the instance's place.
    fn place(&self, primitive: &Primitive) -> Primitive {
        let mut placed = primitive.clone();
        for vertex in &mut placed.vertices {
            let turned = self.turn(vertex.position);
            vertex.position = std::array::from_fn(|k| self.position[k] + turned[k] * self.scale);
            vertex.normal = self.turn(vertex.normal);
            let [x, y, z, w] = vertex.tangent;
            let [x, y, z] = self.turn([x, y, z]);
            vertex.tangent = [x, y, z, w];
        }
        placed
    }
}

/// `primitive` drawn from both sides: each triangle without a reversed twin gains one.
fn two_sided(mut primitive: Primitive) -> Primitive {
    let key = |corners: [u32; 3], primitive: &Primitive| {
        let points = corners.map(|corner| {
            primitive.vertices[corner as usize]
                .position
                .map(|value| (value * 10_000.).round() as i64)
        });
        // The same triangle from any of its corners, in its winding.
        let first = (0..3).min_by_key(|&k| points[k]).unwrap_or(0);
        [
            points[first],
            points[(first + 1) % 3],
            points[(first + 2) % 3],
        ]
    };
    let faces = primitive
        .indices
        .chunks_exact(3)
        .map(|face| [face[0], face[1], face[2]])
        .collect::<Vec<_>>();
    let wound = faces
        .iter()
        .map(|&face| key(face, &primitive))
        .collect::<HashSet<_>>();
    let mut reversed = Vec::new();
    for [a, b, c] in faces {
        if !wound.contains(&key([a, c, b], &primitive)) {
            reversed.extend([a, c, b]);
        }
    }
    primitive.indices.extend(reversed);
    primitive
}

/// A set's material: its texture times its light, alpha tested at half, as Reach's decorator
/// program draws it.
fn material(set: &Tag, texture: Tag) -> Material {
    Material {
        tag: Some(set.clone()),
        options: BTreeMap::from([
            ("albedo".to_owned(), "default".to_owned()),
            ("alpha_test".to_owned(), "simple".to_owned()),
        ]),
        properties: vec![Property {
            template: set.clone(),
            constants: BTreeMap::from([("albedo_color".to_owned(), [1.; 4])]),
            textures: vec![Mapping {
                role: "base_map".to_owned(),
                bitmap: Some(texture),
                transform: [1., 1., 0., 0.],
                frame: 0,
                address: 0,
                functions: Vec::new(),
            }],
            functions: Vec::new(),
        }],
    }
}

/// The names of render model `tag`'s instances (`+0x20`, 60-byte rows naming each first), in order.
fn instance_names(cache: &Cache, tag: &Tag) -> Result<Vec<String>> {
    cache
        .block(tag.address()? + MODEL_INSTANCES, MODEL_INSTANCE_SIZE)?
        .into_iter()
        .map(|row| cache.string_id(cache.u32(row)?))
        .collect()
}

/// A decorator set's material, by index among the BSP's materials, each of its types'
/// primitives in the type's own space, two sided, none for a type its model lacks, and the path
/// of the model they come from.
type Set = (usize, Vec<Vec<Primitive>>, String);

/// Reads set `tag`, adding its material to `materials`.
fn read_set(
    cache: &Cache,
    pages: &mut Pages,
    tag: &Tag,
    materials: &mut Vec<Material>,
) -> Result<Set> {
    let at = tag.address()?;
    let texture = cache
        .reference(at + SET_TEXTURE, Some("bitm"))?
        .with_context(|| format!("Decorator set {} names no texture", tag.path))?;
    let first = cache
        .reference(at + SET_MODELS, Some("mode"))?
        .with_context(|| format!("Decorator set {} names no model", tag.path))?;
    // Types name the first model's instances. A later model holds the same instances in the same
    // order under names of its own, as Hang 'Em High's grass instance `0x230D` is `0x2311` in its
    // second model. A later model with a different count, as Asylum's `forge_flowers_lod2` holds
    // 2 of `forge_flowers`'s 3, cannot be paired with the types, so that set draws its first.
    let first_names = instance_names(cache, &first)?;
    let mut render = first.clone();
    let mut drawn_names = first_names.clone();
    if let Some(later) = cache.reference(
        at + SET_MODELS + DRAWN_DETAIL * SET_MODEL_SIZE,
        Some("mode"),
    )? {
        let names = instance_names(cache, &later)?;
        if names.len() == first_names.len() {
            render = later;
            drawn_names = names;
        }
    }
    let model = model::read_static(cache, pages, &render, &BTreeMap::new())
        .with_context(|| format!("Decorator set {}", tag.path))?;
    materials.push(material(tag, texture));
    let index = materials.len() - 1;
    let types = cache
        .block(at + SET_TYPES, 4)?
        .into_iter()
        .map(|row| {
            let name = cache.string_id(cache.u32(row)?)?;
            let Some(drawn) = first_names
                .iter()
                .position(|known| *known == name)
                .map(|position| &drawn_names[position])
            else {
                return Ok(Vec::new());
            };
            Ok(model
                .primitives
                .iter()
                .filter(|primitive| primitive.instance.as_deref() == Some(drawn.as_str()))
                .map(|primitive| {
                    let mut primitive = two_sided(primitive.clone());
                    primitive.material = Some(index);
                    primitive
                })
                .collect::<Vec<_>>())
        })
        .collect::<Result<Vec<_>>>()?;
    Ok((index, types, render.path))
}

/// The decorators of the structure BSP `tag` at `at`: each instance's primitives with the probe of
/// its light, whose materials follow `materials`, to which each set's material is added, and a
/// report. A BSP without decorator sets gives none.
#[allow(clippy::type_complexity)]
pub(super) fn read(
    cache: &Cache,
    pages: &mut Pages,
    tag: &Tag,
    at: usize,
    materials: &mut Vec<Material>,
) -> Result<(Vec<(Vec<Primitive>, lightmap::Texel)>, Value)> {
    let palette = cache.block(at + PALETTE, PALETTE_SIZE)?;
    if palette.is_empty() {
        return Ok((Vec::new(), json!({"sets": 0})));
    }
    let geometry = Geometry::read(cache, tag, at + GEOMETRY, true)
        .with_context(|| format!("Decorator geometry of {}", tag.path))?;
    let mut sets = Vec::with_capacity(palette.len());
    let mut buffers = Vec::with_capacity(palette.len());
    for (index, &row) in palette.iter().enumerate() {
        let set = cache
            .reference(row, Some("dctr"))?
            .with_context(|| format!("Decorator set {index} of {} is missing", tag.path))?;
        let (count, declaration, bytes) = geometry.vertex_buffer(cache, pages, index)?;
        ensure!(
            declaration == INSTANCE_DECLARATION && bytes.len() == count * INSTANCE_SIZE,
            "Decorator set {index} of {} holds no instance buffer",
            tag.path
        );
        let loaded = read_set(cache, pages, &set, materials)?;
        sets.push((set, loaded));
        buffers.push(bytes);
    }
    let mut out = Vec::<(Vec<Primitive>, lightmap::Texel)>::new();
    let mut placed = vec![0usize; sets.len()];
    let mut untyped = vec![0usize; sets.len()];
    let mut grids = 0;
    for cluster in cache.block(at + CLUSTERS, CLUSTER_SIZE)? {
        for grid in cache.block(cluster + GRIDS, GRID_SIZE)? {
            grids += 1;
            let count = usize::try_from(cache.i16(grid)?)?;
            let set = usize::try_from(cache.u8(grid + GRID_SET)? as i8)?;
            let start = usize::try_from(cache.u32(grid + GRID_START)?)?;
            let corner = cache.floats::<3>(grid + GRID_CORNER)?;
            let extent = cache.floats::<3>(grid + GRID_EXTENT)?;
            let (_, (_, types, _)) = sets
                .get(set)
                .with_context(|| format!("A decorator grid names set {set} of {}", sets.len()))?;
            let bytes = buffers[set]
                .get(start..start + count * INSTANCE_SIZE)
                .with_context(|| {
                    format!("A decorator grid of set {set} reaches past its buffer")
                })?;
            for row in bytes.chunks_exact(INSTANCE_SIZE) {
                let instance = Instance::decode(row, corner, extent)?;
                let primitives = types.get(instance.kind).filter(|kind| !kind.is_empty());
                let Some(primitives) = primitives else {
                    untyped[set] += 1;
                    continue;
                };
                placed[set] += 1;
                let group = primitives
                    .iter()
                    .map(|primitive| {
                        let mut primitive = instance.place(primitive);
                        primitive.instance = Some(format!("decorator{set}"));
                        primitive
                    })
                    .collect();
                out.push((
                    group,
                    lightmap::Texel::ambient(instance.light, instance.sun),
                ));
            }
        }
    }
    let triangles = out
        .iter()
        .flat_map(|(group, _)| group)
        .map(|primitive| primitive.indices.len() / 3)
        .sum::<usize>();
    let report = json!({
        "sets": sets.iter().zip(placed.iter().zip(&untyped)).map(|((set, (material, types, model)), (placed, untyped))| json!({
            "set": set.path,
            "model": model,
            "material": material,
            "types": types.iter().map(Vec::len).collect::<Vec<_>>(),
            "placed": placed,
            "untyped": untyped,
        })).collect::<Vec<_>>(),
        "grids": grids,
        "placed": placed.iter().sum::<usize>(),
        "untyped": untyped.iter().sum::<usize>(),
        "triangles": triangles,
    });
    Ok((out, report))
}

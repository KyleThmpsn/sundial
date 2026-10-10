//! Read-only Shadowkeep static geometry: the map and prop meshes that sit outside the entity
//! family, either on their own or as a placed group with one transform per instance.
//!
//! Layout reference: alkahest's `crates/alkahest-data/src/statics.rs` on branch `prebl-0.5`
//! (cohaereo/alkahest, GPL-3.0). Every offset used here was re-checked against installed
//! pre-Beyond Light packages; the ignored tests name the tags that pin each result.
//!
//! Statics expand their declared positions with a uniform scale and offset. Vertex formats,
//! stream selection and indices share the checked readers used by entity models.
use super::*;
use std::collections::{BTreeMap, btree_map::Entry};

/// `SStaticMesh`: one static model's techniques, special meshes and decode transform.
const STATIC_MESH: u32 = 0x8080_71A7;
/// `SStaticMeshData`: the opaque mesh groups, parts and buffer sets, named at `STATIC_MESH` +8.
const STATIC_MESH_DATA: u32 = 0x8080_7194;
/// `SStaticMeshInstances`: placed statics with one transform per instance.
const STATIC_INSTANCES: u32 = 0x8080_966D;
/// The technique (material) class a draw names, shared with entity models.
const TECHNIQUE: u32 = 0x8080_71E8;

const MAX_GROUPS: usize = 4096;
const MAX_BUFFER_SETS: usize = 1024;
const MAX_INSTANCES: usize = 8192;
const MAX_STATICS: usize = 1024;

/// Class ids this module can open.
pub(crate) fn is_static(class: u32) -> bool {
    matches!(class, STATIC_MESH | STATIC_INSTANCES)
}

/// Decodes a static mesh (and its instances, if the class is a static instance group) into the
/// shared preview Model.
pub(crate) fn load(manager: &PackageManager, tag: u32) -> Result<Model, String> {
    let entry = manager
        .get_entry(tag)
        .ok_or("The selected resource is missing")?;
    let mut model = Model::default();
    let layouts = vertex::Layouts::read(manager)?;
    match entry.reference {
        STATIC_MESH => {
            let geometry = read_static(manager, tag, &layouts, &mut model)?;
            if !fits(&geometry, &model) {
                return Err("This static exceeds the preview budget.".into());
            }
            place(&geometry, Placement::IDENTITY, &mut model);
            model.tags.push(tag);
        }
        STATIC_INSTANCES => load_instances(manager, tag, &layouts, &mut model)?,
        _ => return Err("This resource is not a static mesh.".into()),
    }
    if model.triangles.is_empty() {
        return Err("This static has no supported triangles.".into());
    }
    Ok(model)
}

/// One static in its own model space, decoded once and then placed as many times as the
/// instance table asks for.
#[derive(Default)]
struct Geometry {
    vertices: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    triangles: Vec<[u32; 3]>,
    /// One entry per triangle, indexing `Model::textures`.
    textures: Vec<Option<usize>>,
}

/// How a static expands its quantised vertices: `SStaticMesh` +0x60 and +0x6C for positions,
/// +0x70 and +0x78 for texture coordinates.
struct Transform {
    offset: [f32; 3],
    scale: f32,
    uv_scale: [f32; 2],
    uv_offset: [f32; 2],
}

impl Transform {
    fn read(bytes: &[u8]) -> Result<Self, String> {
        let value = |at: usize| -> Result<f32, String> {
            let value = f32::from_bits(u32_at(bytes, at)?);
            if value.is_finite() {
                Ok(value)
            } else {
                Err("Invalid static transform".into())
            }
        };
        Ok(Self {
            offset: [value(0x60)?, value(0x64)?, value(0x68)?],
            scale: value(0x6C)?,
            uv_scale: [value(0x70)?, value(0x74)?],
            uv_offset: [value(0x78)?, value(0x7C)?],
        })
    }
}

/// Where one instance of a static sits. The scale is uniform: the three floats at +0x1C read
/// as a per-axis scale everywhere but the second and third were 1.0 in every row observed, and
/// only the uniform reading reproduces the group's own occlusion bounds.
#[derive(Clone, Copy)]
struct Placement {
    rotation: [f32; 4],
    translation: [f32; 3],
    scale: f32,
}

impl Placement {
    const IDENTITY: Self = Self {
        rotation: [0.0, 0.0, 0.0, 1.0],
        translation: [0.0; 3],
        scale: 1.0,
    };

    fn read(bytes: &[u8], row: usize) -> Result<Self, String> {
        let value = |at: usize| -> Result<f32, String> {
            let value = f32::from_bits(u32_at(bytes, at)?);
            if value.is_finite() {
                Ok(value)
            } else {
                Err("Invalid static placement".into())
            }
        };
        Ok(Self {
            rotation: [
                value(row)?,
                value(row + 4)?,
                value(row + 8)?,
                value(row + 0x0C)?,
            ],
            translation: [value(row + 0x10)?, value(row + 0x14)?, value(row + 0x18)?],
            scale: value(row + 0x1C)?,
        })
    }

    fn point(&self, point: [f32; 3]) -> [f32; 3] {
        let turned = self.direction(point.map(|axis| axis * self.scale));
        [
            turned[0] + self.translation[0],
            turned[1] + self.translation[1],
            turned[2] + self.translation[2],
        ]
    }

    /// `v + 2w(q x v) + 2q x (q x v)`, the usual quaternion rotation without a matrix.
    fn direction(&self, vector: [f32; 3]) -> [f32; 3] {
        let [x, y, z, w] = self.rotation;
        let axis = [x, y, z];
        let first = cross(axis, vector).map(|value| value * 2.0);
        let second = cross(axis, first);
        [
            vector[0] + w * first[0] + second[0],
            vector[1] + w * first[1] + second[1],
            vector[2] + w * first[2] + second[2],
        ]
    }
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// One drawn range, from either the opaque mesh groups or the special mesh list.
struct Draw {
    stage: u8,
    layout: u16,
    lod: u8,
    primitive: u16,
    start: u32,
    count: u32,
    /// The index, first vertex and second vertex buffer headers.
    buffers: [u32; 3],
    technique: u32,
}

fn read_static(
    manager: &PackageManager,
    tag: u32,
    layouts: &vertex::Layouts,
    model: &mut Model,
) -> Result<Geometry, String> {
    let bytes = checked(manager, tag, STATIC_MESH)?;
    let transform = Transform::read(&bytes)?;
    let mut draws = opaque_draws(manager, &bytes)?;
    draws.append(&mut special_draws(&bytes)?);
    let (level, lenient) =
        detail_level(&draws).ok_or("This static has no supported detail level")?;
    if level != 0 {
        note(
            model,
            format!("Showing detail level {level}. Higher-detail geometry is absent."),
        );
    }
    draws.retain(|draw| lod_visible(draw.lod, level) && (lenient || stage_drawn(draw.stage)));
    build(manager, &transform, &draws, layouts, model)
}

/// The opaque half of a static. Each group names a part and a render stage, and the technique
/// table is parallel to the group list.
fn opaque_draws(manager: &PackageManager, bytes: &[u8]) -> Result<Vec<Draw>, String> {
    let (technique_count, techniques) = array(bytes, 0x10, 0x8080_0014, 4, MAX_GROUPS)?;
    let data = checked(manager, u32_at(bytes, 8)?, STATIC_MESH_DATA)?;
    let (group_count, groups) = array(&data, 0x08, 0x8080_719B, 8, MAX_GROUPS)?;
    let (part_count, parts) = array(&data, 0x18, 0x8080_719A, 12, MAX_GROUPS)?;
    let (set_count, sets) = array(&data, 0x28, 0x8080_7199, 16, MAX_BUFFER_SETS)?;
    let mut draws = Vec::with_capacity(group_count);
    for group in 0..group_count {
        let group_row = groups + group * 8;
        let part = usize::from(u16_at(&data, group_row)?);
        if part >= part_count {
            return Err("A static mesh group points outside its part table".into());
        }
        let part = parts + part * 12;
        let set = usize::from(byte(&data, part + 8)?);
        if set >= set_count {
            return Err("A static mesh part points outside its buffer table".into());
        }
        let set = sets + set * 16;
        draws.push(Draw {
            stage: byte(&data, group_row + 2)?,
            layout: u16::from(byte(&data, group_row + 4)?),
            lod: byte(&data, part + 10)?,
            primitive: u16::from(byte(&data, part + 11)?),
            start: u32_at(&data, part)?,
            count: u32_at(&data, part + 4)?,
            buffers: [
                u32_at(&data, set)?,
                u32_at(&data, set + 4)?,
                u32_at(&data, set + 8)?,
            ],
            technique: if group < technique_count {
                u32_at(bytes, techniques + group * 4)?
            } else {
                0
            },
        });
    }
    Ok(draws)
}

/// Transparents, light shaft occluders and other stages that carry their own buffers and
/// technique rather than sharing the opaque tables.
fn special_draws(bytes: &[u8]) -> Result<Vec<Draw>, String> {
    let (count, rows) = array(bytes, 0x20, 0x8080_7193, 0x20, MAX_GROUPS)?;
    let mut draws = Vec::with_capacity(count);
    for index in 0..count {
        let row = rows + index * 0x20;
        draws.push(Draw {
            stage: byte(bytes, row)?,
            // Native Shadowkeep stores this as a u16 at +2. The byte at +1 is padding.
            layout: u16_at(bytes, row + 2)?,
            lod: byte(bytes, row + 4)?,
            primitive: u16::from(byte(bytes, row + 6)?),
            start: u32_at(bytes, row + 0x14)?,
            count: u32_at(bytes, row + 0x18)?,
            buffers: [
                u32_at(bytes, row + 8)?,
                u32_at(bytes, row + 0x0C)?,
                u32_at(bytes, row + 0x10)?,
            ],
            technique: u32_at(bytes, row + 0x1C)?,
        });
    }
    Ok(draws)
}

/// The first detail level any drawn part covers, and whether the stage filter had to be
/// relaxed to find one. A static whose every part sits in a skipped stage is better drawn
/// once than not at all, and the duplicate check below keeps that from doubling the shell.
fn detail_level(draws: &[Draw]) -> Option<(u8, bool)> {
    [false, true].into_iter().find_map(|lenient| {
        let level = (0..4).find(|&level| {
            draws
                .iter()
                .any(|draw| lod_visible(draw.lod, level) && (lenient || stage_drawn(draw.stage)))
        })?;
        Some((level, lenient))
    })
}

/// Gbuffer (0), decals (1), investment decals (2) and additive decals (6). Shadow generation
/// (3), the depth prepass (12) and light shaft occlusion (9) repeat the same index ranges, and
/// transparents (7) want blending the preview does not do.
fn stage_drawn(stage: u8) -> bool {
    matches!(stage, 0 | 1 | 2 | 6)
}

fn lod_visible(category: u8, level: u8) -> bool {
    // Bungie's LOD categories are coverage sets (01, 012, ...), not a sorted rank.
    const MASKS: [u8; 11] = [1, 3, 7, 15, 2, 6, 14, 4, 12, 8, 1];
    MASKS
        .get(category as usize)
        .is_some_and(|mask| mask & (1 << level) != 0)
}

/// Buffers and materials already read while decoding one static.
#[derive(Default)]
struct Cache {
    /// Keyed by the vertex buffer pair: the first vertex it added and how many it holds.
    meshes: BTreeMap<([u32; 2], u16), (usize, usize, bool)>,
    /// Keyed by the index buffer: its index width and payload.
    indices: BTreeMap<u32, (usize, Vec<u8>)>,
    materials: BTreeMap<u32, Option<usize>>,
}

fn build(
    manager: &PackageManager,
    transform: &Transform,
    draws: &[Draw],
    layouts: &vertex::Layouts,
    model: &mut Model,
) -> Result<Geometry, String> {
    let mut geometry = Geometry::default();
    let mut cache = Cache::default();
    let mut drawn = BTreeSet::new();
    for draw in draws {
        // Stages that survived the filter still repeat index ranges; draw each range once.
        if draw.count > 0 && drawn.insert((draw.buffers, draw.start, draw.count, draw.primitive)) {
            append_draw(
                manager,
                transform,
                draw,
                layouts,
                &mut cache,
                &mut geometry,
                model,
            )?;
        }
    }
    Ok(geometry)
}

fn append_draw(
    manager: &PackageManager,
    transform: &Transform,
    draw: &Draw,
    layouts: &vertex::Layouts,
    cache: &mut Cache,
    geometry: &mut Geometry,
    model: &mut Model,
) -> Result<(), String> {
    let vertices = [draw.buffers[1], draw.buffers[2]];
    let key = (vertices, draw.layout);
    if let Entry::Vacant(slot) = cache.meshes.entry(key) {
        slot.insert(read_mesh(
            manager,
            vertices,
            draw.layout,
            layouts,
            transform,
            geometry,
            model,
        )?);
    }
    let (base, count, has_uv) = cache.meshes[&key];
    if let Entry::Vacant(slot) = cache.indices.entry(draw.buffers[0]) {
        slot.insert(vertex::indices(manager, draw.buffers[0])?);
    }
    let triangles = {
        let (width, data) = &cache.indices[&draw.buffers[0]];
        super::decode::triangles(
            data,
            *width,
            draw.start as usize,
            draw.count as usize,
            draw.primitive,
            count,
        )?
    };
    if geometry.triangles.len() + triangles.len() > MAX_TRIANGLES {
        return Err("This static exceeds the preview triangle budget.".into());
    }
    let texture = if has_uv {
        technique_texture(manager, draw.technique, cache, model)
    } else {
        None
    };
    geometry
        .textures
        .extend(std::iter::repeat_n(texture, triangles.len()));
    geometry.triangles.extend(
        triangles
            .into_iter()
            .map(|triangle| triangle.map(|vertex| vertex + base as u32)),
    );
    Ok(())
}

/// The color texture a draw's technique binds. `texture::material` owns the preview's
/// binding policy and its texture budget, so a static shares both with entity models.
fn technique_texture(
    manager: &PackageManager,
    tag: u32,
    cache: &mut Cache,
    model: &mut Model,
) -> Option<usize> {
    if let Some(&texture) = cache.materials.get(&tag) {
        return texture;
    }
    let texture = manager
        .get_entry(tag)
        .filter(|entry| entry.reference == TECHNIQUE)
        .and_then(|_| match super::texture::material(manager, tag, model) {
            Ok(index) => Some(index),
            Err(error) => {
                note(model, error);
                None
            }
        });
    cache.materials.insert(tag, texture);
    texture
}

fn read_mesh(
    manager: &PackageManager,
    buffers: [u32; 2],
    layout: u16,
    layouts: &vertex::Layouts,
    transform: &Transform,
    geometry: &mut Geometry,
    model: &mut Model,
) -> Result<(usize, usize, bool), String> {
    let base = geometry.vertices.len();
    let mut decoded = layouts.read_vertices(
        manager,
        layout,
        [buffers[0], buffers[1], 0, 0],
        MAX_VERTICES - base,
    )?;
    decoded.transform(
        [transform.scale; 3],
        transform.offset,
        [
            transform.uv_scale[0],
            transform.uv_scale[1],
            transform.uv_offset[0],
            transform.uv_offset[1],
        ],
    )?;
    let count = decoded.positions.len();
    geometry
        .vertices
        .extend(decoded.positions.into_iter().map(|p| [p[0], p[1], p[2]]));
    geometry.uvs.extend(decoded.uvs);
    geometry.normals.extend(decoded.normals);
    for notice in decoded.notices {
        note(model, notice);
    }
    Ok((base, count, decoded.has_uv))
}

/// The position stride, rejecting anything this module has not been shown to read. Each
/// accepted stride was checked by decoding a whole buffer and reproducing the model-space
/// bounding box centre the static stores at +0x50.
#[cfg(test)]
fn position_stride(header: &[u8], payload: &[u8]) -> Result<usize, String> {
    let stride = usize::from(u16_at(header, 4)?);
    if !matches!(stride, 8 | 12 | 28 | 32)
        || payload.is_empty()
        || !payload.len().is_multiple_of(stride)
        || u32_at(header, 0)? as usize != payload.len()
    {
        return Err("Unsupported static buffer layout".into());
    }
    Ok(stride)
}

fn load_instances(
    manager: &PackageManager,
    tag: u32,
    layouts: &vertex::Layouts,
    model: &mut Model,
) -> Result<(), String> {
    let bytes = checked(manager, tag, STATIC_INSTANCES)?;
    let (transform_count, transforms) = array(&bytes, 0x40, 0x8080_71A3, 0x30, MAX_INSTANCES)?;
    let (static_count, statics) = array(&bytes, 0x58, 0x8080_967D, 4, MAX_STATICS)?;
    let (group_count, groups) = array(&bytes, 0x68, 0x8080_7190, 8, MAX_STATICS)?;
    let mut cache = BTreeMap::new();
    let mut skipped = 0usize;
    for group in 0..group_count {
        let row = groups + group * 8;
        let count = usize::from(u16_at(&bytes, row)?);
        let start = usize::from(u16_at(&bytes, row + 2)?);
        let index = usize::from(u16_at(&bytes, row + 4)?);
        if index >= static_count
            || start
                .checked_add(count)
                .is_none_or(|end| end > transform_count)
        {
            return Err("A static placement points outside its tables".into());
        }
        let child = u32_at(&bytes, statics + index * 4)?;
        let Some(geometry) = instance_geometry(manager, child, layouts, &mut cache, model) else {
            skipped += count;
            continue;
        };
        for instance in start..start + count {
            let placement = Placement::read(&bytes, transforms + instance * 0x30)?;
            if fits(geometry, model) {
                place(geometry, placement, model);
            } else {
                skipped += 1;
            }
        }
    }
    if skipped > 0 {
        note(
            model,
            format!("{skipped} placements are not shown. They exceed the preview budget."),
        );
    }
    Ok(())
}

/// One placed static's geometry, decoded at most once. A static this build cannot read is
/// noted and skipped so one odd prop does not hide the rest of a map cell.
#[allow(clippy::map_entry)]
fn instance_geometry<'a>(
    manager: &PackageManager,
    tag: u32,
    layouts: &vertex::Layouts,
    cache: &'a mut BTreeMap<u32, Option<Geometry>>,
    model: &mut Model,
) -> Option<&'a Geometry> {
    if !cache.contains_key(&tag) {
        // Nothing more will fit, so stop opening statics that could not be drawn anyway.
        if model.vertices.len() >= MAX_VERTICES || model.triangles.len() >= MAX_TRIANGLES {
            return None;
        }
        let textures = model.textures.len();
        let decoded = match read_static(manager, tag, layouts, model) {
            Ok(decoded) => {
                model.tags.push(tag);
                Some(decoded)
            }
            Err(error) => {
                model.textures.truncate(textures);
                note(model, error);
                None
            }
        };
        // The borrow of `model` above rules out the entry API here.
        cache.insert(tag, decoded);
    }
    cache.get(&tag)?.as_ref()
}

fn fits(geometry: &Geometry, model: &Model) -> bool {
    model.vertices.len() + geometry.vertices.len() <= MAX_VERTICES
        && model.triangles.len() + geometry.triangles.len() <= MAX_TRIANGLES
}

/// Appends one placement of a decoded static. Statics carry no gear plate, dye slot or
/// alpha-clip mask, so those per-triangle lanes stay at their neutral values.
fn place(geometry: &Geometry, placement: Placement, model: &mut Model) {
    let base = model.vertices.len() as u32;
    model.vertices.extend(
        geometry
            .vertices
            .iter()
            .map(|&point| placement.point(point)),
    );
    model.normals.extend(
        geometry
            .normals
            .iter()
            .map(|&normal| placement.direction(normal)),
    );
    model.uvs.extend(geometry.uvs.iter().copied());
    // Statics are never skinned; keep the lane the same length as the vertices regardless.
    model.weights.resize_with(model.vertices.len(), || None);
    model.triangles.extend(
        geometry
            .triangles
            .iter()
            .map(|triangle| triangle.map(|vertex| vertex + base)),
    );
    let triangles = geometry.triangles.len();
    model.triangle_textures.extend(&geometry.textures);
    model
        .triangle_dyes
        .extend(std::iter::repeat_n(0, triangles));
    model
        .triangle_clip
        .extend(std::iter::repeat_n(false, triangles));
    model
        .triangle_constant
        .extend(std::iter::repeat_n(None, triangles));
    model
        .triangle_gearstacks
        .extend(std::iter::repeat_n(None, triangles));
    model
        .triangle_normals
        .extend(std::iter::repeat_n(None, triangles));
}

fn note(model: &mut Model, message: String) {
    if !model.notices.contains(&message) {
        model.notices.push(message);
    }
}

fn byte(data: &[u8], at: usize) -> Result<u8, String> {
    Ok(bytes_at::<1>(data, at)?[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draw(stage: u8, lod: u8) -> Draw {
        Draw {
            stage,
            layout: 0,
            lod,
            primitive: 3,
            start: 0,
            count: 3,
            buffers: [1, 2, 3],
            technique: 0,
        }
    }

    #[test]
    fn a_static_with_nothing_but_skipped_stages_still_draws_once() {
        let draws = [draw(7, 1), draw(9, 1)];
        assert_eq!(detail_level(&draws), Some((0, true)));
        // Every part lives at a lower detail category: the preview drops to that level.
        assert_eq!(detail_level(&[draw(0, 7)]), Some((2, false)));
        assert_eq!(detail_level(&[draw(0, 200)]), None);
    }

    #[test]
    fn a_placement_scales_then_rotates_then_translates() {
        let half = std::f32::consts::FRAC_1_SQRT_2;
        let placement = Placement {
            rotation: [0.0, 0.0, half, half],
            translation: [1.0, 2.0, 3.0],
            scale: 2.0,
        };
        // Scaled to twice its length, turned a quarter turn about Z onto +Y, then offset.
        let point = placement.point([1.0, 0.0, 0.0]);
        assert!((point[0] - 1.0).abs() < 1e-5, "{point:?}");
        assert!((point[1] - 4.0).abs() < 1e-5, "{point:?}");
        assert!((point[2] - 3.0).abs() < 1e-5, "{point:?}");
        // Directions turn without picking up the scale or the translation.
        let direction = placement.direction([1.0, 0.0, 0.0]);
        assert!((direction[1] - 1.0).abs() < 1e-5, "{direction:?}");
        assert_eq!(
            Placement::IDENTITY.point([4.0, -5.0, 6.0]),
            [4.0, -5.0, 6.0]
        );
    }

    #[test]
    fn an_unreadable_vertex_layout_is_named_rather_than_guessed() {
        // A header whose stride this module has not been shown to read, and one whose payload
        // length disagrees with the size the header states.
        let header = |stride: u16, size: u32| {
            let mut bytes = [0u8; 12];
            bytes[..4].copy_from_slice(&size.to_le_bytes());
            bytes[4..6].copy_from_slice(&stride.to_le_bytes());
            bytes
        };
        assert_eq!(
            position_stride(&header(20, 40), &[0; 40]),
            Err("Unsupported static buffer layout".into())
        );
        assert_eq!(
            position_stride(&header(8, 40), &[0; 32]),
            Err("Unsupported static buffer layout".into())
        );
        assert_eq!(position_stride(&header(8, 32), &[0; 32]), Ok(8));
    }

    fn manager() -> PackageManager {
        let packages = crate::test_support::preview_packages();
        crate::investment::discovery::open_packages(std::path::Path::new(&packages)).unwrap()
    }

    /// 0x81504044 is a Black Garden prop: six mesh groups over two parts, the same two index
    /// ranges repeated for the gbuffer, shadow and depth prepass stages, and the second part
    /// held at a lower detail category.
    #[test]
    #[ignore = "Requires SUNDIAL_PREVIEW_PACKAGES and installed Shadowkeep packages"]
    fn a_static_draws_its_gbuffer_part_once() {
        let model = load(&manager(), 0x8150_4044).unwrap();
        assert_eq!(model.tags, vec![0x8150_4044]);
        assert_eq!(model.vertices.len(), 1044);
        assert_eq!(model.triangles.len(), 636);
        assert_eq!(model.uvs.len(), model.vertices.len());
        assert_eq!(model.normals.len(), model.vertices.len());
        assert_eq!(model.triangle_textures.len(), model.triangles.len());
        assert_eq!(model.triangle_dyes.len(), model.triangles.len());
        assert!(model.animation.is_none());
        assert!(
            model
                .normals
                .iter()
                .all(|n| (n.iter().map(|v| v * v).sum::<f32>() - 1.0).abs() < 0.01)
        );
        assert!(!model.textures.is_empty());
    }

    /// 0x81504055 places sixteen statics 108 times with its own rotation, uniform scale and
    /// world translation per instance.
    #[test]
    #[ignore = "Requires SUNDIAL_PREVIEW_PACKAGES and installed Shadowkeep packages"]
    fn an_instance_group_places_its_statics_in_world_space() {
        let manager = manager();
        let single = load(&manager, 0x8150_4044).unwrap();
        let placed = load(&manager, 0x8150_4055).unwrap();
        assert!(placed.tags.contains(&0x8150_4044));
        assert!(placed.triangles.len() > single.triangles.len());
        // Every transform in this group sits hundreds of units down the Y axis, while the
        // single static stays within a few units of its own origin.
        assert!(placed.vertices.iter().all(|point| point[1] < -500.0));
        assert!(single.vertices.iter().all(|point| point[1].abs() < 100.0));
    }

    /// 0x81504043 is the mesh data half of 0x81504044. It reads, but only through the static
    /// that owns it, so opening it directly has to say so rather than draw half a model.
    #[test]
    #[ignore = "Requires SUNDIAL_PREVIEW_PACKAGES and installed Shadowkeep packages"]
    fn the_mesh_data_tag_is_left_to_the_static_that_owns_it() {
        let manager = manager();
        let class = manager.get_entry(0x8150_4043_u32).expect("entry").reference;
        assert_eq!(class, STATIC_MESH_DATA);
        assert!(!is_static(class));
        let error = load(&manager, 0x8150_4043).err().expect("refused");
        assert_eq!(error, "This resource is not a static mesh.");
    }
}

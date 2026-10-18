//! A level as native map pieces and world collision. Each piece is an entity copied from a stock
//! map prop that has a render model component and a physical body component. Its render owner
//! draws part of the level's render geometry with a native entity model, and its body holds only
//! a minute stand-in, since the level's collision is the world's: one compressed mesh per part,
//! each placed at its own center by a static compound, as a stock world collision is. Render and
//! collision geometry inside the play area (the spawn points' bounds and `PLAY_MARGIN`) and
//! outside it are each split by halving space at the median until each part fits one model or
//! one mesh, so no piece or instance mixes the arena with distant scenery. Every collision
//! polygon of the structure and of each placed instance becomes a fan of triangles. The
//! scenario's placed objects join the level as both render geometry and collision, and its player
//! spawn points come along, moved like the pieces. Its sky becomes the map's sky objects
//! ([`sky`]), apart from the pieces.
use super::{
    bsps, collision, objects, read_all,
    spawn::{self, Spawn},
    variant,
};
use crate::{
    halo_reach::{cache::Cache, material::Material, native_material, resource::Pages},
    presentation::{
        Graph,
        geometry::{self, Mesh},
        put,
    },
    tiger::{
        payload::Payload,
        physics::{Instance, bare_mesh, compressed_mesh, static_compound},
        reader::Reader,
    },
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::Path,
};

mod atmosphere;
pub mod sky;

const MODEL_CLASS: u32 = 0x8080_73A5;
const MESH_CLASS: u32 = 0x8080_7378;
const ENTITY_CLASS: u32 = 0x8080_9C0F;
const OWNER_CLASS: u32 = 0x8080_9C36;
const RENDER_CLASS: u32 = 0x8080_72BD;
const BODY_CLASS: u32 = 0x8080_8A0C;
/// An entity's components, 12-byte rows that each name an owner.
const ENTITY_COMPONENTS: usize = 0x10;
const COMPONENT_SIZE: usize = 12;
/// An owner's self-relative pointer to its component resource.
const OWNER_RESOURCE: usize = 0x18;
/// The render model in a render resource, and the packfile and body rows in a body resource.
const RENDER_MODEL: usize = 0x1DC;
/// A render resource's culling sphere, center then radius, in the entity's space. A piece keeping
/// the template prop's sphere drew only while that sphere near the origin was in view.
const RENDER_SPHERE: usize = 0xA0;
const BODY_COLLISION: usize = 0x380;
const BODY_ROWS: usize = 0x388;
/// A body row: its saved collision filter and its Havok motion type.
const BODY_ROW_SIZE: usize = 0x90;
const BODY_ROW_CLASS: u32 = 0x8080_8C07;
const BODY_FILTER: usize = 0x18;
const BODY_MOTION: usize = 0x80;
/// The saved filter of fixed stock map props, layer 7, the layer of world collision instances.
const WORLD_FILTER: u32 = 0x000D_0307;
/// Havok's fixed motion type.
const FIXED_MOTION: u32 = 5;
/// Vertices one piece's model holds.
const MAX_VERTICES: usize = 60_000;
/// Collision triangles one collision part holds.
const MAX_COLLISION_TRIANGLES: usize = 20_000;
/// How far, in meters, the play area reaches past the spawn points' bounds on each axis. Pieces
/// placed at their own centers, mixing the arena with scenery kilometers away, sat far from the
/// arena and did not draw. Hang 'Em High's canyon walls and upper structures rise 470 m
/// above its floor: 128 m across and 64 m up left 24,465 triangles within 1 km of the spawns to
/// the scenery, and these margins leave 154.
const PLAY_MARGIN: [f32; 3] = [256., 256., 512.];
/// An entity model's bounding radius, and the two further floats a stock prop sets to it.
const MODEL_RADIUS: usize = 0x8C;
const MODEL_EXTENTS: [usize; 2] = [0x28, 0x2C];
/// A draw record's detail category (`8080737E +0x1B`), the distance bands it draws in. Stock
/// models chain categories by band, such as 0, 4, 7 and 9 or 1 and 7, and a model with one set of
/// records takes 2, which draws in every band: 92 of 600 sampled stock entity models, most of them
/// environment and sandbox props.
const DRAW_DETAIL: usize = 0x1B;
const EVERY_BAND: u8 = 2;
/// Size of the stand-in triangles, in meters.
const STAND_IN: f32 = 0.002;
/// The attribute stream's stride, and its half-float detail coordinates, which carry lightmap
/// coordinates for the level's materials.
const ATTRIBUTE_STRIDE: usize = 24;
const DETAIL_COORDINATES: usize = 20;

/// Native tags the pieces copy: a stock map prop's entity, its render and physical body owners,
/// its model, whose first mesh carries the pieces' geometry, and its Havok packfile, which holds
/// the root and compressed mesh classes. The world collision copies a stock world collision's
/// main packfile, a static compound, and one of its bare compressed mesh packfiles.
pub struct Templates {
    pub entity: u32,
    pub render: u32,
    pub body: u32,
    pub model: u32,
    pub collision: u32,
    pub world_compound: u32,
    pub world_mesh: u32,
    /// The donor sky's tags, which the level's sky objects copy, when the donor places a sky.
    pub sky: Option<sky::Templates>,
    /// The donor's sound point table and the decoder for the level's sounds, when the map brings
    /// the level's ambience.
    pub ambience: Option<super::ambience::Templates>,
    /// The atmosphere volume texture the donor's atmosphere names, which the map's copy recolors.
    pub atmosphere: Option<u32>,
}

/// A user data of a world collision instance whose shape is the packfile at that index of its
/// record's packfile list. Lower values name the record's mesh records instead.
const PACKFILE_SHAPE: u64 = 0x1_0000;

/// One placed piece, by graph symbol, with its world bounds in meters.
#[derive(Clone, Debug)]
pub struct Piece {
    pub entity: String,
    pub minimum: [f32; 3],
    pub maximum: [f32; 3],
    /// Where the piece's entity sits, the middle of the spawn points' bounds, the same for every
    /// piece. Its model's geometry and culling sphere are relative to it.
    pub position: [f32; 3],
}

/// `payload` with every word naming `tag` patched to `symbol`.
fn retarget(payload: &mut [u8], patches: &mut Vec<Value>, tag: u32, symbol: &str) -> Result<usize> {
    let mut count = 0;
    for at in (0..payload.len().saturating_sub(3)).step_by(4) {
        if u32::from_le_bytes(payload[at..at + 4].try_into()?) == tag {
            put(payload, at, &u32::MAX.to_le_bytes())?;
            patches.push(json!({"offset": at, "symbol": symbol}));
            count += 1;
        }
    }
    Ok(count)
}

/// Adds piece `name`'s owners and entity. Every owner the template entity's components name is
/// copied, so the entity names only the piece's own owners and records the client registers at
/// boot. The render owner names `model` and culls by `sphere`, the physical body owner names
/// `collision`, and the entity names each copy wherever the template named its source.
fn add_entity(
    native: &mut Reader,
    graph: &mut Graph,
    templates: &Templates,
    name: &str,
    model: &str,
    sphere: [f32; 4],
    collision: &str,
) -> Result<String> {
    let entity = native.tag(templates.entity, Some(ENTITY_CLASS))?;
    let owners = entity
        .array(ENTITY_COMPONENTS, COMPONENT_SIZE, None)?
        .into_iter()
        .map(|row| entity.u32(row))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        owners.contains(&templates.render) && owners.contains(&templates.body),
        "Template entity does not name its render and physical body owners"
    );
    let mut symbols = Vec::with_capacity(owners.len());
    for (index, &tag) in owners.iter().enumerate() {
        let owner = native.tag(tag, Some(OWNER_CLASS))?;
        let resource = owner.pointer(OWNER_RESOURCE)?;
        let mut payload = owner.0.clone();
        let mut patches = Vec::new();
        let symbol = format!("{name}-owner{index}");
        retarget(&mut payload, &mut patches, tag, &symbol)?;
        let slot = if tag == templates.render {
            ensure!(
                owner.u32(resource - 4)? == RENDER_CLASS,
                "Template render owner {tag:08X} is not a render model"
            );
            for (k, value) in sphere.into_iter().enumerate() {
                put(
                    &mut payload,
                    resource + RENDER_SPHERE + k * 4,
                    &value.to_le_bytes(),
                )?;
            }
            Some((RENDER_MODEL, model))
        } else if tag == templates.body {
            let rows = owner.array(resource + BODY_ROWS, BODY_ROW_SIZE, Some(BODY_ROW_CLASS))?;
            ensure!(
                owner.u32(resource - 4)? == BODY_CLASS && rows.len() == 1,
                "Template body owner {tag:08X} must be a physical body with one body"
            );
            // Level geometry is a fixed body in the world's collision layer, whatever the
            // template prop was.
            for row in rows {
                put(&mut payload, row + BODY_FILTER, &WORLD_FILTER.to_le_bytes())?;
                put(&mut payload, row + BODY_MOTION, &FIXED_MOTION.to_le_bytes())?;
            }
            Some((BODY_COLLISION, collision))
        } else {
            None
        };
        if let Some((slot, target)) = slot {
            put(&mut payload, resource + slot, &u32::MAX.to_le_bytes())?;
            patches.push(json!({"offset": resource + slot, "symbol": target}));
        }
        graph.add(&symbol, tag, &payload, None, patches)?;
        symbols.push(symbol);
    }
    let mut payload = entity.0.clone();
    let mut patches = Vec::new();
    for (&tag, symbol) in owners.iter().zip(&symbols) {
        retarget(&mut payload, &mut patches, tag, symbol)?;
    }
    let symbol = format!("{name}-entity");
    graph.add(&symbol, templates.entity, &payload, None, patches)?;
    Ok(symbol)
}

/// The map's world collision by graph symbol: its static compound and, in user data order, the
/// mesh packfiles its instances name.
pub struct World {
    pub compound: String,
    pub meshes: Vec<String>,
}

/// The pieces' graph, written under the output folder.
pub struct Converted {
    pub nodes: Vec<Value>,
    pub pieces: Vec<Piece>,
    pub spawns: Vec<Spawn>,
    pub world: World,
    /// The level's sky objects, in draw order.
    pub sky: Vec<sky::Object>,
    /// The direction the level's baked light mostly comes from, toward the light with z up,
    /// when it has a lightmap.
    pub light: Option<[f32; 3]>,
    /// The symbol of the sound point table that places the level's ambience, when it has any.
    pub ambience: Option<String>,
    /// The symbol of the atmosphere volume the map's atmosphere names in place of the donor's,
    /// when the donor names one and the level has baked light.
    pub atmosphere: Option<String>,
    pub report: Value,
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|k| a[k] - b[k])
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn length(a: [f32; 3]) -> f32 {
    dot(a, a).sqrt()
}

fn scale(a: [f32; 3], s: f32) -> [f32; 3] {
    a.map(|v| v * s)
}

/// A collision triangle in meters.
type Face = [[f32; 3]; 3];

/// A minute triangle at `at`, all a piece's body holds, since the world collision carries the
/// level's own.
fn stand_in_face(at: [f32; 3]) -> Face {
    [
        at,
        [at[0] + STAND_IN, at[1], at[2]],
        [at[0], at[1] + STAND_IN, at[2]],
    ]
}

fn unit(v: [f32; 3]) -> Option<[f32; 3]> {
    let size = length(v);
    (size.is_finite() && size > 1e-6).then(|| scale(v, 1. / size))
}

/// Any unit vector perpendicular to `n`.
fn perpendicular(n: [f32; 3]) -> [f32; 3] {
    let other = if n[0].abs() < 0.9 {
        [1., 0., 0.]
    } else {
        [0., 1., 0.]
    };
    unit(cross(n, other)).unwrap_or([1., 0., 0.])
}

/// A render triangle: the primitive it comes from and that primitive's three vertices.
struct Triangle {
    primitive: u32,
    corners: [u32; 3],
}

/// Splits `items` at the median of the longest axis of their `center`s until every part
/// satisfies `fits`.
fn split<T>(
    items: Vec<T>,
    center: &dyn Fn(&T) -> [f32; 3],
    fits: &dyn Fn(&[T]) -> bool,
    out: &mut Vec<Vec<T>>,
) {
    if items.len() <= 1 || fits(&items) {
        if !items.is_empty() {
            out.push(items);
        }
        return;
    }
    let (low, high) = bounds(items.iter().map(center));
    let axis = (0..3)
        .max_by(|&a, &b| (high[a] - low[a]).total_cmp(&(high[b] - low[b])))
        .unwrap_or(0);
    let mut items = items;
    items.sort_by(|a, b| center(a)[axis].total_cmp(&center(b)[axis]));
    let upper = items.split_off(items.len() / 2);
    split(items, center, fits, out);
    split(upper, center, fits, out);
}

/// One piece's mesh, from triangles that together use at most `MAX_VERTICES` vertices, and each
/// vertex's lightmap coordinates, `neutral` where the level's lightmap does not cover it.
fn mesh(
    level: &super::Level,
    triangles: &[&Triangle],
    offset: [f32; 3],
    neutral: [f32; 2],
) -> Result<(Mesh, Vec<[f32; 2]>)> {
    let mut mesh = Mesh {
        positions: Vec::new(),
        attributes: Vec::new(),
        groups: Vec::new(),
        weights: Vec::new(),
        bones: 0,
    };
    let mut lightmap = Vec::new();
    let mut index = HashMap::<(u32, u32), u32>::new();
    let mut groups = BTreeMap::<u32, Vec<[u32; 3]>>::new();
    for triangle in triangles {
        let primitive = &level.primitives[triangle.primitive as usize];
        let points = triangle
            .corners
            .map(|c| primitive.vertices[c as usize].position);
        let face = unit(cross(sub(points[1], points[0]), sub(points[2], points[0])))
            .unwrap_or([0., 0., 1.]);
        let mut out = [0; 3];
        for (slot, &corner) in out.iter_mut().zip(&triangle.corners) {
            *slot = match index.get(&(triangle.primitive, corner)) {
                Some(&at) => at,
                None => {
                    let vertex = &primitive.vertices[corner as usize];
                    let normal = unit(vertex.normal).unwrap_or(face);
                    let tangent = unit(vertex.tangent[..3].try_into().unwrap())
                        .unwrap_or_else(|| perpendicular(normal));
                    let at = u32::try_from(mesh.positions.len())?;
                    mesh.positions
                        .push(std::array::from_fn(|k| vertex.position[k] + offset[k]));
                    mesh.attributes.push([
                        vertex.uv[0],
                        vertex.uv[1],
                        normal[0],
                        normal[1],
                        normal[2],
                        1.,
                        tangent[0],
                        tangent[1],
                        tangent[2],
                        if vertex.tangent[3] < 0. { -1. } else { 1. },
                    ]);
                    // Every vertex follows the rigid object's own transform.
                    mesh.weights.push([255, 0, 0, 0, 0, 0, 0, 0]);
                    lightmap.push(vertex.lightmap.unwrap_or(neutral));
                    index.insert((triangle.primitive, corner), at);
                    at
                }
            };
        }
        let key = primitive.material.map_or(u32::MAX, |m| m as u32);
        groups.entry(key).or_default().push(out);
    }
    mesh.groups = groups.into_iter().collect();
    Ok((mesh, lightmap))
}

/// Adds piece `name`'s model: its header and its three buffers, copied from the template's mesh
/// row, with every symbol the encoder names prefixed by the piece.
#[allow(clippy::too_many_arguments)]
fn add_model(
    native: &mut Reader,
    graph: &mut Graph,
    templates: &Templates,
    template: &Payload,
    row: usize,
    stages: &BTreeMap<u32, u8>,
    mesh: &Mesh,
    lightmap: &[[f32; 2]],
    name: &str,
) -> Result<String> {
    let used = mesh
        .groups
        .iter()
        .map(|(key, _)| (*key, stages.get(key).copied().unwrap_or(0)))
        .collect();
    let mut encoded = geometry::encode_with_stages(mesh, template, row, &used)?;
    // The template prop keeps its bounding radius in all four floats at +0x20, as stock props
    // with its flags do. The encoder writes the first two, and the last two would keep the prop's.
    let radius = Payload(encoded.header.clone()).f32(MODEL_RADIUS)?;
    for offset in MODEL_EXTENTS {
        put(&mut encoded.header, offset, &radius.to_le_bytes())?;
    }
    // The encoder's category 0 is the nearest band alone. Pieces past it did not draw, and the
    // arena's upper structures, 300 m from the spawns, were missing.
    let records = (encoded.header.len() - sky::DRAW_RECORDS) / sky::DRAW_RECORD_SIZE;
    for record in 0..records {
        put(
            &mut encoded.header,
            sky::DRAW_RECORDS + record * sky::DRAW_RECORD_SIZE + DRAW_DETAIL,
            &[EVERY_BAND],
        )?;
    }
    ensure!(
        encoded.streams[1].len() == lightmap.len() * ATTRIBUTE_STRIDE,
        "Piece {name} has {} attribute bytes for {} lightmap coordinates",
        encoded.streams[1].len(),
        lightmap.len()
    );
    for (record, uv) in encoded.streams[1]
        .chunks_exact_mut(ATTRIBUTE_STRIDE)
        .zip(lightmap)
    {
        for (k, value) in uv.iter().enumerate() {
            let at = DETAIL_COORDINATES + k * 2;
            record[at..at + 2].copy_from_slice(&half::f16::from_f32(*value).to_le_bytes());
        }
    }
    let mut patches = encoded.patches.clone();
    for (index, kind) in ["positions", "attributes", "indices"]
        .into_iter()
        .enumerate()
    {
        let tag = template.u32(row + [0, 4, 16][index])?;
        let mut header = native.tag(tag, None)?.0.clone();
        put(
            &mut header,
            if index == 2 { 8 } else { 0 },
            &u32::try_from(encoded.streams[index].len())?.to_le_bytes(),
        )?;
        if index == 2 {
            // Weighted meshes index with 32 bits.
            header[1] = 1;
        } else {
            put(
                &mut header,
                4,
                &(if index == 0 { 16u16 } else { 24u16 }).to_le_bytes(),
            )?;
        }
        let symbol = format!("{name}-{kind}-header");
        let data = format!("{name}-{kind}-data");
        graph.add(&symbol, tag, &header, Some(&data), vec![])?;
        graph.add(
            &data,
            native.reference(tag)?,
            &encoded.streams[index],
            Some(&symbol),
            vec![],
        )?;
        for patch in &mut patches {
            if patch["symbol"] == format!("{kind}-header") {
                patch["symbol"] = json!(symbol);
            }
        }
    }
    let symbol = format!("{name}-model");
    graph.add(&symbol, templates.model, &encoded.header, None, patches)?;
    if let Some(node) = graph.nodes.iter_mut().find(|node| node["symbol"] == symbol) {
        node["model"] = json!(true);
    }
    Ok(symbol)
}

fn bounds(points: impl Iterator<Item = [f32; 3]>) -> ([f32; 3], [f32; 3]) {
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

/// Converts the structure BSPs the scenario of `map` in `maps` loads into pieces whose graph is
/// written to `output`, moved by `offset` meters. A map `variant` file places its Forge objects and
/// spawn points in place of the scenario's own placements of every object its palette offers.
/// `packages` holds the Shadowkeep packages the templates come from.
#[allow(clippy::cognitive_complexity)]
pub fn convert(
    maps: &Path,
    map: &str,
    variant: Option<&Path>,
    packages: &Path,
    output: &Path,
    offset: [f32; 3],
    templates: &Templates,
) -> Result<Converted> {
    fs::create_dir_all(output)?;
    let cache = Cache::open(&maps.join(map))?;
    let mut pages = Pages::default();
    let variant = variant
        .map(|path| variant::read(&cache, path))
        .transpose()?;
    let tags = bsps(&cache)?
        .into_iter()
        .map(|bsp| bsp.tag)
        .collect::<Vec<_>>();
    let mut level = read_all(&cache, &mut pages, &tags)?;
    // A surface whose alpha test discards every pixel draws nothing and casts no shadow. The
    // structure's own collision still holds any such surface's wall.
    let hidden = level
        .materials
        .iter()
        .map(|material| material.never_drawn(&cache, &mut pages))
        .collect::<Result<Vec<_>>>()?;
    let drawn_primitives = level.primitives.len();
    level.primitives.retain(|primitive| {
        !primitive
            .material
            .is_some_and(|material| hidden.get(material).copied().unwrap_or(false))
    });
    let hidden_primitives = drawn_primitives - level.primitives.len();
    let hidden_materials = level
        .materials
        .iter()
        .zip(&hidden)
        .filter(|&(_, &never)| never)
        .filter_map(|(material, _)| material.tag.as_ref().map(|tag| tag.path.clone()))
        .collect::<Vec<_>>();
    // Each BSP's collision places that BSP's instances.
    let mut polygons = Vec::new();
    let mut structure_polygons = 0;
    for (index, tag) in tags.iter().enumerate() {
        let collision = collision::read(&cache, &mut pages, tag)?;
        let instanced = level
            .instances
            .iter()
            .filter(|instance| instance.bsp == index)
            .map(|instance| collision.placed(instance))
            .collect::<Result<Vec<_>>>()?;
        structure_polygons += collision.structure.len();
        polygons.extend(collision.structure);
        polygons.extend(instanced.into_iter().flatten());
    }
    let placed = objects::read(&cache, &mut pages, variant.as_ref())?;
    let base = level.materials.len();
    level.materials.extend(placed.materials);
    level
        .primitives
        .extend(placed.primitives.into_iter().map(|mut primitive| {
            primitive.material = primitive.material.map(|material| base + material);
            primitive
        }));
    // The sky draws as the map's sky objects, apart from the pieces, with its materials among the
    // level's.
    let sky_primitives = placed
        .sky
        .into_iter()
        .map(|(mut primitive, colors)| {
            primitive.material = primitive.material.map(|material| base + material);
            (primitive, colors)
        })
        .collect::<Vec<_>>();
    let spawns = spawn::read(&cache, variant.as_ref())?
        .into_iter()
        .map(|spawn| Spawn {
            position: std::array::from_fn(|k| spawn.position[k] + offset[k]),
            ..spawn
        })
        .collect::<Vec<_>>();
    let mut native = Reader::new(packages, &output.join("native"), false)?;
    let template = native.tag(templates.model, Some(MODEL_CLASS))?;
    let row = *template
        .array(16, 136, Some(MESH_CLASS))?
        .first()
        .context("Template model has no mesh")?;
    let havok = native.tag(templates.collision, None)?.0.clone();
    let world_compound = native.tag(templates.world_compound, None)?.0.clone();
    let world_mesh = native.tag(templates.world_mesh, None)?.0.clone();

    let mut triangles = Vec::new();
    for (index, primitive) in level.primitives.iter().enumerate() {
        for face in primitive.indices.chunks_exact(3) {
            triangles.push(Triangle {
                primitive: u32::try_from(index)?,
                corners: [face[0], face[1], face[2]],
            });
        }
    }
    let mut faces = Vec::<Face>::new();
    for polygon in &polygons {
        let points = polygon
            .points
            .iter()
            .map(|p| std::array::from_fn(|k| p[k] + offset[k]))
            .collect::<Vec<[f32; 3]>>();
        for i in 1..points.len().saturating_sub(1) {
            faces.push([points[0], points[i], points[i + 1]]);
        }
    }
    faces.extend(
        placed
            .faces
            .iter()
            .map(|face| face.map(|point| std::array::from_fn(|k| point[k] + offset[k]))),
    );
    let triangle_center = |t: &Triangle| -> [f32; 3] {
        let primitive = &level.primitives[t.primitive as usize];
        let points = t.corners.map(|c| primitive.vertices[c as usize].position);
        std::array::from_fn(|k| (points[0][k] + points[1][k] + points[2][k]) / 3.)
    };
    let (spawn_low, spawn_high) = bounds(spawns.iter().map(|spawn| spawn.position));
    let arena: [f32; 3] = std::array::from_fn(|k| (spawn_low[k] + spawn_high[k]) / 2.);
    let in_play = |point: [f32; 3]| {
        (0..3).all(|k| {
            point[k] >= spawn_low[k] - PLAY_MARGIN[k] && point[k] <= spawn_high[k] + PLAY_MARGIN[k]
        })
    };
    let fits_model = |group: &[Triangle]| {
        // Three corners each is the most a group can use.
        group.len() * 3 <= MAX_VERTICES || {
            let mut seen = std::collections::HashSet::new();
            group.iter().all(|t| {
                t.corners.iter().for_each(|&c| {
                    seen.insert((t.primitive, c));
                });
                seen.len() <= MAX_VERTICES
            })
        }
    };
    let mut renders = Vec::new();
    let (play, scenery): (Vec<_>, Vec<_>) = triangles
        .into_iter()
        .partition(|t| in_play(triangle_center(t)));
    for group in [play, scenery] {
        split(group, &triangle_center, &fits_model, &mut renders);
    }
    let face_center =
        |f: &Face| -> [f32; 3] { std::array::from_fn(|k| (f[0][k] + f[1][k] + f[2][k]) / 3.) };
    let mut shapes = Vec::new();
    let (play, scenery): (Vec<_>, Vec<_>) =
        faces.into_iter().partition(|f| in_play(face_center(f)));
    for group in [play, scenery] {
        split(
            group,
            &face_center,
            &|group: &[Face]| group.len() <= MAX_COLLISION_TRIANGLES,
            &mut shapes,
        );
    }

    let mut used = BTreeMap::<u32, Material>::new();
    for primitive in &level.primitives {
        let key = primitive.material.map_or(u32::MAX, |m| m as u32);
        used.entry(key).or_insert_with(|| {
            primitive
                .material
                .and_then(|m| level.materials.get(m))
                .cloned()
                .unwrap_or(Material {
                    tag: None,
                    options: BTreeMap::new(),
                    properties: Vec::new(),
                })
        });
    }
    ensure!(!used.is_empty(), "The level draws nothing");
    let stages = used
        .iter()
        .map(|(&key, material)| {
            (
                key,
                if material.alpha_mode() == "BLEND" {
                    7
                } else {
                    0
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut graph = Graph {
        root: output.to_path_buf(),
        nodes: Vec::new(),
    };
    let neutral = level
        .lightmap
        .as_ref()
        .map_or([0.; 2], |atlas| atlas.neutral());
    let lightmap = level
        .lightmap
        .as_ref()
        .map(|atlas| {
            native_material::lightmap_textures(&mut native, &mut graph, atlas, sky::veil(&cache)?)
        })
        .transpose()?;
    let material_report = native_material::build(
        &cache,
        &mut pages,
        &mut native,
        &mut graph,
        &used,
        native_material::Binding {
            palette: &[0],
            rigid: true,
            objects: &BTreeMap::new(),
            overrides: &BTreeMap::new(),
            lenses: &BTreeMap::new(),
            paint: &BTreeMap::new(),
            lightmap: lightmap.as_ref(),
        },
    )?;

    let mut pieces = Vec::new();
    let mut world = World {
        compound: "world-collision".into(),
        meshes: Vec::new(),
    };
    let mut instances = Vec::new();
    let mut vertices = 0usize;
    let mut drawn = 0usize;
    let mut placed_faces = 0usize;
    for (part, group) in renders.iter().enumerate() {
        let name = format!("piece{part}");
        let (mut mesh, coordinates) =
            mesh(&level, &group.iter().collect::<Vec<_>>(), offset, neutral)?;
        let (low, high) = bounds(mesh.positions.iter().copied());
        vertices += mesh.positions.len();
        drawn += mesh.groups.iter().map(|(_, f)| f.len()).sum::<usize>();
        // Every piece's entity sits at the arena's middle, and its geometry is relative to it, with
        // a culling sphere around that point reaching its farthest vertex. Pieces placed at their
        // own centers 250 to 370 m from the spawns did not draw, and pillars split between a near
        // piece and a high one lost their tops at the split.
        let radius = mesh
            .positions
            .iter()
            .map(|&position| {
                let d = sub(position, arena);
                (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
            })
            .fold(0f32, f32::max);
        for position in &mut mesh.positions {
            *position = sub(*position, arena);
        }
        let model = add_model(
            &mut native,
            &mut graph,
            templates,
            &template,
            row,
            &stages,
            &mesh,
            &coordinates,
            &name,
        )?;
        let bytes = compressed_mesh(&havok, &[stand_in_face([0.; 3])])?;
        let collision = format!("{name}-collision");
        graph.add(&collision, templates.collision, &bytes, None, Vec::new())?;
        let entity = add_entity(
            &mut native,
            &mut graph,
            templates,
            &name,
            &model,
            [0., 0., 0., radius],
            &collision,
        )?;
        pieces.push(Piece {
            entity,
            minimum: low,
            maximum: high,
            position: arena,
        });
    }
    for (part, group) in shapes.iter().enumerate() {
        let (low, high) = bounds(group.iter().flatten().copied());
        let center: [f32; 3] = std::array::from_fn(|k| (low[k] + high[k]) / 2.);
        let faces = group
            .iter()
            .map(|face| face.map(|point| sub(point, center)))
            .collect::<Vec<Face>>();
        placed_faces += faces.len();
        let (bytes, _) = bare_mesh(&world_mesh, &faces)?;
        let symbol = format!("collision{part}");
        graph.add(&symbol, templates.world_mesh, &bytes, None, Vec::new())?;
        instances.push(Instance {
            translation: center,
            bounds: (low, high),
            user_data: PACKFILE_SHAPE | u64::try_from(part)?,
            triangles: faces,
        });
        world.meshes.push(symbol);
    }
    let (sky, sky_report) = match &templates.sky {
        Some(sky_templates) => sky::add(
            &cache,
            &mut pages,
            &mut native,
            &mut graph,
            sky_templates,
            &level.materials,
            &sky_primitives,
            offset,
            level.lightmap.as_ref().map(|atlas| atlas.level),
        )?,
        None => (
            Vec::new(),
            json!({"objects": 0, "primitives": sky_primitives.len(), "reason": "The donor places no sky objects to copy"}),
        ),
    };
    let light = level.lightmap.as_ref().and_then(sky::baked_light_direction);
    let (ambience, ambience_report) = match &templates.ambience {
        Some(ambience_templates) => super::ambience::add(
            &cache,
            maps,
            map,
            &mut native,
            &mut graph,
            ambience_templates,
            offset,
        )?,
        None => (
            None,
            json!({"points": 0, "reason": "No sound point table or no audio decoder"}),
        ),
    };
    let light_color = level.lightmap.as_ref().and_then(sky::baked_light_color);
    let (atmosphere, atmosphere_report) = match (templates.atmosphere, light_color) {
        (Some(volume), Some(color)) => {
            let (symbol, report) = atmosphere::add(&mut native, &mut graph, volume, color)?;
            (Some(symbol), report)
        }
        _ => (
            None,
            json!({"reason": "The donor names no atmosphere volume or the level has no baked light"}),
        ),
    };
    let document = json!({"nodes": graph.nodes});
    for (symbol, bytes) in crate::tiger::material::bind(&document, output)? {
        let node = graph
            .nodes
            .iter()
            .find(|node| node["symbol"] == symbol)
            .context("Bound shader node")?;
        fs::write(
            output.join(node["file"].as_str().context("Shader payload")?),
            bytes,
        )?;
    }
    ensure!(!pieces.is_empty(), "The level converted to no pieces");
    let compound = static_compound(&world_compound, &world_mesh, &instances)?;
    graph.add(
        &world.compound,
        templates.world_compound,
        &compound,
        None,
        Vec::new(),
    )?;
    let report = json!({
        "bsps": tags.iter().map(|tag| tag.path.as_str()).collect::<Vec<_>>(),
        "pieces": pieces.len(),
        "render_parts": renders.len(),
        "collision_parts": shapes.len(),
        "vertices": vertices,
        "triangles": drawn,
        "structure_polygons": structure_polygons,
        "collision_polygons": polygons.len(),
        "collision_triangles": placed_faces,
        "world_collision_instances": instances.len(),
        "materials": material_report,
        "objects": placed.placed,
        "objects_hidden": placed.hidden,
        "hidden_primitives": hidden_primitives,
        "hidden_materials": hidden_materials,
        "variant": variant.as_ref().map(|variant| json!({
            "title": variant.title,
            "map_id": variant.map_id,
            "objects": variant.placements.len(),
        })),
        "lightmap": level.lightmap.as_ref().map(|atlas| [atlas.width, atlas.height]),
        "lightmap_probes": level.lightmap.as_ref().map(|atlas| atlas.probes),
        "lightmap_averages": level.lightmap.as_ref().map(|atlas| atlas.averages),
        "lighting_averages": level.lighting_averages,
        "decorators": level.decorators,
        "spawns": spawns.len(),
        "offset": offset,
        "sky": sky_report,
        "ambience": ambience_report,
        "atmosphere": atmosphere_report,
        "baked_light_direction": light,
        "baked_light_color": light_color,
    });
    fs::write(
        output.join("map-pieces.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(Converted {
        nodes: graph.nodes,
        pieces,
        spawns,
        world,
        sky,
        light,
        ambience,
        atmosphere,
        report,
    })
}

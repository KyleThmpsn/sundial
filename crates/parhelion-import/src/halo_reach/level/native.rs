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
    collections::{BTreeMap, BTreeSet, HashMap},
    fs,
    path::Path,
};

mod atmosphere;
mod cubemaps;
mod fog;
mod junctions;
mod occlusion;
pub mod sky;
pub use cubemaps::CubemapVolume;

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
/// The widest range of texture coordinates one piece's model holds on either axis, in units or in
/// repeats of base maps that repeat less often ([`uv_tiling`]). A model stores them as 16-bit integers over its own range,
/// so one surface tiled across hundreds of units left every other surface in its model a step of
/// about 0.008: Forge World's cliffs and boulders, whose maps tile 80 and 250 times, drew as a fine
/// weave and stripes. A range of 64 keeps the step near 0.001 of a repeat.
const UV_RANGE: f32 = 64.;
/// The widest span, in units or repeats ([`uv_tiling`]), of a triangle of band 0 ([`uv_band`]).
const BAND_SPAN: f32 = 8.;
/// The longest period, in units or in repeats of a base map that repeats less often ([`uv_tiling`]),
/// whose triangles share pieces held to their span band's range ([`period_class`]).
const TRIANGLE_PERIOD: f32 = 16.;
/// Map roles a surface samples by other coordinates than its texture coordinates: diffuse and
/// specular lookup tables, the direction table, palettes, and directional environment maps.
const LOOKUP_ROLES: [&str; 5] = [
    "g_sample_vmf_diffuse",
    "g_diffuse_power_specular",
    "g_direction_lut",
    "palette",
    "environment_map",
];
/// The step, in meters, every piece's 16-bit positions take, on one grid from the level's origin. A
/// piece stores positions over its own center and range, so two pieces rounded one shared corner to
/// different points: Reflection's pieces spanned a median 767 m, a step of 12 mm, and slivers of sky
/// showed between its stair treads and risers. On one grid a shared corner lands on one point. A
/// piece too wide for this step takes the next power of two that holds it.
const POSITION_STEP: f32 = 1. / 256.;
/// The widest a play-area piece may reach on any axis, in meters, so it keeps [`POSITION_STEP`].
/// A part of fewer than [`WIDE_PIECE_TRIANGLES`] triangles stays whole however wide, so large
/// triangles do not split into a piece each. At 63 m and a 1 mm step, Reflection took 1,074
/// pieces, more entries than one map package's companions can name.
const MAX_PIECE_EXTENT: f32 = 255.;
const WIDE_PIECE_TRIANGLES: usize = 64;
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
/// main packfile, a static compound, and the first of its mesh packfiles, `world_meshes`, that is
/// a bare compressed mesh. The Dead Cliffs' and Distant Shore's first one is, and the first of
/// `pvp_manhattan`'s is not.
pub struct Templates {
    pub entity: u32,
    pub render: u32,
    pub body: u32,
    pub model: u32,
    pub collision: u32,
    pub world_compound: u32,
    pub world_meshes: Vec<u32>,
    /// The donor sky's tags, which the level's sky objects copy, when the donor places a sky.
    pub sky: Option<sky::Templates>,
    /// The donor's sound point table and the decoder for the level's sounds, when the map brings
    /// the level's ambience.
    pub ambience: Option<super::ambience::Templates>,
    /// Source mission recordings to retain beside the map graph. Playback remains untranslated.
    pub mission_audio: Option<super::mission::AudioOptions>,
    /// Explicit script closure to compile for the Sunrise Lua host, without installing it.
    pub mission_execution: Option<super::mission::execution::Options>,
    /// The atmosphere volume texture the donor's atmosphere names, which the map's copy recolors.
    pub atmosphere: Option<u32>,
    /// The distance lookup the donor's atmosphere names, which the map's black copy replaces.
    pub atmosphere_lookup: Option<u32>,
    /// How many occlusion probes the map's copy of the donor's occlusion record takes from the
    /// level, when the donor places one.
    pub occlusion_probes: Option<usize>,
    /// Whether the donor places cubemap volumes, which the map's own stand in for.
    pub cubemaps: bool,
}

/// Whether a stock prop's Havok packfile `bytes` can serve as the pieces' body template: a physics
/// component packfile holding a compressed mesh shape, which each piece's stand-in copies.
/// Distant Shore's first prop's does. Midtown's (`pvp_sabre`) first prop's holds only convex
/// shapes.
pub fn usable_prop_collision(bytes: &[u8]) -> bool {
    crate::tiger::physics::holds(
        bytes,
        "s_physics_component_havok_data",
        "hkpBvCompressedMeshShape",
    )
}

/// A user data of a world collision instance whose shape is the packfile at that index of its
/// record's packfile list. Lower values name the record's mesh records instead.
const PACKFILE_SHAPE: u64 = 0x1_0000;

/// One placed piece, by graph symbol, with its world bounds in meters.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
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
#[derive(serde::Serialize, serde::Deserialize)]
pub struct World {
    pub compound: String,
    pub meshes: Vec<String>,
}

/// The pieces' graph, written under the output folder.
#[derive(serde::Serialize, serde::Deserialize)]
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
    /// The symbol of the black distance lookup the map's atmosphere names in place of the donor's,
    /// when the donor names one and the level's surfaces draw their own fog.
    pub atmosphere_lookup: Option<String>,
    /// The level's occlusion probes, each a position in the map's coordinates and a sky
    /// visibility, as many as `Templates::occlusion_probes` asks for, or none.
    pub occlusion: Vec<[f32; 4]>,
    /// The level's cubemap volumes, when the donor places volumes and the level has baked light.
    pub cubemaps: Vec<CubemapVolume>,
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
    /// The texture coordinate shift its corners take ([`triangle`]), and the least and
    /// greatest of its shifted coordinates.
    shift: [f32; 2],
    uv: [[f32; 2]; 2],
    /// How many times its material's most repeated map repeats per texture coordinate unit, per
    /// axis ([`uv_tiling`]).
    tiling: [f32; 2],
    /// Its period class ([`period_class`]).
    period: u32,
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

/// The texture coordinate shift, per axis, that leaves every map of `material` sampling the same
/// texels: the smallest whole number of units, up to 1,024, that is a whole number of repeats of
/// every wrapping map's tiling and an even number of every mirroring map's. A map that clamps on
/// an axis, or tilings no such number fits, allow no shift on it (zero).
fn uv_period(material: Option<&Material>) -> [f32; 2] {
    // Water combines independently transformed main, auxiliary and detail waves. A period
    // derived only from the texture constants does not preserve all three samples.
    if material.is_some_and(Material::water) {
        return [0.; 2];
    }
    let Some(property) = material.and_then(|material| material.properties.first()) else {
        return [0.; 2];
    };
    std::array::from_fn(|axis| {
        let mut repeats = Vec::new();
        for mapping in &property.textures {
            if mapping.bitmap.is_none() || LOOKUP_ROLES.contains(&mapping.role.as_str()) {
                continue;
            }
            // A shift that is periodic at the stored scale need not be periodic later in an
            // animated scale curve. Translation animation alone preserves the static period.
            if mapping.functions.iter().any(|function| {
                function.kind == 2 || function.kind == if axis == 0 { 3 } else { 4 }
            }) {
                return 0.;
            }
            let mode = if axis == 0 {
                mapping.address & 15
            } else {
                mapping.address >> 4
            };
            let multiple = match mode {
                0 => 1.,
                2 => 2.,
                _ => return 0.,
            };
            repeats.push((mapping.transform[axis].abs(), multiple));
        }
        (1..=1024)
            .map(|units| units as f32)
            .find(|&units| {
                repeats.iter().all(|&(tiling, multiple)| {
                    let count = tiling * units / multiple;
                    (count - count.round()).abs() < 1e-3
                })
            })
            .unwrap_or(0.)
    })
}

/// How many times `material`'s base map, or its most repeated map without one, repeats per texture
/// coordinate unit, per axis, up to once. Texture coordinates quantize over a piece's range, and what
/// streaks is the error in repeats of the map that gives the surface its look, so a map repeating
/// less than once a unit lets its surface's spans and ranges count its repeats. Counted in units,
/// Reflection's distant towers, which repeat once in 200 (`ref_zen_metal_bldg_white_b`), split
/// 203,584 triangles over 501 units into most of 3,880 pieces. Counting the repeats of every map,
/// however often it repeats, split Boardwalk into 719 pieces and Tip of the Spear into 389, past
/// their packages' entries, so maps that repeat more than once a unit count units.
fn uv_tiling(material: Option<&Material>) -> [f32; 2] {
    let Some(property) = material.and_then(|material| material.properties.first()) else {
        return [1.; 2];
    };
    let maps = property
        .textures
        .iter()
        .filter(|mapping| {
            mapping.bitmap.is_some() && !LOOKUP_ROLES.contains(&mapping.role.as_str())
        })
        .collect::<Vec<_>>();
    let base = maps
        .iter()
        .find(|mapping| mapping.role == "base_map")
        .map(|mapping| vec![*mapping])
        .unwrap_or(maps);
    std::array::from_fn(|axis| {
        if base.iter().any(|mapping| {
            mapping.functions.iter().any(|function| {
                function.kind == 2 || function.kind == if axis == 0 { 3 } else { 4 }
            })
        }) {
            return 1.;
        }
        base.iter()
            .map(|mapping| mapping.transform[axis].abs())
            .filter(|tiling| tiling.is_finite() && *tiling > 0.)
            .reduce(f32::max)
            .map_or(1., |tiling| tiling.min(1.))
    })
}

/// A triangle of `primitive`'s with `corners`, its texture coordinates shifted toward zero by the
/// whole number of its material's periods (`period`) nearest its own corners' mean, so its
/// coordinates lie within half a period of zero however far out its surface reaches, and
/// neighbours whose means round alike keep sharing their corners. A map that clamps leaves its axis
/// unshifted. Its period class ([`period_class`]) groups it with triangles whose shifts leave as
/// wide a spread.
fn triangle(
    level: &super::Level,
    primitive: u32,
    corners: [u32; 3],
    period: [f32; 2],
    tiling: [f32; 2],
) -> Triangle {
    let vertices = &level.primitives[primitive as usize].vertices;
    let uvs = corners.map(|corner| vertices[corner as usize].uv);
    let shift = std::array::from_fn(|axis| {
        if period[axis] > 0. {
            let mean = uvs.iter().map(|uv| uv[axis]).sum::<f32>() / 3.;
            period[axis] * (mean / period[axis]).round()
        } else {
            0.
        }
    });
    let low = std::array::from_fn(|axis| {
        uvs.iter().map(|uv| uv[axis]).fold(f32::INFINITY, f32::min) - shift[axis]
    });
    let high = std::array::from_fn(|axis| {
        uvs.iter()
            .map(|uv| uv[axis])
            .fold(f32::NEG_INFINITY, f32::max)
            - shift[axis]
    });
    Triangle {
        primitive,
        corners,
        shift,
        uv: [low, high],
        tiling,
        period: period_class(std::array::from_fn(|axis| period[axis] * tiling[axis])),
    }
}

/// The class of a triangle whose shifts leave its coordinates within half of `period` (counted as
/// [`uv_tiling`] gives) of zero, per axis: 0 up to [`TRIANGLE_PERIOD`], then one more for each
/// doubling. Its pieces hold a range of twice the class's longest period ([`period_range`]).
fn period_class(period: [f32; 2]) -> u32 {
    let longest = period[0].max(period[1]);
    if longest.is_finite() && longest > TRIANGLE_PERIOD {
        (longest / TRIANGLE_PERIOD).log2().ceil().clamp(1., 20.) as u32
    } else {
        0
    }
}

/// The range a piece of period class `class` and span band `band` holds: [`UV_RANGE`] doubled once
/// per band, or twice the class's longest period when that is wider. Shifted by periods of 100 and
/// 200 units, Boardwalk's tower facades lie anywhere within 100 units of zero, and held to the band's
/// range they split every piece beside them into 1,310.
fn period_range(class: u32, band: u32) -> f32 {
    let band = UV_RANGE * 2f32.powi(band as i32);
    if class == 0 {
        band
    } else {
        band.max(2. * TRIANGLE_PERIOD * 2f32.powi(class as i32))
    }
}

/// The widest span, on either axis and counted as [`uv_tiling`] gives, of `triangle` by itself.
fn triangle_span(triangle: &Triangle) -> f32 {
    (0..2)
        .map(|axis| (triangle.uv[1][axis] - triangle.uv[0][axis]) * triangle.tiling[axis])
        .fold(0f32, f32::max)
}

/// The band of a triangle spanning `span` ([`uv_tiling`]): 0 up to [`BAND_SPAN`], then one
/// more for each doubling. A band's pieces hold a range of [`UV_RANGE`] doubled once per band, so
/// a triangle of a band above 0 spans at least 4,096 of its piece's steps.
fn uv_band(span: f32) -> u32 {
    if span.is_finite() && span > BAND_SPAN {
        1 + (span / BAND_SPAN).log2().floor().clamp(0., 30.) as u32
    } else {
        0
    }
}

/// The class of a triangle whose base map repeats `tiling` times a unit ([`uv_tiling`]): 0 for
/// once or more, then one more for each halving, up to 10.
fn tiling_class(tiling: f32) -> u32 {
    if tiling.is_finite() && tiling > 0. && tiling < 1. {
        (-tiling.log2()).ceil().clamp(0., 10.) as u32
    } else {
        0
    }
}

/// The widest range, on either axis, of `triangles`' shifted texture coordinates, counted as the
/// most repeated map among them gives ([`uv_tiling`]).
fn uv_range(triangles: &[Triangle]) -> f32 {
    let mut low = [f32::INFINITY; 2];
    let mut high = [f32::NEG_INFINITY; 2];
    let mut tiling = [0f32; 2];
    for triangle in triangles {
        for axis in 0..2 {
            low[axis] = low[axis].min(triangle.uv[0][axis]);
            high[axis] = high[axis].max(triangle.uv[1][axis]);
            tiling[axis] = tiling[axis].max(triangle.tiling[axis]);
        }
    }
    (0..2)
        .map(|axis| (high[axis] - low[axis]) * tiling[axis])
        .fold(0., f32::max)
}

/// The horizontal cell size, in meters, of the grid [`ground_light`] bins lightmapped floors in.
const GROUND_CELL: f32 = 4.;
/// The steepest lightmapped face, as its normal's least upward component, that counts as ground.
const GROUND_FACING: f32 = 0.3;
/// How far below a primitive's middle, in meters, its ground may lie.
const GROUND_DEPTH: f32 = 64.;
/// The least first lightmap coordinate a placed object's vertex keeps, so its negation stays below
/// zero: the least normal half float, a quarter of a texel of an atlas 4,096 texels wide.
const PLACED_LEAST: f32 = 1. / 16_384.;

/// The lightmap coordinates each primitive without lightmap coordinates of its own takes: those of
/// the lightmapped ground straight below its middle, where a downward ray from there first meets an
/// upward-facing lightmapped face, as Halo lights an object from the lightmap beneath it. Placed
/// scenery such as crates otherwise took the atlas's neutral texel, the level's average ambient
/// with no lobe, and on Breakneck, whose sun comes from its lobes, drew several times darker than
/// the floor they stand on. Primitives with no ground within [`GROUND_DEPTH`] keep the neutral texel.
fn ground_light(level: &super::Level) -> HashMap<u32, [f32; 2]> {
    let cell = |x: f32, y: f32| {
        (
            (x / GROUND_CELL).floor() as i32,
            (y / GROUND_CELL).floor() as i32,
        )
    };
    let mut grid = HashMap::<(i32, i32), Vec<(usize, usize)>>::new();
    for (index, primitive) in level.primitives.iter().enumerate() {
        if primitive.vertices.iter().any(|v| v.lightmap.is_none()) {
            continue;
        }
        for (face, corners) in primitive.indices.chunks_exact(3).enumerate() {
            let points = [0, 1, 2].map(|k| primitive.vertices[corners[k] as usize].position);
            let Some(normal) = unit(cross(sub(points[1], points[0]), sub(points[2], points[0])))
            else {
                continue;
            };
            if normal[2] < GROUND_FACING {
                continue;
            }
            let low = cell(
                points.iter().map(|p| p[0]).fold(f32::MAX, f32::min),
                points.iter().map(|p| p[1]).fold(f32::MAX, f32::min),
            );
            let high = cell(
                points.iter().map(|p| p[0]).fold(f32::MIN, f32::max),
                points.iter().map(|p| p[1]).fold(f32::MIN, f32::max),
            );
            for x in low.0..=high.0 {
                for y in low.1..=high.1 {
                    grid.entry((x, y)).or_default().push((index, face));
                }
            }
        }
    }
    let mut ground = HashMap::new();
    for (index, primitive) in level.primitives.iter().enumerate() {
        if primitive.vertices.is_empty() || primitive.vertices.iter().all(|v| v.lightmap.is_some())
        {
            continue;
        }
        let (low, high) = bounds(primitive.vertices.iter().map(|v| v.position));
        let middle: [f32; 3] = std::array::from_fn(|k| (low[k] + high[k]) / 2.);
        let mut best: Option<(f32, [f32; 2])> = None;
        for &(owner, face) in grid.get(&cell(middle[0], middle[1])).into_iter().flatten() {
            let lit = &level.primitives[owner];
            let corners = &lit.indices[face * 3..face * 3 + 3];
            let vertices = [0, 1, 2].map(|k| &lit.vertices[corners[k] as usize]);
            let [a, b, c] = vertices.map(|v| v.position);
            // Barycentric coordinates of the middle in the face's horizontal projection.
            let area = (b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1]);
            if area.abs() < 1e-8 {
                continue;
            }
            let wb =
                ((middle[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (middle[1] - a[1])) / area;
            let wc =
                ((b[0] - a[0]) * (middle[1] - a[1]) - (middle[0] - a[0]) * (b[1] - a[1])) / area;
            let wa = 1. - wb - wc;
            if wa < 0. || wb < 0. || wc < 0. {
                continue;
            }
            let z = wa * a[2] + wb * b[2] + wc * c[2];
            if z > middle[2]
                || middle[2] - z > GROUND_DEPTH
                || best.is_some_and(|(top, _)| z <= top)
            {
                continue;
            }
            let uv =
                [vertices[0], vertices[1], vertices[2]].map(|v| v.lightmap.unwrap_or_default());
            best = Some((
                z,
                [0, 1].map(|k| wa * uv[0][k] + wb * uv[1][k] + wc * uv[2][k]),
            ));
        }
        if let Some((_, uv)) = best {
            ground.insert(u32::try_from(index).unwrap_or(u32::MAX), uv);
        }
    }
    ground
}

fn lightmap_coordinates(
    vertex: &super::super::model::Vertex,
    primitive: u32,
    ground: &HashMap<u32, [f32; 2]>,
    neutral: [f32; 2],
    placed: u32,
) -> [f32; 2] {
    let mut coordinates = vertex
        .lightmap
        .or_else(|| ground.get(&primitive).copied())
        .unwrap_or(neutral);
    if primitive >= placed {
        coordinates[0] = -coordinates[0].max(PLACED_LEAST);
    }
    coordinates
}

/// One piece's mesh, from triangles that together use at most `MAX_VERTICES` vertices, and each
/// vertex's lightmap coordinates: its own, else its primitive's `ground` coordinates
/// ([`ground_light`]), else `neutral`. A placed object's primitives, those from `placed` on, take
/// their coordinates with the first negated, which marks them for the surfaces' lighting
/// (`native_material::hybrid_lighting`) and keeps the half float's precision. Each
/// triangle's texture coordinates move by whole periods of its maps ([`triangle`]) toward zero, so
/// the model's 16-bit coordinates keep their precision.
fn mesh(
    level: &super::Level,
    triangles: &[&Triangle],
    offset: [f32; 3],
    ground: &HashMap<u32, [f32; 2]>,
    neutral: [f32; 2],
    placed: u32,
    vertex_data: Option<&native_material::vertex_data::Table>,
) -> Result<(Mesh, Vec<[f32; 2]>)> {
    let mut mesh = Mesh {
        positions: Vec::new(),
        attributes: Vec::new(),
        groups: Vec::new(),
        weights: Vec::new(),
        bones: 0,
    };
    let mut lightmap = Vec::new();
    // A corner is shared by the triangles of its primitive that shift its coordinates alike.
    let mut index = HashMap::<(u32, u32, [u32; 2]), u32>::new();
    let mut groups = BTreeMap::<u32, Vec<[u32; 3]>>::new();
    for triangle in triangles {
        let primitive = &level.primitives[triangle.primitive as usize];
        let points = triangle
            .corners
            .map(|c| primitive.vertices[c as usize].position);
        let face = unit(cross(sub(points[1], points[0]), sub(points[2], points[0])))
            .unwrap_or([0., 0., 1.]);
        let shift = triangle.shift;
        let shift_key = shift.map(f32::to_bits);
        let mut out = [0; 3];
        for (slot, &corner) in out.iter_mut().zip(&triangle.corners) {
            *slot = match index.get(&(triangle.primitive, corner, shift_key)) {
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
                        vertex.uv[0] - shift[0],
                        vertex.uv[1] - shift[1],
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
                    let mut coordinates =
                        lightmap_coordinates(vertex, triangle.primitive, ground, neutral, placed);
                    if let Some(value) = vertex_data
                        .and_then(|table| table.coordinates.get(&(triangle.primitive, corner)))
                    {
                        coordinates = *value;
                    }
                    lightmap.push(coordinates);
                    index.insert((triangle.primitive, corner, shift_key), at);
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
/// row, with every symbol the encoder names prefixed by the piece. The materials `shadowless`
/// names cast no shadow.
#[allow(clippy::too_many_arguments)]
fn add_model(
    native: &mut Reader,
    graph: &mut Graph,
    templates: &Templates,
    template: &Payload,
    row: usize,
    stages: &BTreeMap<u32, u8>,
    shadowless: &BTreeSet<u32>,
    mesh: &Mesh,
    lightmap: &[[f32; 2]],
    name: &str,
) -> Result<String> {
    let used = mesh
        .groups
        .iter()
        .map(|(key, _)| (*key, stages.get(key).copied().unwrap_or(0)))
        .collect();
    let mut encoded =
        geometry::encode_on_grid(mesh, template, row, &used, shadowless, POSITION_STEP)?;
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

/// Converts the structure BSPs the scenario of `map` in `maps` loads in its zone sets `zone_sets`,
/// together, into pieces whose graph is written to `output`, moved by `offset` meters. Players
/// start where the first zone set's starting locations stand. A map `variant` file places its Forge
/// objects and spawn points in place of the scenario's own placements of every object its palette
/// offers. `packages` holds the Shadowkeep packages the templates come from.
#[allow(clippy::cognitive_complexity, clippy::too_many_arguments)]
pub fn convert(
    maps: &Path,
    map: &str,
    zone_sets: &[usize],
    variant: Option<&Path>,
    packages: &Path,
    output: &Path,
    offset: [f32; 3],
    templates: &Templates,
) -> Result<Converted> {
    convert_with_progress(
        maps,
        map,
        zone_sets,
        variant,
        packages,
        output,
        offset,
        templates,
        &mut |_| {},
    )
}

/// Converts a level while reporting real stages and completed-piece counts to its caller.
#[allow(clippy::cognitive_complexity, clippy::too_many_arguments)]
pub fn convert_with_progress(
    maps: &Path,
    map: &str,
    zone_sets: &[usize],
    variant: Option<&Path>,
    packages: &Path,
    output: &Path,
    offset: [f32; 3],
    templates: &Templates,
    report: &mut dyn FnMut(&str),
) -> Result<Converted> {
    let &start_set = zone_sets.first().context("The map loads no zone set")?;
    ensure!(
        !output.join("controller.lua").exists() && !output.join("execution.json").exists(),
        "Use a fresh map output when an earlier mission controller is present"
    );
    fs::create_dir_all(output)?;
    let mut pages = Pages::default();
    let source = maps.join(map).canonicalize()?;
    pages.record(&source)?;
    if let Some(path) = variant {
        pages.record(&path.canonicalize()?)?;
    }
    report("Opening Source Level and Mission Scripts");
    let cache = Cache::open(&source)?;
    let mission = super::mission::from_cache(&cache)?;
    // Validate the selected script closure before doing the expensive geometry conversion.
    let controller = templates
        .mission_execution
        .as_ref()
        .map(|options| super::mission::execution::compile(&mission, options))
        .transpose()?;
    let variant = variant
        .map(|path| variant::read(&cache, path))
        .transpose()?;
    let tags = bsps(&cache, zone_sets)?
        .into_iter()
        .map(|bsp| bsp.tag)
        .collect::<Vec<_>>();
    report(&format!(
        "Reading Geometry and Lightmaps / {} BSPs",
        tags.len()
    ));
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
    // A seam joining two loaded BSPs is open ([`collision`]).
    let open_seams = collision::shared_seams(&cache, &tags)?;
    let mut open_seam_surfaces = 0;
    for (index, tag) in tags.iter().enumerate() {
        report(&format!(
            "Reading Collision / BSP {}/{} / {}",
            index + 1,
            tags.len(),
            tag.path
        ));
        let collision = collision::read(&cache, &mut pages, tag, &open_seams)?;
        open_seam_surfaces += collision.open_seam_surfaces;
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
    let placed = objects::read(&cache, &mut pages, variant.as_ref(), zone_sets)?;
    // Placed objects' primitives follow the structure's, and the T-junction repair below only adds
    // vertices, so they keep these indices.
    let placed_from = u32::try_from(level.primitives.len())?;
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
    // Reach draws a sky's opaque surfaces with depth before its blended layers, and the native sky
    // stage draws in order without it. So opaque surfaces come first, the farthest first, and the
    // blended layers follow in Reach's order. An opaque surface lit only by vertex light that is
    // zero everywhere, with no self illumination, can only paint black. Where another opaque
    // surface remains as the backdrop it is left out: Forge World's `simple\white` surface sits
    // 8.5 km out, nearer than its 25 km blue dome, and drew a black patch over the dome. A sky with
    // no other opaque surface, as Sword Base's, keeps it as its backdrop.
    let keyed = placed
        .sky
        .into_iter()
        .map(|(mut primitive, colors)| {
            primitive.material = primitive.material.map(|material| base + material);
            let material = primitive
                .material
                .and_then(|material| level.materials.get(material));
            let opaque = material.is_some_and(|material| material.alpha_mode() == "OPAQUE");
            let unlit = colors
                .iter()
                .all(|color| color.iter().all(|&channel| channel <= 1e-4));
            let glows = material.is_some_and(|material| {
                material
                    .options
                    .get("self_illumination")
                    .is_some_and(|option| !matches!(option.as_str(), "off" | "none"))
            });
            let distance = primitive
                .vertices
                .iter()
                .map(|vertex| {
                    let p = vertex.position;
                    f64::from((p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt())
                })
                .sum::<f64>()
                / primitive.vertices.len().max(1) as f64;
            (
                opaque,
                opaque && unlit && !glows,
                distance,
                primitive,
                colors,
            )
        })
        .collect::<Vec<_>>();
    let backdrop = keyed.iter().any(|(opaque, black, ..)| *opaque && !*black);
    let mut blackened = Vec::new();
    let mut keyed = keyed
        .into_iter()
        .filter_map(|(opaque, black, distance, primitive, colors)| {
            if black && backdrop {
                blackened.push(
                    primitive
                        .material
                        .and_then(|material| level.materials.get(material))
                        .and_then(|material| material.tag.as_ref())
                        .map(|tag| tag.path.clone()),
                );
                return None;
            }
            Some((opaque, distance, primitive, colors))
        })
        .collect::<Vec<_>>();
    keyed.sort_by(|a, b| {
        b.0.cmp(&a.0).then_with(|| {
            if a.0 {
                b.1.total_cmp(&a.1)
            } else {
                std::cmp::Ordering::Equal
            }
        })
    });
    let sky_order = keyed
        .iter()
        .map(|(opaque, distance, primitive, _)| {
            json!({
                "material": primitive
                    .material
                    .and_then(|material| level.materials.get(material))
                    .and_then(|material| material.tag.as_ref())
                    .map(|tag| tag.path.as_str()),
                "opaque": opaque,
                "mean_distance": distance,
            })
        })
        .collect::<Vec<_>>();
    let sky_primitives = keyed
        .into_iter()
        .map(|(_, _, primitive, colors)| (primitive, colors))
        .collect::<Vec<_>>();
    let spawns = spawn::read(&cache, variant.as_ref(), start_set)?
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
    let mut world_mesh_template = None;
    for &tag in &templates.world_meshes {
        let bytes = native.tag(tag, None)?.0.clone();
        if crate::tiger::physics::rooted(&bytes, "hkpBvCompressedMeshShape") {
            world_mesh_template = Some((tag, bytes));
            break;
        }
    }
    let (world_mesh_tag, world_mesh) = world_mesh_template
        .context("The donor's world collision has no bare compressed mesh packfile")?;

    // A triangle with another surface's corner on its edge is cut there, so the corner rounds to
    // one point in both.
    let t_junctions = junctions::split(&mut level.primitives);
    let periods = level
        .primitives
        .iter()
        .map(|primitive| uv_period(primitive.material.and_then(|m| level.materials.get(m))))
        .collect::<Vec<_>>();
    let tilings = level
        .primitives
        .iter()
        .map(|primitive| uv_tiling(primitive.material.and_then(|m| level.materials.get(m))))
        .collect::<Vec<_>>();
    let mut triangles = Vec::new();
    for (index, primitive) in level.primitives.iter().enumerate() {
        for face in primitive.indices.chunks_exact(3) {
            triangles.push(triangle(
                &level,
                u32::try_from(index)?,
                [face[0], face[1], face[2]],
                periods[index],
                tilings[index],
            ));
        }
    }
    // Each material's period, triangles, the range its shifted texture coordinates cover and its
    // widest triangle's span, for the report.
    report(&format!(
        "Partitioning Geometry / {} Triangles",
        triangles.len()
    ));
    let mut uv_materials = BTreeMap::<u32, (Value, usize, [f32; 2], [f32; 2], f32)>::new();
    for t in &triangles {
        let primitive = &level.primitives[t.primitive as usize];
        let key = primitive.material.map_or(u32::MAX, |m| m as u32);
        let entry = uv_materials.entry(key).or_insert_with(|| {
            (
                json!(periods[t.primitive as usize]),
                0,
                [f32::INFINITY; 2],
                [f32::NEG_INFINITY; 2],
                0.,
            )
        });
        entry.1 += 1;
        for axis in 0..2 {
            entry.2[axis] = entry.2[axis].min(t.uv[0][axis]);
            entry.3[axis] = entry.3[axis].max(t.uv[1][axis]);
        }
        entry.4 = entry.4.max(triangle_span(t));
    }
    let uv_report = uv_materials
        .iter()
        .map(|(&key, (period, triangles, low, high, widest))| {
            json!({
                "material": level
                    .materials
                    .get(key as usize)
                    .and_then(|material| material.tag.as_ref())
                    .map(|tag| tag.path.clone()),
                "period": period,
                "triangles": triangles,
                "range": [high[0] - low[0], high[1] - low[1]],
                "widest": widest,
            })
        })
        .collect::<Vec<_>>();
    let mut band_report = Vec::new();
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
    // Points in the map's space, moved by `offset` as the spawn points are.
    let in_play = |point: [f32; 3]| {
        (0..3).all(|k| {
            point[k] >= spawn_low[k] - PLAY_MARGIN[k] && point[k] <= spawn_high[k] + PLAY_MARGIN[k]
        })
    };
    // Texture coordinates are 16-bit steps over a piece's range, so a triangle spanning a sliver
    // of that range collapses to one texel and streaks. Each triangle shifts its coordinates by
    // whole periods toward zero (`triangle`), and triangles draw in bands of similar span
    // (`uv_band`), each band's pieces holding a range that every one of its triangles spans
    // thousands of steps of. Shifting and grouping whole surfaces left Breakneck's and Countdown's
    // floors and catwalks streaked: one piece held triangles spanning 2,700 and 160,000 units
    // beside triangles spanning a fraction of one, steps of 0.04 and 2.7. Splitting by span with
    // whole-surface shifts took 871 and 503 pieces, past a package's entries.
    let fits_model = |group: &[Triangle], limit: f32| {
        // Three corners each is the most a group can use.
        let vertices = group.len() * 3 <= MAX_VERTICES || {
            let mut seen = std::collections::HashSet::new();
            group.iter().all(|t| {
                let shift = t.shift.map(f32::to_bits);
                t.corners.iter().for_each(|&c| {
                    seen.insert((t.primitive, c, shift));
                });
                seen.len() <= MAX_VERTICES
            })
        };
        vertices && uv_range(group) <= limit
    };
    // A play-area piece also stays within MAX_PIECE_EXTENT, so its positions keep POSITION_STEP.
    let compact = |group: &[Triangle]| {
        group.len() < WIDE_PIECE_TRIANGLES || {
            let (low, high) = bounds(group.iter().flat_map(|t| {
                let primitive = &level.primitives[t.primitive as usize];
                t.corners.map(|c| primitive.vertices[c as usize].position)
            }));
            (0..3).all(|k| high[k] - low[k] <= MAX_PIECE_EXTENT)
        }
    };
    let mut renders = Vec::new();
    // The level's triangles are in Reach's space and the collision faces already moved. Taking the
    // triangles unmoved put Reflection's arena, moved 735 m down, wholly outside the play area, so
    // its pieces took the scenery's coarser positions and no extent cap.
    let (play, scenery): (Vec<_>, Vec<_>) = triangles.into_iter().partition(|t| {
        let center = triangle_center(t);
        in_play(std::array::from_fn(|k| center[k] + offset[k]))
    });
    // Triangles also group by how often their base maps repeat ([`tiling_class`]). A piece's range
    // counts as its most repeated base map gives, so a surface repeating once in 200 units beside
    // one repeating every unit would hold the other to its own range and split pieces they shared.
    for (region, in_play) in [(play, true), (scenery, false)] {
        let mut bands = BTreeMap::<(u32, u32, [u32; 2]), Vec<Triangle>>::new();
        for triangle in region {
            bands
                .entry((
                    uv_band(triangle_span(&triangle)),
                    triangle.period,
                    triangle.tiling.map(tiling_class),
                ))
                .or_default()
                .push(triangle);
        }
        for ((band, period, class), group) in bands {
            let limit = period_range(period, band);
            // Why groups failed to fit, for the report: too many vertices, too wide a texture
            // coordinate range, or too wide a play-area piece.
            let refused = std::cell::Cell::new([0usize; 3]);
            let fits = |group: &[Triangle]| {
                let model = fits_model(group, limit);
                let near = !in_play || compact(group);
                if !(model && near) {
                    let mut counts = refused.get();
                    if !model && uv_range(group) > limit {
                        counts[1] += 1;
                    } else if !model {
                        counts[0] += 1;
                    } else {
                        counts[2] += 1;
                    }
                    refused.set(counts);
                }
                model && near
            };
            let (before, triangles) = (renders.len(), group.len());
            split(group, &triangle_center, &fits, &mut renders);
            band_report.push(json!({
                "play": in_play,
                "band": band,
                "period": period,
                "tiling": class,
                "triangles": triangles,
                "pieces": renders.len() - before,
                "refused": refused.get(),
            }));
        }
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
    // An alpha-tested surface casts no shadow, as the pieces' shared shadow material draws every
    // triangle solid. Forge World's 113,424 ground cover cards then shadowed the grass they stand
    // in, so sunlit slopes and shores drew in Destiny's shadow beside Reach's lighter baked tree
    // shadows. Reach's lightmaps already hold its foliage's shadows on the structure.
    let shadowless = used
        .iter()
        .filter(|(_, material)| material.alpha_mode() == "MASK")
        .map(|(&key, _)| key)
        .collect::<BTreeSet<_>>();
    let mut graph = Graph {
        root: output.to_path_buf(),
        nodes: Vec::new(),
    };
    let neutral = level
        .lightmap
        .as_ref()
        .map_or([0.; 2], |atlas| atlas.neutral());
    let ground = if level.lightmap.is_some() {
        ground_light(&level)
    } else {
        HashMap::new()
    };
    // Where the lightmap shows the level's sun, Destiny's sun lights the surfaces in its place.
    let sun = level.lightmap.as_ref().and_then(sky::baked_sun);
    // The surfaces draw the level's own fog in place of Destiny's.
    let (fog, fog_report) = fog::read(&cache, &tags, offset[2])?;
    report("Converting Lightmap Textures");
    let lightmap = level
        .lightmap
        .as_ref()
        .map(|atlas| {
            native_material::lightmap_textures(
                &mut native,
                &mut graph,
                atlas,
                sky::veil(&cache)?,
                sun.map(|sun| native_material::Sun {
                    direction: sun.direction,
                    level: sun.level,
                    visibility: sun.visibility,
                }),
                fog,
            )
        })
        .transpose()?;
    let vertex_data = native_material::vertex_data::add(
        &mut native,
        &mut graph,
        &level.primitives,
        level
            .lightmap
            .as_ref()
            .map_or(1., |atlas| atlas.scale * atlas.level),
        |primitive, vertex| lightmap_coordinates(vertex, primitive, &ground, neutral, placed_from),
    )?;
    report(&format!(
        "Converting Materials and Textures / {} Materials",
        used.len()
    ));
    let material_report = native_material::build_with_progress(
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
            vertex_data: vertex_data.as_ref().map(|table| table.symbol.as_str()),
        },
        report,
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
        report(&format!(
            "Writing Render Geometry / Piece {}/{}",
            part + 1,
            renders.len()
        ));
        let name = format!("piece{part}");
        let (mut mesh, coordinates) = mesh(
            &level,
            &group.iter().collect::<Vec<_>>(),
            offset,
            &ground,
            neutral,
            placed_from,
            vertex_data.as_ref(),
        )?;
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
            &shadowless,
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
        report(&format!(
            "Writing Collision / Mesh {}/{}",
            part + 1,
            shapes.len()
        ));
        let (low, high) = bounds(group.iter().flatten().copied());
        let center: [f32; 3] = std::array::from_fn(|k| (low[k] + high[k]) / 2.);
        let faces = group
            .iter()
            .map(|face| face.map(|point| sub(point, center)))
            .collect::<Vec<Face>>();
        placed_faces += faces.len();
        let (bytes, _) = bare_mesh(&world_mesh, &faces)?;
        let symbol = format!("collision{part}");
        graph.add(&symbol, world_mesh_tag, &bytes, None, Vec::new())?;
        instances.push(Instance {
            translation: center,
            bounds: (low, high),
            user_data: PACKFILE_SHAPE | u64::try_from(part)?,
            triangles: faces,
        });
        world.meshes.push(symbol);
    }
    report("Converting Sky Geometry");
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
    // Destiny's sun shines from the baked sun the surfaces leave to it, else from the baked key
    // light.
    let light = sun
        .map(|sun| sun.direction)
        .or_else(|| level.lightmap.as_ref().and_then(sky::baked_light_direction));
    report("Decoding and Converting Ambient Audio");
    let (ambience, ambience_report) = match &templates.ambience {
        Some(ambience_templates) => super::ambience::add(
            &cache,
            &mut pages,
            &tags,
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
    report("Decoding Mission Audio");
    let mission_audio = match &templates.mission_audio {
        Some(options) => {
            super::mission::audio::export(&mission, options, &mut pages, &output.join("audio"))?
        }
        None => json!({"status":"undecoded","reason":"No mission audio decoder configured",
            "playback":"unsupported"}),
    };
    // The atmosphere takes the sun's color where Destiny's sun lights the surfaces in its place.
    let light_color = sun
        .map(|sun| sun.color)
        .or_else(|| level.lightmap.as_ref().and_then(sky::baked_light_color));
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
    // Lightmapped surfaces draw the level's fog, or none where Reach drew none, so Destiny's fog
    // gives way on the map.
    let (atmosphere_lookup, lookup_report) = match (
        templates.atmosphere_lookup,
        level.lightmap.is_some(),
    ) {
        (Some(lookup), true) => {
            let (symbol, report) = atmosphere::clear_lookup(&mut native, &mut graph, lookup)?;
            (Some(symbol), report)
        }
        _ => (
            None,
            json!({"reason": "The donor names no atmosphere lookup or the level has no baked light"}),
        ),
    };
    report("Preparing Lighting and Occlusion Probes");
    let (occlusion, occlusion_report) = match templates.occlusion_probes {
        Some(count) => occlusion::probes(
            &level,
            &spawns
                .iter()
                .map(|spawn| sub(spawn.position, offset))
                .collect::<Vec<_>>(),
            count,
            offset,
        ),
        None => (
            Vec::new(),
            json!({"reason": "The donor places no occlusion probes"}),
        ),
    };
    report("Converting Reflection Cubemaps");
    let (cubemaps, cubemaps_report) = if templates.cubemaps && level.lightmap.is_some() {
        cubemaps::volumes(
            &cache,
            &mut pages,
            &mut native,
            &mut graph,
            &level,
            super::loaded_bsps(&cache, zone_sets)?,
            offset,
        )?
    } else {
        (
            Vec::new(),
            json!({"reason": "The donor places no cubemap volumes or the level has no baked light"}),
        )
    };
    report("Binding Native Shaders");
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
    report("Verifying Source Files and Writing Map Reports");
    pages.verify_inputs()?;
    let mission_bytes = serde_json::to_vec_pretty(&mission)?;
    fs::write(output.join("mission.json"), &mission_bytes)?;
    let mission_execution = match controller {
        Some(controller) => {
            fs::write(output.join("controller.lua"), controller.source)?;
            fs::write(
                output.join("execution.json"),
                serde_json::to_vec_pretty(&controller.report)?,
            )?;
            serde_json::to_value(controller.report)?
        }
        None => json!({"execution":"not_selected"}),
    };
    let mut report = json!({
        "source_build": cache.build,
        "source_inputs": pages.inputs,
        "source_resources": pages.resources,
        "mission": {
            "file": "mission.json",
            "execution": mission.execution,
            "scripts": mission.scripts.len(),
            "globals": mission.globals.len(),
            "active_expressions": mission.nodes.iter().flatten().count(),
            "startup": mission.analysis.startup,
            "continuous": mission.analysis.continuous,
            "operations": mission.analysis.operations,
            "limits": mission.analysis.limits,
            "audio": mission_audio,
            "controller": mission_execution,
        },
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
        "t_junctions": t_junctions,
        "uv_materials": uv_report,
        "uv_bands": band_report,
        "objects_absent": placed.absent,
        "objects_untranslated_motion": placed.motion,
        "hidden_primitives": hidden_primitives,
        "hidden_materials": hidden_materials,
        "variant": variant.as_ref().map(|variant| json!({
            "title": variant.title,
            "map_id": variant.map_id,
            "objects": variant.placements.len(),
        })),
        "lightmap": level.lightmap.as_ref().map(|atlas| [atlas.width, atlas.height]),
        "lightmap_probes": level.lightmap.as_ref().map(|atlas| atlas.probes),
        "vertex_data": vertex_data.as_ref().map(|table| json!({
            "texture":table.symbol, "vertices":table.coordinates.len(),
            "records":table.records, "dimensions":[2048,table.height],
            "method":"Vertex texture loads preserve linear lighting and separate water body coordinates."
        })),
        "water_coordinates": level.primitives.iter().enumerate().filter_map(|(index, primitive)| {
            let material = primitive.material.and_then(|index| level.materials.get(index))?;
            material.water().then(|| json!({
                "primitive":index,
                "material":material.tag.as_ref().map(|tag| tag.path.as_str()),
                "source_coordinates":primitive.vertices.iter().filter(|vertex| vertex.water_uv.is_some()).count(),
                "base_uv_fallbacks":primitive.vertices.iter().filter(|vertex| vertex.water_uv.is_none()).count(),
            }))
        }).collect::<Vec<_>>(),
        "lightmap_averages": level.lightmap.as_ref().map(|atlas| atlas.averages),
        "lighting_averages": level.lighting_averages,
        "decorators": level.decorators,
        "spawns": spawns.len(),
        "offset": offset,
        "sky": sky_report,
        "sky_order": sky_order,
        "sky_blackened": blackened,
        "ambience": ambience_report,
        "atmosphere": atmosphere_report,
        "atmosphere_lookup": lookup_report,
        "fog": fog_report,
        "occlusion": occlusion_report,
        "cubemaps": cubemaps_report,
        "baked_light_direction": light,
        "baked_light_color": light_color,
        "baked_sun": sun,
    });
    report["open_seams"] = json!(open_seams);
    report["open_seam_surfaces"] = json!(open_seam_surfaces);
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
        atmosphere_lookup,
        occlusion,
        cubemaps,
        report,
    })
}

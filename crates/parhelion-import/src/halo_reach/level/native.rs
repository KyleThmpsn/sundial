//! A level as native map pieces and world collision. Each piece is a native static mesh, and one
//! static instance group per area places its pieces as a stock bubble places its static geometry
//! (`statics`), so a piece takes no world object of its own. The level's collision is the
//! world's: one compressed mesh per part, each placed at its own center by a static compound, as
//! a stock world collision is. Objects that move or carry gameplay state stay entities, as native
//! maps keep them. Scripted scenery poses use render entities with their own materials. Render and
//! collision geometry inside the play area (the spawn points' bounds and `PLAY_MARGIN`) and
//! outside it are each split by halving space at the median until each part fits one model or
//! one mesh, so no piece or instance mixes the arena with distant scenery. Every collision
//! polygon of the structure and of each placed instance becomes a fan of triangles. The
//! scenario's placed objects join the level as both render geometry and collision, and its player
//! spawn points come along, moved like the pieces. Its sky becomes the map's sky objects
//! ([`sky`]), apart from the pieces.
use super::{
    bsps, collision, objects,
    spawn::{self, Spawn},
    variant,
};
use crate::{
    halo_reach::{cache::Cache, material::Material, native, resource::Pages},
    presentation::{
        Graph,
        geometry::{self, Mesh},
        put,
    },
    tiger::{
        payload::Payload,
        physics::{Instance, bare_mesh, static_compound},
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
pub mod lights;
mod occlusion;
mod props;
pub mod sky;
mod statics;
pub use cubemaps::CubemapVolume;
pub use props::{Prop, Templates as PropTemplates};
pub use statics::Templates as StaticTemplates;

const MODEL_CLASS: u32 = 0x8080_73A5;
const MESH_CLASS: u32 = 0x8080_7378;
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
/// The attribute stream's stride, and its half-float detail coordinates, which carry lightmap
/// coordinates for the level's materials.
const ATTRIBUTE_STRIDE: usize = 24;
const DETAIL_COORDINATES: usize = 20;

/// Native tags the pieces copy: a stock entity model, whose first mesh's buffer headers the
/// pieces' buffers copy, and the donor's static instance group records ([`statics::Templates`]).
/// The world collision copies a stock world collision's main packfile, a static compound, and the
/// first of its mesh packfiles, `world_meshes`, that is a bare compressed mesh. The Dead Cliffs'
/// and Distant Shore's first one is, and the first of `pvp_manhattan`'s is not.
#[derive(serde::Serialize)]
pub struct Templates {
    pub model: u32,
    pub props: PropTemplates,
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
    /// The 64 by 64 image the donor's atmosphere names at `+0x9C`, which the map's open copy
    /// replaces.
    pub atmosphere_image: Option<u32>,
    /// How many occlusion probes the map's copy of the donor's occlusion record takes from the
    /// level, when the donor places one.
    pub occlusion_probes: Option<usize>,
    /// Whether the donor places cubemap volumes, which the map's own stand in for.
    pub cubemaps: bool,
    /// The donor's static instance group records, which the level's statics copy.
    pub statics: statics::Templates,
}

/// A user data of a world collision instance whose shape is the packfile at that index of its
/// record's packfile list. Lower values name the record's mesh records instead.
const PACKFILE_SHAPE: u64 = 0x1_0000;

/// One piece, its static mesh by graph symbol, with its world bounds in meters.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Piece {
    pub mesh: String,
    pub minimum: [f32; 3],
    pub maximum: [f32; 3],
    /// Where the piece's instance sits, the middle of the first area's spawn points' bounds, the
    /// same for every piece. Its static mesh's geometry is relative to it.
    pub position: [f32; 3],
}

/// The map's world collision by graph symbol: its static compound and, in user data order, the
/// mesh packfiles its instances name.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct World {
    pub compound: String,
    pub meshes: Vec<String>,
}

/// One of a campaign map's connected areas: the zone sets that load together, converted once
/// with the map's other areas. Its pieces are those of the BSPs and placed objects it loads,
/// shared with every other area that loads them, and its world collision holds their meshes,
/// with the faces of each seam between two BSPs it loads left out.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Area {
    pub zone_sets: Vec<usize>,
    #[serde(default)]
    pub props: Vec<Prop>,
    /// Indices in [`Converted::pieces`].
    pub pieces: Vec<usize>,
    pub world: World,
    /// The preheader of its static instance group, which places its pieces.
    pub statics: String,
    /// Its player spawn points, from its first zone set.
    pub spawns: Vec<Spawn>,
    /// The sound point table that places the ambience of its structure BSPs, when they have any.
    pub ambience: Option<String>,
    /// Its static group's instances in group order, which its tome's Umbra objects and its
    /// visibility record list.
    pub visibility: Vec<Visible>,
}

/// One static instance as the area's precomputed visibility sees it: the identifier its bounds
/// row carries and its world box in the map's coordinates.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Visible {
    pub id: u32,
    pub minimum: [f32; 3],
    pub maximum: [f32; 3],
}

/// The pieces' graph, written under the output folder.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Converted {
    pub nodes: Vec<Value>,
    #[serde(default)]
    pub props: Vec<Prop>,
    pub pieces: Vec<Piece>,
    pub spawns: Vec<Spawn>,
    pub world: World,
    /// The preheader of the static instance group that places the pieces, the first area's when
    /// the level has connected areas.
    pub statics: String,
    /// The level's sky objects, in draw order.
    pub sky: Vec<sky::Object>,
    /// The selected analytical sun direction, toward the light with z up.
    pub light: Option<[f32; 3]>,
    /// The symbol of the sound point table that places the level's ambience, the first area's
    /// when the level has connected areas, when it has any.
    pub ambience: Option<String>,
    /// The first area's static instances, which its tome's Umbra objects and its visibility
    /// record list.
    #[serde(default)]
    pub visibility: Vec<Visible>,
    /// The symbol of the atmosphere volume the map's atmosphere names in place of the donor's,
    /// when the donor names one and the level has baked light.
    pub atmosphere: Option<String>,
    /// The symbol of the black distance lookup the map's atmosphere names in place of the donor's,
    /// when the donor names one and the level's surfaces draw their own fog.
    pub atmosphere_lookup: Option<String>,
    /// The symbol of the open image the map's atmosphere names at `+0x9C` in place of the donor's.
    #[serde(default)]
    pub atmosphere_image: Option<String>,
    /// The level's occlusion probes, each a position in the map's coordinates and a sky
    /// visibility, as many as `Templates::occlusion_probes` asks for, or none.
    pub occlusion: Vec<[f32; 4]>,
    /// The level's cubemap volumes, when the donor places volumes and the level has baked light.
    pub cubemaps: Vec<CubemapVolume>,
    /// The level's lights, each a position in the map's coordinates, a range in meters and a color
    /// in Destiny's units ([`lights`]).
    #[serde(default)]
    pub lights: Vec<lights::MapLight>,
    /// The connected areas, when converted as a campaign map's ([`convert_areas`]). The first's
    /// spawns and world are `spawns` and `world`.
    #[serde(default)]
    pub areas: Vec<Area>,
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
    // Each center is taken once. Taken in every comparison, the sorts of Nightfall's 15.7
    // million triangles took about 40 seconds longer.
    let mut keyed = items
        .into_iter()
        .map(|item| (center(&item)[axis], item))
        .collect::<Vec<_>>();
    keyed.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut items = keyed.into_iter().map(|(_, item)| item).collect::<Vec<_>>();
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
    // Decals also need their unshifted projection coordinates for clipping and sprite selection.
    if material.is_some_and(|material| material.water() || material.decal()) {
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
/// upward-facing lightmapped face. This is a translation approximation, not Halo's probe lookup. Placed
/// scenery such as crates otherwise took the atlas's neutral texel, the level's average ambient
/// with no lobe, and on Breakneck, whose sun comes from its lobes, drew several times darker than
/// the floor they stand on. Primitives with no ground within [`GROUND_DEPTH`] keep the neutral texel.
fn ground_light(level: &super::Level) -> HashMap<u32, [f32; 2]> {
    let middles = level
        .primitives
        .iter()
        .enumerate()
        .filter(|(_, primitive)| {
            !primitive.vertices.is_empty()
                && !primitive.vertices.iter().all(|v| v.lightmap.is_some())
        })
        .map(|(index, primitive)| {
            let (low, high) = bounds(primitive.vertices.iter().map(|v| v.position));
            (
                index,
                std::array::from_fn::<f32, 3, _>(|k| (low[k] + high[k]) / 2.),
            )
        })
        .collect::<Vec<_>>();
    let grid = ground_faces(level, &middles);
    let mut ground = HashMap::new();
    for &(index, middle) in &middles {
        if let Some(uv) = ground_below(level, &grid, middle) {
            ground.insert(u32::try_from(index).unwrap_or(u32::MAX), uv);
        }
    }
    ground
}

/// The [`GROUND_CELL`] grid cell holding horizontal position `(x, y)`.
fn ground_cell(x: f32, y: f32) -> (i32, i32) {
    (
        (x / GROUND_CELL).floor() as i32,
        (y / GROUND_CELL).floor() as i32,
    )
}

/// Every upward-facing lightmapped face, as its primitive and face index, binned in the cells
/// below the primitive middles. Each middle looks up only its own cell, so faces are binned in
/// those cells alone. A backdrop face kilometres across covers millions of cells, and binning
/// every cell held Nightfall's conversion for most of a minute.
fn ground_faces(
    level: &super::Level,
    middles: &[(usize, [f32; 3])],
) -> HashMap<(i32, i32), Vec<(usize, usize)>> {
    let mut grid = HashMap::<(i32, i32), Vec<(usize, usize)>>::new();
    for (_, middle) in middles {
        grid.entry(ground_cell(middle[0], middle[1])).or_default();
    }
    let looked_up = grid.keys().copied().collect::<Vec<_>>();
    for (index, primitive) in level.primitives.iter().enumerate() {
        if middles.is_empty() || primitive.vertices.iter().any(|v| v.lightmap.is_none()) {
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
            let low = ground_cell(
                points.iter().map(|p| p[0]).fold(f32::MAX, f32::min),
                points.iter().map(|p| p[1]).fold(f32::MAX, f32::min),
            );
            let high = ground_cell(
                points.iter().map(|p| p[0]).fold(f32::MIN, f32::max),
                points.iter().map(|p| p[1]).fold(f32::MIN, f32::max),
            );
            bin_face(&mut grid, &looked_up, low, high, (index, face));
        }
    }
    grid
}

/// Add `face` to every looked-up cell from `low` to `high`, walking whichever is fewer: the face's
/// cell range or the looked-up cells.
fn bin_face(
    grid: &mut HashMap<(i32, i32), Vec<(usize, usize)>>,
    looked_up: &[(i32, i32)],
    low: (i32, i32),
    high: (i32, i32),
    face: (usize, usize),
) {
    let covered =
        (i64::from(high.0) - i64::from(low.0) + 1) * (i64::from(high.1) - i64::from(low.1) + 1);
    if covered <= looked_up.len() as i64 {
        for x in low.0..=high.0 {
            for y in low.1..=high.1 {
                if let Some(faces) = grid.get_mut(&(x, y)) {
                    faces.push(face);
                }
            }
        }
    } else {
        for &(x, y) in looked_up {
            if (low.0..=high.0).contains(&x)
                && (low.1..=high.1).contains(&y)
                && let Some(faces) = grid.get_mut(&(x, y))
            {
                faces.push(face);
            }
        }
    }
}

/// The lightmap coordinates of the highest ground face straight below `middle`, within
/// [`GROUND_DEPTH`].
fn ground_below(
    level: &super::Level,
    grid: &HashMap<(i32, i32), Vec<(usize, usize)>>,
    middle: [f32; 3],
) -> Option<[f32; 2]> {
    let mut best: Option<(f32, [f32; 2])> = None;
    for &(owner, face) in grid
        .get(&ground_cell(middle[0], middle[1]))
        .into_iter()
        .flatten()
    {
        let lit = &level.primitives[owner];
        let corners = &lit.indices[face * 3..face * 3 + 3];
        let vertices = [0, 1, 2].map(|k| &lit.vertices[corners[k] as usize]);
        let [a, b, c] = vertices.map(|v| v.position);
        // Barycentric coordinates of the middle in the face's horizontal projection.
        let area = (b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1]);
        if area.abs() < 1e-8 {
            continue;
        }
        let wb = ((middle[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (middle[1] - a[1])) / area;
        let wc = ((b[0] - a[0]) * (middle[1] - a[1]) - (middle[0] - a[0]) * (b[1] - a[1])) / area;
        let wa = 1. - wb - wc;
        if wa < 0. || wb < 0. || wc < 0. {
            continue;
        }
        let z = wa * a[2] + wb * b[2] + wc * c[2];
        if z > middle[2] || middle[2] - z > GROUND_DEPTH || best.is_some_and(|(top, _)| z <= top) {
            continue;
        }
        let uv = [vertices[0], vertices[1], vertices[2]].map(|v| v.lightmap.unwrap_or_default());
        best = Some((
            z,
            [0, 1].map(|k| wa * uv[0][k] + wb * uv[1][k] + wc * uv[2][k]),
        ));
    }
    best.map(|(_, uv)| uv)
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
/// (`native::material::hybrid_lighting`) and keeps the half float's precision. Each
/// triangle's texture coordinates move by whole periods of its maps ([`triangle`]) toward zero, so
/// the model's 16-bit coordinates keep their precision.
fn mesh(
    level: &super::Level,
    triangles: &[&Triangle],
    offset: [f32; 3],
    ground: &HashMap<u32, [f32; 2]>,
    neutral: [f32; 2],
    placed: u32,
    vertex_data: Option<&native::material::vertex_data::Table>,
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
                    // The rigid vertex and shadow programs use the entity transform directly.
                    // Native layout 139 needs no bone bytes and supports 16-bit indices.
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
#[allow(clippy::too_many_arguments)]
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
    convert_areas(
        maps,
        map,
        zone_sets,
        &[],
        variant,
        packages,
        output,
        offset,
        templates,
        report,
    )
}

/// Converts a level whose zone sets `zone_sets` load as the connected `areas`, each a list of
/// them, or as one area when `areas` is empty. Every BSP and placed object converts once: each
/// piece and collision mesh holds one BSP's or object's geometry and lists the areas that load it
/// ([`Area`]), and the decorators keep within each area's budget.
#[allow(clippy::cognitive_complexity, clippy::too_many_arguments)]
pub fn convert_areas(
    maps: &Path,
    map: &str,
    zone_sets: &[usize],
    areas: &[Vec<usize>],
    variant: Option<&Path>,
    packages: &Path,
    output: &Path,
    offset: [f32; 3],
    templates: &Templates,
    report: &mut dyn FnMut(&str),
) -> Result<Converted> {
    ensure!(!zone_sets.is_empty(), "The map loads no zone set");
    ensure!(
        areas
            .iter()
            .all(|area| !area.is_empty() && area.iter().all(|set| zone_sets.contains(set))),
        "Each area loads some of the map's zone sets"
    );
    ensure!(areas.len() <= 64, "A map holds at most 64 connected areas");
    let split_areas = !areas.is_empty();
    let areas = if split_areas {
        areas.to_vec()
    } else {
        vec![zone_sets.to_vec()]
    };
    let compute_before = crate::compute::snapshot();
    let shaders_before = crate::tiger::shader::cache_snapshot();
    let started = std::time::Instant::now();
    // Each step's seconds, for the report's timing.
    let mut steps = serde_json::Map::new();
    let mut lap = started;
    let mut time = |step: &str| {
        steps.insert(step.to_owned(), json!(lap.elapsed().as_secs_f64()));
        lap = std::time::Instant::now();
    };
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
    // Each area's BSPs, by scenario mask and by index in `tags`.
    let loaded = areas
        .iter()
        .map(|sets| super::loaded_bsps(&cache, sets))
        .collect::<Result<Vec<_>>>()?;
    let bits = tags
        .iter()
        .map(|tag| super::bsp_mask(&cache, std::slice::from_ref(tag)))
        .collect::<Result<Vec<_>>>()?;
    let groups = loaded
        .iter()
        .map(|&loaded| {
            (0..tags.len())
                .filter(|&bsp| bits[bsp] & loaded != 0)
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    // The areas that load what the structure BSPs `bsps` hold (a primitive's `bsps`), bit `n` for
    // the `n`th area.
    let membership = |bsps: u32| -> u64 {
        loaded
            .iter()
            .enumerate()
            .filter(|&(_, &loaded)| bsps == 0 || bsps & loaded != 0)
            .fold(0, |mask, (area, _)| mask | 1 << area)
    };
    time("source");
    report(&format!(
        "Reading Geometry and Lightmaps / {} BSPs",
        tags.len()
    ));
    let mut level = super::read_areas(&cache, &mut pages, &tags, &groups)?;
    time("geometry_and_lightmaps");
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
    time("hidden_materials");
    // Each BSP's collision places that BSP's instances, and each polygon lists the areas whose
    // collision holds it. A seam joining two BSPs an area loads is open in that area
    // ([`collision`]).
    let mut polygons = Vec::new();
    let mut polygon_areas = Vec::new();
    let mut structure_polygons = 0;
    let open_seams = groups
        .iter()
        .map(|group| {
            let loaded = group
                .iter()
                .map(|&bsp| tags[bsp].clone())
                .collect::<Vec<_>>();
            collision::shared_seams(&cache, &loaded)
        })
        .collect::<Result<Vec<_>>>()?;
    let mut open_seam_surfaces = 0;
    for (index, tag) in tags.iter().enumerate() {
        report(&format!(
            "Reading Collision / BSP {}/{} / {}",
            index + 1,
            tags.len(),
            tag.path
        ));
        let collision = collision::read(&cache, &mut pages, tag, &BTreeSet::new())?;
        let areas = membership(bits[index]);
        let instanced = level
            .instances
            .iter()
            .filter(|instance| instance.bsp == index)
            .map(|instance| collision.placed(instance))
            .collect::<Result<Vec<_>>>()?;
        for (polygon, seam) in collision.structure.into_iter().zip(collision.seams) {
            let open = seam.map_or(0, |seam| {
                open_seams
                    .iter()
                    .enumerate()
                    .filter(|(_, open)| open.contains(&seam))
                    .fold(0u64, |mask, (area, _)| mask | 1 << area)
            });
            if areas & open != 0 {
                open_seam_surfaces += 1;
            }
            if areas & !open != 0 {
                structure_polygons += 1;
                polygons.push(polygon);
                polygon_areas.push(areas & !open);
            }
        }
        for polygon in instanced.into_iter().flatten() {
            polygons.push(polygon);
            polygon_areas.push(areas);
        }
    }
    time("collision_read");
    let placed = objects::read(&cache, &mut pages, variant.as_ref(), zone_sets, &mission)?;
    time("objects");
    // Placed objects' primitives follow the structure's, and the T-junction repair below only adds
    // vertices, so they keep these indices.
    let placed_from = u32::try_from(level.primitives.len())?;
    let base = level.materials.len();
    level.materials.extend(placed.materials);
    let prop_primitives = placed
        .props
        .into_iter()
        .map(|primitives| {
            primitives
                .into_iter()
                .map(|mut primitive| {
                    primitive.material = primitive.material.map(|material| base + material);
                    primitive
                })
                .collect()
        })
        .collect::<Vec<Vec<_>>>();
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
    // Whether the areas draw different skies, which the map's one set of sky objects cannot.
    let sky_areas = placed
        .sky
        .iter()
        .map(|(primitive, _)| membership(primitive.bsps))
        .collect::<BTreeSet<_>>();
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
    let area_spawns = areas
        .iter()
        .map(|sets| {
            Ok(spawn::read(&cache, variant.as_ref(), sets[0])?
                .into_iter()
                .map(|spawn| Spawn {
                    position: std::array::from_fn(|k| spawn.position[k] + offset[k]),
                    ..spawn
                })
                .collect::<Vec<_>>())
        })
        .collect::<Result<Vec<_>>>()?;
    let spawns = area_spawns[0].clone();
    let mut native = Reader::new(packages, &output.join("native"), false)?;
    let analytical = super::lighting::materials(&cache, &mut level)?;
    let sun =
        Some(super::lighting::read(&cache, super::lighting::SKY)?).filter(|light| light.present());
    let template = native
        .tag(templates.model, Some(MODEL_CLASS))
        .context("Piece model template")?;
    let row = *template
        .array(16, 136, Some(MESH_CLASS))?
        .first()
        .context("Template model has no mesh")?;
    let world_compound = native
        .tag(templates.world_compound, None)
        .context("World compound template")?
        .0
        .clone();
    let mut world_mesh_template = None;
    for &tag in &templates.world_meshes {
        let bytes = native
            .tag(tag, None)
            .with_context(|| format!("Piece buffer {tag:08X}"))?
            .0
            .clone();
        if crate::tiger::physics::rooted(&bytes, "hkpBvCompressedMeshShape") {
            world_mesh_template = Some((tag, bytes));
            break;
        }
    }
    let (world_mesh_tag, world_mesh) = world_mesh_template
        .context("The donor's world collision has no bare compressed mesh packfile")?;
    time("sky_order_spawns_and_templates");

    // A triangle with another surface's corner on its edge is cut there, so the corner rounds to
    // one point in both.
    let t_junctions = junctions::split(&mut level.primitives)?;
    time("t_junctions");
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
    time("triangles");
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
    let mut face_areas = Vec::new();
    for (polygon, &areas) in polygons.iter().zip(&polygon_areas) {
        let points = polygon
            .points
            .iter()
            .map(|p| std::array::from_fn(|k| p[k] + offset[k]))
            .collect::<Vec<[f32; 3]>>();
        for i in 1..points.len().saturating_sub(1) {
            faces.push([points[0], points[i], points[i + 1]]);
            face_areas.push(areas);
        }
    }
    for (face, &bsps) in placed.faces.iter().zip(&placed.face_bsps) {
        faces.push(face.map(|point| std::array::from_fn(|k| point[k] + offset[k])));
        face_areas.push(membership(bsps));
    }
    let triangle_center = |t: &Triangle| -> [f32; 3] {
        let primitive = &level.primitives[t.primitive as usize];
        let points = t.corners.map(|c| primitive.vertices[c as usize].position);
        std::array::from_fn(|k| (points[0][k] + points[1][k] + points[2][k]) / 3.)
    };
    // Every area's spawn bounds hold play, and the first's center is the pieces' origin.
    let play_boxes = area_spawns
        .iter()
        .map(|spawns| bounds(spawns.iter().map(|spawn| spawn.position)))
        .collect::<Vec<_>>();
    let (spawn_low, spawn_high) = play_boxes[0];
    // Area spawn bounds have different fractional centers. An unsnapped entity origin shifts
    // every piece's quantization lattice, opening cracks where the same BSP spans two areas.
    // Metre-aligned origins also align the coarser power-of-two steps used by distant pieces.
    let arena: [f32; 3] = std::array::from_fn(|k| ((spawn_low[k] + spawn_high[k]) / 2.).round());
    // Points in the map's space, moved by `offset` as the spawn points are.
    let in_play = |point: [f32; 3]| {
        play_boxes.iter().any(|(low, high)| {
            (0..3).all(|k| {
                point[k] >= low[k] - PLAY_MARGIN[k] && point[k] <= high[k] + PLAY_MARGIN[k]
            })
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
    // The areas that draw each of `renders`.
    let mut render_areas = Vec::new();
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
    // They group first by the areas that draw them, so each piece serves the same areas.
    for (region, in_play) in [(play, true), (scenery, false)] {
        let mut bands = BTreeMap::<(u64, u32, u32, [u32; 2]), Vec<Triangle>>::new();
        for triangle in region {
            bands
                .entry((
                    membership(level.primitives[triangle.primitive as usize].bsps),
                    uv_band(triangle_span(&triangle)),
                    triangle.period,
                    triangle.tiling.map(tiling_class),
                ))
                .or_default()
                .push(triangle);
        }
        // Each band splits on its own, so bands split in parallel and their pieces follow in band
        // order.
        let bands = bands
            .into_iter()
            .map(|(key, group)| (key, std::sync::Mutex::new(group)))
            .collect::<Vec<_>>();
        let split_bands = crate::cancellation::parallel_map(&bands, |(key, group)| {
            let (_, band, period, _) = *key;
            let group = std::mem::take(&mut *group.lock().unwrap_or_else(|e| e.into_inner()));
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
            let triangles = group.len();
            let mut pieces = Vec::new();
            split(group, &triangle_center, &fits, &mut pieces);
            (pieces, triangles, refused.get())
        })?;
        for (((areas, band, period, class), _), (pieces, triangles, refused)) in
            bands.into_iter().zip(split_bands)
        {
            band_report.push(json!({
                "areas": areas,
                "play": in_play,
                "band": band,
                "period": period,
                "tiling": class,
                "triangles": triangles,
                "pieces": pieces.len(),
                "refused": refused,
            }));
            renders.extend(pieces);
            render_areas.resize(renders.len(), areas);
        }
    }
    time("render_split");
    let face_center =
        |f: &Face| -> [f32; 3] { std::array::from_fn(|k| (f[0][k] + f[1][k] + f[2][k]) / 3.) };
    let mut shapes = Vec::new();
    // The areas whose collision holds each of `shapes`.
    let mut shape_areas = Vec::new();
    let mut by_areas = BTreeMap::<u64, Vec<Face>>::new();
    for (face, areas) in faces.into_iter().zip(face_areas) {
        if areas != 0 {
            by_areas.entry(areas).or_default().push(face);
        }
    }
    for (areas, faces) in by_areas {
        let (play, scenery): (Vec<_>, Vec<_>) =
            faces.into_iter().partition(|f| in_play(face_center(f)));
        for group in [play, scenery] {
            split(
                group,
                &face_center,
                &|group: &[Face]| group.len() <= MAX_COLLISION_TRIANGLES,
                &mut shapes,
            );
            shape_areas.resize(shapes.len(), areas);
        }
    }

    time("collision_split");
    let mut used = BTreeMap::<u32, Material>::new();
    for primitive in &level.primitives {
        let key = primitive.material.map_or(u32::MAX, |m| m as u32);
        used.entry(key).or_insert_with(|| {
            primitive
                .material
                .and_then(|m| level.materials.get(m))
                .cloned()
                .unwrap_or(Material {
                    decal: None,
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
    let ground = if level.lightmap.is_some() {
        ground_light(&level)
    } else {
        HashMap::new()
    };
    time("used_materials_and_ground_light");
    // The surfaces draw the level's own fog in place of Destiny's.
    let (fog, fog_report) = fog::read(&cache, &tags, offset[2])?;
    let (radiance, camera_report) =
        sky::radiance(&cache, level.lightmap.as_ref().map(|atlas| atlas.level))?;
    time("fog_and_radiance");
    report("Converting Lightmap Textures");
    let lightmap = level
        .lightmap
        .as_ref()
        .map(|atlas| {
            native::material::lightmap_textures(
                &mut native,
                &mut graph,
                atlas,
                sky::veil(&cache)?,
                sun,
                fog,
            )
        })
        .transpose()?;
    let lightmap = lightmap.map(|mut lightmap| {
        lightmap.analytical = analytical;
        lightmap.origin = offset;
        lightmap.radiance = radiance;
        lightmap
    });
    time("lightmap_textures");
    let vertex_data = native::material::vertex_data::add(
        &mut native,
        &mut graph,
        &level.primitives,
        level
            .lightmap
            .as_ref()
            .map_or(1., |atlas| atlas.scale * atlas.level),
        |primitive, vertex| lightmap_coordinates(vertex, primitive, &ground, neutral, placed_from),
    )?;
    time("vertex_data");
    report(&format!(
        "Converting Materials and Textures / {} Materials",
        used.len()
    ));
    let material_report = native::material::build_with_progress(
        &cache,
        &mut pages,
        &mut native,
        &mut graph,
        &used,
        native::material::Binding {
            palette: &[0],
            rigid: true,
            objects: &BTreeMap::new(),
            overrides: &BTreeMap::new(),
            lenses: &BTreeMap::new(),
            paint: &BTreeMap::new(),
            lightmap: lightmap.as_ref(),
            vertex_data: vertex_data.as_ref().map(|table| table.symbol.as_str()),
            // Every level surface draws on the pieces' static meshes (`statics`).
            statics: true,
            prefix: "",
        },
        report,
    )?;
    time("materials");

    let mut pieces = Vec::new();
    let mut world = World {
        compound: "world-collision".into(),
        meshes: Vec::new(),
    };
    let mut instances = Vec::new();
    let mut vertices = 0usize;
    let mut drawn = 0usize;
    let mut placed_faces = 0usize;
    let solid = statics::solid(&used);
    let clipped = statics::clipped(&used);
    // A piece too wide for POSITION_STEP stores its positions on a coarser power-of-two step, so
    // a corner it shares with a finer piece rounded to two points. Paradiso's play area holds
    // 1/128 m and 1/64 m pieces beside 1/256 m ones, and thin lines of sky showed along its
    // ground. Each corner
    // takes the coarsest step of the pieces that hold it, which a finer piece also stores exactly.
    let piece_steps = renders
        .iter()
        .map(|group| {
            let (low, high) = bounds(group.iter().flat_map(|t| {
                let primitive = &level.primitives[t.primitive as usize];
                t.corners.map(|c| primitive.vertices[c as usize].position)
            }));
            let half = (0..3).map(|k| (high[k] - low[k]) / 2.).fold(0., f32::max);
            // Snapping moves the extent by up to a step, so two steps of margin keep the encoder
            // from doubling it again.
            let mut step = POSITION_STEP;
            while half + 2. * step > 32767. * step {
                step *= 2.;
            }
            step
        })
        .collect::<Vec<_>>();
    let corner_key = |position: [f32; 3]| position.map(|v| (v / POSITION_STEP).round() as i64);
    let mut corner_steps = HashMap::<[i64; 3], f32>::new();
    for (group, &step) in renders.iter().zip(&piece_steps) {
        for t in group {
            let primitive = &level.primitives[t.primitive as usize];
            for c in t.corners {
                let position = primitive.vertices[c as usize].position;
                let key = corner_key(std::array::from_fn(|k| position[k] + offset[k]));
                let entry = corner_steps.entry(key).or_insert(step);
                *entry = entry.max(step);
            }
        }
    }
    time("piece_steps");
    let render_started = std::time::Instant::now();
    let workers = crate::cancellation::workers()?;
    for (batch, groups) in renders.chunks(workers).enumerate() {
        report(&format!(
            "Preparing Render Geometry / Piece {}/{}",
            batch * workers + 1,
            renders.len()
        ));
        let prepared = crate::cancellation::parallel(groups, |group| {
            let (mut mesh, coordinates) = mesh(
                &level,
                &group.iter().collect::<Vec<_>>(),
                offset,
                &ground,
                neutral,
                placed_from,
                vertex_data.as_ref(),
            )?;
            // Snap in shared world coordinates before subtracting the instances' origin. This
            // also removes half-step rounding differences between individual statics.
            let piece = batch * workers
                + groups
                    .iter()
                    .position(|candidate| std::ptr::eq(candidate, group))
                    .context("Render piece outside its batch")?;
            let step = piece_steps[piece];
            for position in &mut mesh.positions {
                let corner = corner_steps
                    .get(&corner_key(*position))
                    .copied()
                    .unwrap_or(step)
                    .max(step);
                *position = std::array::from_fn(|axis| {
                    (position[axis] / corner).round() * corner - arena[axis]
                });
            }
            let mut streams = geometry::encode_static(&mesh, step)?;
            let minimum = std::array::from_fn(|k| streams.bounds.0[k] + arena[k]);
            let maximum = std::array::from_fn(|k| streams.bounds.1[k] + arena[k]);
            ensure!(
                streams.attributes.len() == coordinates.len() * ATTRIBUTE_STRIDE,
                "Piece has {} attribute bytes for {} lightmap coordinates",
                streams.attributes.len(),
                coordinates.len()
            );
            for (record, uv) in streams
                .attributes
                .chunks_exact_mut(ATTRIBUTE_STRIDE)
                .zip(&coordinates)
            {
                for (k, value) in uv.iter().enumerate() {
                    let at = DETAIL_COORDINATES + k * 2;
                    record[at..at + 2].copy_from_slice(&half::f16::from_f32(*value).to_le_bytes());
                }
            }
            let draws = mesh
                .groups
                .iter()
                .map(|(key, _)| statics::Draw {
                    key: *key,
                    stage: stages.get(key).copied().unwrap_or(0),
                    solid: solid.contains(key),
                    clipped: clipped.contains(key),
                })
                .collect::<Vec<_>>();
            Ok((
                streams,
                draws,
                minimum,
                maximum,
                mesh.positions.len(),
                mesh.groups
                    .iter()
                    .map(|(_, faces)| faces.len())
                    .sum::<usize>(),
            ))
        })?;
        for (local, (streams, draws, minimum, maximum, count, triangles)) in
            prepared.into_iter().enumerate()
        {
            let part = batch * workers + local;
            report(&format!(
                "Writing Render Geometry / Piece {}/{}",
                part + 1,
                renders.len()
            ));
            let name = format!("piece{part}");
            vertices += count;
            drawn += triangles;
            let mesh = statics::mesh(
                &mut native,
                &mut graph,
                &templates.statics,
                &template,
                row,
                &streams,
                &draws,
                &name,
            )?;
            pieces.push(Piece {
                mesh,
                minimum,
                maximum,
                position: arena,
            });
        }
    }
    let render_seconds = render_started.elapsed().as_secs_f64();
    let collision_started = std::time::Instant::now();
    for (batch, groups) in shapes.chunks(workers).enumerate() {
        report(&format!(
            "Preparing Collision / Mesh {}/{}",
            batch * workers + 1,
            shapes.len()
        ));
        let prepared = crate::cancellation::parallel(groups, |group| {
            let (low, high) = bounds(group.iter().flatten().copied());
            let center: [f32; 3] = std::array::from_fn(|k| (low[k] + high[k]) / 2.);
            let faces = group
                .iter()
                .map(|face| face.map(|point| sub(point, center)))
                .collect::<Vec<Face>>();
            let (bytes, _) = bare_mesh(&world_mesh, &faces)?;
            Ok((bytes, low, high, center, faces))
        })?;
        for (local, (bytes, low, high, center, faces)) in prepared.into_iter().enumerate() {
            let part = batch * workers + local;
            report(&format!(
                "Writing Collision / Mesh {}/{}",
                part + 1,
                shapes.len()
            ));
            placed_faces += faces.len();
            let symbol = format!("collision{part}");
            graph.add(&symbol, world_mesh_tag, &bytes, None, Vec::new())?;
            instances.push(Instance {
                translation: center,
                bounds: (low, high),
                user_data: PACKFILE_SHAPE | u64::try_from(part)?,
                triangles: faces.into(),
            });
            world.meshes.push(symbol);
        }
    }
    let collision_seconds = collision_started.elapsed().as_secs_f64();
    time("render_and_collision");
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
    // Native sun direction comes from the scenario sky's analytical light. Baked VMF lobes
    // describe a separate lighting term and do not establish the sun's direction.
    let light = sun.map(|sun| sun.vector);
    time("sky");
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
            &loaded,
        )?,
        None => (
            vec![None; areas.len()],
            json!({"points": 0, "reason": "No sound point table or no audio decoder"}),
        ),
    };
    time("ambience");
    report("Decoding Mission Audio");
    let mission_audio = match &templates.mission_audio {
        Some(options) => {
            super::mission::audio::export(&mission, options, &mut pages, &output.join("audio"))?
        }
        None => json!({"status":"undecoded","reason":"No mission audio decoder configured",
            "playback":"unsupported"}),
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
    time("mission_audio_and_atmosphere_lookup");
    report("Preparing Lighting and Occlusion Probes");
    let (occlusion, open_light, occlusion_report) = match templates.occlusion_probes {
        Some(count) => occlusion::probes(
            &level,
            &area_spawns
                .iter()
                .flatten()
                .map(|spawn| sub(spawn.position, offset))
                .collect::<Vec<_>>(),
            &super::squads::read_squads(&cache, [0.; 3], super::loaded_bsps(&cache, zone_sets)?)?
                .into_iter()
                .flat_map(|squad| squad.points)
                .map(|point| point.spawn.position)
                .collect::<Vec<_>>(),
            &super::interior_boxes(&cache, &tags)?,
            count,
            offset,
        )?,
        None => (
            Vec::new(),
            None,
            json!({"reason": "The donor places no occlusion probes"}),
        ),
    };
    // The atmosphere takes the sun's color where Destiny's sun lights the surfaces in its place,
    // and the open probes' light as the sky's brightness.
    let light_color = sun.map(|sun| {
        let peak = sun.intensity.into_iter().fold(0f32, f32::max);
        sun.intensity.map(|v| v / peak)
    });
    let (atmosphere, atmosphere_report) = match (templates.atmosphere, light_color) {
        (Some(volume), Some(color)) => {
            let sky = open_light.map(|light| atmosphere::sky_luminance(light, radiance));
            let (symbol, report) = atmosphere::add(&mut native, &mut graph, volume, color, sky)?;
            (Some(symbol), report)
        }
        _ => (
            None,
            json!({"reason": "The donor names no atmosphere volume or the level has no baked light"}),
        ),
    };
    let (atmosphere_image, image_report) = match (templates.atmosphere_image, &atmosphere) {
        (Some(image), Some(_)) => {
            let (symbol, report) = atmosphere::open_image(&mut native, &mut graph, image)?;
            (Some(symbol), report)
        }
        _ => (
            None,
            json!({"reason": "The donor names no atmosphere image or the map keeps the donor's atmosphere"}),
        ),
    };
    time("occlusion_and_atmosphere");
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
            radiance,
        )?
    } else {
        (
            Vec::new(),
            json!({"reason": "The donor places no cubemap volumes or the level has no baked light"}),
        )
    };
    time("cubemaps");
    report("Reading Lights");
    let (lights, lights_report) = match level.lightmap.as_ref() {
        Some(atlas) => lights::read(
            &cache,
            super::loaded_bsps(&cache, zone_sets)?,
            offset,
            radiance,
            atlas.level,
        )?,
        None => (
            Vec::new(),
            json!({"reason": "The level has no baked light"}),
        ),
    };
    time("lights");
    report("Converting Posed Props");
    let (props, prop_materials) = props::build(
        &cache,
        &mut pages,
        &mut native,
        &mut graph,
        templates,
        &prop_primitives,
        &level.materials,
        offset,
        report,
    )?;
    time("props");
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
    time("bind_shaders");
    ensure!(!pieces.is_empty(), "The level converted to no pieces");
    // Each area's static compound places its own meshes, numbered in its own order.
    let area_shapes = (0..areas.len())
        .map(|area| {
            (0..instances.len())
                .filter(|&shape| shape_areas[shape] & 1u64 << area != 0)
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    // A compound encodes every mesh it places again, so the areas' compounds encode in parallel.
    let compounds = crate::cancellation::parallel_map(&area_shapes, |shapes| {
        if shapes.is_empty() {
            return Ok(None);
        }
        let placed = shapes
            .iter()
            .enumerate()
            .map(|(local, &shape)| {
                let instance = &instances[shape];
                Ok(Instance {
                    translation: instance.translation,
                    bounds: instance.bounds,
                    user_data: PACKFILE_SHAPE | u64::try_from(local)?,
                    triangles: instance.triangles.clone(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        static_compound(&world_compound, &world_mesh, &placed).map(Some)
    })?;
    let mut area_rows = Vec::with_capacity(areas.len());
    let mut area_report = Vec::with_capacity(areas.len());
    for ((area, zone_sets), (shapes, compound)) in areas
        .iter()
        .enumerate()
        .zip(area_shapes.iter().zip(compounds))
    {
        let bit = 1u64 << area;
        let area_pieces = (0..pieces.len())
            .filter(|&piece| render_areas[piece] & bit != 0)
            .collect::<Vec<_>>();
        ensure!(
            !shapes.is_empty() && !area_pieces.is_empty(),
            "Area {area} converted to no pieces or no collision"
        );
        let symbol = if area == 0 {
            world.compound.clone()
        } else {
            format!("{}-{area}", world.compound)
        };
        let compound = compound?.context("Area compound")?;
        graph.add(
            &symbol,
            templates.world_compound,
            &compound,
            None,
            Vec::new(),
        )?;
        let placed = area_pieces
            .iter()
            .map(|&piece| statics::Instance {
                mesh: &pieces[piece].mesh,
                position: pieces[piece].position,
                minimum: pieces[piece].minimum,
                maximum: pieces[piece].maximum,
            })
            .collect::<Vec<_>>();
        let group = if area == 0 {
            "statics".to_owned()
        } else {
            format!("statics-{area}")
        };
        let statics = statics::group(&mut native, &mut graph, &templates.statics, &placed, &group)?;
        let visibility = statics::identifiers(&group, placed.len())
            .into_iter()
            .zip(&placed)
            .map(|(id, instance)| Visible {
                id,
                minimum: instance.minimum,
                maximum: instance.maximum,
            })
            .collect();
        area_report.push(json!({
            "zone_sets": zone_sets,
            "bsps": groups[area].iter().map(|&bsp| tags[bsp].path.as_str()).collect::<Vec<_>>(),
            "pieces": area_pieces.len(),
            "collision_meshes": shapes.len(),
            "spawns": area_spawns[area].len(),
            "open_seams": open_seams[area],
        }));
        area_rows.push(Area {
            zone_sets: zone_sets.clone(),
            props: props
                .iter()
                .filter(|prop| membership(prop.bsps) & bit != 0)
                .cloned()
                .collect(),
            pieces: area_pieces,
            world: World {
                compound: symbol,
                meshes: shapes
                    .iter()
                    .map(|&shape| world.meshes[shape].clone())
                    .collect(),
            },
            statics,
            spawns: area_spawns[area].clone(),
            ambience: ambience[area].clone(),
            visibility,
        });
    }
    let world = area_rows[0].world.clone();
    let statics = area_rows[0].statics.clone();
    time("areas");
    report("Verifying Source Files and Writing Map Reports");
    pages.verify_inputs()?;
    native.finish()?;
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
    time("finish");
    let mut report = json!({
        "compute": crate::compute::snapshot().since(&compute_before),
        "shaders": crate::tiger::shader::cache_snapshot().since(&shaders_before),
        "timing": { "conversion_seconds": started.elapsed().as_secs_f64(),
            "render_seconds": render_seconds, "collision_seconds": collision_seconds, "workers": workers,
            "steps": steps },
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
        "objects_source_poses": placed.poses,
        "posed_entities": props,
        "prop_materials": prop_materials,
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
        "camera": camera_report,
            "sky": sky_report,
        "sky_order": sky_order,
        "sky_blackened": blackened,
        "ambience": ambience_report,
        "atmosphere": atmosphere_report,
        "atmosphere_lookup": lookup_report,
        "atmosphere_image": image_report,
        "fog": fog_report,
        "occlusion": occlusion_report,
        "cubemaps": cubemaps_report,
        "lights": lights_report,
        "analytical_light_direction": light,
        "analytical_light_color": light_color,
        "analytical_sun": sun,
        "analytical_materials": lightmap.as_ref().map(|map| &map.analytical),
    });
    report["open_seams"] = json!(open_seams[0]);
    report["open_seam_surfaces"] = json!(open_seam_surfaces);
    report["sky_areas"] = json!(sky_areas);
    if split_areas {
        report["areas"] = json!(area_report);
    }
    fs::write(
        output.join("map-pieces.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(Converted {
        nodes: graph.nodes,
        props,
        pieces,
        spawns,
        world,
        statics,
        sky,
        light,
        ambience: area_rows[0].ambience.clone(),
        visibility: area_rows[0].visibility.clone(),
        atmosphere,
        atmosphere_lookup,
        atmosphere_image,
        occlusion,
        cubemaps,
        lights,
        areas: if split_areas { area_rows } else { Vec::new() },
        report,
    })
}

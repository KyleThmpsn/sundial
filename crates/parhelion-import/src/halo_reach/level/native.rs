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
    bsps, objects,
    spawn::{self, Spawn},
    variant,
};
use crate::{
    halo_reach::{
        cache::{Cache, Tag},
        material::Material,
        model::Primitive,
        native,
        resource::Pages,
    },
    presentation::{Graph, geometry::Mesh, put},
    tiger::{payload::Payload, reader::Reader},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs,
    path::Path,
    sync::Arc,
    time::Instant,
};

mod area;
mod atmosphere;
mod coverage;
mod cubemaps;
mod culling;
mod decorators;
mod fog;
mod grade;
pub use fog::Adjustment as FogAdjustment;
mod junctions;
pub mod lights;
mod occlusion;
mod partition;
mod pieces;
mod plan;
mod props;
pub mod sky;
mod statics;
mod world;
pub use cubemaps::CubemapVolume;
pub use decorators::{Occlusion as DecoratorOcclusion, Templates as DecoratorTemplates};
pub use plan::Plan;
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
/// The attribute stream's stride, and its two half-float detail lanes, whose bits carry lightmap
/// coordinates for the level's materials as fixed point
/// ([`native::material::vertex_data::coordinate_bits`]) or name a vertex data record.
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
    /// The stock records Destiny's instanced decorators copy, when the level's decorators draw
    /// as those. Without them they draw as copies among the pieces.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decorators: Option<DecoratorTemplates>,
    /// The map's changes to the level's fog.
    #[serde(skip_serializing_if = "FogAdjustment::is_default")]
    pub fog: FogAdjustment,
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
    /// Its static group's instances in group order, then the clusters of its decorator records,
    /// which its tome's Umbra objects and its visibility record list.
    pub visibility: Vec<Visible>,
    /// The decorator records it places, by symbol, when its decorators draw as Destiny's
    /// instanced decorators.
    #[serde(default)]
    pub decorators: Vec<String>,
    /// The interiors of the structure BSPs it loads ([`Interior`]).
    #[serde(default)]
    pub interiors: Vec<Interior>,
}

/// An interior region of the level, a box in the map's coordinates: merged structure clusters
/// whose probes see open sky in fewer than 5% of their rays and whose floors hold less than a
/// quarter of the open probes' floor light (`occlusion`). Reach lit an object from the floor
/// below it, so its interiors' objects took the interior's light alone. An area lists at most 64.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Interior {
    pub minimum: [f32; 3],
    pub maximum: [f32; 3],
}

/// The structure cluster that holds `point`, as an index in `clusters`: the smallest box, by
/// volume, that holds it, the first of equal ones. Reach's cluster boxes overlap where its
/// clusters' shapes do, and the smallest box holding a point is the likeliest to be the cluster
/// whose space holds it. This stands in for Reach's own cluster lookup through its BSP tree, which
/// the converter does not read. `None` when no box holds it.
pub fn cluster_at(clusters: &[super::streaming::Bounds], point: [f32; 3]) -> Option<usize> {
    clusters
        .iter()
        .enumerate()
        .filter(|(_, cluster)| {
            (0..3).all(|k| cluster.minimum[k] <= point[k] && point[k] <= cluster.maximum[k])
        })
        .map(|(index, cluster)| {
            let volume = (0..3)
                .map(|k| cluster.maximum[k] - cluster.minimum[k])
                .product::<f32>();
            (volume, index)
        })
        .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
        .map(|(_, index)| index)
}

/// One static instance or decorator cluster as the area's precomputed visibility sees it: the
/// identifier its bounds row carries and its world box in the map's coordinates.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Visible {
    pub id: u32,
    pub minimum: [f32; 3],
    pub maximum: [f32; 3],
    /// Whether the visibility record keys it in the upper half of its 64-bit key. A bounds row
    /// holds a 64-bit key at `+0x20`: a static instance's identifier sits in its lower half and
    /// a decorator cluster's at `+0x24`, its upper half, as Distant Shore's record keys them.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub upper: bool,
}

/// The native sun a converted level asks Destiny for: global channel 8's intensity and channel 9's
/// color, whose product `GlobalLighting+0x10` holds. A dynamic object under the native sun takes
/// `albedo * 0.25 * S * x * W^0.7 / pi`, and the level's source light reaches the screen as
/// `albedo * irradiance / (pi * level) * radiance * 0.25 * x`, so the two agree for
/// `S = lum(I) * radiance / (level * OPEN^0.7)` at the common open visibility. Other visibility
/// values retain Shadowkeep's direct-light attenuation. The native Burley response and source
/// Lambert diffuse are not identical. `I` is the level's principal analytical sun.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct NativeSun {
    pub intensity: f32,
    /// The sun's color at unit luminance.
    pub color: [f32; 3],
}

impl NativeSun {
    /// Rec. 709 luminance weights, as the native lighting rows splat luminance.
    const LUMINANCE: [f32; 3] = [0.2126, 0.7152, 0.0722];

    /// The native sun matching analytical light `intensity` over a level whose baked light has the
    /// average lit level `level` and converts at `radiance`. A level without a present analytical
    /// sun asks for none.
    pub(crate) fn matching(intensity: [f32; 3], radiance: f32, level: f32) -> Self {
        let luminance = dot(intensity, Self::LUMINANCE);
        if !(luminance.is_finite() && luminance > 0. && level > 0.) {
            return Self {
                intensity: 0.,
                color: [1.; 3],
            };
        }
        Self {
            intensity: luminance * radiance
                / (level * native::lighting::OPEN.powf(native::lighting::DIRECT_POWER)),
            color: intensity.map(|channel| channel / luminance),
        }
    }
}

/// The bounds of Shadowkeep's automatic exposure a converted level asks for, in base-2 stops of
/// the frame's exposure scale `x` (`Frame+0x1C`, `cb13[1].x`): global channel 5 (`EC8E74D8`) the
/// lower and channel 6 (`B07F7C2A`) the upper. The exposure update (`0123E600` in the captured
/// Shadowkeep image) clamps both its target and the adapted exposure to them, and the frame takes
/// `x = 2^E`.
///
/// The level's baked light reaches the screen as `irradiance / level * radiance * 0.25 * x`, and
/// Reach's as `irradiance * 2^s`, taking the product of its two exposure scales as `2^s` of the
/// camera effects. The two agree at `E = s + log2(level / (0.25 * radiance))`, so Reach's
/// exposure range's top maps to the upper bound. A manual target maps to both bounds. The upper
/// bound stays within Shadowkeep's stock range: above its stock maximum a dark level's adaptation
/// would run past anything Reach shows. Automatic mode keeps the stock lower bound. Mapped from
/// Reach's lowest exposure, it held the Forge World maps' exposure at 2.2 stops or more and
/// Reflection's at 1 or more, where Paradiso's meter had settled at 0.84. Bright walls and floors
/// then drew washed out in game while the native-lit weapon did not. The mapping compares linear scene values, not
/// Reach's gamma curve with Shadowkeep's tone mapping, and a bright level is left to
/// Shadowkeep's meter.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct NativeExposure {
    pub minimum: f32,
    pub maximum: f32,
}

impl NativeExposure {
    /// Shadowkeep's stock bounds: the stock channel table's defaults for channels 5 and 6.
    pub const STOCK: [f32; 2] = [-3., 3.];

    /// The bounds matching Reach's exposure `range` in stops (or its manual `target`, clamped to
    /// the range, when not `automatic`) over a level whose baked light has the average lit level
    /// `level` and converts at `radiance`.
    pub(crate) fn matching(
        automatic: bool,
        target: f32,
        range: [f32; 2],
        radiance: f32,
        level: f32,
    ) -> Result<Self> {
        ensure!(
            target.is_finite()
                && range.iter().all(|value| value.is_finite())
                && range[0] <= range[1]
                && level.is_finite()
                && level > 0.
                && radiance.is_finite()
                && radiance > 0.,
            "Camera exposure requires finite ordered bounds and positive lighting scales"
        );
        let offset = (level / (native::material::REFERENCE_ILLUMINATION * radiance)).log2();
        ensure!(
            offset.is_finite() && range[0] <= range[1],
            "Camera exposure range {range:?} has no native equivalent at level {level} and \
             radiance {radiance}"
        );
        let high = if automatic {
            range[1]
        } else {
            target.clamp(range[0], range[1])
        };
        let [floor, ceiling] = Self::STOCK;
        let maximum = (high + offset).clamp(floor, ceiling);
        Ok(Self {
            minimum: if automatic { floor } else { maximum },
            maximum,
        })
    }
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
    /// The native sun that matches the level's analytical sun against its baked light, when the
    /// level has baked light ([`NativeSun`]).
    #[serde(default)]
    pub native_sun: Option<NativeSun>,
    /// The automatic exposure bounds that hold the level within Reach's exposure range, when the
    /// level has baked light ([`NativeExposure`]).
    #[serde(default)]
    pub native_exposure: Option<NativeExposure>,
    /// The native screen effect carrying the scenario's display curve and colour filter.
    #[serde(default)]
    pub color_grade: Option<String>,
    /// The symbol of the sound point table that places the level's ambience, the first area's
    /// when the level has connected areas, when it has any.
    pub ambience: Option<String>,
    /// The first area's static instances and decorator clusters, which its tome's Umbra objects
    /// and its visibility record list.
    #[serde(default)]
    pub visibility: Vec<Visible>,
    /// The first area's decorator records, by symbol, when the decorators draw as Destiny's
    /// instanced decorators.
    #[serde(default)]
    pub decorators: Vec<String>,
    /// The ambient visibility of every decorator record's instances, which each area's occlusion
    /// record maps for the records it places.
    #[serde(default)]
    pub decorator_occlusion: Option<DecoratorOcclusion>,
    /// The symbol of the atmosphere volume the map's atmosphere names in place of the donor's,
    /// when the donor names one and the level has baked light.
    pub atmosphere: Option<String>,
    /// The symbol of the second atmosphere volume, in the hue of the level's interiors, that the
    /// map's atmosphere names at `+0x94` and its interior environment volumes blend to through
    /// global channel 43, when the map has its own atmosphere volume and the level interiors.
    #[serde(default)]
    pub interior_atmosphere: Option<String>,
    /// The symbol of the black distance lookup the map's atmosphere names in place of the donor's,
    /// when the donor names one and the level's surfaces draw their own fog.
    pub atmosphere_lookup: Option<String>,
    /// The symbol of the open image the map's atmosphere names at `+0x9C` in place of the donor's.
    #[serde(default)]
    pub atmosphere_image: Option<String>,
    /// The level's occlusion probes, each a position in the map's coordinates and a sky
    /// visibility, as many as `Templates::occlusion_probes` asks for, or none.
    pub occlusion: Vec<[f32; 4]>,
    /// The boxes of the level's structure clusters in the map's coordinates, leaving out each
    /// structure BSP's outside cluster, whose box holds all its others. A probe stands in the
    /// smallest box that holds it ([`cluster_at`]).
    #[serde(default)]
    pub clusters: Vec<super::streaming::Bounds>,
    /// The first area's interiors, the whole level's when it has no connected areas
    /// ([`Interior`]).
    #[serde(default)]
    pub interiors: Vec<Interior>,
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
    /// The level from above over every area's play area, when any surface lies there ([`Plan`]).
    #[serde(default)]
    pub plan: Option<Plan>,
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

/// Splits `items` at the median of their selected `coordinates` until every part
/// satisfies `fits`. Halves of at least `PARALLEL_SPLIT` items split on their own threads, and
/// the lower half's parts still come first.
fn split<T: Send>(
    items: Vec<T>,
    coordinates: &(dyn Fn(&[T]) -> Vec<f32> + Sync),
    fits: &(dyn Fn(&[T]) -> bool + Sync),
    out: &mut Vec<Vec<T>>,
) {
    if items.len() <= 1 || fits(&items) {
        if !items.is_empty() {
            out.push(items);
        }
        return;
    }
    // Keep coordinate production outside the sort comparisons, including source vertex reads.
    let mut keyed = coordinates(&items)
        .into_iter()
        .zip(items)
        .collect::<Vec<_>>();
    keyed.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut items = keyed.into_iter().map(|(_, item)| item).collect::<Vec<_>>();
    let upper = items.split_off(items.len() / 2);
    if upper.len() < PARALLEL_SPLIT {
        split(items, coordinates, fits, out);
        split(upper, coordinates, fits, out);
        return;
    }
    let mut upper_parts = Vec::new();
    std::thread::scope(|scope| {
        scope.spawn(|| split(upper, coordinates, fits, &mut upper_parts));
        split(items, coordinates, fits, out);
    });
    out.append(&mut upper_parts);
}

/// Coordinates on the longest world-space axis, used for vertex capacity, spatial extent and
/// collision partitions. Render groups with excessive UV range choose that domain first.
fn spatial_coordinates<T>(items: &[T], center: impl Fn(&T) -> [f32; 3]) -> Vec<f32> {
    let centers = items.iter().map(center).collect::<Vec<_>>();
    let (low, high) = bounds(centers.iter().copied());
    let axis = (0..3)
        .max_by(|&a, &b| (high[a] - low[a]).total_cmp(&(high[b] - low[b])))
        .unwrap_or(0);
    centers.into_iter().map(|point| point[axis]).collect()
}

/// The least half [`split`] gives its own thread. Nightfall's largest render band holds 3.7
/// million triangles, which serial median sorts left splitting alone after every other band.
const PARALLEL_SPLIT: usize = 250_000;

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
    uv_ranges(triangles).into_iter().fold(0., f32::max)
}

fn uv_ranges(triangles: &[Triangle]) -> [f32; 2] {
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
    std::array::from_fn(|axis| (high[axis] - low[axis]) * tiling[axis])
}

/// The horizontal cell size, in meters, of the grid [`ground_light`] bins lightmapped floors in.
const GROUND_CELL: f32 = 4.;
/// The steepest lightmapped face, as its normal's least upward component, that counts as ground.
const GROUND_FACING: f32 = 0.3;
/// How far below a primitive's top, in meters, its ground may lie.
const GROUND_DEPTH: f32 = 64.;
/// The least first lightmap coordinate a placed object's vertex keeps, so its negation stays below
/// zero once the vertex program decodes it: one step of the stored fixed point.
const PLACED_LEAST: f32 = 1. / native::material::vertex_data::COORDINATE_STEPS;

/// The lightmap coordinates each primitive without lightmap coordinates of its own takes: those of
/// the lightmapped ground straight below its middle, where a downward ray from the top of its bounds
/// first meets a covered upward-facing lightmapped face. Projected decals and transmitting layers
/// cannot supply the floor's lighting, and a cutout must cover the ray's texture coordinate.
/// This is a translation approximation, not Halo's
/// probe lookup. Placed scenery such as crates otherwise took the atlas's neutral texel, the level's
/// average ambient with no lobe, and on Breakneck, whose sun comes from its lobes, drew several
/// times darker than the floor they stand on. A ray from the middle passed under the ground an
/// object is sunk into: Paradiso's Forge `ff_rock_flat` lies in a slope whose surface stands above
/// its middle, and the ray met a face beneath whose coordinates fall on Reach's unbaked background
/// (no ambient light and no sun), which drew the slab magenta in full sun. Primitives with no ground
/// within [`GROUND_DEPTH`] keep the neutral texel.
fn ground_light(level: &super::Level, coverage: &[coverage::Coverage]) -> HashMap<u32, [f32; 2]> {
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
                [(low[0] + high[0]) / 2., (low[1] + high[1]) / 2., high[2]],
            )
        })
        .collect::<Vec<_>>();
    let grid = ground_faces(level, coverage, &middles);
    let mut ground = HashMap::new();
    for &(index, middle) in &middles {
        if let Some(uv) = ground_below(level, coverage, &grid, middle) {
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
    coverage: &[coverage::Coverage],
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
        if primitive
            .material
            .is_some_and(|index| matches!(coverage[index], coverage::Coverage::Excluded(_)))
        {
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

/// The lightmap coordinates of the highest ground face straight below `middle`, a primitive's
/// middle at the height of its top, within [`GROUND_DEPTH`].
fn ground_below(
    level: &super::Level,
    coverage: &[coverage::Coverage],
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
        if let Some(material) = lit.material {
            let uv = [0, 1]
                .map(|k| wa * vertices[0].uv[k] + wb * vertices[1].uv[k] + wc * vertices[2].uv[k]);
            if !coverage[material].covered(uv) {
                continue;
            }
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
/// vertex's detail lane bits: its vertex data record's, else its lightmap coordinates in fixed point
/// ([`native::material::vertex_data::coordinate_bits`]), its own, else its primitive's `ground`
/// coordinates ([`ground_light`]), else `neutral`. A placed object's primitives, those from `placed`
/// on, take their coordinates with the first negated, which marks them for the surfaces' lighting
/// (`native::material::hybrid_lighting`) and keeps the fixed point's precision. Each
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
) -> Result<(Mesh, Vec<[u16; 2]>)> {
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
                        lightmap_coordinates(vertex, triangle.primitive, ground, neutral, placed)
                            .map(native::material::vertex_data::coordinate_bits);
                    if let Some(bits) = vertex_data
                        .and_then(|table| table.coordinates.get(&(triangle.primitive, corner)))
                    {
                        coordinates = *bits;
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
#[allow(clippy::too_many_arguments)]
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
    let split_areas = !areas.is_empty();
    let areas = selected_areas(zone_sets, areas)?;
    let compute_before = crate::compute::snapshot();
    let shaders_before = crate::tiger::shader::cache_snapshot();
    let mut timing = Timing::start();
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
    let loading = Loading::read(&cache, zone_sets, &areas)?;
    timing.lap("source");

    // Source geometry, collision and placed objects.
    report(&format!(
        "Reading Geometry and Lightmaps / {} BSPs",
        loading.tags.len()
    ));
    // With the stock decorator templates, decorators stay instances for Destiny's instanced
    // decorators and take no share of the areas' triangle budget.
    let mut level = super::read_decorated_areas(
        &cache,
        &mut pages,
        &loading.tags,
        &loading.groups,
        templates.decorators.is_some(),
    )?;
    timing.lap("geometry_and_lightmaps");
    let hidden = hide_undrawn(&cache, &mut pages, &mut level)?;
    timing.lap("hidden_materials");
    let polygons = world::read(&cache, &mut pages, &loading, &level, report)?;
    timing.lap("collision_read");
    let placed = objects::read(&cache, &mut pages, variant.as_ref(), zone_sets, &mission)?;
    timing.lap("objects");
    let joined = join_objects(
        &mut level,
        placed.materials,
        placed.props,
        placed.primitives,
        placed.sky,
    )?;
    let sky_order = SkyOrder::arrange(&cache, &level.materials, joined.sky, &loading);
    let area_spawns = read_spawns(&cache, variant.as_ref(), &areas, offset)?;
    let spawns = area_spawns[0].clone();
    let plan = plan::write(
        &level.primitives,
        &level.materials,
        offset,
        &area_spawns.concat(),
        output,
    )?;
    let mut native = Reader::new(packages, &output.join("native"), false)?;
    let mut analytical = super::lighting::materials(&cache, &mut level)?;
    // Surfaces with a dynamic environment map split by the cube of the cluster they stand in.
    let dynamic_cubes = cubemaps::dynamic(
        &cache,
        &mut level,
        super::loaded_bsps(&cache, zone_sets)?,
        &mut analytical,
    )?;
    let sun =
        Some(super::lighting::read(&cache, super::lighting::SKY)?).filter(|light| light.present());
    let donor = Donor::read(&mut native, templates)?;
    timing.lap("sky_order_spawns_and_templates");

    // Render and collision partitions.
    // Classify authored face pairs before subdivision changes their exact floating-point vertices.
    // Material variants are already assigned, so this uses the same keys as the emitted surfaces.
    let paired_faces = culling::paired(&level);
    // A triangle with another surface's corner on its edge is cut there, so the corner rounds to
    // one point in both.
    let t_junctions = junctions::split(&mut level.primitives)?;
    timing.lap("t_junctions");
    let (triangles, periods) = partition::triangles(&level)?;
    timing.lap("triangles");
    report(&format!(
        "Partitioning Geometry / {} Triangles",
        triangles.len()
    ));
    let uv_report = partition::uv_report(&level, &triangles, &periods);
    timing.lap("texture_coordinate_report");
    let (faces, face_areas) = world::faces(
        &polygons,
        &placed.faces,
        &placed.face_bsps,
        &loading,
        offset,
    );
    let play = PlayArea::new(&area_spawns);
    let renders = partition::renders(&level, triangles, &play, &loading, offset, &mut timing)?;
    let shapes = world::shapes(faces, face_areas, &play);
    timing.lap("collision_split");

    // Lighting, fog and materials.
    let used = used_materials(&level)?;
    let mut graph = Graph::new(output.to_path_buf(), Vec::new());
    // Nodes are only appended here, so their symbols can be indexed ([`Graph::index_symbols`]).
    graph.index_symbols();
    let neutral = level
        .lightmap
        .as_ref()
        .map_or([0.; 2], |atlas| atlas.neutral());
    let coverage = if level.lightmap.is_some() || templates.occlusion_probes.is_some() {
        coverage::read(&cache, &mut pages, &level)?
    } else {
        Vec::new()
    };
    let ground = if level.lightmap.is_some() {
        ground_light(&level, &coverage)
    } else {
        HashMap::new()
    };
    timing.lap("used_materials_and_ground_light");
    let camera = Camera::read(
        &cache,
        &mut graph,
        &mut native,
        &loading.tags,
        &level,
        templates,
        offset,
    )?;
    timing.lap("fog_and_radiance");
    report("Preparing Lighting and Occlusion Probes");
    let cluster_boxes = read_cluster_boxes(&cache, &loading.tags)?;
    let occlusion::Probes {
        probes: occlusion,
        open_light,
        open_hue,
        colors: light_colors,
        interior_hue,
        interior,
        report: occlusion_report,
    } = occlusion_probes(
        &cache,
        &level,
        &coverage,
        &area_spawns,
        &cluster_boxes,
        zone_sets,
        templates,
        offset,
    )?;
    timing.lap("occlusion_probes");
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
                camera.fog,
            )
        })
        .transpose()?;
    // A level with baked light asks for the native sun that matches its own, and a level without
    // an analytical sun asks for none.
    let native_sun = level.lightmap.as_ref().map(|atlas| match sun {
        Some(sun) => NativeSun::matching(sun.intensity, camera.radiance, atlas.level),
        None => NativeSun::matching([0.; 3], camera.radiance, atlas.level),
    });
    let sky_draws =
        templates.sky.is_some() && !sky_order.primitives.is_empty() && !level.primitives.is_empty();
    let lightmap = lightmap.map(|lightmap| {
        calibrate_lightmap(lightmap, analytical, offset, &camera, open_light, sky_draws)
    });
    timing.lap("lightmap_textures");
    let vertex_data = add_vertex_data(
        &mut native,
        &mut graph,
        &level,
        &ground,
        neutral,
        joined.placed_from,
    )?;
    timing.lap("vertex_data");
    report(&format!(
        "Converting Materials and Textures / {} Materials",
        used.len()
    ));
    let material_report = build_materials(
        &cache,
        &mut pages,
        &mut native,
        &mut graph,
        &used,
        lightmap.as_ref(),
        vertex_data.as_ref(),
        &paired_faces,
        report,
    )?;
    timing.lap("materials");

    // The pieces' static meshes and the world collision's meshes.
    let steps = pieces::Steps::new(&level, &renders.groups, offset);
    timing.lap("piece_steps");
    let workers = crate::cancellation::workers()?;
    let written = pieces::write(
        &mut native,
        &mut graph,
        &templates.statics,
        &donor,
        &level,
        &renders.groups,
        &steps,
        &used,
        offset,
        play.arena,
        &ground,
        neutral,
        joined.placed_from,
        vertex_data.as_ref(),
        workers,
        report,
    )?;
    let meshes = world::write(&mut graph, shapes, &donor, workers, report)?;
    timing.lap("render_and_collision");

    // The map's sky, sound, atmosphere, reflections, lights, props and decorators.
    report("Converting Sky Geometry");
    let (sky, sky_report) = add_sky(
        &cache,
        &mut pages,
        &mut native,
        &mut graph,
        templates,
        &level,
        &sky_order.primitives,
        offset,
        &camera,
        zone_sets,
    )?;
    // Opaque surfaces that left their fog to the screen fog sheet must have one.
    ensure!(
        !lightmap
            .as_ref()
            .is_some_and(|lightmap| lightmap.screen_fog)
            || sky_report["screen_fog"]["surface"].is_object(),
        "The level's opaque surfaces leave their fog to a screen fog sheet the sky did not draw"
    );
    // Native sun direction comes from the scenario sky's analytical light. Baked VMF lobes
    // describe a separate lighting term and do not establish the sun's direction.
    let light = sun.map(|sun| sun.vector);
    timing.lap("sky");
    report("Decoding and Converting Ambient Audio");
    let (ambience, ambience_report) = add_ambience(
        &cache,
        &mut pages,
        &loading,
        maps,
        map,
        &mut native,
        &mut graph,
        templates,
        offset,
    )?;
    timing.lap("ambience");
    report("Decoding Mission Audio");
    let mission_audio = export_mission_audio(&mission, templates, &mut pages, output)?;
    let (atmosphere_lookup, lookup_report) =
        clear_atmosphere_lookup(&mut native, &mut graph, templates, level.lightmap.is_some())?;
    timing.lap("mission_audio_and_atmosphere_lookup");
    let area_interiors = area_interiors(&loading, &cluster_boxes, &interior, offset);
    let clusters = clusters(&cluster_boxes, offset);
    let atmosphere = Atmosphere::add(
        &mut native,
        &mut graph,
        templates,
        sun,
        open_light,
        open_hue,
        interior_hue,
        camera.radiance,
    )?;
    timing.lap("occlusion_and_atmosphere");
    report("Converting Reflection Cubemaps");
    let (cubemaps, cubemaps_report) = reflection_cubemaps(
        &cache,
        &mut pages,
        &mut native,
        &mut graph,
        templates,
        &level,
        zone_sets,
        offset,
        camera.radiance,
        open_light,
        &light_colors,
    )?;
    timing.lap("cubemaps");
    report("Reading Lights");
    let (lights, lights_report) = read_lights(
        &cache,
        &mut pages,
        &mut native,
        &mut graph,
        &level,
        zone_sets,
        variant.as_ref(),
        offset,
        camera.radiance,
    )?;
    timing.lap("lights");
    report("Converting Posed Props");
    let (props, prop_materials) = props::build(
        &cache,
        &mut pages,
        &mut native,
        &mut graph,
        templates,
        &joined.props,
        &level,
        &coverage,
        lightmap.as_ref(),
        super::loaded_bsps(&cache, zone_sets)?,
        offset,
        report,
    )?;
    timing.lap("props");
    let decorated = add_decorators(
        &cache,
        &mut pages,
        &mut native,
        &mut graph,
        templates,
        &mut level,
        &loading,
        offset,
        open_light,
        report,
    )?;
    timing.lap("decorators");
    report("Binding Native Shaders");
    bind_shaders(&graph, output)?;
    timing.lap("bind_shaders");

    // Each area's rows, then the files beside the graph.
    ensure!(
        !written.pieces.is_empty(),
        "The level converted to no pieces"
    );
    let (area_rows, area_report) = area::rows(
        &mut native,
        &mut graph,
        templates,
        &donor,
        &areas,
        &loading,
        &written.pieces,
        &renders.areas,
        &meshes,
        &decorated,
        &props,
        &area_spawns,
        &ambience,
        &area_interiors,
        &polygons.open_seams,
    )?;
    let world = area_rows[0].world.clone();
    let statics = area_rows[0].statics.clone();
    timing.lap("areas");
    report("Verifying Source Files and Writing Map Reports");
    pages.verify_inputs()?;
    native.finish()?;
    let mission_execution = write_mission(output, &mission, controller)?;
    timing.lap("finish");
    let mut report = json!({
        "compute": crate::compute::snapshot().since(&compute_before),
        "shaders": crate::tiger::shader::cache_snapshot().since(&shaders_before),
        "timing": { "conversion_seconds": timing.started.elapsed().as_secs_f64(),
            "render_seconds": written.seconds, "collision_seconds": meshes.seconds, "workers": workers,
            "steps": timing.steps },
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
        "bsps": loading.tags.iter().map(|tag| tag.path.as_str()).collect::<Vec<_>>(),
        "pieces": written.pieces.len(),
        "render_parts": renders.groups.len(),
        "collision_parts": meshes.instances.len(),
        "vertices": written.vertices,
        "triangles": written.triangles,
        "structure_polygons": polygons.structure,
        "collision_polygons": polygons.polygons.len(),
        "collision_triangles": meshes.triangles,
        "world_collision_instances": meshes.instances.len(),
        "materials": material_report,
        "paired_surface_materials": paired_faces,
        "objects": placed.placed,
        "objects_hidden": placed.hidden,
        "t_junctions": t_junctions,
        "uv_materials": uv_report,
        "uv_bands": renders.bands,
        "objects_absent": placed.absent,
        "objects_untranslated_motion": placed.motion,
        "objects_untranslated_lifetimes": {
            "placements": placed.transient,
            "source": placed.lifetimes,
            "limit": "Placements marked Not Automatically are omitted from the static map. Their render geometry, collision and attached contributions require an explicit creation request. Script analysis describes paired creation and destruction but does not execute requests, folders, branches or runtime state. Explicit Forge variant placements retain their own placement contract."
        },
        "objects_source_poses": placed.poses,
        "objects_lighting": placed.lighting,
        "posed_entities": props,
        "prop_materials": prop_materials,
        "hidden_primitives": hidden.primitives,
        "hidden_materials": hidden.materials,
        "outside_instances": level.instances.iter().filter(|instance| instance.outside()).count(),
        "variant": variant.as_ref().map(|variant| json!({
            "title": variant.title,
            "map_id": variant.map_id,
            "objects": variant.placements.len(),
        })),
        "lightmap": level.lightmap.as_ref().map(|atlas| [atlas.width, atlas.height]),
        "lightmap_sampling": level.lightmap.as_ref().map(|atlas| atlas.sampling.report()),
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
        "camera": camera.report,
        "sky": sky_report,
        "sky_order": sky_order.order,
        "sky_blackened": sky_order.blackened,
        "ambience": ambience_report,
        "atmosphere": atmosphere.report,
        "atmosphere_lookup": lookup_report,
        "atmosphere_image": atmosphere.image_report,
        "fog": camera.fog_report,
        "occlusion": occlusion_report,
        "cubemaps": cubemaps_report,
        "lights": lights_report,
        "analytical_light_direction": light,
        "analytical_light_color": atmosphere.light_color,
        "analytical_sun": sun,
        "analytical_materials": lightmap.as_ref().map(|map| &map.analytical),
        "dynamic_environment_variants": dynamic_cubes,
    });
    report["open_seams"] = json!(polygons.open_seams[0]);
    report["objects_lighting"]["vertex_scale"] = json!(
        level
            .lightmap
            .as_ref()
            .map_or(1., |atlas| atlas.scale * atlas.level)
    );
    report["interiors"] = json!(area_rows[0].interiors);
    report["open_seam_surfaces"] = json!(polygons.open_seam_surfaces);
    report["sky_areas"] = json!(sky_order.areas);
    report["decorators_instanced"] = decorated.report;
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
        pieces: written.pieces,
        spawns,
        world,
        statics,
        sky,
        light,
        native_sun,
        native_exposure: camera.native_exposure,
        color_grade: camera.color_grade,
        ambience: area_rows[0].ambience.clone(),
        visibility: area_rows[0].visibility.clone(),
        decorators: area_rows[0].decorators.clone(),
        decorator_occlusion: decorated.occlusion,
        atmosphere: atmosphere.volume,
        interior_atmosphere: atmosphere.interior,
        atmosphere_lookup,
        atmosphere_image: atmosphere.image,
        occlusion,
        clusters,
        interiors: area_rows[0].interiors.clone(),
        cubemaps,
        lights,
        areas: if split_areas { area_rows } else { Vec::new() },
        plan,
        report,
    })
}

/// The areas a conversion converts: `areas`, each loading some of `zone_sets`, or one area
/// loading all of them when `areas` is empty.
fn selected_areas(zone_sets: &[usize], areas: &[Vec<usize>]) -> Result<Vec<Vec<usize>>> {
    ensure!(!zone_sets.is_empty(), "The map loads no zone set");
    ensure!(
        areas
            .iter()
            .all(|area| !area.is_empty() && area.iter().all(|set| zone_sets.contains(set))),
        "Each area loads some of the map's zone sets"
    );
    ensure!(areas.len() <= 64, "A map holds at most 64 connected areas");
    Ok(if areas.is_empty() {
        vec![zone_sets.to_vec()]
    } else {
        areas.to_vec()
    })
}

/// Each conversion step's seconds, for the report's timing.
struct Timing {
    started: Instant,
    /// When the previous step ended.
    last: Instant,
    steps: serde_json::Map<String, Value>,
}

impl Timing {
    fn start() -> Self {
        let started = Instant::now();
        Self {
            started,
            last: started,
            steps: serde_json::Map::new(),
        }
    }

    /// Records the seconds since the previous step as `step`'s.
    fn lap(&mut self, step: &str) {
        self.steps
            .insert(step.to_owned(), json!(self.last.elapsed().as_secs_f64()));
        self.last = Instant::now();
    }
}

/// The structure BSPs a conversion loads, and which of its areas load each.
struct Loading {
    /// The BSPs the zone sets load, in scenario order.
    tags: Vec<Tag>,
    /// Each area's BSPs, by scenario mask.
    loaded: Vec<u32>,
    /// Each of `tags` by scenario mask.
    bits: Vec<u32>,
    /// Each area's BSPs, by index in `tags`.
    groups: Vec<Vec<usize>>,
}

impl Loading {
    fn read(cache: &Cache, zone_sets: &[usize], areas: &[Vec<usize>]) -> Result<Self> {
        let tags = bsps(cache, zone_sets)?
            .into_iter()
            .map(|bsp| bsp.tag)
            .collect::<Vec<_>>();
        let loaded = areas
            .iter()
            .map(|sets| super::loaded_bsps(cache, sets))
            .collect::<Result<Vec<_>>>()?;
        let bits = tags
            .iter()
            .map(|tag| super::bsp_mask(cache, std::slice::from_ref(tag)))
            .collect::<Result<Vec<_>>>()?;
        let groups = loaded
            .iter()
            .map(|&loaded| {
                (0..tags.len())
                    .filter(|&bsp| bits[bsp] & loaded != 0)
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        Ok(Self {
            tags,
            loaded,
            bits,
            groups,
        })
    }

    /// How many areas the conversion converts.
    fn areas(&self) -> usize {
        self.loaded.len()
    }

    /// The areas that load what the structure BSPs `bsps` hold (a primitive's `bsps`), bit `n`
    /// for the `n`th area. What no BSP holds loads in every area.
    fn membership(&self, bsps: u32) -> u64 {
        self.loaded
            .iter()
            .enumerate()
            .filter(|&(_, &loaded)| bsps == 0 || bsps & loaded != 0)
            .fold(0, |mask, (area, _)| mask | 1 << area)
    }
}

/// The level's primitives and materials left out because they never draw ([`hide_undrawn`]).
struct Hidden {
    primitives: usize,
    /// The materials' tag paths.
    materials: Vec<String>,
}

/// Leaves out the level's primitives whose material never draws. A surface whose alpha test
/// discards every pixel draws nothing and casts no shadow. The structure's own collision still
/// holds any such surface's wall.
fn hide_undrawn(cache: &Cache, pages: &mut Pages, level: &mut super::Level) -> Result<Hidden> {
    let hidden = level
        .materials
        .iter()
        .map(|material| material.never_drawn(cache, pages))
        .collect::<Result<Vec<_>>>()?;
    let drawn_primitives = level.primitives.len();
    level.primitives.retain(|primitive| {
        !primitive
            .material
            .is_some_and(|material| hidden.get(material).copied().unwrap_or(false))
    });
    let primitives = drawn_primitives - level.primitives.len();
    let materials = level
        .materials
        .iter()
        .zip(&hidden)
        .filter(|&(_, &never)| never)
        .filter_map(|(material, _)| material.tag.as_ref().map(|tag| tag.path.clone()))
        .collect::<Vec<_>>();
    Ok(Hidden {
        primitives,
        materials,
    })
}

/// A sky primitive, about the origin, and each of its vertices' color: the light Reach baked into
/// the sky's vertex color stream.
type SkyPrimitive = (Primitive, Vec<[f32; 3]>);

/// The placed objects' geometry, with its materials among the level's ([`join_objects`]).
struct Joined {
    /// The first placed object's primitive among the level's. Placed objects' primitives follow
    /// the structure's, and the T-junction repair only adds vertices, so they keep these indices.
    placed_from: u32,
    /// Each scripted pose's primitives, which stay separate render entities.
    props: Vec<Vec<Primitive>>,
    /// The skies' primitives, which draw as the map's sky objects apart from the pieces.
    sky: Vec<SkyPrimitive>,
}

/// Joins the placed objects' `materials` to the level's and their `primitives` to its own, with
/// their materials, the scripted poses' `props` and the skies' `sky` renumbered to match.
fn join_objects(
    level: &mut super::Level,
    materials: Vec<Material>,
    props: Vec<Vec<Primitive>>,
    primitives: Vec<Primitive>,
    sky: Vec<SkyPrimitive>,
) -> Result<Joined> {
    let placed_from = u32::try_from(level.primitives.len())?;
    let base = level.materials.len();
    level.materials.extend(materials);
    let renumber = |mut primitive: Primitive| {
        primitive.material = primitive.material.map(|material| base + material);
        primitive
    };
    let props = props
        .into_iter()
        .map(|primitives| primitives.into_iter().map(renumber).collect())
        .collect::<Vec<Vec<_>>>();
    level
        .primitives
        .extend(primitives.into_iter().map(renumber));
    let sky = sky
        .into_iter()
        .map(|(primitive, colors)| (renumber(primitive), colors))
        .collect();
    Ok(Joined {
        placed_from,
        props,
        sky,
    })
}

/// The skies' surfaces in the order the map's sky objects draw them.
struct SkyOrder {
    /// The areas each sky surface loads in, which show whether the areas draw different skies,
    /// which the map's one set of sky objects cannot.
    areas: BTreeSet<u64>,
    /// Each surface's material, opacity, mean distance, sort layer and source, for the report.
    order: Vec<Value>,
    primitives: Vec<SkyPrimitive>,
    /// The materials of the black opaque surfaces left out.
    blackened: Vec<Option<String>>,
}

impl SkyOrder {
    /// Orders the skies' surfaces `sky`, whose materials are among `materials`.
    ///
    /// Reach draws a sky's opaque surfaces with depth before its blended layers, and the native
    /// sky stage draws in order without it. So opaque surfaces come first, the farthest first, and
    /// the blended layers follow in Reach's order. An opaque surface lit only by vertex light that
    /// is zero everywhere, with no self illumination, can only paint black. Where another opaque
    /// surface remains as the backdrop it is left out: Forge World's `simple\white` surface sits
    /// 8.5 km out, nearer than its 25 km blue dome, and drew a black patch over the dome. A sky
    /// with no other opaque surface, as Sword Base's, keeps it as its backdrop.
    fn arrange(
        cache: &Cache,
        materials: &[Material],
        sky: Vec<SkyPrimitive>,
        loading: &Loading,
    ) -> Self {
        let areas = sky
            .iter()
            .map(|(primitive, _)| loading.membership(primitive.bsps))
            .collect::<BTreeSet<_>>();
        let keyed = sky
            .into_iter()
            .map(|(primitive, colors)| {
                let material = primitive
                    .material
                    .and_then(|material| materials.get(material));
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
                            .and_then(|material| materials.get(material))
                            .and_then(|material| material.tag.as_ref())
                            .map(|tag| tag.path.clone()),
                    );
                    return None;
                }
                let layer = sky::sort_layer(
                    cache,
                    primitive
                        .material
                        .and_then(|material| materials.get(material)),
                );
                Some((opaque, distance, layer, primitive, colors))
            })
            .collect::<Vec<_>>();
        // Blended layers follow Reach's sort for a sky ([`sky::sort_layer`]): by their shaders'
        // sort layer, then in mesh and part order, which the stable sort keeps.
        keyed.sort_by(|a, b| {
            b.0.cmp(&a.0).then_with(|| {
                if a.0 {
                    b.1.total_cmp(&a.1)
                } else {
                    a.2.cmp(&b.2)
                }
            })
        });
        // Without depth between sky surfaces, the one behind draws first ([`sky::depth_order`]).
        let keyed = sky::depth_order(keyed, |(opaque, _, _, primitive, _)| (*opaque, primitive));
        let order = keyed
            .iter()
            .map(|(opaque, distance, layer, primitive, _)| {
                json!({
                    "material": primitive
                        .material
                        .and_then(|material| materials.get(material))
                        .and_then(|material| material.tag.as_ref())
                        .map(|tag| tag.path.as_str()),
                    "opaque": opaque,
                    "mean_distance": distance,
                    "sort_layer": layer,
                    "section": primitive.section,
                    "part": primitive.part,
                })
            })
            .collect::<Vec<_>>();
        let primitives = keyed
            .into_iter()
            .map(|(_, _, _, primitive, colors)| (primitive, colors))
            .collect::<Vec<_>>();
        Self {
            areas,
            order,
            primitives,
            blackened,
        }
    }
}

/// Each area's player spawn points, from its first zone set, moved by `offset`.
fn read_spawns(
    cache: &Cache,
    variant: Option<&variant::Variant>,
    areas: &[Vec<usize>],
    offset: [f32; 3],
) -> Result<Vec<Vec<Spawn>>> {
    areas
        .iter()
        .map(|sets| {
            Ok(spawn::read(cache, variant, sets[0])?
                .into_iter()
                .map(|spawn| Spawn {
                    position: std::array::from_fn(|k| spawn.position[k] + offset[k]),
                    ..spawn
                })
                .collect::<Vec<_>>())
        })
        .collect()
}

/// The play area: every area's spawn bounds, in the map's space, and [`PLAY_MARGIN`] past them.
struct PlayArea {
    boxes: Vec<([f32; 3], [f32; 3])>,
    /// The pieces' origin, the middle of the first area's spawn bounds ([`Piece::position`]).
    arena: [f32; 3],
}

impl PlayArea {
    fn new(area_spawns: &[Vec<Spawn>]) -> Self {
        let boxes = area_spawns
            .iter()
            .map(|spawns| bounds(spawns.iter().map(|spawn| spawn.position)))
            .collect::<Vec<_>>();
        let (spawn_low, spawn_high) = boxes[0];
        // Area spawn bounds have different fractional centers. An unsnapped entity origin shifts
        // every piece's quantization lattice, opening cracks where the same BSP spans two areas.
        // Metre-aligned origins also align the coarser power-of-two steps used by distant pieces.
        let arena = std::array::from_fn(|k| ((spawn_low[k] + spawn_high[k]) / 2.).round());
        Self { boxes, arena }
    }

    /// Whether `point`, in the map's space, moved by `offset` as the spawn points are, is in play.
    fn contains(&self, point: [f32; 3]) -> bool {
        self.boxes.iter().any(|(low, high)| {
            (0..3).all(|k| {
                point[k] >= low[k] - PLAY_MARGIN[k] && point[k] <= high[k] + PLAY_MARGIN[k]
            })
        })
    }
}

/// The donor's native records the pieces and the world collision copy ([`Templates`]).
struct Donor {
    /// The stock entity model, and the row of its first mesh, whose buffer headers the pieces'
    /// buffers copy.
    model: Arc<Payload>,
    row: usize,
    /// The stock world collision's static compound, which each area's compound copies.
    world_compound: Vec<u8>,
    /// The first of its mesh packfiles that is a bare compressed mesh, which each collision mesh
    /// copies.
    world_mesh_tag: u32,
    world_mesh: Vec<u8>,
}

impl Donor {
    fn read(native: &mut Reader, templates: &Templates) -> Result<Self> {
        let model = native
            .tag(templates.model, Some(MODEL_CLASS))
            .context("Piece model template")?;
        let row = *model
            .array(16, 136, Some(MESH_CLASS))?
            .first()
            .context("Template model has no mesh")?;
        let world_compound = native
            .tag(templates.world_compound, None)
            .context("World compound template")?
            .0
            .clone();
        let mut world_mesh = None;
        for &tag in &templates.world_meshes {
            let bytes = native
                .tag(tag, None)
                .with_context(|| format!("Piece buffer {tag:08X}"))?
                .0
                .clone();
            if crate::tiger::physics::rooted(&bytes, "hkpBvCompressedMeshShape") {
                world_mesh = Some((tag, bytes));
                break;
            }
        }
        let (world_mesh_tag, world_mesh) = world_mesh
            .context("The donor's world collision has no bare compressed mesh packfile")?;
        Ok(Self {
            model,
            row,
            world_compound,
            world_mesh_tag,
            world_mesh,
        })
    }
}

/// The materials the level's primitives draw, by key, a default material for primitives without
/// one.
fn used_materials(level: &super::Level) -> Result<BTreeMap<u32, Material>> {
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
    Ok(used)
}

/// The level's fog and camera, as its surfaces and Destiny's exposure take them.
struct Camera {
    /// The level's own fog, which the surfaces draw in place of Destiny's.
    fog: Option<native::material::Fog>,
    fog_report: Value,
    radiance: f32,
    report: Value,
    /// The fog's exposure scales: Reach's automatic exposure range, or its manual target at both.
    fog_exposure: [f32; 2],
    color_grade: Option<String>,
    native_exposure: Option<NativeExposure>,
    /// Reach's preferred self illumination exposure and its change ([`sky::self_illumination`]).
    self_illumination: [f32; 2],
}

impl Camera {
    /// Reads the fog of the structure BSPs `tags`, lifted by `offset`, and the scenario's camera,
    /// adding the display grade to `graph` when the level has baked light.
    fn read(
        cache: &Cache,
        graph: &mut Graph,
        native: &mut Reader,
        tags: &[Tag],
        level: &super::Level,
        templates: &Templates,
        offset: [f32; 3],
    ) -> Result<Self> {
        let (fog, fog_report) = fog::read(cache, tags, offset[2], templates.fog)?;
        let (radiance, mut report) =
            sky::radiance(cache, level.lightmap.as_ref().map(|atlas| atlas.level))?;
        let (automatic, target, range) = sky::exposure(cache)?;
        let fog_exposure = if automatic {
            range.map(f32::exp2)
        } else {
            [target.clamp(range[0], range[1]).exp2(); 2]
        };
        let color_grade = if level.lightmap.is_some() {
            let (symbol, grade_report) = grade::add(graph, native, cache)?;
            report["display_grade"] = grade_report;
            symbol
        } else {
            None
        };
        // A level with baked light holds Shadowkeep's automatic exposure within Reach's range.
        let native_exposure = level
            .lightmap
            .as_ref()
            .map(|atlas| NativeExposure::matching(automatic, target, range, radiance, atlas.level))
            .transpose()?;
        report["native_exposure"] = json!(native_exposure.map(|exposure| json!({
            "minimum_stops": exposure.minimum,
            "maximum_stops": exposure.maximum,
            "stock_stops": NativeExposure::STOCK,
            "channels": [5, 6],
            "method": "Manual exposure locks both bounds to the mapped target. Automatic exposure keeps the stock minimum and maps the source maximum. Both use the existing native range.",
        })));
        let [preferred_illumination, illumination_change] = sky::self_illumination(cache)?;
        report["self_illumination_exposure"] = json!(level.lightmap.as_ref().map(|atlas| {
            let offset =
                (atlas.level / (native::material::REFERENCE_ILLUMINATION * radiance)).log2();
            let [scale, power] = native::material::self_illumination_exposure(
                preferred_illumination,
                illumination_change,
                offset,
            );
            json!({
                "preferred_stops": preferred_illumination,
                "change": illumination_change,
                "offset_stops": offset,
                "scale": scale,
                "power": power,
            })
        }));
        Ok(Self {
            fog,
            fog_report,
            radiance,
            report,
            fog_exposure,
            color_grade,
            native_exposure,
            self_illumination: [preferred_illumination, illumination_change],
        })
    }
}

/// A structure cluster's box, with its BSP's index in the conversion's BSPs.
type ClusterBox = (usize, ([f32; 3], [f32; 3]));

/// Each structure cluster's box, leaving out each BSP's outside cluster, with its BSP's index in
/// `tags`.
fn read_cluster_boxes(cache: &Cache, tags: &[Tag]) -> Result<Vec<ClusterBox>> {
    Ok(tags
        .iter()
        .enumerate()
        .map(|(bsp, tag)| {
            Ok(super::interior_boxes(cache, std::slice::from_ref(tag))?
                .into_iter()
                .map(move |cluster| (bsp, cluster)))
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect())
}

/// The level's occlusion probes, as many as the donor's occlusion record takes, about every
/// area's spawn points and the squads' points, in the structure clusters `cluster_boxes`.
#[allow(clippy::too_many_arguments)]
fn occlusion_probes(
    cache: &Cache,
    level: &super::Level,
    coverage: &[coverage::Coverage],
    area_spawns: &[Vec<Spawn>],
    cluster_boxes: &[ClusterBox],
    zone_sets: &[usize],
    templates: &Templates,
    offset: [f32; 3],
) -> Result<occlusion::Probes> {
    Ok(match templates.occlusion_probes {
        Some(count) => occlusion::probes(
            level,
            coverage,
            &area_spawns
                .iter()
                .flatten()
                .map(|spawn| sub(spawn.position, offset))
                .collect::<Vec<_>>(),
            &super::squads::read_squads(cache, [0.; 3], super::loaded_bsps(cache, zone_sets)?)?
                .into_iter()
                .flat_map(|squad| squad.points)
                .map(|point| point.spawn.position)
                .collect::<Vec<_>>(),
            &cluster_boxes
                .iter()
                .map(|&(_, cluster)| cluster)
                .collect::<Vec<_>>(),
            count,
            offset,
        )?,
        None => occlusion::Probes::none(json!({"reason": "The donor places no occlusion probes"})),
    })
}

/// The lightmap's calibration for the level's surfaces: its analytical materials, the level's
/// origin `offset`, the camera's radiance and fog exposure, the open probes' light as the
/// reference, and self illumination at the level's exposure offset. Opaque surfaces leave their
/// fog to the sky's screen fog sheet when the level has fog and `sky_draws`.
fn calibrate_lightmap(
    mut lightmap: native::material::Lightmap,
    analytical: BTreeMap<u32, super::lighting::Analytical>,
    offset: [f32; 3],
    camera: &Camera,
    open_light: Option<f32>,
    sky_draws: bool,
) -> native::material::Lightmap {
    lightmap.analytical = analytical;
    lightmap.origin = offset;
    lightmap.radiance = camera.radiance;
    lightmap.fog_exposure = Some(camera.fog_exposure);
    lightmap.reference = open_light.unwrap_or(1.);
    // Self illumination keeps Reach's own exposure against the frame's, at the level's exposure
    // offset ([`NativeExposure`]).
    let exposure_offset =
        (lightmap.level / (native::material::REFERENCE_ILLUMINATION * camera.radiance)).log2();
    if exposure_offset.is_finite() {
        let [preferred, change] = camera.self_illumination;
        lightmap.self_illumination =
            native::material::self_illumination_exposure(preferred, change, exposure_offset);
    }
    // The sky draws the screen fog sheet over the level when it draws at all, and the
    // opaque surfaces then leave their fog to it ([`sky::add`]).
    lightmap.screen_fog = lightmap.fog.is_some() && sky_draws;
    lightmap
}

/// The vertex data texture of the level's primitives ([`native::material::vertex_data::add`]),
/// whose lightmap coordinates fall back to `ground` and `neutral` ([`lightmap_coordinates`]).
/// Water that cannot read the scene depth fades by the opaque ground below it.
fn add_vertex_data(
    native: &mut Reader,
    graph: &mut Graph,
    level: &super::Level,
    ground: &HashMap<u32, [f32; 2]>,
    neutral: [f32; 2],
    placed_from: u32,
) -> Result<Option<native::material::vertex_data::Table>> {
    let water_depths = super::water::depths(&level.primitives, |primitive| {
        primitive
            .material
            .and_then(|material| level.materials.get(material))
            .is_some_and(|material| {
                material.alpha_mode() == "OPAQUE" && !material.decal() && !material.water()
            })
    });
    native::material::vertex_data::add(
        native,
        graph,
        &level.primitives,
        level
            .lightmap
            .as_ref()
            .map_or(1., |atlas| atlas.scale * atlas.level),
        |primitive, vertex| lightmap_coordinates(vertex, primitive, ground, neutral, placed_from),
        &water_depths,
    )
}

/// Converts the materials `used` for the pieces' static meshes, with the level's `lightmap` and
/// `vertex_data`.
#[allow(clippy::too_many_arguments)]
fn build_materials(
    cache: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    graph: &mut Graph,
    used: &BTreeMap<u32, Material>,
    lightmap: Option<&native::material::Lightmap>,
    vertex_data: Option<&native::material::vertex_data::Table>,
    paired_faces: &BTreeSet<u32>,
    report: &mut dyn FnMut(&str),
) -> Result<Vec<Value>> {
    native::material::build_with_progress(
        cache,
        pages,
        native,
        graph,
        used,
        native::material::Binding {
            palette: &[0],
            rigid: true,
            objects: &BTreeMap::new(),
            overrides: &BTreeMap::new(),
            lenses: &BTreeMap::new(),
            paint: &BTreeMap::new(),
            lightmap,
            vertex_data: vertex_data.map(|table| table.symbol.as_str()),
            // Every level surface draws on the pieces' static meshes (`statics`).
            statics: true,
            paired_faces,
            prefix: "",
            prepass: false,
        },
        report,
    )
}

/// The level's sky objects, copying the donor's sky when it places one.
#[allow(clippy::too_many_arguments)]
fn add_sky(
    cache: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    graph: &mut Graph,
    templates: &Templates,
    level: &super::Level,
    primitives: &[SkyPrimitive],
    offset: [f32; 3],
    camera: &Camera,
    zone_sets: &[usize],
) -> Result<(Vec<sky::Object>, Value)> {
    Ok(match &templates.sky {
        Some(sky_templates) => sky::add(
            cache,
            pages,
            native,
            graph,
            sky_templates,
            &level.materials,
            primitives,
            offset,
            level.lightmap.as_ref().map(|atlas| atlas.level),
            // The sky takes the level's fog where the level's surfaces draw it in Destiny's place.
            camera.fog.filter(|_| level.lightmap.is_some()),
            super::loaded_bsps(cache, zone_sets)?,
            // So do the patchy fog's sheets, over the level's pieces.
            camera.fog_report["tag"]
                .as_str()
                .filter(|_| level.lightmap.is_some()),
            (!level.primitives.is_empty()).then(|| {
                bounds(
                    level
                        .primitives
                        .iter()
                        .flat_map(|primitive| primitive.vertices.iter().map(|v| v.position)),
                )
            }),
        )?,
        None => (
            Vec::new(),
            json!({"objects": 0, "primitives": primitives.len(), "reason": "The donor places no sky objects to copy"}),
        ),
    })
}

/// Each area's ambience, the sound point table placing its structure BSPs' sounds, when the map
/// brings the level's ambience.
#[allow(clippy::too_many_arguments)]
fn add_ambience(
    cache: &Cache,
    pages: &mut Pages,
    loading: &Loading,
    maps: &Path,
    map: &str,
    native: &mut Reader,
    graph: &mut Graph,
    templates: &Templates,
    offset: [f32; 3],
) -> Result<(Vec<Option<String>>, Value)> {
    Ok(match &templates.ambience {
        Some(ambience_templates) => super::ambience::add(
            cache,
            pages,
            &loading.tags,
            maps,
            map,
            native,
            graph,
            ambience_templates,
            offset,
            &loading.loaded,
        )?,
        None => (
            vec![None; loading.areas()],
            json!({"points": 0, "reason": "No sound point table or no audio decoder"}),
        ),
    })
}

/// The mission's recordings, retained beside the map graph when the templates ask for them.
fn export_mission_audio(
    mission: &super::mission::Program,
    templates: &Templates,
    pages: &mut Pages,
    output: &Path,
) -> Result<Value> {
    Ok(match &templates.mission_audio {
        Some(options) => {
            super::mission::audio::export(mission, options, pages, &output.join("audio"))?
        }
        None => json!({"status":"undecoded","reason":"No mission audio decoder configured",
            "playback":"unsupported"}),
    })
}

/// The black distance lookup the map's atmosphere names in place of the donor's, when the donor
/// names one and the level is `baked`. Lightmapped surfaces draw the level's fog, or none where
/// Reach drew none, so Destiny's fog gives way on the map.
fn clear_atmosphere_lookup(
    native: &mut Reader,
    graph: &mut Graph,
    templates: &Templates,
    baked: bool,
) -> Result<(Option<String>, Value)> {
    Ok(match (templates.atmosphere_lookup, baked) {
        (Some(lookup), true) => {
            let (symbol, report) = atmosphere::clear_lookup(native, graph, lookup)?;
            (Some(symbol), report)
        }
        _ => (
            None,
            json!({"reason": "The donor names no atmosphere lookup or the level has no baked light"}),
        ),
    })
}

/// Each area's interiors, from the clusters of the BSPs it loads, where `interior` counts each
/// cluster's interior probes.
fn area_interiors(
    loading: &Loading,
    cluster_boxes: &[ClusterBox],
    interior: &[Option<usize>],
    offset: [f32; 3],
) -> Vec<Vec<Interior>> {
    loading
        .groups
        .iter()
        .map(|group| {
            let interiors = cluster_boxes
                .iter()
                .zip(interior)
                .filter(|((bsp, _), _)| group.contains(bsp))
                .filter_map(|(&(_, cluster), &probes)| Some((cluster, probes?)))
                .collect::<Vec<_>>();
            occlusion::interiors(&interiors, offset)
        })
        .collect()
}

/// The structure clusters' boxes in the map's coordinates ([`Converted::clusters`]).
fn clusters(cluster_boxes: &[ClusterBox], offset: [f32; 3]) -> Vec<super::streaming::Bounds> {
    cluster_boxes
        .iter()
        .map(|&(_, (low, high))| super::streaming::Bounds {
            minimum: std::array::from_fn(|k| low[k] + offset[k]),
            maximum: std::array::from_fn(|k| high[k] + offset[k]),
        })
        .collect()
}

/// The atmosphere volumes and open image the map's atmosphere names in place of the donor's.
struct Atmosphere {
    volume: Option<String>,
    /// The second volume, in the hue of the level's interiors ([`Converted::interior_atmosphere`]).
    interior: Option<String>,
    report: Value,
    image: Option<String>,
    image_report: Value,
    /// The analytical sun's hue, over its largest channel.
    light_color: Option<[f32; 3]>,
}

impl Atmosphere {
    /// The atmosphere takes the hue of the open probes' floor light, the lightmap Reach lit an
    /// object on open ground by, or the sun's where no probe stands on a lightmapped floor, and
    /// the open probes' light as the sky's brightness.
    #[allow(clippy::too_many_arguments)]
    fn add(
        native: &mut Reader,
        graph: &mut Graph,
        templates: &Templates,
        sun: Option<super::lighting::Analytical>,
        open_light: Option<f32>,
        open_hue: Option<[f32; 3]>,
        interior_hue: Option<[f32; 3]>,
        radiance: f32,
    ) -> Result<Self> {
        let light_color = sun.map(|sun| {
            let peak = sun.intensity.into_iter().fold(0f32, f32::max);
            sun.intensity.map(|v| v / peak)
        });
        let (volume, interior, report) = match (templates.atmosphere, open_hue.or(light_color)) {
            (Some(volume), Some(color)) => {
                let sky = open_light.map(|light| atmosphere::diffuse_luminance(light, radiance));
                let (symbol, mut report) =
                    atmosphere::add(native, graph, volume, color, sky, "atmosphere-volume")?;
                report["hue_source"] = json!(if open_hue.is_some() {
                    "open_floor_light"
                } else {
                    "analytical_sun"
                });
                report["sun_hue"] = json!(light_color);
                // A level with interiors gives them a second volume at the same brightness in
                // their floors' hue, so the probes' visibility keeps its meaning indoors.
                let interior = match (open_hue, interior_hue) {
                    (Some(_), Some(hue)) => {
                        let (symbol, interior_report) = atmosphere::add(
                            native,
                            graph,
                            volume,
                            hue,
                            sky,
                            "atmosphere-interior-volume",
                        )?;
                        report["interior"] = interior_report;
                        Some(symbol)
                    }
                    _ => None,
                };
                (Some(symbol), interior, report)
            }
            _ => (
                None,
                None,
                json!({"reason": "The donor names no atmosphere volume or the level has no baked light"}),
            ),
        };
        let (image, image_report) = match (templates.atmosphere_image, &volume) {
            (Some(image), Some(_)) => {
                let (symbol, report) = atmosphere::open_image(native, graph, image)?;
                (Some(symbol), report)
            }
            _ => (
                None,
                json!({"reason": "The donor names no atmosphere image or the map keeps the donor's atmosphere"}),
            ),
        };
        Ok(Self {
            volume,
            interior,
            report,
            image,
            image_report,
            light_color,
        })
    }
}

/// The level's reflection cubemap volumes, when the donor places volumes and the level has baked
/// light, calibrated by the open probes' light and the floor colors `light_colors`.
#[allow(clippy::too_many_arguments)]
fn reflection_cubemaps(
    cache: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    graph: &mut Graph,
    templates: &Templates,
    level: &super::Level,
    zone_sets: &[usize],
    offset: [f32; 3],
    radiance: f32,
    open_light: Option<f32>,
    light_colors: &[([f32; 3], [f32; 3])],
) -> Result<(Vec<CubemapVolume>, Value)> {
    if templates.cubemaps && level.lightmap.is_some() {
        cubemaps::volumes(
            cache,
            pages,
            native,
            graph,
            super::loaded_bsps(cache, zone_sets)?,
            offset,
            radiance,
            open_light
                .context("Native reflection volumes require a calibrated ambient probe set")?,
            light_colors,
        )
    } else {
        Ok((
            Vec::new(),
            json!({"reason": "The donor places no cubemap volumes or the level has no baked light"}),
        ))
    }
}

/// The level's lights, when it has baked light.
#[allow(clippy::too_many_arguments)]
fn read_lights(
    cache: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    graph: &mut Graph,
    level: &super::Level,
    zone_sets: &[usize],
    variant: Option<&variant::Variant>,
    offset: [f32; 3],
    radiance: f32,
) -> Result<(Vec<lights::MapLight>, Value)> {
    match level.lightmap.as_ref() {
        Some(atlas) => lights::read(
            cache,
            pages,
            native,
            graph,
            super::loaded_bsps(cache, zone_sets)?,
            variant,
            offset,
            radiance,
            atlas.level,
        ),
        None => Ok((
            Vec::new(),
            json!({"reason": "The level has no baked light"}),
        )),
    }
}

/// The level's decorators as Destiny's instanced decorators, with each area's records, when the
/// donor's decorator templates are given and the level kept its decorators as instances. Without
/// them the decorators already draw as copies among the pieces.
#[allow(clippy::too_many_arguments)]
fn add_decorators(
    cache: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    graph: &mut Graph,
    templates: &Templates,
    level: &mut super::Level,
    loading: &Loading,
    offset: [f32; 3],
    open_light: Option<f32>,
    report: &mut dyn FnMut(&str),
) -> Result<decorators::Added> {
    Ok(match (&templates.decorators, level.instanced.take()) {
        (Some(decorator_templates), Some(instanced)) => {
            report("Converting Instanced Decorators");
            decorators::add(
                cache,
                pages,
                native,
                graph,
                decorator_templates,
                instanced,
                &|bsps: u32| loading.membership(bsps),
                offset,
                loading.areas(),
                open_light,
                level.lightmap.as_ref().map_or(1., |atlas| atlas.level),
            )?
        }
        _ => decorators::Added {
            areas: vec![(Vec::new(), Vec::new()); loading.areas()],
            report: json!({"mode": "static"}),
            occlusion: None,
        },
    })
}

/// Binds the graph's native shaders and writes their payloads beside the graph.
fn bind_shaders(graph: &Graph, output: &Path) -> Result<()> {
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
    Ok(())
}

/// Writes the mission beside the map graph, with its compiled controller when one was selected,
/// and returns the controller's report.
fn write_mission(
    output: &Path,
    mission: &super::mission::Program,
    controller: Option<super::mission::execution::Controller>,
) -> Result<Value> {
    fs::write(
        output.join("mission.json"),
        serde_json::to_vec_pretty(mission)?,
    )?;
    Ok(match controller {
        Some(controller) => {
            fs::write(output.join("controller.lua"), controller.source)?;
            fs::write(
                output.join("execution.json"),
                serde_json::to_vec_pretty(&controller.report)?,
            )?;
            serde_json::to_value(controller.report)?
        }
        None => json!({"execution":"not_selected"}),
    })
}

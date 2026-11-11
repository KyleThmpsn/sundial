//! A map's own map root chain. The map root names one bubble parent, which names a bubble, whose
//! map containers list placement tables. The map's copies keep the donor's gameplay records, such
//! as volumes and its atmosphere, and drop its geometry: a donor container's tables that
//! place only static geometry, terrain, water or decorators give way to an empty table in a copy
//! of that container, and so do its precomputed visibility, which describes that geometry, its
//! barriers' collision, and the lights, lens flares, cubemaps and decals that light and mark it.
//! Sound emitters and the donor's entities that draw, collide or play an effect move far below
//! the map in copies of their tables, and the donor's spawn set gives way to one holding the map's
//! own spawn points. The donor's world collision gives way to the map's own in a copy of its table
//! and record, and its sky object collection to one holding the map's own sky. A donor whose
//! bubble places no sky, as a mission's may not, has the map's own container place a copy of a
//! Crucible world's sky table instead. Its sun draws no
//! sun disc or lens flare and, given the map's baked light direction, holds that direction in
//! copies of its sun record and direction track. One container of
//! the map's own places each converted piece's entity, copied from the donor's first prop: an
//! entity-only placement whose entity has a render model and a fixed physical body in the world's
//! collision layer. The same container places the map's ambience using a native sound point
//! table, preferring the donor's and finding a compatible stock table when it has none. Its
//! static group's table also places the map's decorator records, each row a copy of a stock
//! decorator row ([`decorators`]).
//! The map root also names a bubble layout that lists the
//! bubble by name, and its copy and the bubble parent's name the map's bubble.
use std::path::Path;

use parhelion_import::halo_reach::level::spawn::Spawn;
use sundial::package_authoring::PackageManager;
use tiger_pkg::TagHash;

use crate::{
    AuthoringResult, NewTagSpec, NewTagStorageMode,
    error::invalid,
    shared_tag_dependency_index::native::{Companions, native_companion},
    tag_payload::{
        append_native_array, array_at, read_i64, read_tag, read_u16, read_u32, read_u64,
        synchronize_payload_size, write_relative_pointer, write_u32,
    },
};

use super::world::{BubbleName, Owner, renamed_counted};

mod cubemaps;
mod environment;
mod lights;
pub(super) use cubemaps::{Cubemap, Grid as CubemapGrid};
pub(super) use environment::Environment;
pub(super) use lights::{Cone as LightCone, Light};

const MAP_ROOT_CLASS: u32 = 0x8080_91DE;
/// The map root names its bubble layout here, whose counted name is the world's.
const ROOT_LAYOUT: usize = 0x08;
const LAYOUT_CLASS: u32 = 0x8080_9962;
const LAYOUT_NAME: usize = 0x08;
const BUBBLE_PARENT_CLASS: u32 = 0x8080_7DAE;
const BUBBLE_CLASS: u32 = 0x8080_91E0;
const CONTAINER_CLASS: u32 = 0x8080_8A54;
const TABLE_CLASS: u32 = 0x8080_99D6;
const TABLE_ROW_CLASS: u32 = 0x8080_99D8;
const ENTITY_CLASS: u32 = 0x8080_9C0F;
const RENDER_CLASS: u32 = 0x8080_72BD;
const BODY_CLASS: u32 = 0x8080_8A0C;
/// An effect flow component, the particle effects an entity plays.
const EFFECT_CLASS: u32 = 0x8080_84E9;
/// Entity components that light the donor's scenery, naming shadowing-light records
/// (`80807140`) and lens-flare records (`80806F68`).
const LIGHT_CLASS: u32 = 0x8080_7134;
const FLARE_CLASS: u32 = 0x8080_6CBE;
/// An entity's components, 12-byte rows naming each owner.
const ENTITY_COMPONENTS: usize = 0x10;
const COMPONENT_SIZE: usize = 12;
/// An owner's self-relative pointer to its component resource.
const OWNER_RESOURCE: usize = 0x18;
/// The render model in a render resource.
const RENDER_MODEL: usize = 0x1DC;
/// A map root's bubble parents, a bubble's map containers: 16-byte tag rows.
const ROOT_PARENTS: usize = 0x10;
const BUBBLE_CONTAINERS: usize = 0x08;
const TAG_ROW_SIZE: usize = 0x10;
/// A tag row that names a tag directly rather than by 64-bit hash.
const DIRECT_TAG_ROW: u32 = 1;
const PARENT_BUBBLE: usize = 0x08;
/// A bubble parent's bounds records (`8080964E`), 16-byte tag rows. A record is three least and
/// greatest corner pairs, `float4`s with w 1. `trophy_hall`'s parent lists two, the empty
/// `80FEB3E9`, whose corners are `FLT_MAX` inverted, and `8151A4CB`, numbered beside its main
/// container `8151A4CA`. What the client bounds with each box was not traced.
const PARENT_BOUNDS: usize = 0x40;
const BOUNDS_CLASS: u32 = 0x8080_964E;
/// How far the map's own bounds reach past its pieces, in meters: the first and third boxes, and
/// the second, which every surveyed record holds wider. The Other Side's main record holds
/// (-580, -94, -1643) to (168, 1421, 61), (-1600, -269, -2514) to (442, 2343, 101) and
/// (-656, -99, -1640) to (163, 1458, 101).
const BOUNDS_MARGIN: f32 = 16.;
const WIDE_BOUNDS_MARGIN: f32 = 256.;
/// A container's placement tables, by `u32` tag.
const CONTAINER_TABLES: usize = 0x28;
/// A placement table's rows: entity, rotation, translation, identity and resource pointer.
const TABLE_ROWS: usize = 0x08;
const ROW_SIZE: usize = 0x90;
const ROW_ROTATION: usize = 0x10;
const ROW_TRANSLATION: usize = 0x20;
const ROW_SCALE: usize = 0x2C;
const ROW_RESOURCE: usize = 0x78;
/// A row's 64-bit placement identity, which spawn rules name their points by.
const ROW_IDENTITY: usize = 0x70;
/// Both pointer fields declared by the native `808099D8` layout. The second is often null.
const ROW_POINTERS: [usize; 2] = [ROW_RESOURCE, 0x80];
/// Entries the chain takes before any filtered container: four owners with their companions,
/// the placement table, then the bubble layout.
const FIXED_ENTRIES: usize = 10;
/// Placement resource classes of the donor's visible geometry: static instance groups, terrain,
/// water and decorators.
const GEOMETRY: [u32; 4] = [0x8080_71B3, 0x8080_714B, 0x8080_6DE0, 0x8080_71AD];
/// The placement resource class of the donor's precomputed visibility: a record naming raw blobs
/// that begin like Umbra occlusion tomes, over the donor's bounds. It describes the donor's
/// geometry, so it gives way to an empty table as that geometry does.
const VISIBILITY: u32 = 0x8080_7246;
/// Placement resource classes of the donor's barriers: records (`80808BB4` and `80806FAF`) naming
/// a packfile of convex hulls in a static compound, outside its world collision. Distant Shore's
/// players met them as walls that pushed them back. They give way to an empty table too.
const BARRIERS: [u32; 2] = [0x8080_89CD, 0x8080_6FAD];
/// Placement resource classes of the donor's lighting and surface detail, which belong to its own
/// geometry: light collections (`8080713A` records), shadowing lights (`80807140`), lens flares
/// (`80806F68`), cubemap collections (`80806B8C`), decal collections (`80806E68`) and road decals
/// (`80806DF3`). They light the donor's surfaces, reflect its scenery and mark its ground. A table
/// of the donor's cubemap volumes ([`CUBEMAP_VOLUME`]) gives way to the map's own
/// ([`cubemaps::table`]). So does a volume over the donor's play area (`80808CB6` records): The Dead Cliffs' names
/// two grid records (`80808CB8`, `80808CBA`) and the box from (-132.5, -100.5, -4.5) to
/// (100.5, 68.5, 44), which describe its own arena. With it kept, Paradiso's held weapon drew with a
/// strong blue cast, and without it in natural colors. Distant Shore places none. So does the
/// resource list beside the donor's ambient occlusion (`80806F8D`, rows of textures, cube
/// textures, models and materials per static instance). With Distant Shore's kept, the held weapon
/// drew a bright teal over Hang 'Em High, and without it a darker, faintly green metal. A table of
/// them gives way to an empty table too, and in a table with other kinds their rows move far below
/// the map. The donor's static ambient occlusion and probes (`80807268`, rows of a position and
/// monochrome occlusion coefficients) stay for the objects Destiny lights. With them dropped, the
/// held weapon and players drew nearly black wherever the sun did not reach them, indoors and in
/// shade alike.
const DETAIL: [u32; 8] = [
    0x8080_6F5A,
    0x8080_7133,
    0x8080_6CBF,
    0x8080_6B87,
    0x8080_6E62,
    0x8080_6DF1,
    0x8080_8C76,
    0x8080_6F78,
];
/// A placement resource for one cubemap volume, kept inline: a box whose players, weapons and other
/// objects Destiny lights take their reflections from its cube texture. In a table with other kinds
/// the donor's rows move far below the map with the detail rows.
const CUBEMAP_VOLUME: u32 = 0x8080_6B7F;
/// Placement resource classes of the donor's sound emitters, points and paths, both naming
/// `80809802` records. Their rows move far below the map.
const SOUND_POINT: u32 = 0x8080_6B5B;
const LEFTOVERS: [u32; 2] = [SOUND_POINT, 0x8080_6B59];
/// A placement resource that places sky objects, naming its sky object collection at `+0x10`. A
/// collection keeps its objects at `+0x08`, each object's bounds at `+0x18`, their indices at
/// `+0x28` and the bounds of all at `+0x40` and `+0x50`.
const SKY: u32 = 0x8080_6F91;
const SKY_COLLECTION: usize = 0x10;
const SKY_COLLECTION_CLASS: u32 = 0x8080_6F95;
const SKY_OBJECTS: usize = 0x08;
const SKY_BOUNDS: usize = 0x18;
const SKY_INDICES: usize = 0x28;
const SKY_EXTENT: usize = 0x40;
/// The collection's fixed fields end where its first array begins.
const SKY_FIXED: usize = 0x60;
/// A sky object: its transform, world bounds, model reference, draw order and fixed fields.
const SKY_OBJECT_CLASS: u32 = 0x8080_6F97;
const SKY_OBJECT_SIZE: usize = 0x80;
const SKY_OBJECT_BOUNDS: usize = 0x40;
const SKY_OBJECT_REFERENCE: usize = 0x60;
const SKY_OBJECT_ORDER: usize = 0x64;
const SKY_BOUNDS_CLASS: u32 = 0x8080_9673;
const SKY_BOUNDS_SIZE: usize = 0x30;
const SKY_INDEX_CLASS: u32 = 0x8080_0007;
/// A sky object's model reference names its entity model at `+0x08`.
const SKY_REFERENCE_CLASS: u32 = 0x8080_6F9B;
const SKY_REFERENCE_MODEL: usize = 0x08;
/// A placement resource for the sun. It names its sun record at `+0x10`, a record of two sun
/// materials at `+0x1C` and a lens flare at `+0x20`, `FFFFFFFF` for none. A sun record keeps four
/// direction tracks at `+0x10`, and a track 1,800 unit directions toward the light, z up, in an
/// array whose descriptor is at `+0x30`.
const SUN: u32 = 0x8080_6F61;
const SUN_RECORD: usize = 0x10;
const SUN_MATERIALS: usize = 0x1C;
const SUN_FLARE: usize = 0x20;
/// The sun's float at `+0x14`: 1,800 in Distant Shore's and `mission_proton`'s, 7,200 in Midtown's
/// and The Dead Cliffs'. Every map's sun takes 1,800. With it, Breakneck's and Nightfall's
/// `GlobalLighting+0x10` both read 10, and their sun rows differed only by the frame's exposure.
/// Whether 7,200 changed the sun was not measured.
const SUN_SCALAR: usize = 0x14;
const SUN_SCALAR_VALUE: f32 = 1800.;
/// The lens flare record class, and how far into a sun or atmosphere resource its references are
/// sought.
const LENS_FLARE_RECORD: u32 = 0x8080_6F68;
const FLARE_SEARCH: usize = 0x140;
const SUN_RECORD_CLASS: u32 = 0x8080_6F64;
const SUN_TRACKS: usize = 0x10;
/// The sun record's leading floats, read as the sun's time of day windows: `mission_proton`'s
/// `(0, 1, 0, 1)`, Midtown's `(1, 0, 1, 0)` and Distant Shore's `(0.667, 0.833, 0.667, 0.833)`.
/// Every map's sun takes `mission_proton`'s. Breakneck's row 62 reads zero with either, since its
/// global lighting program carries the sun in row 65, and its sun color reads 10 with these. The
/// windows reading is an inference from the values, and their effect was not isolated.
const SUN_WINDOWS: [f32; 4] = [0., 1., 0., 1.];
const SUN_TRACK_SLOTS: usize = 4;
const SUN_TRACK_CLASS: u32 = 0x8080_8EF3;
const SUN_TRACK_DIRECTIONS: usize = 0x30;
const SUN_DIRECTION_CLASS: u32 = 0x8080_0090;
/// A placement resource for the atmosphere, kept inline, which names the volume texture that
/// colors the sun and sky light on dynamic objects at `+0x90`. Distant Shore places it in its sun's
/// table.
const ATMOSPHERE: u32 = 0x8080_7086;
const ATMOSPHERE_VOLUME: usize = 0x90;
/// A second volume, or none, which the sky reflection generator samples beside the first and
/// blends toward by global channel 43. Midtown names one (`80B91A92`), and Distant Shore and
/// `mission_proton` none. A map with its own atmosphere volume names its interior volume here,
/// which its interior environment volumes select through channel 43, or none. With none, the
/// global lighting sun read 15.1 on Nightfall, where Midtown's two-volume atmosphere lights
/// itself through time of day channels with that sun at zero.
const ATMOSPHERE_SECOND_VOLUME: usize = 0x94;
/// The atmosphere's 2D distance lookup, 16 by 1,024 in Distant Shore's, which a map whose
/// surfaces draw their own fog replaces with a black copy.
const ATMOSPHERE_LOOKUP: usize = 0x98;
/// The atmosphere's 64 by 64 grayscale image (`8080708B`), which differs by donor, and which a map
/// with its own atmosphere volume replaces with an open copy (`atmosphere::open_image`).
const ATMOSPHERE_IMAGE: usize = 0x9C;
/// A placement resource for the donor's static ambient occlusion, naming its record at `+0x10`.
/// The record names three buffers at `+0x08`, `+0x20` and `+0x38`, which hold the occlusion of
/// the donor's own static geometry, and at `+0x50` an array of probes that indexed draws read
/// through the instance indices at `+0x60` ([`occlusion_record`]): 48-byte rows of a position and 1, a sky visibility (0.505 on open ground, the
/// open half of the sphere), six small directional terms and zero. Distant Shore's first probe sits
/// at the origin with 0.505 and nothing else, and its others range from 0.009 to 0.641.
const OCCLUSION: u32 = 0x8080_6F38;
const OCCLUSION_RECORD: usize = 0x10;
const OCCLUSION_RECORD_CLASS: u32 = 0x8080_7268;
const OCCLUSION_BUFFERS: [usize; 3] = [0x08, 0x20, 0x38];
const OCCLUSION_PROBES: usize = 0x50;
const OCCLUSION_PROBE_CLASS: u32 = 0x8080_726D;
const OCCLUSION_PROBE_SIZE: usize = 48;
/// The lowest elevation a baked light direction may give the sun, as the sine of its angle. A
/// direction lower than this keeps the donor's tracks.
const SUN_LOWEST: f32 = 0.05;
/// An entity model's meshes, each part's material, and a mesh's draw stage ranges.
const MODEL_CLASS: u32 = 0x8080_73A5;
const MODEL_MESHES: usize = 0x10;
const MESH_SIZE: usize = 136;
const MESH_PARTS: usize = 0x18;
const MESH_STAGES: usize = 0x28;
const PART_SIZE: usize = 32;
const MATERIAL_CLASS: u32 = 0x8080_71E8;
/// The transparent stage and the light-shaft stage, the two a stock sky object draws in.
const TRANSPARENT_STAGE: usize = 7;
const LIGHT_SHAFT_STAGE: usize = 9;
/// Where moved rows sit, in meters below the map's origin.
pub(super) const BURIED: f32 = -5000.;
/// A placement resource that registers a spawn set with the client's spawn point manager, and
/// the spawn set tag it names.
const SPAWN_SET: u32 = 0x8080_9160;
const SPAWN_SET_TAG: usize = 0x10;
/// A spawn set: its points, each a rotation, a position and the hash of the set's name, then
/// zeros.
const SPAWN_POINTS_CLASS: u32 = 0x8080_9162;
const SPAWN_POINTS: usize = 0x08;
const SPAWN_POINT_CLASS: u32 = 0x8080_9164;
const SPAWN_POINT_SIZE: usize = 48;
const SPAWN_POINT_SET: usize = 0x20;
/// A placement resource that places the world's static collision, naming its record at `+0x10`.
/// The record lists mesh records at `+0x18`, the material of each mesh packfile's palette entry
/// at `+0x28`, where each packfile's entries start at `+0x38` (`FFFF` for none), the mesh
/// packfiles at `+0x48`, and names its main packfile, a static compound, at `+0x58`.
const WORLD_COLLISION: u32 = 0x8080_8A13;
const WORLD_RECORD: usize = 0x10;
const WORLD_RECORD_CLASS: u32 = 0x8080_8A15;
const WORLD_MESH_RECORDS: usize = 0x18;
const WORLD_PALETTE_MATERIALS: usize = 0x28;
const WORLD_PALETTE_STARTS: usize = 0x38;
const WORLD_PACKFILES: usize = 0x48;
const WORLD_COMPOUND: usize = 0x58;
/// A mesh record names its packfile at `+0x14`.
const WORLD_MESH_RECORD_CLASS: u32 = 0x8080_92DB;
const WORLD_MESH_RECORD_PACKFILE: usize = 0x14;

/// One table slot of a filtered container copy.
#[derive(Clone)]
enum Slot {
    /// A donor table, kept or standing in for a geometry table.
    Table(u32),
    /// A copy with donor geometry removed or leftover rows moved far below the map.
    Moved { template: u32, payload: Vec<u8> },
    /// A copy of a donor spawn set table whose rows at `fields` name a copy of spawn set `set`
    /// holding the map's own points.
    Spawns {
        template: u32,
        payload: Vec<u8>,
        fields: Vec<usize>,
        set: TagHash,
        points: Vec<u8>,
    },
    /// A copy of the donor's world collision table whose row at `field` names a copy of world
    /// collision record `record` that holds the map's own collision. `donor` lists the record and
    /// the packfiles and mesh records it names, which the copy's container no longer loads.
    World {
        template: u32,
        payload: Vec<u8>,
        field: usize,
        record: TagHash,
        record_payload: Vec<u8>,
        donor: Vec<TagHash>,
    },
    /// A copy of the donor's sun table, whose sun at `resource` names no sun materials and no lens
    /// flare, and with the map's light direction names a copy of sun record `record` whose tracks
    /// hold that direction throughout. An atmosphere the table places at `atmosphere` names the
    /// map's own atmosphere volume when the map brings one.
    Sun {
        template: u32,
        payload: Vec<u8>,
        resource: usize,
        record: TagHash,
        record_payload: Vec<u8>,
        track: TagHash,
        track_payload: Vec<u8>,
        atmosphere: Option<usize>,
    },
    /// A copy of the donor's sky table whose row at `field` names a copy of sky object collection
    /// `collection` that holds the map's own sky objects.
    Sky {
        template: u32,
        payload: Vec<u8>,
        field: usize,
        collection: TagHash,
        collection_payload: Vec<u8>,
    },
    /// A copy of the donor's table of cubemap volumes that places the map's own volumes.
    Cubemaps { template: u32, payload: Vec<u8> },
    /// A copy of the donor's ambient occlusion table whose row at `field` names a copy of
    /// occlusion record `record` whose probes, after the first, are the map's own.
    Occlusion {
        template: u32,
        payload: Vec<u8>,
        field: usize,
        record: TagHash,
        record_payload: Vec<u8>,
    },
}

/// A donor container a filtered copy stands in for: its owner with its shared-tag companion, or
/// its tag and payload alone when its package enrolls none, whose tables the bubble's companion
/// loads.
#[derive(Clone)]
enum Container {
    Enrolled(Owner),
    Unenrolled { tag: TagHash, payload: Vec<u8> },
}

impl Container {
    fn tag(&self) -> TagHash {
        match self {
            Self::Enrolled(owner) => owner.tag,
            Self::Unenrolled { tag, .. } => *tag,
        }
    }

    fn payload(&self) -> &[u8] {
        match self {
            Self::Enrolled(owner) => &owner.payload,
            Self::Unenrolled { payload, .. } => payload,
        }
    }

    /// Entries a copy takes: itself, and its companion when the donor has one.
    fn entries(&self) -> usize {
        match self {
            Self::Enrolled(_) => 2,
            Self::Unenrolled { .. } => 1,
        }
    }
}

impl Slot {
    /// Entries a slot's copies take: none for a kept table, the table for moved rows or the map's
    /// cubemap volumes, the table and the set or record it names for the others, and for the sun
    /// its table, then copies of its sun record and direction track when the map has a `sun`.
    fn entries(&self, sun: bool) -> usize {
        match self {
            Self::Table(_) => 0,
            Self::Moved { .. } | Self::Cubemaps { .. } => 1,
            Self::Spawns { .. }
            | Self::World { .. }
            | Self::Sky { .. }
            | Self::Occlusion { .. } => 2,
            Self::Sun { .. } if sun => 3,
            Self::Sun { .. } => 1,
        }
    }
}

/// What becomes of one of the donor bubble's containers.
#[derive(Clone)]
enum Kept {
    Unchanged,
    /// A copy whose geometry tables give way to an empty table, whose leftovers move and whose
    /// spawn set holds the map's points.
    Filtered {
        container: Container,
        slots: Vec<Slot>,
    },
}

/// What becomes of one placement table of a donor container.
enum Table {
    Unchanged,
    /// A table of geometry, visibility, barriers or surface detail, whose slot the bubble's empty
    /// table fills.
    Geometry,
    /// A copy that takes the table's slot.
    Replaced(Slot),
}

/// One of the map's pieces, which the map's static instance group places: the least and greatest
/// corners of its geometry, in the map's space.
pub(super) struct Piece {
    pub minimum: [f32; 3],
    pub maximum: [f32; 3],
}

/// The resource kind of a placement row that places a static instance group, whose 24-byte
/// resource names the group's preheader at `+0x10`.
const STATIC_GROUP_KIND: u32 = 0x8080_71B3;
const STATIC_GROUP_PREHEADER: usize = 0x10;
const STATIC_PREHEADER_CLASS: u32 = 0x8080_6EF4;
const STATIC_PREHEADER_INSTANCES: usize = 0x08;
const STATIC_INSTANCES_CLASS: u32 = 0x8080_966D;
const STATIC_INSTANCES_BOUNDS: usize = 0x18;
const STATIC_INSTANCES_STATICS: usize = 0x58;
const STATIC_BOUNDS_CLASS: u32 = 0x8080_9671;
const STATIC_MESH_CLASS: u32 = 0x8080_71A7;
const STATIC_MESH_DATA: usize = 0x08;
const STATIC_MESH_TECHNIQUES: usize = 0x10;
const STATIC_DATA_CLASS: u32 = 0x8080_7194;
const STATIC_DATA_GROUPS: usize = 0x08;
const STATIC_GROUP_SIZE: usize = 8;
/// The depth prepass and shadow stages, and the position-only vertex layout stock statics draw
/// them with.
const DEPTH_STAGE: u8 = 12;
const SHADOW_STAGE: u8 = 3;
const POSITION_LAYOUT: u8 = 120;
/// A material's mode at `+0x08`, 2 for a vertex-only material.
const MATERIAL_MODE: usize = 0x08;
const VERTEX_ONLY: u32 = 2;

/// The donor's static instance group: its one-row placement table, the offset of the preheader
/// tag in that row's resource, the row's entity, which the map's container loads, and the records
/// the map's statics copy.
#[derive(Clone)]
struct StaticGroup {
    table: Vec<u8>,
    preheader: usize,
    entity: TagHash,
    templates: parhelion_import::halo_reach::level::native::StaticTemplates,
}

/// The static instance group `table` places, when its only row places one: Distant Shore's
/// `80EB015A` places entity `80C70CC0` with a resource naming preheader `80EB0159`. Its templates
/// come from the group's first static whose groups draw the depth prepass and shadow stages with
/// position-only, vertex-only techniques.
fn static_group(manager: &PackageManager, table: &[u8]) -> AuthoringResult<Option<StaticGroup>> {
    if read_u64(table, TABLE_ROWS)? != 1 {
        return Ok(None);
    }
    let (_, _, row, _) = array_at(table, TABLE_ROWS)?;
    let pointer = read_i64(table, row + ROW_RESOURCE)?;
    if pointer == 0 {
        return Ok(None);
    }
    let resource = usize::try_from((row + ROW_RESOURCE) as i64 + pointer)
        .map_err(|_| invalid("A static group row's resource lies before its table"))?;
    if resource < 4 || read_u32(table, resource - 4)? != STATIC_GROUP_KIND {
        return Ok(None);
    }
    let preheader_at = resource + STATIC_GROUP_PREHEADER;
    let preheader = TagHash(read_u32(table, preheader_at)?);
    let checked = |tag: TagHash, expected: u32, what: &str| -> AuthoringResult<Vec<u8>> {
        if class(manager, tag) != Some(expected) {
            return Err(invalid(format!(
                "A donor static group's {what} {tag} is not class {expected:08X}"
            )));
        }
        read_tag(manager, tag, what)
    };
    let preheader_payload = checked(preheader, STATIC_PREHEADER_CLASS, "preheader")?;
    let instances = TagHash(read_u32(&preheader_payload, STATIC_PREHEADER_INSTANCES)?);
    let set = checked(instances, STATIC_INSTANCES_CLASS, "instance set")?;
    let bounds = TagHash(read_u32(&set, STATIC_INSTANCES_BOUNDS)?);
    checked(bounds, STATIC_BOUNDS_CLASS, "bounds record")?;
    let (count, _, statics, _) = array_at(&set, STATIC_INSTANCES_STATICS)?;
    for index in 0..count {
        let mesh = TagHash(read_u32(&set, statics + index * 4)?);
        if class(manager, mesh) != Some(STATIC_MESH_CLASS) {
            continue;
        }
        let mesh_payload = read_tag(manager, mesh, "static mesh")?;
        let data = TagHash(read_u32(&mesh_payload, STATIC_MESH_DATA)?);
        if class(manager, data) != Some(STATIC_DATA_CLASS) {
            continue;
        }
        let data_payload = read_tag(manager, data, "static mesh data")?;
        let (techniques, _, technique_rows, _) = array_at(&mesh_payload, STATIC_MESH_TECHNIQUES)?;
        let (groups, _, group_rows, _) = array_at(&data_payload, STATIC_DATA_GROUPS)?;
        if groups != techniques {
            continue;
        }
        let mut depth = None;
        let mut shadow = None;
        for group in 0..groups {
            let at = group_rows + group * STATIC_GROUP_SIZE;
            let (stage, layout) = (data_payload[at + 2], data_payload[at + 4]);
            let technique = read_u32(&mesh_payload, technique_rows + group * 4)?;
            if layout != POSITION_LAYOUT
                || read_tag(manager, TagHash(technique), "static technique")
                    .ok()
                    .and_then(|payload| read_u32(&payload, MATERIAL_MODE).ok())
                    != Some(VERTEX_ONLY)
            {
                continue;
            }
            match stage {
                DEPTH_STAGE => depth = depth.or(Some(technique)),
                SHADOW_STAGE => shadow = shadow.or(Some(technique)),
                _ => {}
            }
        }
        let (Some(depth), Some(shadow)) = (depth, shadow) else {
            continue;
        };
        return Ok(Some(StaticGroup {
            table: table.to_vec(),
            preheader: preheader_at,
            entity: TagHash(read_u32(table, row)?),
            templates: parhelion_import::halo_reach::level::native::StaticTemplates {
                preheader: preheader.0,
                instances: instances.0,
                bounds: bounds.0,
                mesh: mesh.0,
                data: data.0,
                depth,
                shadow,
            },
        }));
    }
    Err(invalid(
        "A donor static group has no static with position-only depth and shadow techniques",
    ))
}

/// One of the map's sky objects: its model reference, where it sits, its world bounds and the
/// order it draws in among the map's sky objects.
pub(super) struct SkyObject {
    pub reference: TagHash,
    pub position: [f32; 3],
    pub minimum: [f32; 3],
    pub maximum: [f32; 3],
    pub order: f32,
}

/// A donor prop: the entity model its render owner draws, whose first mesh's buffer headers the
/// map's statics copy.
#[derive(Clone, Copy)]
pub(super) struct Prop {
    pub model: u32,
    pub entity: u32,
    pub render: u32,
}

/// The map's own world collision: its static compound and the mesh packfiles its instances name.
pub(super) struct World<'a> {
    pub compound: TagHash,
    pub meshes: &'a [TagHash],
}

/// What the placement table places: the map's static instance group by its preheader, its props
/// by entity and position, and its decorator records.
pub(super) struct Placements<'a> {
    pub statics: TagHash,
    pub props: &'a [(TagHash, [f32; 3])],
    pub decorators: &'a [TagHash],
}

/// The map's atmosphere, which the donor's beside its sun names in its place: its volume, its
/// second volume for interiors, its distance lookup and its image.
#[derive(Clone, Copy)]
pub(super) struct Atmosphere {
    pub volume: Option<TagHash>,
    pub interior: Option<TagHash>,
    pub lookup: Option<TagHash>,
    pub image: Option<TagHash>,
}

/// The map's ambient occlusion ([`occlusion_record`]): its probes, the probe each cluster of the
/// area's own tome takes, and the decorator occlusion buffer with each placed decorator record's
/// identity and first row.
#[derive(Clone, Copy)]
pub(super) struct Occlusion<'a> {
    pub probes: &'a [super::lighting::Probe],
    pub clusters: Option<&'a [u16]>,
    pub decorators: Option<(TagHash, &'a [(u64, u32)])>,
}

/// The converted tags and records a chain's copies name, gathered before its entries are assigned.
pub(super) struct Inputs<'a> {
    pub placements: Placements<'a>,
    pub pieces: &'a [Piece],
    pub spawns: &'a [Spawn],
    pub world: World<'a>,
    pub sky: &'a [SkyObject],
    /// The map's sound point table, which its own container places.
    pub sounds: Option<TagHash>,
    pub atmosphere: Atmosphere,
    pub occlusion: Occlusion<'a>,
    pub cubemaps: &'a [Cubemap],
    /// The textures the map's spot lights project, by their lights' gel indices.
    pub gels: &'a [TagHash],
    /// The map-owned native screen effect used by its global environment volume.
    pub color_grade: Option<TagHash>,
    /// Every tag the pieces, sky, sounds and atmosphere use, which the container's companion loads.
    pub content: &'a [TagHash],
}

#[derive(Clone)]
pub(super) struct Chain {
    root: Owner,
    /// The map root's bubble layout and its payload, which already names the map.
    layout: TagHash,
    layout_payload: Vec<u8>,
    parent: Owner,
    bubble_name: BubbleName,
    bubble: Owner,
    /// One per container row of the donor bubble, in order.
    kept: Vec<Kept>,
    container: Owner,
    /// The donor's first prop: its placement table and tags. The map's static instance group's
    /// table takes the table's place.
    prop_table: TagHash,
    prop_row: Vec<u8>,
    prop: Prop,
    /// The donor's static instance group, whose table and records the map's statics copy.
    statics: StaticGroup,
    /// A native sound point table, preferring one placed by the donor.
    sound_points: Option<u32>,
    /// The direction the map's baked light mostly comes from, toward the light with z up.
    light: Option<[f32; 3]>,
    /// Without a sky in the donor's bubble, a sky table of another world ([`SKY_WORLDS`]), whose
    /// copy the map's own container places to hold the map's sky.
    borrowed_sky: Option<Slot>,
    /// The light collection table and point light the map's lights copy, Distant Shore's whatever
    /// the donor ([`lights::LIGHT_WORLD`]).
    light_template: Option<lights::Template>,
    /// The map's own lights.
    lights: Vec<Light>,
    /// The environment volume table, set, channel record and packfile the map's environment
    /// copies, Distant Shore's whatever the donor ([`environment::template`]).
    environment_template: Option<environment::Template>,
    /// The map's own environment: its native sun and its interiors.
    environment: Option<Environment>,
    /// The donor parent's bounds records, in order, each that bounds something as the type-16
    /// owner the map's copy widens to hold the level ([`widened_bounds`]).
    bounds_rows: Vec<(TagHash, Option<Owner>)>,
    /// The stock decorator placement the map's decorator records' rows copy, when the level's
    /// decorators draw as Destiny's instanced decorators ([`Decorators`]).
    decorators: Option<Decorators>,
}

/// The resource kind of a placement row that places a decorator, whose 24-byte resource names
/// its record (`80807164`) at `+0x10`, as a static group's names its preheader.
const DECORATOR_KIND: u32 = 0x8080_71AD;
const DECORATOR_RECORD: usize = 0x10;
const DECORATOR_RESOURCE_SIZE: usize = 0x18;

/// A stock decorator placement: its row, which places the stock decorator entity at the origin
/// with no turn, its resource and the stock tags a container placing it loads.
#[derive(Clone)]
pub(super) struct Decorators {
    row: Vec<u8>,
    resource: Vec<u8>,
    loads: Vec<TagHash>,
}

/// The decorator placement `table` places, when its only row places one: Distant Shore's
/// `80EB0311` places entity `80C70D0D` with a resource naming record `80EB0310`, and its other
/// decorator table the same entity. `loads` are the stock techniques the copies copy, which the
/// map's container loads with the row's entity and the programs and samplers they name.
pub(super) fn decorators(
    manager: &PackageManager,
    table: TagHash,
    loads: &[TagHash],
) -> AuthoringResult<Decorators> {
    let payload = read_tag(manager, table, "decorator table")?;
    if read_u64(&payload, TABLE_ROWS)? != 1 {
        return Err(invalid(
            "The stock decorator table places more than one row",
        ));
    }
    let (_, _, row, _) = array_at(&payload, TABLE_ROWS)?;
    let pointer = read_i64(&payload, row + ROW_RESOURCE)?;
    let resource = usize::try_from((row + ROW_RESOURCE) as i64 + pointer)
        .map_err(|_| invalid("The stock decorator row's resource lies before its table"))?;
    if pointer == 0 || resource < 4 || read_u32(&payload, resource - 4)? != DECORATOR_KIND {
        return Err(invalid(
            "The stock decorator table's row places no decorator",
        ));
    }
    let resource = payload
        .get(resource..resource + DECORATOR_RESOURCE_SIZE)
        .ok_or_else(|| invalid("The stock decorator resource is truncated"))?
        .to_vec();
    let entity = TagHash(read_u32(&payload, row)?);
    if class(manager, entity) != Some(ENTITY_CLASS) {
        return Err(invalid("The stock decorator row places no entity"));
    }
    let mut stock = vec![entity];
    for &technique in loads {
        stock.push(technique);
        stock.extend(technique_stage_tags(manager, technique)?);
    }
    Ok(Decorators {
        row: payload[row..row + ROW_SIZE].to_vec(),
        resource,
        loads: stock,
    })
}

/// A technique's six stages, each with its program at `+0x00` (none as `FFFFFFFF`) and its sampler
/// rows (`808073F3`, the sampler's tag at row `+0x00`) at `+0x40`.
const TECHNIQUE_STAGES: [usize; 6] = [0x48, 0xE8, 0x188, 0x228, 0x2C8, 0x368];
const STAGE_SAMPLERS: usize = 0x40;
const SAMPLER_ROW_CLASS: u32 = 0x8080_73F3;
const SAMPLER_ROW_SIZE: usize = 16;

/// Each program and sampler technique `technique`'s stages name, and the data entry each of those
/// names. A container that loads a stock technique leaves the programs and samplers it names
/// unloaded, so on a map whose donor activity does not load them a copy that draws with them reads
/// an unloaded tag. Distant Shore's grass draws with pixel programs in its own activity packages,
/// which a campaign donor never loads. Loading a program's header leaves its bytecode, the data
/// entry its entry references, unloaded in turn: on a converted Paradiso the grass surface pixel
/// program `80FE1882` was resident and its bytecode `80FE1881` was not, and every decorator drew
/// its whole cards into depth with no color.
fn technique_stage_tags(
    manager: &PackageManager,
    technique: TagHash,
) -> AuthoringResult<Vec<TagHash>> {
    let payload = read_tag(manager, technique, "technique")?;
    let mut tags = Vec::new();
    for stage in TECHNIQUE_STAGES {
        let program = read_u32(&payload, stage)?;
        if program != 0 && program != u32::MAX {
            tags.push(TagHash(program));
        }
        if read_u64(&payload, stage + STAGE_SAMPLERS)? == 0 {
            continue;
        }
        let (count, _, rows, row_class) = array_at(&payload, stage + STAGE_SAMPLERS)?;
        if row_class != SAMPLER_ROW_CLASS {
            return Err(invalid(format!(
                "Technique {technique} stage 0x{stage:X} samplers are class {row_class:08X}"
            )));
        }
        for row in 0..count {
            let sampler = read_u32(&payload, rows + row * SAMPLER_ROW_SIZE)?;
            if sampler != 0 && sampler != u32::MAX {
                tags.push(TagHash(sampler));
            }
        }
    }
    // A plain tag entry (type 8) references its class, and any other entry its data.
    let data = tags
        .iter()
        .filter_map(|&tag| manager.get_entry(tag))
        .filter(|entry| entry.file_type != 8)
        .map(|entry| TagHash(entry.reference))
        .filter(|&data| manager.get_entry(data).is_some())
        .collect::<Vec<_>>();
    tags.extend(data);
    Ok(tags)
}

/// Crucible worlds whose sky table and sky objects a map copies when its donor's bubble places no
/// sky, as `mission_proton`'s does not, in the order tried. Without one, a converted campaign
/// stretch drew Destiny's sky in place of its own.
const SKY_WORLDS: [&str; 2] = ["pvp_sabre", "pvp_greenhouse_2"];

/// The first sky table, with objects, that the bubbles of world `world` place.
fn world_sky(manager: &PackageManager, world: &str) -> AuthoringResult<Option<Slot>> {
    let Ok((root, _)) = super::donor_tag(manager, &format!("map:{world}:root")) else {
        return Ok(None);
    };
    let root = read_tag(manager, root, "map root")?;
    for parent in tag_rows(manager, &root, ROOT_PARENTS)? {
        if class(manager, parent) != Some(BUBBLE_PARENT_CLASS) {
            continue;
        }
        let bubble = TagHash(read_u32(
            &read_tag(manager, parent, "bubble parent")?,
            PARENT_BUBBLE,
        )?);
        if class(manager, bubble) != Some(BUBBLE_CLASS) {
            continue;
        }
        let bubble = read_tag(manager, bubble, "bubble")?;
        for container in tag_rows(manager, &bubble, BUBBLE_CONTAINERS)? {
            if class(manager, container) != Some(CONTAINER_CLASS) {
                continue;
            }
            let payload = read_tag(manager, container, "map container")?;
            if read_u64(&payload, CONTAINER_TABLES)? == 0 {
                continue;
            }
            let (count, _, rows, _) = array_at(&payload, CONTAINER_TABLES)?;
            for index in 0..count {
                let table = read_u32(&payload, rows + index * 4)?;
                // Only a table of sky objects matters here, so a table that cannot be copied is
                // passed over.
                if let Ok(Table::Replaced(slot @ Slot::Sky { .. })) = kept_table(manager, table) {
                    return Ok(Some(slot));
                }
            }
        }
    }
    Ok(None)
}

/// The light template of the first table among those `containers` list that places a light
/// collection with a point light ([`lights::template`]). A table that cannot serve is passed over,
/// as the map's lights are optional.
fn light_template(
    manager: &PackageManager,
    containers: &[TagHash],
) -> AuthoringResult<Option<lights::Template>> {
    for &container in containers {
        if class(manager, container) != Some(CONTAINER_CLASS) {
            continue;
        }
        let payload = read_tag(manager, container, "map container")?;
        if read_u64(&payload, CONTAINER_TABLES)? == 0 {
            continue;
        }
        let (count, _, rows, _) = array_at(&payload, CONTAINER_TABLES)?;
        for index in 0..count {
            let table = read_u32(&payload, rows + index * 4)?;
            if let Ok(Some(template)) = lights::template(manager, table) {
                return Ok(Some(template));
            }
        }
    }
    Ok(None)
}

/// The box `pieces` span, which the map's environment volume holds.
fn piece_bounds(pieces: &[Piece]) -> AuthoringResult<([f32; 3], [f32; 3])> {
    if pieces.is_empty() {
        return Err(invalid(
            "The map places no pieces for its environment to hold",
        ));
    }
    let mut low = [f32::INFINITY; 3];
    let mut high = [f32::NEG_INFINITY; 3];
    for piece in pieces {
        for k in 0..3 {
            low[k] = low[k].min(piece.minimum[k]);
            high[k] = high[k].max(piece.maximum[k]);
        }
    }
    Ok((low, high))
}

/// The environment template of the first table among those `containers` list that places an
/// environment volume with a channel modifier record ([`environment::template`]). A table that
/// cannot serve is passed over, and without one the map keeps the global channels' defaults.
fn environment_template(
    manager: &PackageManager,
    containers: &[TagHash],
) -> AuthoringResult<Option<environment::Template>> {
    for &container in containers {
        if class(manager, container) != Some(CONTAINER_CLASS) {
            continue;
        }
        let payload = read_tag(manager, container, "map container")?;
        if read_u64(&payload, CONTAINER_TABLES)? == 0 {
            continue;
        }
        let (count, _, rows, _) = array_at(&payload, CONTAINER_TABLES)?;
        for index in 0..count {
            let table = read_u32(&payload, rows + index * 4)?;
            if let Ok(Some(template)) = environment::template(manager, table) {
                return Ok(Some(template));
            }
        }
    }
    Ok(None)
}

/// The containers the bubbles of world `world` place.
fn world_containers(manager: &PackageManager, world: &str) -> AuthoringResult<Vec<TagHash>> {
    let Ok((root, _)) = super::donor_tag(manager, &format!("map:{world}:root")) else {
        return Ok(Vec::new());
    };
    let root = read_tag(manager, root, "map root")?;
    let mut containers = Vec::new();
    for parent in tag_rows(manager, &root, ROOT_PARENTS)? {
        if class(manager, parent) != Some(BUBBLE_PARENT_CLASS) {
            continue;
        }
        let bubble = TagHash(read_u32(
            &read_tag(manager, parent, "bubble parent")?,
            PARENT_BUBBLE,
        )?);
        if class(manager, bubble) != Some(BUBBLE_CLASS) {
            continue;
        }
        let bubble = read_tag(manager, bubble, "bubble")?;
        containers.extend(tag_rows(manager, &bubble, BUBBLE_CONTAINERS)?);
    }
    Ok(containers)
}

/// The first table without rows among those `containers` list, which stands in for the donor's
/// geometry and visibility tables.
fn empty_table(manager: &PackageManager, containers: &[TagHash]) -> AuthoringResult<Option<u32>> {
    for &container in containers {
        if class(manager, container) != Some(CONTAINER_CLASS) {
            continue;
        }
        let payload = read_tag(manager, container, "map container")?;
        if read_u64(&payload, CONTAINER_TABLES)? == 0 {
            continue;
        }
        let (count, _, rows, _) = array_at(&payload, CONTAINER_TABLES)?;
        for index in 0..count {
            let table = read_u32(&payload, rows + index * 4)?;
            if class(manager, TagHash(table)) == Some(TABLE_CLASS)
                && read_u64(
                    &read_tag(manager, TagHash(table), "placement table")?,
                    TABLE_ROWS,
                )? == 0
            {
                return Ok(Some(table));
            }
        }
    }
    Ok(None)
}

/// A nonempty table containing only points with the native ambience cue envelope.
fn sound_table(manager: &PackageManager, table: TagHash) -> AuthoringResult<bool> {
    if class(manager, table) != Some(TABLE_CLASS) {
        return Ok(false);
    }
    let bytes = read_tag(manager, table, "sound point placement template")?;
    if read_u64(&bytes, TABLE_ROWS)? == 0 {
        return Ok(false);
    }
    let (count, _, rows, row_class) = array_at(&bytes, TABLE_ROWS)?;
    if row_class != TABLE_ROW_CLASS
        || rows
            .checked_add(count.saturating_mul(ROW_SIZE))
            .is_none_or(|end| end > bytes.len())
    {
        return Ok(false);
    }
    let mut first = None;
    for row in (0..count).map(|i| rows + i * ROW_SIZE) {
        let relative = read_i64(&bytes, row + ROW_RESOURCE)?;
        let Some(point) = (row + ROW_RESOURCE)
            .checked_add_signed(relative as isize)
            .filter(|&at| relative != 0 && at >= 4 && at <= bytes.len().saturating_sub(0x20))
        else {
            return Ok(false);
        };
        if read_u32(&bytes, point - 4)? != SOUND_POINT {
            return Ok(false);
        }
        first.get_or_insert(read_u32(&bytes, point + 0x10)?);
    }
    let Some(sound) = first.filter(|&tag| class(manager, TagHash(tag)) == Some(0x8080_9802)) else {
        return Ok(false);
    };
    let cue = read_tag(manager, TagHash(sound), "sound point cue template")?;
    if cue.len() < 0x54
        || read_i64(&cue, 0x20)? != 0x20
        || read_u64(&cue, 0x18)? == 0
        || read_u64(&cue, 0x18)? != read_u64(&cue, 0x40)?
        || read_u64(&cue, 0x18)? > ((cue.len() - 0x50) / 4) as u64
        || class(manager, TagHash(read_u32(&cue, 0x10)?)) != Some(0x8080_8E68)
        || manager.get_entry(TagHash(read_u32(&cue, 0x50)?)).is_none()
    {
        return Ok(false);
    }
    let bank = read_tag(
        manager,
        TagHash(read_u32(&cue, 0x14)?),
        "sound point bank template",
    )?;
    Ok(bank.len() >= 12 && bank.starts_with(b"BKHD") && read_u32(&bank, 8)? == 113)
}

/// Prefer a donor point table, then a compatible stock table ordered by size and identity.
/// Geometry donors need not place ambience, so absence there does not disable source audio.
fn sound_points(manager: &PackageManager, containers: &[TagHash]) -> AuthoringResult<Option<u32>> {
    for &container in containers {
        if class(manager, container) != Some(CONTAINER_CLASS) {
            continue;
        }
        let payload = read_tag(manager, container, "map container")?;
        if read_u64(&payload, CONTAINER_TABLES)? == 0 {
            continue;
        }
        let (count, _, rows, _) = array_at(&payload, CONTAINER_TABLES)?;
        for index in 0..count {
            let table = read_u32(&payload, rows + index * 4)?;
            if sound_table(manager, TagHash(table))? {
                return Ok(Some(table));
            }
        }
    }
    let mut candidates = Vec::new();
    for (&package, entries) in &manager.lookup.tag32_entries_by_pkg {
        for (index, entry) in entries.iter().enumerate() {
            // A header, one 144-byte placement and its 32-byte point require 232 bytes.
            if entry.reference == TABLE_CLASS && entry.file_size >= 232 {
                let index = u16::try_from(index)
                    .map_err(|_| invalid("Sound table index exceeds native tag capacity"))?;
                candidates.push((entry.file_size, TagHash::new(package, index).0));
            }
        }
    }
    candidates.sort_unstable();
    for (_, table) in candidates {
        if sound_table(manager, TagHash(table))? {
            return Ok(Some(table));
        }
    }
    Ok(None)
}

/// What becomes of donor container `tag`: unchanged without geometry, visibility, barriers,
/// lighting or surface detail, leftovers, a spawn set with points or a sky with objects, else a
/// filtered copy. The bubble's `empty` table fills the geometry, visibility, barrier and detail
/// tables' slots, and without one they stay. A barrier or detail row in a table that also places
/// other kinds moves far below the map.
fn keep(
    directory: &Path,
    manager: &PackageManager,
    companions: &mut Companions,
    tag: TagHash,
    empty: Option<u32>,
    own: bool,
) -> AuthoringResult<Kept> {
    if class(manager, tag) != Some(CONTAINER_CLASS) {
        return Ok(Kept::Unchanged);
    }
    let payload = read_tag(manager, tag, "map container")?;
    if !own && read_u64(&payload, CONTAINER_TABLES)? == 0 {
        return Ok(Kept::Unchanged);
    }
    let (count, rows) = if read_u64(&payload, CONTAINER_TABLES)? == 0 {
        (0, 0)
    } else {
        let (count, _, rows, _) = array_at(&payload, CONTAINER_TABLES)?;
        (count, rows)
    };
    let mut slots = Vec::with_capacity(count);
    let mut changed = false;
    for index in 0..count {
        let table = read_u32(&payload, rows + index * 4)?;
        let slot = match (kept_table(manager, table)?, empty) {
            (Table::Replaced(slot), _) => slot,
            (Table::Geometry, Some(empty)) => Slot::Table(empty),
            (Table::Geometry, None) => {
                let mut payload = read_tag(manager, TagHash(table), "placement table")?;
                replace_array(&mut payload, TABLE_ROWS, TABLE_ROW_CLASS, ROW_SIZE, 0, &[])?;
                Slot::Moved {
                    template: table,
                    payload: synchronize_payload_size(payload)?,
                }
            }
            (Table::Unchanged, _) => {
                slots.push(Slot::Table(table));
                continue;
            }
        };
        slots.push(slot);
        changed = true;
    }
    if !changed && !own {
        return Ok(Kept::Unchanged);
    }
    let container = if native_companion(directory, manager, tag.0, companions).is_ok() {
        Container::Enrolled(Owner::load_tag(
            directory,
            manager,
            companions,
            tag,
            CONTAINER_CLASS,
            "map container",
        )?)
    } else {
        Container::Unenrolled { tag, payload }
    };
    Ok(Kept::Filtered { container, slots })
}

/// What becomes of donor placement table `table`, by the kinds of resources its rows place: a
/// world collision, spawn set or sky table gives way to the map's own, and a table of mixed kinds
/// to a copy that buries its leftovers and keeps its sun.
fn kept_table(manager: &PackageManager, table: u32) -> AuthoringResult<Table> {
    if class(manager, TagHash(table)) != Some(TABLE_CLASS) {
        return Ok(Table::Unchanged);
    }
    let bytes = read_tag(manager, TagHash(table), "placement table")?;
    if read_u64(&bytes, TABLE_ROWS)? == 0 {
        return Ok(Table::Unchanged);
    }
    let (count, _, table_rows, row_class) = array_at(&bytes, TABLE_ROWS)?;
    if row_class != TABLE_ROW_CLASS
        || count
            .checked_mul(ROW_SIZE)
            .and_then(|size| table_rows.checked_add(size))
            .is_none_or(|end| end > bytes.len())
    {
        return Err(invalid(
            "The donor placement table has an unexpected row layout",
        ));
    }
    let mut kinds = Vec::with_capacity(count);
    let mut resources = Vec::with_capacity(count);
    for row in (0..count).map(|i| table_rows + i * ROW_SIZE) {
        let relative = read_i64(&bytes, row + ROW_RESOURCE)?;
        let resource = (row + ROW_RESOURCE)
            .checked_add_signed(relative as isize)
            .filter(|&at| relative != 0 && at >= 4 && at < bytes.len());
        if relative != 0 && resource.is_none() {
            return Err(invalid("The donor placement resource is outside its table"));
        }
        kinds.push(match resource {
            Some(at) => read_u32(&bytes, at - 4)?,
            None => 0,
        });
        resources.extend(resource);
    }
    let slot = if kinds.iter().all(|&kind| kind == WORLD_COLLISION) {
        world_slot(manager, table, bytes, &resources)?
    } else if kinds.iter().all(|&kind| kind == SPAWN_SET) {
        spawn_slot(manager, table, bytes, &resources)?
    } else if kinds.iter().all(|&kind| kind == SKY) {
        sky_slot(manager, table, bytes, &resources)?
    } else if kinds.iter().all(|&kind| kind == OCCLUSION) {
        occlusion_slot(manager, table, bytes, &resources)?
    } else if kinds.iter().all(|&kind| kind == CUBEMAP_VOLUME) {
        Some(Slot::Cubemaps {
            template: table,
            payload: bytes,
        })
    } else if kinds.iter().all(|&kind| {
        GEOMETRY.contains(&kind)
            || kind == VISIBILITY
            || BARRIERS.contains(&kind)
            || DETAIL.contains(&kind)
    }) {
        return Ok(Table::Geometry);
    } else {
        mixed_slot(manager, table, bytes, table_rows, &kinds)?
    };
    Ok(slot.map_or(Table::Unchanged, Table::Replaced))
}

/// A copy of donor placement table `table`, whose rows at `table_rows` place resources of `kinds`,
/// with geometry, baked visibility and leftover, barrier and detail rows removed. A table with a
/// sun keeps the first sun's row for the map's light, and a sun or atmosphere row that is itself
/// a leftover stays far below the map. Every placement row is a world object the area creates
/// when it loads: buried copies of The Other Side's rows took 564 per Nightfall area, and a
/// switch that created them with the area's own failed creations and left the activity.
fn mixed_slot(
    manager: &PackageManager,
    table: u32,
    mut bytes: Vec<u8>,
    mut table_rows: usize,
    kinds: &[u32],
) -> AuthoringResult<Option<Slot>> {
    // An inline resource configures the placed entity. Its class does not limit which
    // components that entity can contain, so every row needs the component check.
    let mut leftovers = Vec::with_capacity(kinds.len());
    for (index, kind) in kinds.iter().enumerate() {
        let row = table_rows + index * ROW_SIZE;
        leftovers.push(
            LEFTOVERS.contains(kind)
                || BARRIERS.contains(kind)
                || DETAIL.contains(kind)
                || *kind == CUBEMAP_VOLUME
                || leftover_entity(manager, read_u32(&bytes, row)?)?,
        );
    }
    let retained = kinds
        .iter()
        .enumerate()
        .filter(|&(index, kind)| {
            !GEOMETRY.contains(kind)
                && *kind != VISIBILITY
                && (!leftovers[index] || *kind == SUN || *kind == ATMOSPHERE)
        })
        .collect::<Vec<_>>();
    let mut moved = retained.len() != kinds.len();
    if moved {
        (bytes, table_rows) = retain_rows(bytes, table_rows, &retained)?;
    }
    let mut sun = None;
    let mut atmosphere = None;
    for (row, (index, kind)) in (0..retained.len())
        .map(|i| table_rows + i * ROW_SIZE)
        .zip(retained)
    {
        if *kind == SUN && sun.is_none() {
            sun = sun_row(manager, &bytes, row)?;
        }
        if *kind == ATMOSPHERE && atmosphere.is_none() {
            atmosphere = atmosphere_row(&bytes, row)?;
        }
        if leftovers[index] {
            bury(&mut bytes, row);
            moved = true;
        }
    }
    // The donor's lens flares draw the donor's sun, wherever its sun and atmosphere name them:
    // Distant Shore's sun at `+0x20`, `mission_proton`'s at `+0x24` and its atmosphere at `+0x11C`.
    let named = sun
        .as_ref()
        .map(|(resource, ..)| *resource)
        .into_iter()
        .chain(atmosphere)
        .collect::<Vec<_>>();
    for resource in named {
        let end = (resource + FLARE_SEARCH).min(bytes.len());
        for field in (resource..end.saturating_sub(3)).step_by(4) {
            let tag = read_u32(&bytes, field)?;
            if class(manager, TagHash(tag)) == Some(LENS_FLARE_RECORD) {
                write_u32(&mut bytes, field, u32::MAX)?;
                moved = true;
            }
        }
    }
    Ok(
        if let Some((resource, record, record_payload, track, track_payload)) = sun {
            Some(Slot::Sun {
                template: table,
                payload: bytes,
                resource,
                record,
                record_payload,
                track,
                track_payload,
                atmosphere,
            })
        } else if moved {
            Some(Slot::Moved {
                template: table,
                payload: bytes,
            })
        } else {
            None
        },
    )
}

/// Placement table `bytes` with only its `retained` rows of those at `table_rows`, in a new
/// active placement array, and where that array's rows start.
fn retain_rows(
    mut bytes: Vec<u8>,
    table_rows: usize,
    retained: &[(usize, &u32)],
) -> AuthoringResult<(Vec<u8>, usize)> {
    let mut rows = Vec::with_capacity(retained.len() * ROW_SIZE);
    let mut pointers = Vec::with_capacity(retained.len() * ROW_POINTERS.len());
    for &(index, _) in retained {
        let row = table_rows + index * ROW_SIZE;
        rows.extend_from_slice(&bytes[row..row + ROW_SIZE]);
        for offset in ROW_POINTERS {
            let field = row + offset;
            let relative = read_i64(&bytes, field)?;
            let target = if relative == 0 {
                None
            } else {
                Some(
                    field
                        .checked_add_signed(relative as isize)
                        .filter(|&at| at < bytes.len())
                        .ok_or_else(|| {
                            invalid("The donor placement pointer is outside its table")
                        })?,
                )
            };
            pointers.push((offset, target));
        }
    }
    // Keep inline resources at their original offsets. Only the active placement array
    // changes, so resource-local pointers and shared resource targets remain intact.
    replace_array(
        &mut bytes,
        TABLE_ROWS,
        TABLE_ROW_CLASS,
        ROW_SIZE,
        retained.len(),
        &rows,
    )?;
    let table_rows = array_at(&bytes, TABLE_ROWS)?.2;
    for (index, (offset, target)) in pointers.into_iter().enumerate() {
        if let Some(target) = target {
            write_relative_pointer(
                &mut bytes,
                table_rows + index / ROW_POINTERS.len() * ROW_SIZE + offset,
                target,
            )?;
        }
    }
    Ok((synchronize_payload_size(bytes)?, table_rows))
}

/// A copy of donor spawn set table `table` whose rows name the map's own spawn set, when a set it
/// names holds points. Its first such set is the copy's template.
fn spawn_slot(
    manager: &PackageManager,
    table: u32,
    payload: Vec<u8>,
    resources: &[usize],
) -> AuthoringResult<Option<Slot>> {
    let mut fields = Vec::new();
    let mut found = None;
    for &resource in resources {
        let field = resource + SPAWN_SET_TAG;
        let set = TagHash(read_u32(&payload, field)?);
        if class(manager, set) != Some(SPAWN_POINTS_CLASS) {
            continue;
        }
        let points = read_tag(manager, set, "spawn set")?;
        if read_u64(&points, SPAWN_POINTS)? == 0 {
            continue;
        }
        fields.push(field);
        found.get_or_insert((set, points));
    }
    Ok(found.map(|(set, points)| Slot::Spawns {
        template: table,
        payload,
        fields,
        set,
        points,
    }))
}

/// The atmosphere at placement row `row` of table `payload`: its resource, when the resource holds
/// its volume texture field and names a volume there.
fn atmosphere_row(payload: &[u8], row: usize) -> AuthoringResult<Option<usize>> {
    let relative = read_i64(payload, row + ROW_RESOURCE)?;
    let Some(resource) = (row + ROW_RESOURCE)
        .checked_add_signed(relative as isize)
        .filter(|&at| relative != 0 && at + ATMOSPHERE_VOLUME + 4 <= payload.len())
    else {
        return Ok(None);
    };
    Ok((read_u32(payload, resource + ATMOSPHERE_VOLUME)? != u32::MAX).then_some(resource))
}

/// The sun at placement row `row` of table `payload`: its resource, its sun record and that
/// record's first track, when the record has the sun record's layout.
#[allow(clippy::type_complexity)]
fn sun_row(
    manager: &PackageManager,
    payload: &[u8],
    row: usize,
) -> AuthoringResult<Option<(usize, TagHash, Vec<u8>, TagHash, Vec<u8>)>> {
    let relative = read_i64(payload, row + ROW_RESOURCE)?;
    let Some(resource) = (row + ROW_RESOURCE)
        .checked_add_signed(relative as isize)
        .filter(|&at| relative != 0 && at + SUN_FLARE + 4 <= payload.len())
    else {
        return Ok(None);
    };
    let record = TagHash(read_u32(payload, resource + SUN_RECORD)?);
    if class(manager, record) != Some(SUN_RECORD_CLASS) {
        return Ok(None);
    }
    let record_payload = read_tag(manager, record, "sun record")?;
    let Some(track) = (0..SUN_TRACK_SLOTS)
        .map(|slot| read_u32(&record_payload, SUN_TRACKS + slot * 4))
        .collect::<AuthoringResult<Vec<_>>>()?
        .into_iter()
        .map(TagHash)
        .find(|&tag| class(manager, tag) == Some(SUN_TRACK_CLASS))
    else {
        return Ok(None);
    };
    let track_payload = read_tag(manager, track, "sun track")?;
    let (count, _, _, row_class) = array_at(&track_payload, SUN_TRACK_DIRECTIONS)?;
    if count == 0 || row_class != SUN_DIRECTION_CLASS {
        return Ok(None);
    }
    Ok(Some((
        resource,
        record,
        record_payload,
        track,
        track_payload,
    )))
}

/// A copy of donor sun track `donor` whose every direction is `direction`.
fn sun_track(donor: &[u8], direction: [f32; 3]) -> AuthoringResult<Vec<u8>> {
    let mut payload = donor.to_vec();
    let (count, _, rows, row_class) = array_at(donor, SUN_TRACK_DIRECTIONS)?;
    if row_class != SUN_DIRECTION_CLASS || rows + count * 16 > donor.len() {
        return Err(invalid("The donor sun track has an unexpected layout"));
    }
    for index in 0..count {
        let at = rows + index * 16;
        for (k, value) in direction.into_iter().chain([0.]).enumerate() {
            payload[at + k * 4..at + k * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
    }
    Ok(payload)
}

/// A copy of donor sun record `donor` whose every track is `track` and whose sun shines all day
/// ([`SUN_WINDOWS`]).
fn sun_record(donor: &[u8], track: TagHash) -> AuthoringResult<Vec<u8>> {
    let mut payload = donor.to_vec();
    for (k, value) in SUN_WINDOWS.into_iter().enumerate() {
        write_u32(&mut payload, k * 4, value.to_bits())?;
    }
    for slot in 0..SUN_TRACK_SLOTS {
        let field = SUN_TRACKS + slot * 4;
        if read_u32(donor, field)? != u32::MAX {
            write_u32(&mut payload, field, track.0)?;
        }
    }
    Ok(payload)
}

/// A copy of donor sky table `table`, when its one row names a sky object collection that holds
/// objects. Stock maps also place shared empty collections, which stay.
fn sky_slot(
    manager: &PackageManager,
    table: u32,
    payload: Vec<u8>,
    resources: &[usize],
) -> AuthoringResult<Option<Slot>> {
    let [resource] = resources[..] else {
        return Ok(None);
    };
    let field = resource + SKY_COLLECTION;
    let collection = TagHash(read_u32(&payload, field)?);
    if class(manager, collection) != Some(SKY_COLLECTION_CLASS) {
        return Ok(None);
    }
    let collection_payload = read_tag(manager, collection, "sky object collection")?;
    if read_u64(&collection_payload, SKY_OBJECTS)? == 0 {
        return Ok(None);
    }
    Ok(Some(Slot::Sky {
        template: table,
        payload,
        field,
        collection,
        collection_payload,
    }))
}

/// A copy of donor ambient occlusion table `table`, when its one row names an occlusion record
/// with more than one probe.
fn occlusion_slot(
    manager: &PackageManager,
    table: u32,
    payload: Vec<u8>,
    resources: &[usize],
) -> AuthoringResult<Option<Slot>> {
    let [resource] = resources[..] else {
        return Ok(None);
    };
    let field = resource + OCCLUSION_RECORD;
    let record = TagHash(read_u32(&payload, field)?);
    if class(manager, record) != Some(OCCLUSION_RECORD_CLASS) {
        return Ok(None);
    }
    let record_payload = read_tag(manager, record, "ambient occlusion record")?;
    let (count, _, _, row_class) = array_at(&record_payload, OCCLUSION_PROBES)?;
    if count < 2 || row_class != OCCLUSION_PROBE_CLASS {
        return Ok(None);
    }
    Ok(Some(Slot::Occlusion {
        template: table,
        payload,
        field,
        record,
        record_payload,
    }))
}

/// The occlusion record's fixed header, through its selector at `+0x70`, and its arrays: the
/// three buffers' identifier mappings, the probes and the instance-to-probe indices. A mapping row
/// is a 64-bit identifier, a 32-bit offset and four zero bytes. For decorator buffers B and C,
/// the offset is an instance index. Shadowkeep 86657's binding routine at RVA 11634D0 adds the
/// draw's instance index before multiplying by the 16-byte or four-byte stride, respectively.
/// The third buffer holds grass decorators' four-byte instance rows ([`occlusion_record`]).
const OCCLUSION_HEADER: usize = 0x78;
const OCCLUSION_DECORATOR_BUFFER: usize = 0x38;
const OCCLUSION_DECORATOR_MAPPINGS: usize = 0x40;
const OCCLUSION_MAPPING_CLASS: u32 = 0x8080_726A;
const OCCLUSION_INDICES: usize = 0x60;
const OCCLUSION_INDEX_CLASS: u32 = 0x8080_000A;
const OCCLUSION_ARRAYS: [(usize, usize, u32); 5] = [
    (0x10, 16, OCCLUSION_MAPPING_CLASS),
    (0x28, 16, OCCLUSION_MAPPING_CLASS),
    (OCCLUSION_DECORATOR_MAPPINGS, 16, OCCLUSION_MAPPING_CLASS),
    (
        OCCLUSION_PROBES,
        OCCLUSION_PROBE_SIZE,
        OCCLUSION_PROBE_CLASS,
    ),
    (OCCLUSION_INDICES, 2, OCCLUSION_INDEX_CLASS),
];

/// The map's own occlusion record from donor record `donor`: its header with the selector at
/// `+0x70`, the donor's first probe, the native fallback that every stock record holds at the
/// origin, then `probes`, each a position, visibility and native signed-axis gradients.
///
/// The native consumer reads a lit object's probe through the `+0x60` array, which assigns a probe
/// to each cluster of the bubble's spatial tome, the cluster a connected-region query finds
/// around the object. The buffer lookups match identifiers of the donor's own instance sets. The
/// donor's array assigned the donor tome's clusters to the donor's probes, so with the map's
/// probes in their places it lit the map's objects from arbitrary points. The copy names none of
/// the donor's buffers, and `clusters` assigns each cluster of the area's own tome its probe.
/// Without a tome of the map's own, no cluster is assigned and a lit object takes the native
/// default.
///
/// `decorators` names the map's decorator occlusion buffer and, for each decorator record the area
/// places, its instance header identity and its first row there. The record maps them in its third
/// buffer, as Distant Shore's maps its grass record's identity to its 11,520 instance rows. A
/// decorator whose identity no row maps reads an ambient visibility of zero and draws black.
fn occlusion_record(
    donor: &[u8],
    probes: &[super::lighting::Probe],
    clusters: Option<&[u16]>,
    decorators: Option<(TagHash, &[(u64, u32)])>,
) -> AuthoringResult<Vec<u8>> {
    if donor.len() < OCCLUSION_HEADER || read_u64(donor, 0)? != donor.len() as u64 {
        return Err(invalid("The donor occlusion record has an invalid size"));
    }
    for (descriptor, stride, expected) in OCCLUSION_ARRAYS {
        if read_u64(donor, descriptor)? == 0 {
            continue;
        }
        let (count, _, rows, class) = array_at(donor, descriptor)?;
        if class != expected || rows + count * stride > donor.len() {
            return Err(invalid(format!(
                "The donor occlusion array at {descriptor:#x} has an invalid layout"
            )));
        }
    }
    if probes.len() + 1 >= usize::from(u16::MAX) || probes.iter().any(|probe| !probe.valid()) {
        return Err(invalid(
            "The map's occlusion probes exceed the native range",
        ));
    }
    let (count, _, rows, _) = array_at(donor, OCCLUSION_PROBES)?;
    if count == 0 {
        return Err(invalid("The donor occlusion record holds no probes"));
    }
    let mut payload = donor[..OCCLUSION_HEADER].to_vec();
    for buffer in OCCLUSION_BUFFERS {
        write_u32(&mut payload, buffer, u32::MAX)?;
    }
    for (descriptor, _, _) in OCCLUSION_ARRAYS {
        payload[descriptor..descriptor + 16].fill(0);
    }
    let mut table = donor[rows..rows + OCCLUSION_PROBE_SIZE].to_vec();
    for probe in probes {
        table.extend(probe.row().into_iter().flat_map(f32::to_le_bytes));
    }
    append_native_array(
        &mut payload,
        OCCLUSION_PROBES,
        OCCLUSION_PROBE_CLASS,
        probes.len() + 1,
        &table,
    )?;
    if let Some(clusters) = clusters {
        if clusters
            .iter()
            .any(|&probe| usize::from(probe) > probes.len())
        {
            return Err(invalid("A tome cluster names a probe the record lacks"));
        }
        let rows = clusters
            .iter()
            .flat_map(|probe| probe.to_le_bytes())
            .collect::<Vec<_>>();
        append_native_array(
            &mut payload,
            OCCLUSION_INDICES,
            OCCLUSION_INDEX_CLASS,
            clusters.len(),
            &rows,
        )?;
    }
    if let Some((buffer, records)) = decorators
        && !records.is_empty()
    {
        let mut rows = Vec::with_capacity(records.len() * 16);
        for &(identity, offset) in records {
            rows.extend(identity.to_le_bytes());
            rows.extend(offset.to_le_bytes());
            rows.extend(0u32.to_le_bytes());
        }
        write_u32(&mut payload, OCCLUSION_DECORATOR_BUFFER, buffer.0)?;
        append_native_array(
            &mut payload,
            OCCLUSION_DECORATOR_MAPPINGS,
            OCCLUSION_MAPPING_CLASS,
            records.len(),
            &rows,
        )?;
    }
    synchronize_payload_size(payload)
}

/// The tags a map's sky objects copy from the donor's sky: its first object's model reference,
/// the model that names, and that model's first transparent and light-shaft materials.
fn sky_templates(
    manager: &PackageManager,
    collection: &[u8],
) -> AuthoringResult<parhelion_import::halo_reach::level::native::sky::Templates> {
    let (count, _, rows, row_class) = array_at(collection, SKY_OBJECTS)?;
    if count == 0 || row_class != SKY_OBJECT_CLASS {
        return Err(invalid(format!(
            "The donor sky holds {count} objects of class {row_class:08X}, expected {SKY_OBJECT_CLASS:08X}"
        )));
    }
    let reference = TagHash(read_u32(collection, rows + SKY_OBJECT_REFERENCE)?);
    if class(manager, reference) != Some(SKY_REFERENCE_CLASS) {
        return Err(invalid(format!(
            "The donor's first sky object names {:08X}, not a sky model reference",
            reference.0
        )));
    }
    let model = TagHash(read_u32(
        &read_tag(manager, reference, "sky model reference")?,
        SKY_REFERENCE_MODEL,
    )?);
    if class(manager, model) != Some(MODEL_CLASS) {
        return Err(invalid(format!(
            "The donor's sky model reference names {:08X}, not an entity model",
            model.0
        )));
    }
    let payload = read_tag(manager, model, "sky model")?;
    let (meshes, _, mesh, _) = array_at(&payload, MODEL_MESHES)?;
    if meshes == 0 || mesh + MESH_SIZE > payload.len() {
        return Err(invalid("The donor's sky model has no mesh"));
    }
    let (parts, _, first_part, _) = array_at(&payload, mesh + MESH_PARTS)?;
    let material = |stage: usize| -> AuthoringResult<u32> {
        let start = usize::from(read_u16(&payload, mesh + MESH_STAGES + stage * 2)?);
        let end = usize::from(read_u16(&payload, mesh + MESH_STAGES + stage * 2 + 2)?);
        if start >= end || end > parts {
            return Err(invalid(format!(
                "The donor's sky model draws nothing in stage {stage}"
            )));
        }
        let material = read_u32(&payload, first_part + start * PART_SIZE)?;
        if class(manager, TagHash(material)) != Some(MATERIAL_CLASS) {
            return Err(invalid(format!(
                "The donor's sky model draws stage {stage} with {material:08X}, not a material"
            )));
        }
        Ok(material)
    };
    Ok(
        parhelion_import::halo_reach::level::native::sky::Templates {
            reference: reference.0,
            model: model.0,
            surface: material(TRANSPARENT_STAGE)?,
            shafts: material(LIGHT_SHAFT_STAGE)?,
        },
    )
}

/// A copy of donor sky object collection `donor` holding `sky`. Each object copies the donor's
/// first object with an unrotated, unscaled transform at the object's position, its world bounds,
/// its model reference and its draw order, and the collection's bounds enclose them all.
fn sky_collection(donor: &[u8], sky: &[SkyObject]) -> AuthoringResult<Vec<u8>> {
    let (count, _, rows, row_class) = array_at(donor, SKY_OBJECTS)?;
    if count == 0 || row_class != SKY_OBJECT_CLASS || rows + SKY_OBJECT_SIZE > donor.len() {
        return Err(invalid("The donor sky has an unexpected layout"));
    }
    let template = &donor[rows..rows + SKY_OBJECT_SIZE];
    let float4 = |values: [f32; 3], w: f32| {
        values
            .into_iter()
            .chain([w])
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>()
    };
    let mut objects = Vec::with_capacity(sky.len() * SKY_OBJECT_SIZE);
    let mut bounds = Vec::with_capacity(sky.len() * SKY_BOUNDS_SIZE);
    let mut indices = Vec::with_capacity(sky.len() * 4);
    let mut low = [f32::MAX; 3];
    let mut high = [f32::MIN; 3];
    for (index, object) in sky.iter().enumerate() {
        let mut row = template.to_vec();
        // Column-major: three unit axes, then the position.
        let mut transform = [0f32; 16];
        transform[0] = 1.;
        transform[5] = 1.;
        transform[10] = 1.;
        transform[12..15].copy_from_slice(&object.position);
        transform[15] = 1.;
        for (k, value) in transform.into_iter().enumerate() {
            row[k * 4..k * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        let extent = [float4(object.minimum, 1.), float4(object.maximum, 1.)].concat();
        row[SKY_OBJECT_BOUNDS..SKY_OBJECT_BOUNDS + 32].copy_from_slice(&extent);
        write_u32(&mut row, SKY_OBJECT_REFERENCE, object.reference.0)?;
        row[SKY_OBJECT_ORDER..SKY_OBJECT_ORDER + 4].copy_from_slice(&object.order.to_le_bytes());
        objects.extend_from_slice(&row);
        bounds.extend_from_slice(&extent);
        // Native visibility registration reads 48-byte rows, including a separate
        // 64-bit field after the bounds. Stock sky rows leave the final 16 bytes zero.
        bounds.extend_from_slice(&[0; 16]);
        indices.extend_from_slice(
            &u32::try_from(index)
                .map_err(|_| invalid("Too many sky objects"))?
                .to_le_bytes(),
        );
        for k in 0..3 {
            low[k] = low[k].min(object.minimum[k]);
            high[k] = high[k].max(object.maximum[k]);
        }
    }
    let mut payload = donor[..SKY_FIXED].to_vec();
    // An empty collection keeps the inverted bounds a stock empty collection holds.
    payload[SKY_EXTENT..SKY_EXTENT + 16].copy_from_slice(&float4(low, 1.));
    payload[SKY_EXTENT + 16..SKY_EXTENT + 32].copy_from_slice(&float4(high, 1.));
    append_native_array(
        &mut payload,
        SKY_OBJECTS,
        SKY_OBJECT_CLASS,
        sky.len(),
        &objects,
    )?;
    append_native_array(
        &mut payload,
        SKY_BOUNDS,
        SKY_BOUNDS_CLASS,
        sky.len(),
        &bounds,
    )?;
    append_native_array(
        &mut payload,
        SKY_INDICES,
        SKY_INDEX_CLASS,
        sky.len(),
        &indices,
    )?;
    synchronize_payload_size(payload)
}

/// A copy of donor world collision table `table`, when its one row names a world collision record.
fn world_slot(
    manager: &PackageManager,
    table: u32,
    payload: Vec<u8>,
    resources: &[usize],
) -> AuthoringResult<Option<Slot>> {
    let [resource] = resources[..] else {
        return Ok(None);
    };
    let field = resource + WORLD_RECORD;
    let record = TagHash(read_u32(&payload, field)?);
    if class(manager, record) != Some(WORLD_RECORD_CLASS) {
        return Ok(None);
    }
    let record_payload = read_tag(manager, record, "world collision record")?;
    // The donor's record, its main packfile, its mesh packfiles and its mesh records with the
    // packfiles they name. Its materials stay, since the copy names them too.
    let mut donor = vec![record, TagHash(read_u32(&record_payload, WORLD_COMPOUND)?)];
    if read_u64(&record_payload, WORLD_PACKFILES)? != 0 {
        let (count, _, rows, _) = array_at(&record_payload, WORLD_PACKFILES)?;
        for index in 0..count {
            donor.push(TagHash(read_u32(&record_payload, rows + index * 4)?));
        }
    }
    if read_u64(&record_payload, WORLD_MESH_RECORDS)? != 0 {
        let (count, _, rows, _) = array_at(&record_payload, WORLD_MESH_RECORDS)?;
        for index in 0..count {
            let mesh = TagHash(read_u32(&record_payload, rows + index * 4)?);
            donor.push(mesh);
            if class(manager, mesh) == Some(WORLD_MESH_RECORD_CLASS) {
                let mesh_payload = read_tag(manager, mesh, "world collision mesh record")?;
                donor.push(TagHash(read_u32(
                    &mesh_payload,
                    WORLD_MESH_RECORD_PACKFILE,
                )?));
            }
        }
    }
    Ok(Some(Slot::World {
        template: table,
        payload,
        field,
        record,
        record_payload,
        donor,
    }))
}

/// A copy of donor world collision record `donor` whose main packfile is `world.compound` and
/// whose packfile shapes are `world.meshes`, with no mesh records. Each mesh's one palette entry
/// takes the donor's most used material.
fn world_record(donor: &[u8], world: &World) -> AuthoringResult<Vec<u8>> {
    let mut payload = donor.to_vec();
    let (count, _, rows, materials_class) = array_at(donor, WORLD_PALETTE_MATERIALS)?;
    let mut uses = std::collections::BTreeMap::<u16, usize>::new();
    for index in 0..count {
        *uses.entry(read_u16(donor, rows + index * 2)?).or_default() += 1;
    }
    let material = uses
        .iter()
        .max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0)))
        .map(|(&material, _)| material)
        .ok_or_else(|| invalid("The donor world collision maps no materials"))?;
    replace_array(&mut payload, WORLD_MESH_RECORDS, 0, 4, 0, &[])?;
    let palettes = world
        .meshes
        .iter()
        .flat_map(|_| material.to_le_bytes())
        .collect::<Vec<_>>();
    append_native_array(
        &mut payload,
        WORLD_PALETTE_MATERIALS,
        materials_class,
        world.meshes.len(),
        &palettes,
    )?;
    let (_, _, _, starts_class) = array_at(donor, WORLD_PALETTE_STARTS)?;
    let starts = (0..world.meshes.len())
        .map(|index| u16::try_from(index).map_err(|_| invalid("Too many world collision meshes")))
        .collect::<AuthoringResult<Vec<_>>>()?
        .into_iter()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    append_native_array(
        &mut payload,
        WORLD_PALETTE_STARTS,
        starts_class,
        world.meshes.len(),
        &starts,
    )?;
    let (_, _, _, packfiles_class) = array_at(donor, WORLD_PACKFILES)?;
    let packfiles = world
        .meshes
        .iter()
        .flat_map(|tag| tag.0.to_le_bytes())
        .collect::<Vec<_>>();
    replace_array(
        &mut payload,
        WORLD_PACKFILES,
        packfiles_class,
        4,
        world.meshes.len(),
        &packfiles,
    )?;
    write_u32(&mut payload, WORLD_COMPOUND, world.compound.0)?;
    synchronize_payload_size(payload)
}

/// A copy of donor spawn set `donor` holding `spawns`, under the donor's first point's set name.
fn spawn_set(donor: &[u8], spawns: &[Spawn]) -> AuthoringResult<Vec<u8>> {
    let (_, _, rows, class) = array_at(donor, SPAWN_POINTS)?;
    // The donor's points follow its fixed fields directly, so those fields end at the descriptor.
    let fixed = SPAWN_POINTS + 0x10;
    if class != SPAWN_POINT_CLASS || rows != fixed + 0x18 {
        return Err(invalid("The donor spawn set has an unexpected layout"));
    }
    let name = read_u32(donor, rows + SPAWN_POINT_SET)?;
    let mut points = Vec::with_capacity(spawns.len() * SPAWN_POINT_SIZE);
    for spawn in spawns {
        let half = spawn.yaw / 2.;
        let [x, y, z] = spawn.position;
        for value in [0., 0., half.sin(), half.cos(), x, y, z, 1.] {
            points.extend_from_slice(&f32::to_le_bytes(value));
        }
        points.extend_from_slice(&name.to_le_bytes());
        points.resize(points.len() + SPAWN_POINT_SIZE - SPAWN_POINT_SET - 4, 0);
    }
    let mut payload = donor[..fixed].to_vec();
    append_native_array(
        &mut payload,
        SPAWN_POINTS,
        SPAWN_POINT_CLASS,
        spawns.len(),
        &points,
    )?;
    synchronize_payload_size(payload)
}

/// An owner's component class and the offset of its resource.
fn component(
    manager: &PackageManager,
    owner: u32,
) -> AuthoringResult<Option<(u32, usize, Vec<u8>)>> {
    let Ok(payload) = read_tag(manager, TagHash(owner), "component owner") else {
        return Ok(None);
    };
    let relative = read_i64(&payload, OWNER_RESOURCE)?;
    let Some(resource) = OWNER_RESOURCE
        .checked_add_signed(relative as isize)
        .filter(|&at| relative != 0 && at >= 4 && at + 4 <= payload.len())
    else {
        return Ok(None);
    };
    Ok(Some((read_u32(&payload, resource - 4)?, resource, payload)))
}

/// Whether `entity` is donor scenery: a model, physical body, effect flow, shadowing light or
/// lens flare. These components can also appear on rows with inline placement resources.
/// Distant Shore's invisible walls and kill boxes have only a physical body. The Other Side's
/// light entities reference the same light records as direct shadowing-light placements.
pub(super) fn leftover_entity(manager: &PackageManager, entity: u32) -> AuthoringResult<bool> {
    if class(manager, TagHash(entity)) != Some(ENTITY_CLASS) {
        return Ok(false);
    }
    let payload = read_tag(manager, TagHash(entity), "entity")?;
    if read_u64(&payload, ENTITY_COMPONENTS)? == 0 {
        return Ok(false);
    }
    let (count, _, rows, _) = array_at(&payload, ENTITY_COMPONENTS)?;
    for index in 0..count {
        let owner = read_u32(&payload, rows + index * COMPONENT_SIZE)?;
        if matches!(
            component(manager, owner)?,
            Some((
                RENDER_CLASS | BODY_CLASS | EFFECT_CLASS | LIGHT_CLASS | FLARE_CLASS,
                _,
                _
            ))
        ) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Gives placement table `payload` a new row array of copies of its rows at `sources`, in that
/// order and repeats allowed, and returns their offsets. Each copy's relative pointers keep their
/// source row's targets, the inline resources that stay where they are.
pub(super) fn rebuild_rows(
    payload: &mut Vec<u8>,
    sources: &[usize],
) -> AuthoringResult<Vec<usize>> {
    let mut rows = Vec::with_capacity(sources.len() * ROW_SIZE);
    let mut pointers = Vec::with_capacity(sources.len() * ROW_POINTERS.len());
    for &row in sources {
        rows.extend_from_slice(
            payload
                .get(row..row + ROW_SIZE)
                .ok_or_else(|| invalid("A placement row leaves its table"))?,
        );
        for offset in ROW_POINTERS {
            let field = row + offset;
            let relative = read_i64(payload, field)?;
            let target = if relative == 0 {
                None
            } else {
                Some(
                    field
                        .checked_add_signed(relative as isize)
                        .filter(|&at| at < payload.len())
                        .ok_or_else(|| {
                            invalid("The donor placement pointer is outside its table")
                        })?,
                )
            };
            pointers.push((offset, target));
        }
    }
    append_native_array(payload, TABLE_ROWS, TABLE_ROW_CLASS, sources.len(), &rows)?;
    if sources.is_empty() {
        return Ok(Vec::new());
    }
    let table_rows = array_at(payload, TABLE_ROWS)?.2;
    for (index, (offset, target)) in pointers.into_iter().enumerate() {
        if let Some(target) = target {
            write_relative_pointer(
                payload,
                table_rows + index / ROW_POINTERS.len() * ROW_SIZE + offset,
                target,
            )?;
        }
    }
    *payload = synchronize_payload_size(std::mem::take(payload))?;
    Ok((0..sources.len())
        .map(|index| table_rows + index * ROW_SIZE)
        .collect())
}

/// Sets placement row `row`'s placement identity, the 64-bit value a spawn rule names it by.
pub(super) fn set_row_identity(
    payload: &mut [u8],
    row: usize,
    identity: u64,
) -> AuthoringResult<()> {
    crate::tag_payload::write_u64(payload, row + ROW_IDENTITY, identity)
}

/// Placement row `row`'s placement identity.
pub(super) fn row_identity(payload: &[u8], row: usize) -> AuthoringResult<u64> {
    read_u64(payload, row + ROW_IDENTITY)
}

/// The offsets of the rows of placement table `payload`, none when it has no rows.
pub(super) fn table_rows(payload: &[u8]) -> AuthoringResult<Vec<usize>> {
    if read_u64(payload, TABLE_ROWS)? == 0 {
        return Ok(Vec::new());
    }
    let (count, _, rows, _) = array_at(payload, TABLE_ROWS)?;
    Ok((0..count).map(|index| rows + index * ROW_SIZE).collect())
}

/// Moves placement row `row` far below the map.
pub(super) fn bury(payload: &mut [u8], row: usize) {
    payload[row + ROW_TRANSLATION + 8..row + ROW_TRANSLATION + 12]
        .copy_from_slice(&BURIED.to_le_bytes());
}

/// Moves every row of placement table `payload` whose entity is a donor leftover far below the
/// map, keeping each row and its index. Returns the offsets of the rows that stay, or none when no
/// row moved.
pub(super) fn bury_leftovers(
    manager: &PackageManager,
    payload: &mut [u8],
) -> AuthoringResult<Option<Vec<usize>>> {
    if read_u64(payload, TABLE_ROWS)? == 0 {
        return Ok(None);
    }
    let (count, _, rows, class) = array_at(payload, TABLE_ROWS)?;
    if class != TABLE_ROW_CLASS {
        return Err(invalid(format!(
            "A placement table holds rows of class {class:08X}, expected {TABLE_ROW_CLASS:08X}"
        )));
    }
    let mut moved = false;
    let mut kept = Vec::new();
    for row in (0..count).map(|index| rows + index * ROW_SIZE) {
        if leftover_entity(manager, read_u32(payload, row)?)? {
            bury(payload, row);
            moved = true;
        } else {
            kept.push(row);
        }
    }
    Ok(moved.then_some(kept))
}

/// The translation of placement row `row` of `payload`.
pub(super) fn row_translation(payload: &[u8], row: usize) -> AuthoringResult<[f32; 3]> {
    let mut translation = [0f32; 3];
    for (k, value) in translation.iter_mut().enumerate() {
        *value = f32::from_bits(read_u32(payload, row + ROW_TRANSLATION + k * 4)?);
    }
    Ok(translation)
}

/// Places row `row` of `payload` at `translation`, turned `yaw` radians about the vertical axis.
pub(super) fn place_row(
    payload: &mut [u8],
    row: usize,
    yaw: f32,
    translation: [f32; 3],
) -> AuthoringResult<()> {
    let half = yaw / 2.;
    for (k, value) in [0., 0., half.sin(), half.cos()].into_iter().enumerate() {
        write_u32(payload, row + ROW_ROTATION + k * 4, value.to_bits())?;
    }
    for (k, value) in translation.into_iter().enumerate() {
        write_u32(payload, row + ROW_TRANSLATION + k * 4, value.to_bits())?;
    }
    Ok(())
}

/// The prop `entity` is when a render owner among its components draws a model.
fn prop(manager: &PackageManager, entity: u32) -> AuthoringResult<Option<Prop>> {
    if class(manager, TagHash(entity)) != Some(ENTITY_CLASS) {
        return Ok(None);
    }
    let payload = read_tag(manager, TagHash(entity), "entity")?;
    if read_u64(&payload, ENTITY_COMPONENTS)? == 0 {
        return Ok(None);
    }
    let (count, _, rows, _) = array_at(&payload, ENTITY_COMPONENTS)?;
    for index in 0..count {
        let owner = read_u32(&payload, rows + index * COMPONENT_SIZE)?;
        if let Some((RENDER_CLASS, resource, owner_payload)) = component(manager, owner)? {
            let model = read_u32(&owner_payload, resource + RENDER_MODEL)?;
            if class(manager, TagHash(model)) == Some(MODEL_CLASS) {
                return Ok(Some(Prop {
                    model,
                    entity,
                    render: owner,
                }));
            }
        }
    }
    Ok(None)
}

/// The tags a payload's tag rows at `descriptor` name, direct or by 64-bit hash.
fn tag_rows(
    manager: &PackageManager,
    payload: &[u8],
    descriptor: usize,
) -> AuthoringResult<Vec<TagHash>> {
    let (count, _, rows, _) = array_at(payload, descriptor)?;
    (0..count)
        .map(|index| {
            let row = rows + index * TAG_ROW_SIZE;
            let tag = read_u32(payload, row)?;
            if tag != u32::MAX {
                return Ok(TagHash(tag));
            }
            let hash = read_u64(payload, row + 8)?;
            manager
                .lookup
                .tag64_entries
                .get(&hash)
                .map(|entry| entry.hash32)
                .ok_or_else(|| invalid(format!("Tag row names unknown hash {hash:016X}")))
        })
        .collect()
}

fn class(manager: &PackageManager, tag: TagHash) -> Option<u32> {
    manager.get_entry(tag).map(|entry| entry.reference)
}

/// Whether bounds record `payload` (`8080964E`) bounds anything: `80FEB3E9`'s boxes hold inverted
/// `FLT_MAX` corners.
fn bounds_any(payload: &[u8]) -> AuthoringResult<bool> {
    if payload.len() < 0x60 {
        return Ok(false);
    }
    for index in 0..3 {
        for k in 0..3 {
            let low = f32::from_bits(read_u32(payload, index * 0x20 + k * 4)?);
            let high = f32::from_bits(read_u32(payload, index * 0x20 + 0x10 + k * 4)?);
            if low <= high && low.is_finite() && high.abs() < 1e30 {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// The bounds record `donor` (`8080964E`) with each of its three boxes widened to also hold
/// `pieces`, the first and third reaching [`BOUNDS_MARGIN`] past them and the second
/// [`WIDE_BOUNDS_MARGIN`]. A record whose boxes bound nothing, as `80FEB3E9`'s inverted `FLT_MAX`
/// corners, gives none.
fn widened_bounds(donor: &[u8], pieces: &[Piece]) -> AuthoringResult<Option<Vec<u8>>> {
    if donor.len() < 0x60 || pieces.is_empty() {
        return Ok(None);
    }
    let mut low = [f32::MAX; 3];
    let mut high = [f32::MIN; 3];
    for piece in pieces {
        for k in 0..3 {
            low[k] = low[k].min(piece.minimum[k]);
            high[k] = high[k].max(piece.maximum[k]);
        }
    }
    let value = |at: usize| -> AuthoringResult<f32> { Ok(f32::from_bits(read_u32(donor, at)?)) };
    let mut payload = donor.to_vec();
    let mut bounds_any = false;
    for (index, margin) in [BOUNDS_MARGIN, WIDE_BOUNDS_MARGIN, BOUNDS_MARGIN]
        .into_iter()
        .enumerate()
    {
        let (least, greatest) = (index * 0x20, index * 0x20 + 0x10);
        for k in 0..3 {
            let (donor_low, donor_high) = (value(least + k * 4)?, value(greatest + k * 4)?);
            if !(donor_low <= donor_high && donor_low.is_finite() && donor_high.abs() < 1e30) {
                continue;
            }
            bounds_any = true;
            let widened_low = donor_low.min(low[k] - margin);
            let widened_high = donor_high.max(high[k] + margin);
            write_u32(&mut payload, least + k * 4, widened_low.to_bits())?;
            write_u32(&mut payload, greatest + k * 4, widened_high.to_bits())?;
        }
    }
    Ok(bounds_any.then_some(payload))
}

/// Replaces the tag rows at `descriptor` with `tags`, each named directly.
fn set_tag_rows(payload: &mut Vec<u8>, descriptor: usize, tags: &[TagHash]) -> AuthoringResult<()> {
    let (_, _, _, row_class) = array_at(payload, descriptor)?;
    let mut rows = Vec::with_capacity(tags.len() * TAG_ROW_SIZE);
    for tag in tags {
        rows.extend_from_slice(&tag.0.to_le_bytes());
        rows.extend_from_slice(&DIRECT_TAG_ROW.to_le_bytes());
        rows.extend_from_slice(&0u64.to_le_bytes());
    }
    replace_array(
        payload,
        descriptor,
        row_class,
        TAG_ROW_SIZE,
        tags.len(),
        &rows,
    )
}

/// Replaces the native array at `descriptor`, whose rows are `stride` bytes, with `count` new
/// `rows` appended to the payload, and blanks the rows it held. The old rows stay in the payload
/// otherwise, where nothing reads them but a search of the payload still finds the donor tags they
/// name: the copied container of a converted Paradiso still held the 20 tables of the Midtown
/// container it copies, through which the donor trim ([`super::plan`]) found 3,344 of Midtown's
/// records it does not place.
fn replace_array(
    payload: &mut Vec<u8>,
    descriptor: usize,
    class: u32,
    stride: usize,
    count: usize,
    rows: &[u8],
) -> AuthoringResult<()> {
    if read_u64(payload, descriptor)? != 0 {
        let (held, _, start, _) = array_at(payload, descriptor)?;
        let end = held
            .checked_mul(stride)
            .and_then(|size| start.checked_add(size))
            .filter(|&end| end <= payload.len())
            .ok_or_else(|| invalid("A replaced native array runs past its payload"))?;
        payload[start..end].fill(0);
    }
    append_native_array(payload, descriptor, class, count, rows)
}

impl Chain {
    /// A complete visibility placement row from this donor, retained only as a native template.
    pub(super) fn layout(&self) -> &[u8] {
        &self.layout_payload
    }

    pub(super) fn visibility(
        &self,
        manager: &PackageManager,
    ) -> AuthoringResult<(TagHash, Vec<u8>)> {
        for container in tag_rows(manager, &self.bubble.payload, BUBBLE_CONTAINERS)? {
            let payload = read_tag(manager, container, "visibility container")?;
            let (count, rows) = if read_u64(&payload, CONTAINER_TABLES)? == 0 {
                (0, 0)
            } else {
                let (count, _, rows, _) = array_at(&payload, CONTAINER_TABLES)?;
                (count, rows)
            };
            for index in 0..count {
                let tag = TagHash(read_u32(&payload, rows + index * 4)?);
                let table = read_tag(manager, tag, "visibility table")?;
                if read_u64(&table, TABLE_ROWS)? != 1 {
                    continue;
                }
                let (_, _, row, _) = array_at(&table, TABLE_ROWS)?;
                let relative = read_i64(&table, row + ROW_RESOURCE)?;
                let Some(resource) = (row + ROW_RESOURCE)
                    .checked_add_signed(relative as isize)
                    .filter(|&at| relative != 0 && at >= 4 && at + 24 <= table.len())
                else {
                    continue;
                };
                if read_u32(&table, resource - 4)? == VISIBILITY {
                    return Ok((tag, table));
                }
            }
        }
        Err(invalid(
            "The donor has no standalone visibility placement template",
        ))
    }

    /// The chain of the map root of the donor's world `donor`, for the map `name`. A Crucible
    /// donor's world takes its activity's name.
    #[allow(clippy::cognitive_complexity)]
    pub(super) fn load(
        directory: &Path,
        manager: &PackageManager,
        companions: &mut Companions,
        donor: &str,
        name: &str,
    ) -> AuthoringResult<Self> {
        let (root, _) = super::donor_tag(manager, &format!("map:{donor}:root"))?;
        let root = Owner::load_tag(
            directory,
            manager,
            companions,
            root,
            MAP_ROOT_CLASS,
            "map root",
        )?;
        let bubble_name = BubbleName::new(donor, name);
        let layout = TagHash(read_u32(&root.payload, ROOT_LAYOUT)?);
        if class(manager, layout) != Some(LAYOUT_CLASS) {
            return Err(invalid(format!(
                "Donor {donor}'s map root names no bubble layout"
            )));
        }
        let mut layout_payload = renamed_counted(
            &read_tag(manager, layout, "bubble layout")?,
            LAYOUT_NAME,
            donor,
            name,
            "bubble layout",
        )?;
        if bubble_name.rename(&mut layout_payload) == 0 {
            return Err(invalid(format!(
                "Donor {donor}'s bubble layout does not list its bubble"
            )));
        }
        let parents = tag_rows(manager, &root.payload, ROOT_PARENTS)?;
        let [parent] = parents[..] else {
            return Err(invalid(format!(
                "Donor {donor}'s map root names {} bubble parents, expected one",
                parents.len()
            )));
        };
        let parent = Owner::load_tag(
            directory,
            manager,
            companions,
            parent,
            BUBBLE_PARENT_CLASS,
            "bubble parent",
        )?;
        if !bubble_name.named(&parent.payload) {
            return Err(invalid(format!(
                "Donor {donor}'s bubble parent does not name its bubble {donor}"
            )));
        }
        let mut bounds_rows = Vec::new();
        if read_u64(&parent.payload, PARENT_BOUNDS)? != 0 {
            for tag in tag_rows(manager, &parent.payload, PARENT_BOUNDS)? {
                let owner = if class(manager, tag) == Some(BOUNDS_CLASS)
                    && bounds_any(&read_tag(manager, tag, "bubble bounds")?)?
                {
                    Some(Owner::load_tag(
                        directory,
                        manager,
                        companions,
                        tag,
                        BOUNDS_CLASS,
                        "bubble bounds",
                    )?)
                } else {
                    None
                };
                bounds_rows.push((tag, owner));
            }
        }
        let bubble = TagHash(read_u32(&parent.payload, PARENT_BUBBLE)?);
        let bubble = Owner::load_tag(
            directory,
            manager,
            companions,
            bubble,
            BUBBLE_CLASS,
            "bubble",
        )?;
        // The container that places the prop is the template: its companion loads the prop's
        // entity, owners and the stock records they name.
        let mut found = None;
        let mut statics = None;
        for container in tag_rows(manager, &bubble.payload, BUBBLE_CONTAINERS)? {
            if class(manager, container) != Some(CONTAINER_CLASS) {
                continue;
            }
            if found.is_some() && statics.is_some() {
                break;
            }
            let payload = read_tag(manager, container, "map container")?;
            if read_u64(&payload, CONTAINER_TABLES)? == 0 {
                continue;
            }
            let (count, _, rows, _) = array_at(&payload, CONTAINER_TABLES)?;
            for index in 0..count {
                let tag = TagHash(read_u32(&payload, rows + index * 4)?);
                if class(manager, tag) != Some(TABLE_CLASS) {
                    continue;
                }
                let table = read_tag(manager, tag, "placement table")?;
                // An empty table can end before an array header.
                if read_u64(&table, TABLE_ROWS)? == 0 {
                    continue;
                }
                if statics.is_none() {
                    statics = static_group(manager, &table)?;
                }
                if found.is_some() {
                    continue;
                }
                let (count, _, rows, _) = array_at(&table, TABLE_ROWS)?;
                for index in 0..count {
                    let row = rows + index * ROW_SIZE;
                    if read_i64(&table, row + ROW_RESOURCE)? != 0 {
                        continue;
                    }
                    if let Some(prop) = prop(manager, read_u32(&table, row)?)? {
                        found = Some((container, tag, prop, table[row..row + ROW_SIZE].to_vec()));
                        break;
                    }
                }
            }
        }
        let containers = tag_rows(manager, &bubble.payload, BUBBLE_CONTAINERS)?;
        let empty = empty_table(manager, &containers)?;
        let sound_points = sound_points(manager, &containers)?;
        let template_containers = world_containers(manager, lights::LIGHT_WORLD)?;
        let light_template = light_template(manager, &template_containers)?;
        let environment_template = environment_template(manager, &template_containers)?;
        let kept = containers
            .into_iter()
            .map(|tag| keep(directory, manager, companions, tag, empty, false))
            .collect::<AuthoringResult<Vec<_>>>()?;
        let donor_sky = kept.iter().any(|kept| match kept {
            Kept::Filtered { slots, .. } => {
                slots.iter().any(|slot| matches!(slot, Slot::Sky { .. }))
            }
            Kept::Unchanged => false,
        });
        let mut borrowed_sky = None;
        if !donor_sky {
            for world in SKY_WORLDS {
                borrowed_sky = world_sky(manager, world)?;
                if borrowed_sky.is_some() {
                    break;
                }
            }
        }
        let (container, prop_table, prop, prop_row) = found.ok_or_else(|| {
            invalid(format!(
                "Donor {donor} places no prop with a render model whose buffers the statics copy"
            ))
        })?;
        let statics = statics.ok_or_else(|| {
            invalid(format!(
                "Donor {donor} places no static instance group for the map's statics to copy"
            ))
        })?;
        let container = Owner::load_tag(
            directory,
            manager,
            companions,
            container,
            CONTAINER_CLASS,
            "map container",
        )?;
        Ok(Self {
            root,
            layout,
            layout_payload,
            parent,
            bubble_name,
            bubble,
            kept,
            container,
            prop_table,
            prop_row,
            prop,
            statics,
            sound_points,
            light: None,
            borrowed_sky,
            light_template,
            lights: Vec::new(),
            environment_template,
            environment: None,
            bounds_rows,
            decorators: None,
        })
    }

    /// This chain, as loaded for the donor world `donor`, for the map `name` instead. Only the
    /// bubble layout and the bubble's name differ between the maps of one donor world, as between
    /// a campaign's connected areas, and loading the chain again took about seven seconds for
    /// each. `self` must be as [`Self::load`] left it.
    pub(super) fn renamed(
        &self,
        manager: &PackageManager,
        donor: &str,
        name: &str,
    ) -> AuthoringResult<Self> {
        let bubble_name = BubbleName::new(donor, name);
        let mut layout_payload = renamed_counted(
            &read_tag(manager, self.layout, "bubble layout")?,
            LAYOUT_NAME,
            donor,
            name,
            "bubble layout",
        )?;
        if bubble_name.rename(&mut layout_payload) == 0 {
            return Err(invalid(format!(
                "Donor {donor}'s bubble layout does not list its bubble"
            )));
        }
        Ok(Self {
            layout_payload,
            bubble_name,
            ..self.clone()
        })
    }

    /// Streaming ownership masks belong to the authored graph, including containers whose
    /// placement tables can otherwise be retained unchanged.
    pub(super) fn own_containers(
        &mut self,
        directory: &Path,
        manager: &PackageManager,
        companions: &mut Companions,
    ) -> AuthoringResult<()> {
        let containers = tag_rows(manager, &self.bubble.payload, BUBBLE_CONTAINERS)?;
        for (tag, kept) in containers.into_iter().zip(&mut self.kept) {
            if matches!(kept, Kept::Unchanged) {
                *kept = keep(directory, manager, companions, tag, None, true)?;
            }
        }
        Ok(())
    }

    /// The donor's first table of sound points, which the map's ambience copies.
    pub(super) fn sound_points(&self) -> Option<u32> {
        self.sound_points
    }

    /// The volume texture of the atmosphere the donor places beside its sun, which the map's own
    /// atmosphere volume recolors and stands in for.
    pub(super) fn atmosphere_volume(&self) -> AuthoringResult<Option<u32>> {
        self.atmosphere_field(ATMOSPHERE_VOLUME)
    }

    /// The image of the atmosphere the donor places beside its sun, which the map's open copy
    /// stands in for.
    pub(super) fn atmosphere_image(&self) -> AuthoringResult<Option<u32>> {
        self.atmosphere_field(ATMOSPHERE_IMAGE)
    }

    /// The distance lookup of the atmosphere the donor places beside its sun, which the map's
    /// black copy stands in for.
    pub(super) fn atmosphere_lookup(&self) -> AuthoringResult<Option<u32>> {
        self.atmosphere_field(ATMOSPHERE_LOOKUP)
    }

    /// The tag the donor's atmosphere beside its sun names at `field`, when it names one.
    fn atmosphere_field(&self, field: usize) -> AuthoringResult<Option<u32>> {
        for kept in &self.kept {
            let Kept::Filtered { slots, .. } = kept else {
                continue;
            };
            for slot in slots {
                if let Slot::Sun {
                    payload,
                    atmosphere: Some(resource),
                    ..
                } = slot
                {
                    let tag = read_u32(payload, resource + field)?;
                    return Ok((tag != u32::MAX).then_some(tag));
                }
            }
        }
        Ok(None)
    }

    /// Entries the chain takes ([`Addresses`]).
    pub(super) fn entries(&self) -> usize {
        self.addresses().total
    }

    /// Where the chain's tags go, before any payload is built.
    fn addresses(&self) -> Addresses {
        let sun = self.sun().is_some();
        let mut next = FIXED_ENTRIES;
        let copies = self
            .kept
            .iter()
            .map(|kept| match kept {
                Kept::Unchanged => None,
                Kept::Filtered { container, slots } => {
                    let copy = next;
                    let companion = matches!(container, Container::Enrolled(_)).then_some(copy + 1);
                    next += container.entries();
                    let slots = slots
                        .iter()
                        .map(|slot| {
                            let first = next;
                            next += slot.entries(sun);
                            first
                        })
                        .collect();
                    Some(CopyEntries {
                        copy,
                        companion,
                        slots,
                    })
                }
            })
            .collect();
        let borrowed_sky = next;
        let lights = borrowed_sky
            + self
                .borrowed_sky
                .as_ref()
                .map_or(0, |slot| slot.entries(sun));
        let bounds = lights
            + self
                .light_template
                .as_ref()
                .map_or(0, |template| template.entries(&self.lights));
        let environment = bounds
            + 2 * self
                .bounds_rows
                .iter()
                .filter(|(_, owner)| owner.is_some())
                .count();
        Addresses {
            copies,
            borrowed_sky,
            lights,
            bounds,
            environment,
            total: environment + environment::entries(self.placed_environment()),
        }
    }

    /// The packages of the donor world records the chain copies: its map root's, bubble parent's
    /// and bubble's.
    pub(super) fn donor_packages(&self) -> impl Iterator<Item = u16> {
        [self.root.tag, self.parent.tag, self.bubble.tag]
            .into_iter()
            .map(|tag| tag.pkg_id())
    }

    /// The donor prop, which converted pieces copy.
    pub(super) fn prop(&self) -> Prop {
        self.prop
    }

    /// The donor world collision's main packfile and its mesh packfiles, which the map's own world
    /// collision copies.
    pub(super) fn world_templates(&self) -> AuthoringResult<(u32, Vec<u32>)> {
        let record = self
            .kept
            .iter()
            .filter_map(|kept| match kept {
                Kept::Filtered { slots, .. } => Some(slots),
                Kept::Unchanged => None,
            })
            .flatten()
            .find_map(|slot| match slot {
                Slot::World { record_payload, .. } => Some(record_payload),
                _ => None,
            })
            .ok_or_else(|| invalid("The donor bubble places no world collision record"))?;
        let (count, _, rows, _) = array_at(record, WORLD_PACKFILES)?;
        if count == 0 {
            return Err(invalid("The donor world collision has no mesh packfiles"));
        }
        let meshes = (0..count)
            .map(|index| read_u32(record, rows + index * 4))
            .collect::<AuthoringResult<Vec<_>>>()?;
        Ok((read_u32(record, WORLD_COMPOUND)?, meshes))
    }

    /// Sets the direction the map's baked light mostly comes from, which its sun takes.
    pub(super) fn set_light(&mut self, light: Option<[f32; 3]>) {
        self.light = light;
    }

    /// Sets the map's own lights, which its container places when a light template was found.
    pub(super) fn set_lights(&mut self, lights: Vec<Light>) {
        self.lights = lights;
    }

    /// Sets the map's own environment, which its container places when an environment template
    /// was found.
    pub(super) fn set_environment(&mut self, environment: Option<Environment>) {
        self.environment = environment;
    }

    /// Sets the stock decorator placement the map's decorator records' rows copy.
    pub(super) fn set_decorators(&mut self, decorators: Option<Decorators>) {
        self.decorators = decorators;
    }

    /// The environment the map's container places.
    fn placed_environment(&self) -> Option<&Environment> {
        self.environment_template
            .as_ref()
            .and(self.environment.as_ref())
    }

    /// The direction the map's sun takes: its baked light's, when that lies above the horizon.
    fn sun(&self) -> Option<[f32; 3]> {
        self.light.filter(|direction| {
            direction.iter().all(|v| v.is_finite()) && direction[2] >= SUN_LOWEST
        })
    }

    /// Whether the donor places a table of cubemap volumes, which the map's own volumes replace.
    pub(super) fn cubemaps(&self) -> bool {
        self.kept
            .iter()
            .filter_map(|kept| match kept {
                Kept::Filtered { slots, .. } => Some(slots),
                Kept::Unchanged => None,
            })
            .flatten()
            .any(|slot| matches!(slot, Slot::Cubemaps { .. }))
    }

    /// How many probes the map's occlusion record takes from the map, every one of the donor's
    /// after the first, when the donor places an occlusion record with probes.
    pub(super) fn occlusion_probes(&self) -> AuthoringResult<Option<usize>> {
        self.kept
            .iter()
            .filter_map(|kept| match kept {
                Kept::Filtered { slots, .. } => Some(slots),
                Kept::Unchanged => None,
            })
            .flatten()
            .find_map(|slot| match slot {
                Slot::Occlusion { record_payload, .. } => Some(record_payload),
                _ => None,
            })
            .map(|record| Ok(array_at(record, OCCLUSION_PROBES)?.0 - 1))
            .transpose()
    }

    /// The tags the map's sky objects copy from the donor's sky, when the donor's bubble places a
    /// sky object collection that holds objects, else from the sky the map borrows.
    pub(super) fn sky_templates(
        &self,
        manager: &PackageManager,
    ) -> AuthoringResult<Option<parhelion_import::halo_reach::level::native::sky::Templates>> {
        self.kept
            .iter()
            .filter_map(|kept| match kept {
                Kept::Filtered { slots, .. } => Some(slots),
                Kept::Unchanged => None,
            })
            .flatten()
            .chain(&self.borrowed_sky)
            .find_map(|slot| match slot {
                Slot::Sky {
                    collection_payload, ..
                } => Some(collection_payload),
                _ => None,
            })
            .map(|collection| sky_templates(manager, collection))
            .transpose()
    }

    /// The records the map's statics copy from the donor's static instance group.
    pub(super) fn static_templates(
        &self,
    ) -> parhelion_import::halo_reach::level::native::StaticTemplates {
        self.statics.templates
    }

    /// The placement table: a copy of the donor's static instance group table, whose one row
    /// places the map's static instance group with preheader `preheader`, as a stock bubble's
    /// table places its statics. A row follows for each of `props` and for each of `decorators`,
    /// the map's decorator records, which copies the stock decorator row and names its record in a
    /// resource after the rows.
    fn table(
        &self,
        preheader: TagHash,
        props: &[(TagHash, [f32; 3])],
        decorators: &[TagHash],
    ) -> AuthoringResult<Vec<u8>> {
        let mut payload = self.statics.table.clone();
        write_u32(&mut payload, self.statics.preheader, preheader.0)?;
        let decorator = match (&self.decorators, decorators.is_empty()) {
            (_, true) => None,
            (Some(decorator), false) => Some(decorator),
            (None, false) => {
                return Err(invalid(
                    "The map places decorator records without a stock decorator placement",
                ));
            }
        };
        if !props.is_empty() || !decorators.is_empty() {
            let (count, _, rows, _) = array_at(&payload, TABLE_ROWS)?;
            let mut placements = payload[rows..rows + count * ROW_SIZE].to_vec();
            for (index, &(entity, position)) in props.iter().enumerate() {
                let mut row = self.prop_row.clone();
                write_u32(&mut row, 0, entity.0)?;
                for (axis, value) in [0f32, 0., 0., 1.].into_iter().enumerate() {
                    write_u32(&mut row, ROW_ROTATION + axis * 4, value.to_bits())?;
                }
                for (axis, value) in position.into_iter().enumerate() {
                    write_u32(&mut row, ROW_TRANSLATION + axis * 4, value.to_bits())?;
                }
                write_u32(&mut row, ROW_SCALE, 1f32.to_bits())?;
                row[ROW_IDENTITY..ROW_IDENTITY + 8]
                    .copy_from_slice(&(0x504F_5345_0000_0000u64 | index as u64).to_le_bytes());
                for at in ROW_POINTERS {
                    row[at..at + 8].fill(0);
                }
                placements.extend(row);
            }
            if let Some(decorator) = decorator {
                for _ in decorators {
                    let mut row = decorator.row.clone();
                    for at in ROW_POINTERS {
                        row[at..at + 8].fill(0);
                    }
                    placements.extend(row);
                }
            }
            append_native_array(
                &mut payload,
                TABLE_ROWS,
                TABLE_ROW_CLASS,
                count + props.len() + decorators.len(),
                &placements,
            )?;
            // Relocating the original static rows also relocates their resource pointers.
            let (_, _, relocated, _) = array_at(&payload, TABLE_ROWS)?;
            for index in 0..count {
                for at in ROW_POINTERS {
                    let old = rows + index * ROW_SIZE + at;
                    let delta = read_i64(&payload, old)?;
                    if delta != 0 {
                        let target = usize::try_from(old as i64 + delta)
                            .map_err(|_| invalid("Static placement pointer"))?;
                        write_relative_pointer(
                            &mut payload,
                            relocated + index * ROW_SIZE + at,
                            target,
                        )?;
                    }
                }
            }
            // Each decorator row's resource follows the rows, its kind word just before it.
            if let Some(decorator) = decorator {
                for (index, record) in decorators.iter().enumerate() {
                    while !(payload.len() + 4).is_multiple_of(16) {
                        payload.push(0);
                    }
                    payload.extend_from_slice(&DECORATOR_KIND.to_le_bytes());
                    let resource = payload.len();
                    let mut bytes = decorator.resource.clone();
                    write_u32(&mut bytes, DECORATOR_RECORD, record.0)?;
                    payload.extend_from_slice(&bytes);
                    write_relative_pointer(
                        &mut payload,
                        relocated + (count + props.len() + index) * ROW_SIZE + ROW_RESOURCE,
                        resource,
                    )?;
                }
            }
            payload = synchronize_payload_size(payload)?;
        }
        Ok(payload)
    }

    /// The chain's tags from entry `first`, in the order [`Addresses`] gives their entries, and its
    /// map root. The placement table places the static instance group, props and decorator records
    /// of `inputs`. The donor containers' copies place the map's spawn points, world collision,
    /// sky, cubemap volumes and ambient occlusion, and the sun's names the map's atmosphere. The
    /// map's own container places its sound points, lights and environment, and its companion
    /// loads every tag the pieces, sky, sounds and atmosphere use.
    pub(super) fn tags(
        &self,
        first: TagHash,
        inputs: &Inputs,
    ) -> AuthoringResult<(Vec<NewTagSpec>, TagHash)> {
        let addresses = self.addresses();
        let entries = Entries(first);
        let fixed = Fixed::new(entries)?;
        let copies = self.container_copies(&addresses, entries, inputs)?;
        let own = self.own_tags(&addresses, entries, inputs)?;
        let mut tags = self.fixed_tags(&fixed, &copies, &own, inputs)?;
        for copy in copies {
            tags.extend(copy.specs()?);
        }
        for (template, payload, _) in own.borrowed.into_iter().chain(own.lights) {
            tags.push(spec(template, payload));
        }
        for widened in own.bounds {
            tags.push(spec(widened.owner.tag, widened.payload));
            tags.push(spec(
                widened.owner.companion_template(),
                widened
                    .owner
                    .companion_with(widened.companion, widened.tag, &[])?,
            ));
        }
        for (template, payload, _) in own.environment {
            tags.push(spec(template, payload));
        }
        if tags.len() != addresses.total {
            return Err(invalid(
                "The map root chain wrote other entries than it planned",
            ));
        }
        Ok((tags, fixed.root))
    }

    /// The filtered containers' copies, in the bubble's container order. The first of the donor's
    /// skies holds the map's sky and the first of its cubemap volume tables places the map's
    /// volumes. Any further one holds or places none, so no object, volume or identity is placed
    /// twice.
    fn container_copies<'a>(
        &'a self,
        addresses: &Addresses,
        entries: Entries,
        inputs: &Inputs,
    ) -> AuthoringResult<Vec<ContainerCopy<'a>>> {
        let mut unplaced = Unplaced {
            sky: inputs.sky,
            cubemaps: inputs.cubemaps,
        };
        let mut copies = Vec::new();
        for (kept, planned) in self.kept.iter().zip(&addresses.copies) {
            let (Kept::Filtered { container, slots }, Some(planned)) = (kept, planned) else {
                continue;
            };
            let mut copy = ContainerCopy {
                container,
                tag: entries.at(planned.copy)?,
                companion: planned
                    .companion
                    .map(|entry| entries.at(entry))
                    .transpose()?,
                tables: Vec::with_capacity(slots.len()),
                added: Vec::new(),
                named: Vec::new(),
                replaced: Vec::new(),
            };
            for (slot, &first) in slots.iter().zip(&planned.slots) {
                self.copy_slot(slot, first, entries, inputs, &mut unplaced, &mut copy)?;
            }
            copies.push(copy);
        }
        Ok(copies)
    }

    /// Writes `slot`'s copies into `copy` from entry `first` ([`Slot::entries`]). A kept table
    /// stays as it is.
    fn copy_slot(
        &self,
        slot: &Slot,
        first: usize,
        entries: Entries,
        inputs: &Inputs,
        unplaced: &mut Unplaced,
        copy: &mut ContainerCopy,
    ) -> AuthoringResult<()> {
        if let Slot::Table(table) = slot {
            copy.tables.push(*table);
            return Ok(());
        }
        let table = entries.at(first)?;
        copy.tables.push(table.0);
        match slot {
            Slot::Table(_) => {}
            Slot::Moved { template, payload } => {
                copy.added
                    .push((TagHash(*template), payload.clone(), table));
            }
            Slot::Cubemaps { template, payload } => {
                let volumes = std::mem::take(&mut unplaced.cubemaps);
                copy.added.push((
                    TagHash(*template),
                    cubemaps::table(payload, volumes)?,
                    table,
                ));
                copy.named
                    .extend(volumes.iter().map(|cubemap| cubemap.texture));
            }
            Slot::Spawns {
                template,
                payload,
                fields,
                set,
                points,
            } => {
                let own_set = entries.at(first + 1)?;
                let mut payload = payload.clone();
                for &field in fields {
                    write_u32(&mut payload, field, own_set.0)?;
                }
                copy.added.push((TagHash(*template), payload, table));
                copy.added
                    .push((*set, spawn_set(points, inputs.spawns)?, own_set));
            }
            Slot::World {
                template,
                payload,
                field,
                record,
                record_payload,
                donor,
            } => {
                copy.replaced.extend_from_slice(donor);
                let own_record = entries.at(first + 1)?;
                copy.added.push((
                    TagHash(*template),
                    naming(payload, *field, own_record)?,
                    table,
                ));
                copy.added.push((
                    *record,
                    world_record(record_payload, &inputs.world)?,
                    own_record,
                ));
                // The client binds the record's shapes from what its container loads, as the
                // donor's container loads its own packfiles.
                copy.named.push(inputs.world.compound);
                copy.named.extend_from_slice(inputs.world.meshes);
            }
            Slot::Sun {
                template,
                payload,
                resource,
                record,
                record_payload,
                track,
                track_payload,
                atmosphere,
            } => {
                let mut payload = sun_table(payload, *resource, *atmosphere, inputs.atmosphere)?;
                // New tags are written in the order their entries were taken.
                let mut copies = Vec::new();
                if let Some(direction) = self.sun() {
                    let (own_record, own_track) = (entries.at(first + 1)?, entries.at(first + 2)?);
                    write_u32(&mut payload, resource + SUN_RECORD, own_record.0)?;
                    copies.push((*record, sun_record(record_payload, own_track)?, own_record));
                    copies.push((*track, sun_track(track_payload, direction)?, own_track));
                }
                copy.added.push((TagHash(*template), payload, table));
                copy.added.extend(copies);
            }
            Slot::Sky {
                template,
                payload,
                field,
                collection,
                collection_payload,
            } => {
                copy.replaced.push(*collection);
                let own_collection = entries.at(first + 1)?;
                copy.added.push((
                    TagHash(*template),
                    naming(payload, *field, own_collection)?,
                    table,
                ));
                let objects = std::mem::take(&mut unplaced.sky);
                copy.added.push((
                    *collection,
                    sky_collection(collection_payload, objects)?,
                    own_collection,
                ));
                // The container loads the sky's model references, as the donor's container loads
                // its own sky's.
                copy.named
                    .extend(objects.iter().map(|object| object.reference));
            }
            Slot::Occlusion {
                template,
                payload,
                field,
                record,
                record_payload,
            } => {
                let own_record = entries.at(first + 1)?;
                copy.added.push((
                    TagHash(*template),
                    naming(payload, *field, own_record)?,
                    table,
                ));
                copy_occlusion(*record, record_payload, own_record, inputs.occlusion, copy)?;
            }
        }
        Ok(())
    }

    /// The tags of the map's own beyond the fixed entries ([`Own`]), each at its entry.
    fn own_tags(
        &self,
        addresses: &Addresses,
        entries: Entries,
        inputs: &Inputs,
    ) -> AuthoringResult<Own<'_>> {
        let borrowed = match &self.borrowed_sky {
            None => Vec::new(),
            Some(Slot::Sky {
                template,
                payload,
                field,
                collection,
                collection_payload,
            }) => {
                let (table, own_collection) = (
                    entries.at(addresses.borrowed_sky)?,
                    entries.at(addresses.borrowed_sky + 1)?,
                );
                vec![
                    (
                        TagHash(*template),
                        naming(payload, *field, own_collection)?,
                        table,
                    ),
                    (
                        *collection,
                        sky_collection(collection_payload, inputs.sky)?,
                        own_collection,
                    ),
                ]
            }
            Some(_) => return Err(invalid("The map's borrowed sky is not a sky table")),
        };
        let (lights, techniques) = match &self.light_template {
            Some(template) if !self.lights.is_empty() => (
                template.tags(&self.lights, inputs.gels, &|offset| {
                    entries.at(addresses.lights + offset)
                })?,
                template.techniques(),
            ),
            _ => (Vec::new(), Vec::new()),
        };
        // Each of the parent's bounds records that bounds something gives way to a copy widened to
        // hold the level. With the donor's, its rooms bounded the level: on Tip of the Spear,
        // players walking from the beach toward the hills were pulled back a few steps where The
        // Other Side's boxes end, at y -94 to -99. Each copy and its companion take two entries.
        let mut bounds = Vec::new();
        for owner in self
            .bounds_rows
            .iter()
            .filter_map(|(_, owner)| owner.as_ref())
        {
            let entry = addresses.bounds + 2 * bounds.len();
            bounds.push(Widened {
                owner,
                payload: widened_bounds(&owner.payload, inputs.pieces)?
                    .unwrap_or_else(|| owner.payload.clone()),
                tag: entries.at(entry)?,
                companion: entries.at(entry + 1)?,
            });
        }
        let (environment, environment_entity) =
            match (&self.environment_template, &self.environment) {
                (Some(template), Some(environment)) => (
                    template.tags(
                        environment,
                        inputs.color_grade,
                        piece_bounds(inputs.pieces)?,
                        &|offset| entries.at(addresses.environment + offset),
                    )?,
                    Some(template.entity()),
                ),
                _ => (Vec::new(), None),
            };
        if lights.len() != addresses.bounds - addresses.lights
            || environment.len() != addresses.total - addresses.environment
        {
            return Err(invalid(
                "The map's lights or environment take other entries than planned",
            ));
        }
        Ok(Own {
            borrowed,
            lights,
            techniques,
            bounds,
            environment,
            environment_entity,
        })
    }

    /// The fixed entries' tags ([`Fixed`]): the map root, bubble parent, bubble and the map's own
    /// container with their companions, then the placement table and the bubble layout.
    fn fixed_tags(
        &self,
        fixed: &Fixed,
        copies: &[ContainerCopy],
        own: &Own,
        inputs: &Inputs,
    ) -> AuthoringResult<Vec<NewTagSpec>> {
        let loads = self.container_loads(fixed.table, own, inputs);
        let (bubble_loads, bubble_drops) = bubble_closure(fixed, copies);
        let (parent_payload, parent_loads) = self.parent_payload(fixed, own)?;
        let mut root_payload = self.root.payload.clone();
        write_u32(&mut root_payload, ROOT_LAYOUT, fixed.layout.0)?;
        set_tag_rows(&mut root_payload, ROOT_PARENTS, &[fixed.parent])?;
        // World construction reads each parent and its bounds before requesting a bubble.
        // Keep that metadata resident without enrolling the bubble's geometry closure.
        let mut root_loads = vec![fixed.layout, fixed.parent, fixed.parent_companion];
        root_loads.extend_from_slice(&parent_loads);
        let root_drops = [bubble_drops.as_slice(), &[self.layout]].concat();
        let placements = &inputs.placements;
        Ok(vec![
            spec(self.root.tag, synchronize_payload_size(root_payload)?),
            spec(
                self.root.companion_template(),
                pruned_companion(
                    &self.root,
                    fixed.root,
                    fixed.root_companion,
                    &root_loads,
                    &root_drops,
                )?,
            ),
            spec(self.parent.tag, synchronize_payload_size(parent_payload)?),
            spec(
                self.parent.companion_template(),
                pruned_companion(
                    &self.parent,
                    fixed.parent,
                    fixed.parent_companion,
                    &parent_loads,
                    &bubble_drops,
                )?,
            ),
            spec(
                self.bubble.tag,
                synchronize_payload_size(self.bubble_payload(copies, fixed.container)?)?,
            ),
            spec(
                self.bubble.companion_template(),
                pruned_companion(
                    &self.bubble,
                    fixed.bubble,
                    fixed.bubble_companion,
                    &bubble_loads,
                    &bubble_drops,
                )?,
            ),
            spec(
                self.container.tag,
                synchronize_payload_size(self.container_payload(fixed, own, inputs.sounds)?)?,
            ),
            spec(
                self.container.companion_template(),
                self.container.companion_with(
                    fixed.container_companion,
                    fixed.container,
                    &loads,
                )?,
            ),
            spec(
                self.prop_table,
                self.table(placements.statics, placements.props, placements.decorators)?,
            ),
            spec(self.layout, self.layout_payload.clone()),
        ])
    }

    /// What the map's own container's companion loads: its placement `table` and every converted
    /// tag, as a stock container loads what its tables place, and the stock tags the map's statics,
    /// lights, environment and decorators draw or place with.
    fn container_loads(&self, table: TagHash, own: &Own, inputs: &Inputs) -> Vec<TagHash> {
        let mut loads = vec![table];
        loads.extend_from_slice(inputs.content);
        // The static group's row places the donor's stock static entity, and every static draws
        // its depth prepass and shadows with the donor's stock techniques, which the container
        // loads as the donor's static container loads them.
        loads.extend([
            self.statics.entity,
            TagHash(self.statics.templates.depth),
            TagHash(self.statics.templates.shadow),
        ]);
        loads.extend(own.borrowed.iter().map(|(_, _, tag)| *tag));
        // The map's lights draw with the template's stock techniques, which the container loads
        // as the template's container loads them.
        loads.extend(own.lights.iter().map(|(_, _, tag)| *tag));
        loads.extend_from_slice(&own.techniques);
        // The environment's rows place the template's stock volume entity.
        loads.extend(own.environment.iter().map(|(_, _, tag)| *tag));
        loads.extend(own.environment_entity);
        // The decorator rows place the stock decorator entity, and the decorator models draw with
        // copies of stock techniques whose programs and samplers stay stock.
        if !inputs.placements.decorators.is_empty()
            && let Some(decorator) = &self.decorators
        {
            loads.extend_from_slice(&decorator.loads);
        }
        loads
    }

    /// The bubble parent's copy, which names the map's bubble and each widened bounds record in
    /// its donor's place, and what its companion loads: those records with their companions and
    /// the bounds records kept as they are. Parent and bubble definitions are independently
    /// requested native owners, so a parent must not pin its bubble's complete render and
    /// collision closure.
    fn parent_payload(&self, fixed: &Fixed, own: &Own) -> AuthoringResult<(Vec<u8>, Vec<TagHash>)> {
        let mut payload = self.parent.payload.clone();
        write_u32(&mut payload, PARENT_BUBBLE, fixed.bubble.0)?;
        self.bubble_name.rename(&mut payload);
        if !own.bounds.is_empty() {
            let mut widened = own.bounds.iter().map(|widened| widened.tag);
            let rows = self
                .bounds_rows
                .iter()
                .map(|(tag, owner)| match owner {
                    Some(_) => widened.next().unwrap_or(*tag),
                    None => *tag,
                })
                .collect::<Vec<_>>();
            set_tag_rows(&mut payload, PARENT_BOUNDS, &rows)?;
        }
        let mut loads = own
            .bounds
            .iter()
            .flat_map(|widened| [widened.tag, widened.companion])
            .collect::<Vec<_>>();
        loads.extend(
            self.bounds_rows
                .iter()
                .filter_map(|(tag, owner)| owner.is_none().then_some(*tag)),
        );
        Ok((payload, loads))
    }

    /// The bubble's copy, which names the donor's containers, each filtered one's copy in its
    /// place, then the map's own `container`.
    fn bubble_payload(
        &self,
        copies: &[ContainerCopy],
        container: TagHash,
    ) -> AuthoringResult<Vec<u8>> {
        let mut payload = self.bubble.payload.clone();
        let (count, _, rows, row_class) = array_at(&payload, BUBBLE_CONTAINERS)?;
        if count != self.kept.len() {
            return Err(invalid("The donor bubble's containers changed"));
        }
        let mut copies = copies.iter();
        let mut container_rows = Vec::new();
        for (index, kept) in self.kept.iter().enumerate() {
            match kept {
                Kept::Unchanged => container_rows.extend_from_slice(
                    &payload[rows + index * TAG_ROW_SIZE..rows + (index + 1) * TAG_ROW_SIZE],
                ),
                Kept::Filtered { .. } => {
                    let copy = copies
                        .next()
                        .ok_or_else(|| invalid("A filtered donor container has no copy"))?;
                    container_rows.extend_from_slice(&direct_row(copy.tag));
                }
            }
        }
        container_rows.extend_from_slice(&direct_row(container));
        replace_array(
            &mut payload,
            BUBBLE_CONTAINERS,
            row_class,
            TAG_ROW_SIZE,
            container_rows.len() / TAG_ROW_SIZE,
            &container_rows,
        )?;
        Ok(payload)
    }

    /// The map's own container, whose tables are the placement table, the map's sound points, and
    /// the tables of a borrowed sky, the map's lights and its environment.
    fn container_payload(
        &self,
        fixed: &Fixed,
        own: &Own,
        sounds: Option<TagHash>,
    ) -> AuthoringResult<Vec<u8>> {
        let mut payload = self.container.payload.clone();
        let (_, _, _, table_class) = array_at(&payload, CONTAINER_TABLES)?;
        let first = |tags: &[Added]| tags.first().map(|(_, _, tag)| *tag);
        let tables = std::iter::once(fixed.table)
            .chain(sounds)
            .chain(first(&own.borrowed))
            .chain(first(&own.lights))
            .chain(first(&own.environment))
            .flat_map(|tag| tag.0.to_le_bytes())
            .collect::<Vec<_>>();
        replace_array(
            &mut payload,
            CONTAINER_TABLES,
            table_class,
            4,
            tables.len() / 4,
            &tables,
        )?;
        Ok(payload)
    }
}

/// Where a chain's tags go, as offsets from its first entry, in the order they are written: the
/// fixed entries ([`FIXED_ENTRIES`]), each filtered container's copy with its companion and its
/// slots' copies, a borrowed sky's table and collection, the map's lights, each widened bounds
/// record with its companion, then the map's environment. The plan is made before any payload, so
/// counting the chain's entries and writing its tags read one assignment.
struct Addresses {
    /// Each donor container's entries when it is filtered, in the bubble's order.
    copies: Vec<Option<CopyEntries>>,
    borrowed_sky: usize,
    lights: usize,
    bounds: usize,
    environment: usize,
    total: usize,
}

/// A filtered container's entries: its copy, its companion when the donor has one, and the first
/// of each slot's copies ([`Slot::entries`]).
struct CopyEntries {
    copy: usize,
    companion: Option<usize>,
    slots: Vec<usize>,
}

/// A chain's entries from its first, which all lie in the first's package.
#[derive(Clone, Copy)]
struct Entries(TagHash);

impl Entries {
    fn at(self, offset: usize) -> AuthoringResult<TagHash> {
        let index = self.0.entry_index() as usize + offset;
        if index >= crate::appended_tags::MAX_PACKAGE_ENTRY_COUNT {
            return Err(invalid("The map root chain exceeds one package"));
        }
        Ok(TagHash::new(self.0.pkg_id(), index as u16))
    }
}

/// The chain's fixed entries: the map root, bubble parent, bubble and the map's own container,
/// each with its companion, then the placement table and the bubble layout.
struct Fixed {
    root: TagHash,
    root_companion: TagHash,
    parent: TagHash,
    parent_companion: TagHash,
    bubble: TagHash,
    bubble_companion: TagHash,
    container: TagHash,
    container_companion: TagHash,
    table: TagHash,
    layout: TagHash,
}

impl Fixed {
    fn new(entries: Entries) -> AuthoringResult<Self> {
        Ok(Self {
            root: entries.at(0)?,
            root_companion: entries.at(1)?,
            parent: entries.at(2)?,
            parent_companion: entries.at(3)?,
            bubble: entries.at(4)?,
            bubble_companion: entries.at(5)?,
            container: entries.at(6)?,
            container_companion: entries.at(7)?,
            table: entries.at(8)?,
            layout: entries.at(9)?,
        })
    }
}

/// A new tag: its template, its payload and its own tag.
type Added = (TagHash, Vec<u8>, TagHash);

/// A filtered container's copy: its tag and companion, its tables, the tags its slots add, the
/// stock or converted tags those name, which its companion also loads, and the donor tags it
/// replaces, which no companion loads any longer.
struct ContainerCopy<'a> {
    container: &'a Container,
    tag: TagHash,
    companion: Option<TagHash>,
    tables: Vec<u32>,
    added: Vec<Added>,
    named: Vec<TagHash>,
    replaced: Vec<TagHash>,
}

impl ContainerCopy<'_> {
    /// The copy's tags: the container naming its tables, its companion when the donor has one, then
    /// the tags its slots add.
    fn specs(self) -> AuthoringResult<Vec<NewTagSpec>> {
        let mut payload = self.container.payload().to_vec();
        let (count, rows) = if read_u64(&payload, CONTAINER_TABLES)? == 0 {
            (0, 0)
        } else {
            let (count, _, rows, _) = array_at(&payload, CONTAINER_TABLES)?;
            (count, rows)
        };
        if count != self.tables.len() {
            return Err(invalid("A donor container's tables changed"));
        }
        for (index, &table) in self.tables.iter().enumerate() {
            write_u32(&mut payload, rows + index * 4, table)?;
        }
        let mut tags = vec![spec(self.container.tag(), payload)];
        if let (Container::Enrolled(owner), Some(companion)) = (self.container, self.companion) {
            let mut loads = self
                .added
                .iter()
                .map(|(_, _, tag)| *tag)
                .collect::<Vec<_>>();
            loads.extend_from_slice(&self.named);
            tags.push(spec(
                owner.companion_template(),
                pruned_companion(owner, self.tag, companion, &loads, &self.replaced)?,
            ));
        }
        tags.extend(
            self.added
                .into_iter()
                .map(|(template, payload, _)| spec(template, payload)),
        );
        Ok(tags)
    }
}

/// The map's sky objects and cubemap volumes, until a copy places them.
struct Unplaced<'a> {
    sky: &'a [SkyObject],
    cubemaps: &'a [Cubemap],
}

/// The tags of the map's own beyond the fixed entries: a borrowed sky's table and collection, the
/// map's lights with the stock techniques they draw with, the parent's widened bounds records, and
/// the map's environment with the stock volume entity its rows place.
struct Own<'a> {
    borrowed: Vec<Added>,
    lights: Vec<Added>,
    techniques: Vec<TagHash>,
    bounds: Vec<Widened<'a>>,
    environment: Vec<Added>,
    environment_entity: Option<TagHash>,
}

/// One of the parent's bounds records widened to hold the level ([`widened_bounds`]): its owner,
/// its payload, and the tags of its copy and the copy's companion.
struct Widened<'a> {
    owner: &'a Owner,
    payload: Vec<u8>,
    tag: TagHash,
    companion: TagHash,
}

/// What the bubble's companion loads: the map's own container and its companion, then each copy and
/// its companion, or, for a copy without one, the tags it adds and the tags they name. Beside it,
/// the donor tags the copies replace, which no companion of the chain loads.
fn bubble_closure(fixed: &Fixed, copies: &[ContainerCopy]) -> (Vec<TagHash>, Vec<TagHash>) {
    let mut loads = vec![fixed.container, fixed.container_companion];
    let mut drops = Vec::new();
    for copy in copies {
        drops.extend_from_slice(&copy.replaced);
        loads.push(copy.tag);
        match copy.companion {
            Some(companion) => loads.push(companion),
            None => {
                loads.extend(copy.added.iter().map(|(_, _, tag)| *tag));
                loads.extend_from_slice(&copy.named);
            }
        }
    }
    (loads, drops)
}

/// The copy of the donor's ambient occlusion record `record`, at `own`, which a copy of its table
/// names. The donor's record stays out of the companion, as the donor's sky collection does. Every
/// occlusion record registers under its selector at `+0x70`, and on a converted Paradiso the
/// donor's record, loaded beside the map's own and both under 0, left the map's decorators reading
/// no ambient visibility: a runtime slot held their identity beside -1, and every blade drew black
/// in sun while lit by a local light. That the lookups chose the donor's record is inferred from
/// both being resident.
///
/// Without the map's probes the copy keeps the donor's record and names its occlusion buffers,
/// which the container loads as the donor's loads them through its record. The map's own record
/// names its own decorator buffer instead, and the donor's buffers stay out of the companion with
/// their record.
fn copy_occlusion(
    record: TagHash,
    payload: &[u8],
    own: TagHash,
    occlusion: Occlusion,
    copy: &mut ContainerCopy,
) -> AuthoringResult<()> {
    copy.replaced.push(record);
    let buffers = OCCLUSION_BUFFERS
        .iter()
        .map(|&buffer| read_u32(payload, buffer))
        .collect::<AuthoringResult<Vec<_>>>()?
        .into_iter()
        .filter(|&tag| tag != u32::MAX)
        .map(TagHash);
    if occlusion.probes.is_empty() {
        copy.added.push((record, payload.to_vec(), own));
        copy.named.extend(buffers);
    } else {
        copy.replaced.extend(buffers);
        copy.added.push((
            record,
            occlusion_record(
                payload,
                occlusion.probes,
                occlusion.clusters,
                occlusion.decorators,
            )?,
            own,
        ));
    }
    Ok(())
}

/// A copy of the donor's sun table `payload`, whose sun at `resource` draws no sun disc or lens
/// flare, and whose atmosphere at `row`, when the table places one, names the map's own volumes,
/// distance lookup and image wherever the map brings them.
fn sun_table(
    payload: &[u8],
    resource: usize,
    row: Option<usize>,
    atmosphere: Atmosphere,
) -> AuthoringResult<Vec<u8>> {
    let mut payload = payload.to_vec();
    // The donor's sun disc and lens flare draw Distant Shore's sun. The map's sky draws its own.
    write_u32(&mut payload, resource + SUN_MATERIALS, u32::MAX)?;
    write_u32(&mut payload, resource + SUN_FLARE, u32::MAX)?;
    write_u32(
        &mut payload,
        resource + SUN_SCALAR,
        SUN_SCALAR_VALUE.to_bits(),
    )?;
    let Some(row) = row else {
        return Ok(payload);
    };
    if let Some(volume) = atmosphere.volume {
        write_u32(&mut payload, row + ATMOSPHERE_VOLUME, volume.0)?;
        write_u32(
            &mut payload,
            row + ATMOSPHERE_SECOND_VOLUME,
            atmosphere.interior.map_or(u32::MAX, |volume| volume.0),
        )?;
    }
    for (field, tag) in [
        (ATMOSPHERE_LOOKUP, atmosphere.lookup),
        (ATMOSPHERE_IMAGE, atmosphere.image),
    ] {
        if let Some(tag) = tag {
            write_u32(&mut payload, row + field, tag.0)?;
        }
    }
    Ok(payload)
}

/// A copy of donor table `payload` whose row at `field` names `record`.
fn naming(payload: &[u8], field: usize, record: TagHash) -> AuthoringResult<Vec<u8>> {
    let mut payload = payload.to_vec();
    write_u32(&mut payload, field, record.0)?;
    Ok(payload)
}

/// A tag row that names `tag` directly.
fn direct_row(tag: TagHash) -> Vec<u8> {
    let mut row = Vec::with_capacity(TAG_ROW_SIZE);
    row.extend_from_slice(&tag.0.to_le_bytes());
    row.extend_from_slice(&DIRECT_TAG_ROW.to_le_bytes());
    row.extend_from_slice(&0u64.to_le_bytes());
    row
}

/// `owner`'s companion for its copy at `tag`, written at `companion`, loading `loads` and none of
/// `drops`.
fn pruned_companion(
    owner: &Owner,
    tag: TagHash,
    companion: TagHash,
    loads: &[TagHash],
    drops: &[TagHash],
) -> AuthoringResult<Vec<u8>> {
    crate::shared_tag_dependency_index::remove_dependencies(
        &owner.companion_with(companion, tag, loads)?,
        companion,
        tag,
        drops,
    )
}

fn spec(template: TagHash, payload: Vec<u8>) -> NewTagSpec {
    NewTagSpec {
        template_tag: template,
        payload,
        storage: NewTagStorageMode::InheritTemplate,
    }
}

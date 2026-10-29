//! A multiplayer level's structure BSP geometry in world space. Each cluster draws one mesh of
//! the lightmap BSP data's render geometry, already placed in the level. Instanced geometry
//! draws the other meshes, each placed by a row of the BSP's structure resource. A level loads
//! every BSP its scenario's zone set names: its playable space, usually a shared backdrop and
//! sometimes further scenery.
use super::{
    cache::{Cache, Tag},
    material::{self, Material},
    model::{self, Geometry, Primitive, Section},
    resource::{Pages, Resource},
};
use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    fs,
    path::Path,
};

pub mod activity;
pub mod ambience;
mod collision;
mod decals;
mod decorators;
pub(crate) mod lighting;
pub mod lightmap;
pub mod mission;
pub mod native;
pub mod objectives;
mod objects;
pub mod picture;
pub mod spawn;
pub mod squads;
pub mod streaming;
pub mod variant;
pub mod volumes;
mod water;

/// The group of the lightmap BSP data that holds a BSP's render geometry.
const LIGHTMAP_GROUP: &str = "Lbsp";
/// Its render geometry block, laid out like a render model's at `+0x64`.
const LIGHTMAP_GEOMETRY: usize = 0x78;
/// A structure BSP's own render geometry block, in the same layout: the geometry of a BSP without
/// lightmap BSP data.
const BSP_GEOMETRY: usize = 0x44C;
const CLUSTERS: usize = 0x138;
const CLUSTER_SIZE: usize = 0x8C;
const CLUSTER_MESH: usize = 0x40;
const MATERIALS: usize = 0x144;
const MATERIAL_SIZE: usize = 0x2C;
/// The structure resource whose definition places instanced geometry.
const STRUCTURE_RESOURCE: usize = 0x520;
/// Every field above lies inside this many bytes of the BSP.
const BSP_SIZE: usize = 0x4E4;
/// The instance block in that definition's root, and each row: scale, forward, left and up
/// axes, position in world units, then the mesh it draws.
const INSTANCES: usize = 0x48;
const INSTANCE_SIZE: usize = 0x9C;
const INSTANCE_MESH: usize = 0x34;
/// Meters per Reach world unit.
const METRES_PER_UNIT: f32 = 3.048;
/// The mesh part flag of a part lit per vertex. Its vertices follow the lightmapped parts' in the
/// mesh's vertex buffer, past the end of the mesh's lightmap texture coordinate stream.
const PER_VERTEX_PART: u16 = 1 << 1;
/// The scenario's structure BSP block: each row references a BSP and keeps its flags.
const SCENARIO_BSPS: usize = 0x50;
const SCENARIO_BSP_SIZE: usize = 0xAC;
const SCENARIO_BSP_FLAGS: usize = 0x50;
/// The flag of a BSP that is not normally playable space in a multiplayer level, such as a shared
/// backdrop.
const NOT_PLAYABLE: u16 = 1 << 10;
/// The flag of a shared BSP.
const SHARED: u16 = 1 << 11;
/// The scenario's zone sets, and each one's mask of the BSPs it loads.
const ZONE_SETS: usize = 0xAC;
const ZONE_SET_SIZE: usize = 0x13C;
const ZONE_SET_BSPS: usize = 0x10C;

/// Source cache files whose resource pages can be read by this level. Inspect the cache's own
/// shared-page table so reuse never relies on a fixed list of MCC filenames.
pub fn source_files(maps: &Path, map: &str) -> Result<Vec<std::path::PathBuf>> {
    let root = maps.canonicalize()?;
    let path = root.join(map).canonicalize()?;
    ensure!(
        path.starts_with(&root),
        "Source map escapes its cache directory"
    );
    let cache = Cache::open(&path)?;
    let zone = cache.only("zone")?.address()?;
    let play = cache.only("play")?.offset.unwrap_or(zone + 28);
    let shared = cache.block(play + 12, 264)?;
    let mut paths = BTreeSet::from([path]);
    for page in cache.block(play + 24, 88)? {
        let index = cache.i16(page + 4)?;
        if index < 0 || cache.i32(page + 12)? == 0 {
            continue;
        }
        let row = *shared
            .get(index as usize)
            .context("Shared cache outside table")?;
        let source = cache.string(row, 256)?;
        let name = source
            .rsplit(['/', '\\'])
            .next()
            .context("Shared cache filename")?;
        ensure!(
            !name.is_empty() && name.ends_with(".map") && name != ".map" && !name.contains(':'),
            "Invalid shared cache name"
        );
        let path = root.join(name).canonicalize()?;
        ensure!(
            path.starts_with(&root),
            "Shared cache escapes its directory"
        );
        paths.insert(path);
    }
    Ok(paths.into_iter().collect())
}

/// One placement of an instanced geometry mesh.
#[derive(Clone, Debug, Serialize)]
pub struct Instance {
    /// The structure BSP whose mesh it draws, by its index in [`Level::bsps`].
    pub bsp: usize,
    pub mesh: usize,
    pub scale: f32,
    /// Forward, left and up axes.
    pub axes: [[f32; 3]; 3],
    /// Position in meters.
    pub position: [f32; 3],
}

impl Instance {
    fn place(&self, position: [f32; 3]) -> [f32; 3] {
        let mut out = self.position;
        for (axis, value) in self.axes.iter().zip(position) {
            for k in 0..3 {
                out[k] += axis[k] * value * self.scale;
            }
        }
        out
    }

    fn turn(&self, direction: [f32; 3]) -> [f32; 3] {
        let mut out = [0.; 3];
        for (axis, value) in self.axes.iter().zip(direction) {
            for k in 0..3 {
                out[k] += axis[k] * value;
            }
        }
        out
    }

    /// Moves `primitive`, a primitive of the mesh this instance draws, to its place in the level.
    /// A mirroring placement reverses each triangle's winding.
    fn place_primitive(&self, primitive: &mut Primitive) {
        for vertex in &mut primitive.vertices {
            vertex.position = self.place(vertex.position);
            vertex.normal = self.turn(vertex.normal);
            let [x, y, z, w] = vertex.tangent;
            let [x, y, z] = self.turn([x, y, z]);
            vertex.tangent = [x, y, z, w];
        }
        if self.scale < 0. {
            for face in primitive.indices.chunks_exact_mut(3) {
                face.swap(1, 2);
            }
        }
    }
}

/// A level's world-space geometry, from each structure BSP it loads.
pub struct Level {
    /// The structure BSPs, in the order their geometry follows.
    pub bsps: Vec<Tag>,
    /// Every BSP's materials, one for each shader they share.
    pub materials: Vec<Material>,
    /// Each BSP's cluster meshes, then its instances' meshes placed in the level.
    pub primitives: Vec<Primitive>,
    /// Each BSP's meshes.
    pub meshes: Vec<Meshes>,
    pub instances: Vec<Instance>,
    /// The baked diffuse lighting the primitives' lightmap coordinates sample.
    pub lightmap: Option<lightmap::Atlas>,
    /// The average lit level of the lightmap images, the per-vertex probes and the coloring probes,
    /// in the units each decodes to.
    pub lighting_averages: [f32; 3],
    /// What each BSP's decorators placed, in BSP order.
    pub decorators: Vec<serde_json::Value>,
    /// The decorators as instances of their types, when they draw as Destiny's instanced
    /// decorators ([`read_decorated_areas`]) rather than copies among `primitives`.
    instanced: Option<decorators::Instanced>,
}

/// One structure BSP's meshes, by index in its render geometry.
pub struct Meshes {
    /// Meshes the clusters draw.
    pub clusters: BTreeSet<usize>,
    /// Meshes neither a cluster nor an instance draws.
    pub unused: BTreeSet<usize>,
    /// Each mesh's vertex bounds in its own space, in meters, when it has vertices.
    pub bounds: Vec<Option<[[f32; 3]; 2]>>,
}

/// One structure BSP's geometry before its level joins it to the others. Its primitives' lightmap
/// coordinates are still in its own lightmap image, which `lighting` holds decoded, and each
/// primitive lit per vertex names its lighting probe in `probes` at the same index of
/// `primitive_probes`.
struct Part {
    materials: Vec<Material>,
    primitives: Vec<Primitive>,
    primitive_probes: Vec<Option<usize>>,
    meshes: Meshes,
    instances: Vec<Instance>,
    lighting: Option<lightmap::Lighting>,
    probes: Vec<lightmap::Texel>,
    /// The sum and count of the lit levels of the image, the per-vertex probes and the coloring
    /// probes.
    levels: [(f64, usize); 3],
    /// Its decorators, drawn once every BSP is read ([`decorators::place`]).
    decorated: decorators::Decorators,
    /// What its decorators placed.
    decorators: serde_json::Value,
}

/// One structure BSP a level loads.
pub struct Bsp {
    pub tag: Tag,
    /// Whether the scenario counts it as playable space. A shared backdrop is not.
    pub playable: bool,
    /// Whether the scenario marks it shared, as a multiplayer level's backdrop.
    pub shared: bool,
}

/// Where a relocated block of a resource definition keeps its rows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Space {
    /// The definition itself, in the tag data.
    Definition,
    /// The resource's page.
    Data,
}

/// A structure resource's definition, the root its gestalt entry names and the relocations that
/// point its blocks at their rows. Each relocation target keeps its space in the top four bits.
struct Definition {
    resource: Resource,
    bytes: Vec<u8>,
    root: usize,
    relocations: BTreeMap<usize, u32>,
}

impl Definition {
    fn load(cache: &Cache, tag: &Tag, field: usize) -> Result<Self> {
        let handle = cache.u32(tag.address()? + field)?;
        let resource = cache.resource(tag, handle)?;
        let zone = cache.only("zone")?.address()?;
        let entry = *cache
            .block(zone + 100, 64)?
            .get((handle & 65535) as usize)
            .context("Structure resource outside gestalt")?;
        let root = cache.u32(entry + 36)?;
        ensure!(
            space(root)? == Space::Definition,
            "Structure resource root is outside its definition"
        );
        let relocations = cache
            .block(entry + 40, 8)?
            .into_iter()
            .map(|r| Ok((cache.u32(r)? as usize, cache.u32(r + 4)?)))
            .collect::<Result<BTreeMap<_, _>>>()?;
        Ok(Self {
            bytes: cache
                .meta(resource.fixup_offset, resource.fixup_size)?
                .to_vec(),
            resource,
            root: (root & 0x0fff_ffff) as usize,
            relocations,
        })
    }

    fn word(&self, at: usize) -> Result<[u8; 4]> {
        Ok(self
            .bytes
            .get(at..at + 4)
            .context("Structure resource definition boundary")?
            .try_into()?)
    }

    fn f32(&self, at: usize) -> Result<f32> {
        self.word(at).map(f32::from_le_bytes)
    }

    /// The block at `at`: its row count, and the space and offset its rows of `size` bytes
    /// start at. Rows in the definition must lie inside it.
    fn block(&self, at: usize, size: usize) -> Result<(usize, Space, usize)> {
        let count = usize::try_from(i32::from_le_bytes(self.word(at)?))?;
        if count == 0 {
            return Ok((0, Space::Definition, 0));
        }
        let pointer = *self
            .relocations
            .get(&(at + 4))
            .with_context(|| format!("Block at {at:#x} has no relocation"))?;
        let start = (pointer & 0x0fff_ffff) as usize;
        let space = space(pointer)?;
        let length = count.checked_mul(size).context("Block size overflow")?;
        ensure!(
            space == Space::Data
                || (start <= self.bytes.len() && length <= self.bytes.len() - start),
            "Block at {at:#x} exceeds the definition"
        );
        Ok((count, space, start))
    }

    /// The rows of the block at `at` that lie in the resource's page.
    fn data(
        &self,
        cache: &Cache,
        pages: &mut Pages,
        at: usize,
        size: usize,
    ) -> Result<(usize, Vec<u8>)> {
        let (count, space, start) = self.block(at, size)?;
        if count == 0 {
            return Ok((0, Vec::new()));
        }
        ensure!(space == Space::Data, "Block at {at:#x} is not in the page");
        Ok((
            count,
            pages.bytes(cache, &self.resource, start, count * size)?,
        ))
    }
}

fn space(pointer: u32) -> Result<Space> {
    Ok(match pointer >> 28 {
        2 => Space::Definition,
        4 => Space::Data,
        other => bail!("Unknown relocation space {other}"),
    })
}

/// The mask of the structure BSPs the scenario's zone sets `zone_sets` load, together, and every
/// BSP for a scenario without zone sets. A multiplayer level has one zone set, and a campaign
/// mission one for each stretch of the mission.
pub(super) fn loaded_bsps(cache: &Cache, zone_sets: &[usize]) -> Result<u32> {
    let scenario = cache.only("scnr")?.address()?;
    let rows = cache.block(scenario + ZONE_SETS, ZONE_SET_SIZE)?;
    if rows.is_empty() {
        ensure!(
            zone_sets.iter().all(|&zone_set| zone_set == 0),
            "The scenario has no zone sets"
        );
        return Ok(u32::MAX);
    }
    let mut mask = 0;
    for &zone_set in zone_sets {
        let row = rows.get(zone_set).with_context(|| {
            format!(
                "The scenario has {} zone sets, not {}",
                rows.len(),
                zone_set + 1
            )
        })?;
        mask |= cache.u32(row + ZONE_SET_BSPS)?;
    }
    Ok(mask)
}

/// The structure BSPs the scenario's zone sets `zone_sets` load, in scenario order, whether or not
/// any is playable.
fn loaded(cache: &Cache, zone_sets: &[usize]) -> Result<Vec<Bsp>> {
    let scenario = cache.only("scnr")?.address()?;
    let loaded = loaded_bsps(cache, zone_sets)?;
    let mut bsps = Vec::new();
    for (index, row) in cache
        .block(scenario + SCENARIO_BSPS, SCENARIO_BSP_SIZE)?
        .into_iter()
        .enumerate()
    {
        if index >= 32 || (loaded >> index) & 1 == 0 {
            continue;
        }
        let tag = cache
            .reference(row, Some("sbsp"))?
            .with_context(|| format!("Scenario structure BSP {index} is empty"))?;
        let flags = cache.u16(row + SCENARIO_BSP_FLAGS)?;
        bsps.push(Bsp {
            tag,
            playable: flags & NOT_PLAYABLE == 0,
            shared: flags & SHARED != 0,
        });
    }
    Ok(bsps)
}

/// The mask of the scenario's structure BSPs, bit `n` for its `n`th, that `bsps` name.
pub(super) fn bsp_mask(cache: &Cache, bsps: &[Tag]) -> Result<u32> {
    let scenario = cache.only("scnr")?.address()?;
    let mut mask = 0;
    for (index, row) in cache
        .block(scenario + SCENARIO_BSPS, SCENARIO_BSP_SIZE)?
        .into_iter()
        .enumerate()
        .take(32)
    {
        if let Some(tag) = cache.reference(row, Some("sbsp"))?
            && bsps.iter().any(|loaded| loaded.path == tag.path)
        {
            mask |= 1 << index;
        }
    }
    Ok(mask)
}

/// The structure BSPs the scenario's zone sets `zone_sets` load, in scenario order.
pub fn bsps(cache: &Cache, zone_sets: &[usize]) -> Result<Vec<Bsp>> {
    let bsps = loaded(cache, zone_sets)?;
    ensure!(
        bsps.iter().any(|bsp| bsp.playable),
        "The scenario's zone sets {zone_sets:?} load no playable structure BSP"
    );
    Ok(bsps)
}

/// The bounding boxes of the clusters of `bsps`, in meters, which together hold the space those
/// structure BSPs enclose.
pub(super) fn cluster_boxes<'a>(
    cache: &Cache,
    bsps: impl IntoIterator<Item = &'a Bsp>,
) -> Result<Vec<([f32; 3], [f32; 3])>> {
    let mut boxes = Vec::new();
    for bsp in bsps {
        for row in cache.block(bsp.tag.address()? + CLUSTERS, CLUSTER_SIZE)? {
            let bounds = cache.floats::<6>(row)?.map(|value| value * METRES_PER_UNIT);
            boxes.push((
                [bounds[0], bounds[2], bounds[4]],
                [bounds[1], bounds[3], bounds[5]],
            ));
        }
    }
    Ok(boxes)
}

/// The boxes of the structure clusters of `tags`, in meters, leaving out each structure BSP's
/// outside cluster, the one whose box holds every other cluster's of its BSP.
pub(super) fn interior_boxes(cache: &Cache, tags: &[Tag]) -> Result<Vec<([f32; 3], [f32; 3])>> {
    let mut interior = Vec::new();
    for tag in tags {
        let boxes = cache
            .block(tag.address()? + CLUSTERS, CLUSTER_SIZE)?
            .into_iter()
            .map(|row| {
                let bounds = cache.floats::<6>(row)?.map(|value| value * METRES_PER_UNIT);
                Ok((
                    [bounds[0], bounds[2], bounds[4]],
                    [bounds[1], bounds[3], bounds[5]],
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let holds_all = |(low, high): ([f32; 3], [f32; 3])| {
            boxes.len() > 1
                && boxes
                    .iter()
                    .all(|(l, h)| (0..3).all(|k| low[k] <= l[k] && high[k] >= h[k]))
        };
        interior.extend(boxes.iter().copied().filter(|&b| !holds_all(b)));
    }
    Ok(interior)
}

/// One zone set of a scenario: its name, the structure BSPs it loads and the positions of the
/// player starting locations a map of it spawns players at ([`spawn::starts_in`]), in meters.
#[derive(Clone, Debug, Serialize)]
pub struct ZoneSet {
    pub name: String,
    pub bsps: Vec<String>,
    pub playable: bool,
    pub starts: Vec<[f32; 3]>,
}

/// The name string of a zone set row.
const ZONE_SET_NAME: usize = 0x04;

/// The zone sets of cache `map` in `maps`, in scenario order, which a map recipe names by index.
/// A scenario without zone sets lists one that loads every structure BSP.
pub fn zone_sets(maps: &Path, map: &str) -> Result<Vec<ZoneSet>> {
    let cache = Cache::open(&maps.join(map))?;
    let scenario = cache.only("scnr")?.address()?;
    let rows = cache.block(scenario + ZONE_SETS, ZONE_SET_SIZE)?;
    (0..rows.len().max(1))
        .map(|index| {
            let bsps = loaded(&cache, &[index])?;
            Ok(ZoneSet {
                name: match rows.get(index) {
                    Some(&row) => cache.string(row + ZONE_SET_NAME, 0x100)?,
                    None => String::new(),
                },
                bsps: bsps.iter().map(|bsp| bsp.tag.path.clone()).collect(),
                playable: bsps.iter().any(|bsp| bsp.playable),
                starts: spawn::starts_in(&cache, &bsps)?
                    .iter()
                    .map(|start| start.position)
                    .collect(),
            })
        })
        .collect()
}

/// The first playable structure BSP the scenario's first zone set loads.
fn playable(cache: &Cache) -> Result<Tag> {
    bsps(cache, &[0])?
        .into_iter()
        .find(|bsp| bsp.playable)
        .map(|bsp| bsp.tag)
        .context("The scenario loads no playable structure BSP")
}

/// Reads the placements of `tag`'s instanced geometry from its structure resource, as those of
/// the level's BSP `bsp`.
fn instances(cache: &Cache, tag: &Tag, bsp: usize) -> Result<Vec<Instance>> {
    let definition = Definition::load(cache, tag, STRUCTURE_RESOURCE)?;
    let (count, space, start) = definition.block(definition.root + INSTANCES, INSTANCE_SIZE)?;
    ensure!(
        count == 0 || space == Space::Definition,
        "Instance block is outside the definition"
    );
    let mut out = Vec::with_capacity(count);
    for row in (0..count).map(|i| start + i * INSTANCE_SIZE) {
        let mesh = u16::from_le_bytes(
            definition.bytes[row + INSTANCE_MESH..row + INSTANCE_MESH + 2].try_into()?,
        );
        let mut axes = [[0.; 3]; 3];
        for (a, axis) in axes.iter_mut().enumerate() {
            for (k, value) in axis.iter_mut().enumerate() {
                *value = definition.f32(row + 4 + a * 12 + k * 4)?;
            }
        }
        let mut position = [0.; 3];
        for (k, value) in position.iter_mut().enumerate() {
            *value = definition.f32(row + 0x28 + k * 4)? * METRES_PER_UNIT;
        }
        out.push(Instance {
            bsp,
            mesh: usize::from(mesh),
            scale: definition.f32(row)?,
            axes,
            position,
        });
    }
    Ok(out)
}

/// Reads the geometry each cluster and instance of `tag`, a structure BSP, draws.
pub fn read(cache: &Cache, pages: &mut Pages, tag: &Tag) -> Result<Level> {
    read_all(cache, pages, std::slice::from_ref(tag))
}

/// Reads structure BSPs `tags` as one level. Identical source material selections share an
/// identity across BSPs, including a decal's layer and sprite. One lightmap atlas holds every
/// BSP's lightmap image and lighting probes.
pub fn read_all(cache: &Cache, pages: &mut Pages, tags: &[Tag]) -> Result<Level> {
    read_areas(cache, pages, tags, &[(0..tags.len()).collect()])
}

/// Reads structure BSPs `tags` as one level whose `areas`, each the indices in `tags` of the BSPs
/// that load together, budget their decorators apart (`decorators::place`). Each primitive
/// names its own BSP, so an area draws only what its BSPs hold.
pub fn read_areas(
    cache: &Cache,
    pages: &mut Pages,
    tags: &[Tag],
    areas: &[Vec<usize>],
) -> Result<Level> {
    read_decorated_areas(cache, pages, tags, areas, false)
}

/// [`read_areas`], with `instanced` keeping the decorators as instances of their types for
/// Destiny's instanced decorators (`decorators::instanced`) in place of copies among the level's
/// primitives. Each type then keeps its most detailed model, and the decorators take no share of
/// the areas' triangle budget, which forced coarser models on the copies.
pub fn read_decorated_areas(
    cache: &Cache,
    pages: &mut Pages,
    tags: &[Tag],
    areas: &[Vec<usize>],
    instanced: bool,
) -> Result<Level> {
    let lut = lightmap::direction_lut(cache, pages)?;
    let mut parts = Vec::with_capacity(tags.len());
    for (bsp, tag) in tags.iter().enumerate() {
        parts.push(read_bsp(cache, pages, tag, bsp, lut.as_deref())?);
    }
    let masks = tags
        .iter()
        .map(|tag| bsp_mask(cache, std::slice::from_ref(tag)))
        .collect::<Result<Vec<_>>>()?;
    let instanced = if instanced {
        Some(decorators::instanced(cache, pages, &mut parts, &masks)?)
    } else {
        decorators::place(cache, pages, &mut parts, areas)?;
        None
    };
    for (part, &bsps) in parts.iter_mut().zip(&masks) {
        for primitive in &mut part.primitives {
            primitive.bsps = bsps;
        }
    }
    let mut levels = [(0f64, 0usize); 3];
    for part in &parts {
        for (total, (sum, count)) in levels.iter_mut().zip(part.levels) {
            total.0 += sum;
            total.1 += count;
        }
    }
    let images = parts.iter_mut().map(|part| part.lighting.take()).collect();
    let probes = parts
        .iter_mut()
        .map(|part| std::mem::take(&mut part.probes))
        .collect();
    let (lightmap, placements, probe_coordinates) = match lightmap::pack(images, probes)? {
        Some(packed) => (Some(packed.atlas), packed.placements, packed.probes),
        None => (None, vec![None; tags.len()], vec![Vec::new(); tags.len()]),
    };
    let mut level = Level {
        bsps: tags.to_vec(),
        materials: Vec::new(),
        primitives: Vec::new(),
        meshes: Vec::new(),
        instances: Vec::new(),
        lightmap: None,
        lighting_averages: levels.map(|(sum, count)| (sum / count.max(1) as f64) as f32),
        decorators: Vec::with_capacity(tags.len()),
        instanced,
    };
    for ((part, placement), probe_coordinates) in
        parts.into_iter().zip(placements).zip(probe_coordinates)
    {
        level.decorators.push(part.decorators);
        let mut indices = Vec::with_capacity(part.materials.len());
        for material in part.materials {
            let shared = material.tag.as_ref().and_then(|tag| {
                level.materials.iter().position(|known| {
                    known.tag.as_ref() == Some(tag) && known.decal == material.decal
                })
            });
            indices.push(shared.unwrap_or_else(|| {
                level.materials.push(material);
                level.materials.len() - 1
            }));
        }
        for (mut primitive, probe) in part.primitives.into_iter().zip(part.primitive_probes) {
            primitive.material = primitive
                .material
                .and_then(|material| indices.get(material).copied());
            if let (Some(atlas), Some(placement)) = (&lightmap, placement) {
                for vertex in &mut primitive.vertices {
                    vertex.lightmap = vertex.lightmap.map(|uv| atlas.coordinates(placement, uv));
                }
            }
            if let Some(&uv) = probe.and_then(|probe| probe_coordinates.get(probe)) {
                for vertex in &mut primitive.vertices {
                    vertex.lightmap = Some(uv);
                }
            }
            level.primitives.push(primitive);
        }
        level.meshes.push(part.meshes);
        level.instances.extend(part.instances);
    }
    level.lightmap = lightmap;
    Ok(level)
}

/// Adds `texel` to `probes` and its lit level to the sum and count `level`, and returns its index.
fn add_probe(
    probes: &mut Vec<lightmap::Texel>,
    level: &mut (f64, usize),
    texel: lightmap::Texel,
) -> usize {
    let (sum, count) = lightmap::lit_levels(std::iter::once(&texel));
    level.0 += sum;
    level.1 += count;
    probes.push(texel);
    probes.len() - 1
}

/// Reads the geometry each cluster and instance of `tag`, a structure BSP, draws, as the level's
/// BSP `bsp`. With the per-vertex lighting directions `lut`, each part of an instance or cluster
/// mesh lit per vertex takes a lighting probe.
#[allow(clippy::cognitive_complexity)]
fn read_bsp(
    cache: &Cache,
    pages: &mut Pages,
    tag: &Tag,
    bsp: usize,
    lut: Option<&[[f32; 3]]>,
) -> Result<Part> {
    ensure!(tag.group == "sbsp", "Expected structure BSP");
    let at = tag.address()?;
    cache.meta(at, BSP_SIZE)?;
    // Reach keeps a lightmapped BSP's render geometry in its lightmap BSP data, whose path is the
    // BSP's, and the BSP's own geometry block names an unused resource. A BSP that is never
    // lightmapped has no lightmap BSP data and draws its own geometry, unlit.
    let lightmap = cache
        .tags
        .iter()
        .flatten()
        .find(|candidate| candidate.group == LIGHTMAP_GROUP && candidate.path == tag.path)
        .cloned();
    let (owner, base) = match &lightmap {
        Some(data) => (data, data.address()? + LIGHTMAP_GEOMETRY),
        None => (tag, at + BSP_GEOMETRY),
    };
    let geometry = Geometry::read(cache, owner, base)?;
    let mut materials = cache
        .block(at + MATERIALS, MATERIAL_SIZE)?
        .into_iter()
        .map(|row| material::read(cache, cache.reference(row, None)?))
        .collect::<Result<Vec<_>>>()?;
    let clusters = cache.block(at + CLUSTERS, CLUSTER_SIZE)?;
    let cluster_meshes = cluster_meshes(cache, &clusters)?;
    let instances = instances(cache, tag, bsp)?;
    let sections = geometry.sections.len();
    if let Some(instance) = instances.iter().find(|instance| instance.mesh >= sections) {
        bail!("Instance draws mesh {} of {sections}", instance.mesh);
    }
    let mut meshes = Vec::with_capacity(sections);
    let mut mesh_bounds = Vec::with_capacity(sections);
    let required = cluster_meshes
        .keys()
        .copied()
        .chain(instances.iter().map(|instance| instance.mesh))
        .collect::<BTreeSet<_>>();
    for mesh in 0..sections {
        if !required.contains(&mesh) {
            meshes.push(Vec::new());
            mesh_bounds.push(None);
            continue;
        }
        let (primitives, bounds) = read_mesh(cache, pages, &geometry, owner, mesh, materials.len())
            .with_context(|| format!("Mesh {mesh} of {}", tag.path))?;
        meshes.push(primitives);
        mesh_bounds.push(bounds);
    }
    // Cluster meshes keep lightmap coordinates in a vertex buffer slot of their own, and placed
    // instances in streams the lightmap data names per instance. The coordinates stay in this
    // BSP's own lightmap image until the level packs its images into one atlas.
    let Sources {
        lighting,
        lightmap_streams,
        per_vertex,
        cluster_lighting,
        mut colorings,
    } = match &lightmap {
        Some(data) => Sources::read(cache, pages, data, lut.is_some(), clusters.len())?,
        None => Sources::default(),
    };
    let lit = lighting.is_some();
    let mut levels = [
        lighting
            .as_ref()
            .map_or((0., 0), lightmap::Lighting::levels),
        (0., 0),
        (0., 0),
    ];
    let lightmap_coordinates =
        |pages: &mut Pages, vb: Option<usize>| -> Result<Option<Vec<[f32; 2]>>> {
            let Some(vb) = vb.filter(|_| lit) else {
                return Ok(None);
            };
            let (count, declaration, bytes) = geometry.vertex_buffer(cache, pages, vb)?;
            Ok(Some(lightmap::texcoords(declaration, count, &bytes)?))
        };
    let apply = |primitive: &mut Primitive, coordinates: &[[f32; 2]]| -> Result<()> {
        if primitive.flags & PER_VERTEX_PART != 0 {
            return Ok(());
        }
        for vertex in &mut primitive.vertices {
            vertex.lightmap = Some(
                *coordinates
                    .get(vertex.source as usize)
                    .context("Vertex outside its lightmap coordinates")?,
            );
        }
        Ok(())
    };
    for &mesh in cluster_meshes.keys() {
        let slot = geometry.section_buffer(cache, mesh, lightmap::CLUSTER_SLOT)?;
        if let Some(coordinates) = lightmap_coordinates(pages, slot)? {
            for primitive in &mut meshes[mesh] {
                apply(primitive, &coordinates)?;
            }
        }
    }
    let mut primitives = Vec::new();
    let mut primitive_probes = Vec::new();
    let mut probes = Vec::new();
    // A cluster mesh's parts lit per vertex read the buffer its cluster row names.
    for (&mesh, &cluster) in &cluster_meshes {
        let buffer = cluster_lighting.get(cluster).copied().flatten();
        let vertex_lighting = VertexLighting::read(cache, pages, &geometry, buffer, lut)?;
        for primitive in &meshes[mesh] {
            let mut primitive = primitive.clone();
            if primitive.flags & PER_VERTEX_PART != 0
                && let Some(lighting) = &vertex_lighting
            {
                lighting.apply(&mut primitive)?;
            }
            let probe = vertex_lighting
                .as_ref()
                .filter(|_| primitive.flags & PER_VERTEX_PART != 0)
                .and_then(|lighting| lighting.probe(&primitive));
            primitive_probes.push(probe.map(|texel| add_probe(&mut probes, &mut levels[1], texel)));
            primitives.push(primitive);
        }
    }
    for (index, instance) in instances.iter().enumerate() {
        let coordinates =
            lightmap_coordinates(pages, lightmap_streams.get(index).copied().flatten())?;
        // An instance without lightmap coordinates is lit per vertex when the lightmap data names
        // its lighting buffer, and otherwise as one by its coloring row when it has one.
        let buffer = per_vertex
            .get(index)
            .copied()
            .flatten()
            .filter(|_| coordinates.is_none());
        let vertex_lighting = VertexLighting::read(cache, pages, &geometry, buffer, lut)?;
        let coloring = match (&coordinates, &vertex_lighting) {
            (None, None) => colorings.get_mut(index).and_then(Option::take),
            _ => None,
        };
        let coloring_light = coloring.as_ref().map(|coloring| coloring.analytical_light);
        let coloring_probe =
            coloring.map(|coloring| add_probe(&mut probes, &mut levels[2], coloring.texel));
        for primitive in &meshes[instance.mesh] {
            let mut placed = primitive.clone();
            placed.instance = Some(format!("instance{index}"));
            if let Some(identifier) = coloring_light {
                placed.analytical_light = Some(identifier);
            }
            if let Some(coordinates) = &coordinates {
                apply(&mut placed, coordinates)?;
            }
            // The probe compares each placed normal with its lobe, which is in the level's space.
            instance.place_primitive(&mut placed);
            if let Some(lighting) = &vertex_lighting {
                lighting.apply(&mut placed)?;
            }
            let probe = vertex_lighting
                .as_ref()
                .and_then(|lighting| lighting.probe(&placed));
            primitive_probes.push(
                probe
                    .map(|texel| add_probe(&mut probes, &mut levels[1], texel))
                    .or(coloring_probe),
            );
            primitives.push(placed);
        }
    }
    // Preplaced decals follow, with their decal systems' materials after the BSP's own.
    for decal in decals::read(cache, pages, tag, at, &mut materials)? {
        primitive_probes.push(None);
        primitives.push(decal);
    }
    // Then decorators, each instance lit by a probe of its own baked light. Their probes stay out
    // of the lit levels the report averages.
    let decorated = decorators::read(cache, pages, tag, at, &mut materials, &mut |texel| {
        add_probe(&mut probes, &mut (0., 0), texel)
    })?;
    let drawn: BTreeSet<usize> = instances.iter().map(|i| i.mesh).collect();
    let unused_meshes = (0..geometry.sections.len())
        .filter(|mesh| !cluster_meshes.contains_key(mesh) && !drawn.contains(mesh))
        .collect();
    Ok(Part {
        materials,
        primitives,
        primitive_probes,
        meshes: Meshes {
            clusters: cluster_meshes.into_keys().collect(),
            unused: unused_meshes,
            bounds: mesh_bounds,
        },
        instances,
        lighting,
        probes,
        levels,
        decorated,
        decorators: serde_json::Value::Null,
    })
}

/// Each cluster mesh of a BSP whose clusters are the rows `clusters`, with the first cluster that
/// draws it.
fn cluster_meshes(cache: &Cache, clusters: &[usize]) -> Result<BTreeMap<usize, usize>> {
    let mut meshes = BTreeMap::new();
    for (cluster, &row) in clusters.iter().enumerate() {
        let mesh = cache.i16(row + CLUSTER_MESH)?;
        if mesh >= 0 {
            meshes.entry(usize::try_from(mesh)?).or_insert(cluster);
        }
    }
    Ok(meshes)
}

/// A box's least and greatest corners.
type Bounds = [[f32; 3]; 2];

/// Mesh `mesh` of `geometry`, which `owner` keeps, in its own space: its primitives, whose
/// materials index the BSP's `materials`, and the bounds of its vertices. An empty section
/// draws nothing and has no bounds. Malformed required sections propagate their decode error.
fn read_mesh(
    cache: &Cache,
    pages: &mut Pages,
    geometry: &Geometry,
    owner: &Tag,
    mesh: usize,
    materials: usize,
) -> Result<(Vec<Primitive>, Option<Bounds>)> {
    let Some(Section {
        row,
        vertices,
        indices,
        restart,
        topology,
        parts,
        ..
    }) = geometry.section(cache, pages, owner, mesh, 1)?
    else {
        return Ok((Vec::new(), None));
    };
    let mut primitives = Vec::new();
    let water = water::Coordinates::read(cache, pages, geometry, mesh, &parts)?;
    let mut subparts = BTreeMap::<usize, Vec<usize>>::new();
    for subpart in cache.block(row + 12, 16)? {
        let parent = usize::try_from(cache.i16(subpart + 8)?)?;
        ensure!(parent < parts.len(), "Subpart parent outside mesh");
        subparts.entry(parent).or_default().push(subpart);
    }
    for (index, &part) in parts.iter().enumerate() {
        let ranges = subparts.remove(&index).unwrap_or_default();
        // A non-lightmapped section may have no subparts. Required subpart ranges must cover
        // the parent exactly, since silently drawing the parent would erase light selection.
        let ranges = if ranges.is_empty() {
            vec![(part + 4, None)]
        } else {
            let first = cache.u32(part + 4)?;
            let end = first
                .checked_add(cache.u32(part + 8)?)
                .context("Part range overflow")?;
            let mut next = first;
            let mut ranges = ranges
                .into_iter()
                .map(|row| {
                    Ok((
                        cache.u32(row)?,
                        cache.u32(row + 4)?,
                        row,
                        cache.u32(row + 12)?,
                    ))
                })
                .collect::<Result<Vec<_>>>()?;
            ranges.sort_unstable_by_key(|range| range.0);
            for &(start, count, _, _) in &ranges {
                ensure!(start == next, "Subpart ranges overlap or leave a gap");
                next = start.checked_add(count).context("Subpart range overflow")?;
                ensure!(next <= end, "Subpart exceeds parent part");
            }
            ensure!(next == end, "Subparts do not cover parent part");
            ranges
                .into_iter()
                .map(|(_, _, row, id)| (row, Some(id)))
                .collect()
        };
        for (range, analytical_light) in ranges {
            let mut primitive = model::draw(
                cache, mesh, index, part, range, topology, &indices, restart, &vertices, materials,
            )?;
            primitive.analytical_light = analytical_light;
            if let Some(water) = &water {
                water.apply(cache, part, range, topology, &indices, &mut primitive)?;
            }
            if !primitive.indices.is_empty() {
                primitives.push(primitive);
            }
        }
    }
    Ok((primitives, Some(vertex_bounds(&vertices))))
}

/// The least and greatest position of `vertices` on each axis.
fn vertex_bounds(vertices: &[model::Vertex]) -> Bounds {
    let mut bounds = [[f32::MAX; 3], [f32::MIN; 3]];
    for vertex in vertices {
        for (axis, &value) in vertex.position.iter().enumerate() {
            bounds[0][axis] = bounds[0][axis].min(value);
            bounds[1][axis] = bounds[1][axis].max(value);
        }
    }
    bounds
}

/// What a BSP's lightmap BSP data holds for lighting its geometry: its decoded lightmap image,
/// each instance's lightmap coordinate stream, each instance's and cluster's per-vertex lighting
/// buffer, and each instance's coloring.
#[derive(Default)]
struct Sources {
    lighting: Option<lightmap::Lighting>,
    lightmap_streams: Vec<Option<usize>>,
    per_vertex: Vec<Option<lightmap::PerVertex>>,
    cluster_lighting: Vec<Option<lightmap::PerVertex>>,
    colorings: Vec<Option<lightmap::Coloring>>,
}

impl Sources {
    /// Reads the sources of `data`, the lightmap BSP data of a BSP with `clusters` clusters.
    /// Instances take lightmap coordinates only when its image decodes, and the BSP takes
    /// per-vertex lighting only `with_per_vertex`, when the level knows its lighting directions.
    /// The cluster rows follow the clusters, so they apply only when there is one for each.
    fn read(
        cache: &Cache,
        pages: &mut Pages,
        data: &Tag,
        with_per_vertex: bool,
        clusters: usize,
    ) -> Result<Self> {
        let lighting = lightmap::decode(cache, pages, data)?;
        let lightmap_streams = if lighting.is_some() {
            lightmap::instance_buffers(cache, data)?
        } else {
            Vec::new()
        };
        let (per_vertex, cluster_lighting) = if with_per_vertex {
            (
                lightmap::per_vertex_buffers(cache, data)?,
                Some(lightmap::cluster_buffers(cache, data)?)
                    .filter(|rows| rows.len() == clusters)
                    .unwrap_or_default(),
            )
        } else {
            (Vec::new(), Vec::new())
        };
        Ok(Self {
            lighting,
            lightmap_streams,
            per_vertex,
            cluster_lighting,
            colorings: lightmap::colorings(cache, data)?,
        })
    }
}

/// A per-vertex lighting buffer, read, with the lighting directions its rows index.
struct VertexLighting<'a> {
    bytes: Vec<u8>,
    buffer: lightmap::PerVertex,
    lut: &'a [[f32; 3]],
}

impl<'a> VertexLighting<'a> {
    fn apply(&self, primitive: &mut Primitive) -> Result<()> {
        for vertex in &mut primitive.vertices {
            vertex.lighting = Some(
                lightmap::vertex(&self.bytes, self.buffer, vertex.source, self.lut)
                    .context("Per-vertex lighting does not cover a drawn source vertex")?,
            );
        }
        Ok(())
    }
    /// Reads `buffer` from `geometry` when there is one and the directions `lut` to decode it.
    fn read(
        cache: &Cache,
        pages: &mut Pages,
        geometry: &Geometry,
        buffer: Option<lightmap::PerVertex>,
        lut: Option<&'a [[f32; 3]]>,
    ) -> Result<Option<Self>> {
        let (Some(buffer), Some(lut)) = (buffer, lut) else {
            return Ok(None);
        };
        let bytes = geometry.vertex_buffer(cache, pages, buffer.buffer)?.2;
        Ok(Some(Self { bytes, buffer, lut }))
    }

    /// The lighting probe of `primitive`'s vertices, whose normals are in the level's space.
    fn probe(&self, primitive: &Primitive) -> Option<lightmap::Texel> {
        lightmap::probe(
            &self.bytes,
            self.buffer,
            primitive
                .vertices
                .iter()
                .map(|vertex| (vertex.source, vertex.normal)),
            self.lut,
        )
    }
}

/// What [`export`] wrote.
#[derive(Serialize)]
pub struct Export {
    pub bsp: String,
    pub materials: usize,
    pub primitives: usize,
    pub vertices: usize,
    pub triangles: usize,
    pub cluster_meshes: Vec<usize>,
    pub instances: Vec<Instance>,
    pub unused_meshes: Vec<usize>,
    pub bounds: [[f32; 3]; 2],
    pub mesh_bounds: Vec<Option<[[f32; 3]; 2]>>,
}

/// Writes the playable structure BSP of `map` in `maps` as a Wavefront OBJ, one group per
/// primitive, in meters with Reach's Z up.
pub fn export(maps: &Path, map: &str, output: &Path) -> Result<Export> {
    let cache = Cache::open(&maps.join(map))?;
    let mut pages = Pages::default();
    let bsp = playable(&cache)?;
    let level = read(&cache, &mut pages, &bsp)?;
    let mut obj = String::new();
    let mut base = 1usize;
    let mut bounds = [[f32::MAX; 3], [f32::MIN; 3]];
    let mut triangles = 0;
    for (index, primitive) in level.primitives.iter().enumerate() {
        writeln!(
            obj,
            "g {}mesh{}_part{}_{index}\nusemtl material{}",
            primitive
                .instance
                .as_ref()
                .map_or(String::new(), |name| format!("{name}_")),
            primitive.section,
            primitive.part,
            primitive.material.map_or(-1, |m| m as i64)
        )?;
        for vertex in &primitive.vertices {
            let [x, y, z] = vertex.position;
            for (axis, value) in vertex.position.iter().copied().enumerate() {
                bounds[0][axis] = bounds[0][axis].min(value);
                bounds[1][axis] = bounds[1][axis].max(value);
            }
            writeln!(obj, "v {x} {y} {z}")?;
        }
        for vertex in &primitive.vertices {
            let [u, v] = vertex.uv;
            writeln!(obj, "vt {u} {}", 1.0 - v)?;
        }
        for face in primitive.indices.chunks_exact(3) {
            let [a, b, c] = [0, 1, 2].map(|k| base + face[k] as usize);
            writeln!(obj, "f {a}/{a} {b}/{b} {c}/{c}")?;
            triangles += 1;
        }
        base += primitive.vertices.len();
    }
    fs::write(output, obj)?;
    let meshes = level
        .meshes
        .into_iter()
        .next()
        .context("The level has no structure BSP")?;
    Ok(Export {
        bsp: bsp.path.clone(),
        materials: level.materials.len(),
        primitives: level.primitives.len(),
        vertices: base - 1,
        triangles,
        cluster_meshes: meshes.clusters.into_iter().collect(),
        instances: level.instances,
        unused_meshes: meshes.unused.into_iter().collect(),
        bounds,
        mesh_bounds: meshes.bounds,
    })
}

/// The material of `level` whose shader tag path ends with `material`.
fn material_ending<'a>(level: &'a Level, material: &str) -> Result<&'a Material> {
    level
        .materials
        .iter()
        .find(|m| {
            m.tag
                .as_ref()
                .is_some_and(|tag| tag.path.ends_with(material))
        })
        .with_context(|| format!("No level material ends with {material}"))
}

/// A `pixl` or `vtsh` tag's compiled shader rows, and each row's bytecode slots: a size, then a
/// data reference whose address is `0x0C` further on.
const COMPILED_SHADERS: usize = 0x14;
const COMPILED_SHADER_SIZE: usize = 0x98;
const BYTECODE_SLOTS: [(&str, usize); 3] = [("xenon", 0x04), ("dx9", 0x18), ("durango", 0x2C)];
/// The largest compiled shader a dump writes.
const DUMP_LIMIT: usize = 4 * 1024 * 1024;

/// Writes each compiled shader of the `pixl` or `vtsh` tag at `shader` into `output` as
/// `<prefix><index>-<slot>.bin`, and returns each one's written slots and sizes.
fn write_compiled_shaders(
    cache: &Cache,
    shader: usize,
    prefix: &str,
    output: &Path,
) -> Result<Vec<serde_json::Value>> {
    let mut shaders = Vec::new();
    for (index, row) in cache
        .block(shader + COMPILED_SHADERS, COMPILED_SHADER_SIZE)?
        .into_iter()
        .enumerate()
    {
        let mut slots = Vec::new();
        for (slot, offset) in BYTECODE_SLOTS {
            let size = usize::try_from(cache.u32(row + offset)?)?;
            if size == 0 {
                continue;
            }
            ensure!(size <= DUMP_LIMIT, "Compiled shader exceeds the dump limit");
            let bytes = cache.meta(cache.expand(cache.u32(row + offset + 0x0C)?)?, size)?;
            fs::write(output.join(format!("{prefix}{index}-{slot}.bin")), bytes)?;
            slots.push(serde_json::json!({"slot": slot, "size": size}));
        }
        shaders.push(serde_json::json!({
            "index": index,
            "slots": slots,
            "cache_shader_index": cache.u32(row + 0x8C)?,
        }));
    }
    Ok(shaders)
}

/// Writes the compiled pixel shaders of the template of the level material whose tag path ends
/// with `material` into `output`: each of the template's pixel shader's compiled shaders and its
/// three bytecode slots (Xenon, DX9 and Durango) as `<index>-<slot>.bin`, and `shaders.json` with
/// the template, the pixel shader's entry points (start and count) and the template's texture
/// and float constant names.
pub fn export_shaders(maps: &Path, map: &str, material: &str, output: &Path) -> Result<()> {
    let cache = Cache::open(&maps.join(map))?;
    let mut pages = Pages::default();
    // Every BSP the level loads, since Forge World keeps its cliffs in its second.
    let tags = bsps(&cache, &[0])?
        .into_iter()
        .map(|bsp| bsp.tag)
        .collect::<Vec<_>>();
    let level = read_all(&cache, &mut pages, &tags)?;
    let found = material_ending(&level, material)?;
    let template = &found
        .properties
        .first()
        .context("Material has no shader template")?
        .template;
    let template_at = template.address()?;
    let pixel = cache
        .reference(template_at + 0x10, Some("pixl"))?
        .context("Template has no pixel shader")?;
    let pixel_at = pixel.address()?;
    let entry_points = cache
        .block(pixel_at + 0x04, 2)?
        .into_iter()
        .map(|row| Ok([cache.u8(row)?, cache.u8(row + 1)?]))
        .collect::<Result<Vec<_>>>()?;
    fs::create_dir_all(output)?;
    let shaders = write_compiled_shaders(&cache, pixel_at, "", output)?;
    let names = |at: usize| -> Result<Vec<String>> {
        cache
            .block(template_at + at, 4)?
            .into_iter()
            .map(|row| cache.string_id(cache.u32(row)?))
            .collect()
    };
    let report = serde_json::json!({
        "material": found.tag,
        "template": template,
        "pixel_shader": pixel,
        "pixel_entry_points": entry_points,
        "template_entry_points": cache
            .block(template_at + 0x24, 2)?
            .into_iter()
            .map(|row| cache.u16(row))
            .collect::<Result<Vec<_>>>()?,
        "float_constants": names(0x48)?,
        "textures": names(0x6C)?,
        "shaders": shaders,
    });
    fs::write(
        output.join("shaders.json"),
        serde_json::to_string_pretty(&report)?,
    )?;
    Ok(())
}

/// Writes what the playable structure BSP of `map` lights with beyond its lightmap images into
/// `output`: `lighting.json` with the lightmap BSP data's brightness, a count of its geometry's
/// subparts by analytical light index, its instance rows, coloring rows, per-vertex lighting
/// buffer indices and the first six such buffers (each one's declaration and first bytes), and
/// the compiled vertex shaders of the template of the material whose path ends with `material`
/// as `vertex-<index>-<slot>.bin`, with that material's `g_sample_vmf_diffuse` table as an image.
/// The scenario's cubemap rows (`+0x7B8`, `0x20` bytes: a name, a position, two indices and a
/// block of points) and each structure BSP's cubemap bitmap with its image count and the cube
/// indices its clusters name, written to `cubemaps.json` in `output`.
pub fn export_cubemaps(maps: &Path, map: &str, output: &Path) -> Result<()> {
    let cache = Cache::open(&maps.join(map))?;
    let scenario = cache.only("scnr")?.address()?;
    let cubemaps = cache
        .block(scenario + 0x7B8, 0x20)?
        .into_iter()
        .map(|row| {
            Ok(serde_json::json!({
                "name": cache.u32(row)?,
                "position": cache.floats::<3>(row + 4)?.map(|v| v * METRES_PER_UNIT),
                "word_10": cache.i16(row + 0x10)?,
                "word_12": cache.i16(row + 0x12)?,
                "points": cache.block(row + 0x14, 0x10)?.len(),
            }))
        })
        .collect::<Result<Vec<_>>>()?;
    let bsps = cache
        .block(scenario + 0x50, 0xAC)?
        .into_iter()
        .enumerate()
        .map(|(index, row)| {
            let structure = cache.reference(row, Some("sbsp"))?;
            let bitmap = cache.reference(row + 0x58, Some("bitm")).ok().flatten();
            let images = match &bitmap {
                Some(bitmap) => Some(cache.block(bitmap.address()? + 124, 56)?.len()),
                None => None,
            };
            let mut named = std::collections::BTreeSet::new();
            if let Some(structure) = &structure {
                for cluster in cache.block(structure.address()? + 0x138, 0x8C)? {
                    for entry in cache.block(cluster + 0x80, 0x10)? {
                        named.insert(cache.i16(entry)?);
                    }
                }
            }
            Ok(serde_json::json!({
                "index": index,
                "structure": structure.map(|tag| tag.path),
                "bitmap": bitmap.map(|tag| tag.path),
                "images": images,
                "named": named,
            }))
        })
        .collect::<Result<Vec<_>>>()?;
    fs::create_dir_all(output)?;
    fs::write(
        output.join("cubemaps.json"),
        serde_json::to_string_pretty(&serde_json::json!({"cubemaps": cubemaps, "bsps": bsps}))?,
    )?;
    Ok(())
}

pub fn export_lighting(maps: &Path, map: &str, material: &str, output: &Path) -> Result<()> {
    let cache = Cache::open(&maps.join(map))?;
    let mut pages = Pages::default();
    let bsp = playable(&cache)?;
    let lightmap = cache.find(LIGHTMAP_GROUP, &bsp.path)?.clone();
    let at = lightmap.address()?;
    let geometry = Geometry::read(&cache, &lightmap, at + LIGHTMAP_GEOMETRY)?;
    fs::create_dir_all(output)?;
    let mut lights = BTreeMap::<i32, usize>::new();
    for &mesh in &geometry.sections {
        for subpart in cache.block(mesh + 0x0C, 0x10)? {
            *lights.entry(cache.i32(subpart + 0x0C)?).or_default() += 1;
        }
    }
    let colors = cache
        .block(at + 0x6C, 0x24)?
        .into_iter()
        .map(|row| {
            (0..9)
                .map(|k| {
                    let word = cache.u32(row + k * 4)?;
                    Ok(serde_json::json!([
                        format!("{}", f32::from_bits(word)),
                        word
                    ]))
                })
                .collect::<Result<Vec<_>>>()
        })
        .collect::<Result<Vec<_>>>()?;
    let instances = cache
        .block(at + 0x60, 0x0C)?
        .into_iter()
        .map(|row| {
            Ok([
                i64::from(cache.i16(row)?),
                i64::from(cache.i16(row + 2)?),
                i64::from(cache.i16(row + 4)?),
                i64::from(cache.i16(row + 6)?),
                i64::from(cache.u32(row + 8)?),
            ])
        })
        .collect::<Result<Vec<_>>>()?;
    let per_vertex = cache
        .block(at + 0x48, 4)?
        .into_iter()
        .map(|row| cache.i16(row))
        .collect::<Result<Vec<_>>>()?;
    let mut buffers = Vec::new();
    for vb in per_vertex
        .iter()
        .filter_map(|&vb| usize::try_from(vb).ok())
        .take(6)
    {
        let (count, declaration, bytes) = geometry.vertex_buffer(&cache, &mut pages, vb)?;
        buffers.push(serde_json::json!({
            "buffer": vb,
            "count": count,
            "declaration": format!("{declaration:#x}"),
            "bytes": bytes.len(),
            "first": hex::encode(&bytes[..bytes.len().min(128)]),
        }));
    }
    let level = read(&cache, &mut pages, &bsp)?;
    let found = material_ending(&level, material)?;
    let template = &found
        .properties
        .first()
        .context("Material has no shader template")?
        .template;
    let vertex = cache
        .reference(template.address()?, Some("vtsh"))?
        .context("Template has no vertex shader")?;
    let vertex_at = vertex.address()?;
    let entry_points = cache
        .block(vertex_at + 0x04, 0x0C)?
        .into_iter()
        .map(|row| {
            cache
                .block(row, 2)?
                .into_iter()
                .map(|types| Ok([cache.u8(types)?, cache.u8(types + 1)?]))
                .collect::<Result<Vec<_>>>()
        })
        .collect::<Result<Vec<_>>>()?;
    let vertex_shaders = write_compiled_shaders(&cache, vertex_at, "vertex-", output)?;
    let table = found.mapping(&["g_sample_vmf_diffuse"]).map(|mapping| {
        let bitmap = mapping.bitmap.as_ref().context("VMF table bitmap")?;
        let image = material::image(&cache, &mut pages, bitmap, 0, false)?;
        image.save(output)?;
        Ok::<_, anyhow::Error>(serde_json::json!({
            "image": image.name(),
            "width": image.width,
            "height": image.height,
            "format": image.format,
            "curve": image.curve,
        }))
    });
    let report = serde_json::json!({
        "lightmap": lightmap.path,
        "brightness": cache.f32(at + 0x18)?,
        "analytical_lights": lights,
        "instances": instances,
        "colors": colors,
        "per_vertex_buffers": per_vertex,
        "buffers": buffers,
        "vertex_shader": vertex,
        "vertex_entry_points": entry_points,
        "vertex_shaders": vertex_shaders,
        "vmf_table": table.transpose()?,
    });
    fs::write(
        output.join("lighting.json"),
        serde_json::to_string_pretty(&report)?,
    )?;
    Ok(())
}

/// Writes, for each primitive of the level `map` whose material path contains `pattern`, its
/// instance, center, and the decoded light its vertices find at their lightmap atlas coordinates
/// in units of the atlas's average lit level, with the atlas rows they sample, to `output`.
pub fn export_primitive_lighting(
    maps: &Path,
    map: &str,
    pattern: &str,
    output: &Path,
) -> Result<()> {
    let cache = Cache::open(&maps.join(map))?;
    let mut pages = Pages::default();
    let tags = bsps(&cache, &[0])?
        .into_iter()
        .map(|bsp| bsp.tag)
        .collect::<Vec<_>>();
    let level = read_all(&cache, &mut pages, &tags)?;
    let atlas = level
        .lightmap
        .as_ref()
        .context("The level has no lightmap")?;
    let luminance = |c: [f32; 3]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
    let mut rows = Vec::new();
    for (index, primitive) in level.primitives.iter().enumerate() {
        let Some(path) = primitive
            .material
            .and_then(|m| level.materials.get(m))
            .and_then(|material| material.tag.as_ref())
            .map(|tag| tag.path.as_str())
            .filter(|path| path.contains(pattern))
        else {
            continue;
        };
        let mut levels = Vec::new();
        let mut atlas_rows = BTreeSet::new();
        let mut center = [0f64; 3];
        for vertex in &primitive.vertices {
            for (sum, &value) in center.iter_mut().zip(&vertex.position) {
                *sum += f64::from(value);
            }
            let Some([u, v]) = vertex.lightmap else {
                continue;
            };
            let x = ((u * atlas.width as f32) as usize).min(atlas.width - 1);
            let y = ((v * atlas.height as f32) as usize).min(atlas.height - 1);
            atlas_rows.insert(y);
            let texel = atlas.texel_at(x, y);
            levels.push(0.5 * luminance(texel.lobe) + luminance(texel.ambient));
        }
        let count = primitive.vertices.len().max(1) as f64;
        levels.sort_by(f32::total_cmp);
        rows.push(serde_json::json!({
            "primitive": index,
            "instance": primitive.instance,
            "material": path,
            "vertices": primitive.vertices.len(),
            "without_coordinates": primitive.vertices.len() - levels.len(),
            "center": center.map(|sum| sum / count),
            "level_mean": levels.iter().sum::<f32>() / levels.len().max(1) as f32,
            "level_min": levels.first(),
            "level_median": levels.get(levels.len() / 2),
            "level_max": levels.last(),
            "atlas_rows": [atlas_rows.first(), atlas_rows.last()],
        }));
    }
    fs::create_dir_all(output)?;
    fs::write(
        output.join("primitive-lighting.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "atlas": [atlas.width, atlas.height],
            "scale": atlas.scale,
            "primitives": rows,
        }))?,
    )?;
    Ok(())
}

/// Writes the materials of the playable structure BSP of `map` in `maps` to `output` as JSON: each
/// material's shader tag, options, constants and texture mappings, with every mapped bitmap's size,
/// each detail map's mean stored color, and the triangles the level draws with the material.
pub fn export_materials(maps: &Path, map: &str, output: &Path) -> Result<()> {
    let cache = Cache::open(&maps.join(map))?;
    let mut pages = Pages::default();
    let bsp = playable(&cache)?;
    let level = read(&cache, &mut pages, &bsp)?;
    let mut triangles = vec![0usize; level.materials.len()];
    for primitive in &level.primitives {
        if let Some(slot) = primitive.material.and_then(|m| triangles.get_mut(m)) {
            *slot += primitive.indices.len() / 3;
        }
    }
    let mut rows = Vec::with_capacity(level.materials.len());
    for (index, material) in level.materials.iter().enumerate() {
        let mut sizes = Vec::new();
        for mapping in material.properties.iter().flat_map(|p| &p.textures) {
            let size = match &mapping.bitmap {
                Some(bitmap) => {
                    let images = cache.block(bitmap.address()? + 124, 56)?;
                    let frame = mapping.static_frame(&cache).unwrap_or(0);
                    match images.get(frame) {
                        Some(&row) => serde_json::json!([
                            cache.u16(row)?,
                            cache.u16(row + 2)?,
                            cache.i16(row + 8)?,
                            cache.u8(row + 16)?,
                            cache.u8(row + 17)?
                        ]),
                        None => serde_json::Value::Null,
                    }
                }
                None => serde_json::Value::Null,
            };
            let mean = match &mapping.bitmap {
                Some(bitmap) if mapping.role.starts_with("detail_map") => {
                    let frame = mapping.static_frame(&cache).unwrap_or(0);
                    let image = material::image(&cache, &mut pages, bitmap, frame, false)?;
                    let mut sum = [0u64; 4];
                    for pixel in image.rgba.chunks_exact(4) {
                        for (total, value) in sum.iter_mut().zip(pixel) {
                            *total += u64::from(*value);
                        }
                    }
                    let count = (image.rgba.len() / 4).max(1) as f64;
                    serde_json::json!(sum.map(|total| total as f64 / count / 255.))
                }
                _ => serde_json::Value::Null,
            };
            sizes.push(serde_json::json!({"role": mapping.role, "size": size, "mean": mean}));
        }
        rows.push(serde_json::json!({
            "index": index,
            "triangles": triangles[index],
            "material": material,
            "bitmap_sizes": sizes,
        }));
    }
    fs::write(output, serde_json::to_string_pretty(&rows)?)?;
    Ok(())
}

/// Writes the playable structure BSP's two structure resources of `map` into `output`: each one's
/// gestalt entry, definition block, page data from its start, and definition relocations as
/// pairs of field offset and target.
pub fn dump_structure_resources(maps: &Path, map: &str, output: &Path) -> Result<()> {
    let cache = Cache::open(&maps.join(map))?;
    let mut pages = Pages::default();
    let bsp = playable(&cache)?;
    let at = bsp.address()?;
    let zone = cache.only("zone")?.address()?;
    fs::write(output.join("sbsp-meta.bin"), cache.meta(at, BSP_SIZE)?)?;
    let (start, metadata, base) = cache.metadata();
    fs::write(output.join("metadata.bin"), metadata)?;
    let tags = cache
        .tags
        .iter()
        .flatten()
        .map(|tag| serde_json::json!([tag.datum, tag.group, tag.path, tag.offset]))
        .collect::<Vec<_>>();
    fs::write(
        output.join("tags.json"),
        serde_json::to_string(&serde_json::json!({"start": start, "base": base, "tags": tags}))?,
    )?;
    for field in [0x518usize, STRUCTURE_RESOURCE] {
        let handle = cache.u32(at + field)?;
        let resource = cache.resource(&bsp, handle)?;
        let page = pages.read(&cache, &resource)?;
        let end = resource
            .fixups
            .iter()
            .max()
            .map_or(0, |&last| last + 0x10000)
            .min(page.len() - resource.page_offset);
        let entry = *cache
            .block(zone + 100, 64)?
            .get((handle & 65535) as usize)
            .context("Structure resource outside gestalt")?;
        let relocations = cache
            .block(entry + 40, 8)?
            .into_iter()
            .map(|r| Ok((cache.u32(r)?, cache.u32(r + 4)?)))
            .collect::<Result<Vec<_>>>()?;
        fs::write(
            output.join(format!("resource-{field:x}-entry.bin")),
            cache.meta(entry, 64)?,
        )?;
        fs::write(
            output.join(format!("resource-{field:x}-definition.bin")),
            cache.meta(resource.fixup_offset, resource.fixup_size)?,
        )?;
        fs::write(
            output.join(format!("resource-{field:x}-data.bin")),
            &page[resource.page_offset..resource.page_offset + end],
        )?;
        fs::write(
            output.join(format!("resource-{field:x}-relocations.json")),
            serde_json::to_string(&relocations)?,
        )?;
    }
    Ok(())
}

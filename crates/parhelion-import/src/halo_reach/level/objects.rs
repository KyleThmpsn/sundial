//! Objects the scenario places as part of the level: its crates, such as catwalks, cover, ramps,
//! gravity lifts and props, its scenery and device controls, such as benches, screens and health
//! stations, and the machines that never move, each drawn with its variant's render model at its
//! placement, and its sky, drawn around the level from the origin. With a map variant, the variant's
//! Forge objects stand in for the scenario's placements of everything its palette offers. The sky
//! stays apart from the level's geometry, since Reach draws it in a pass of its own and a native map
//! draws it as sky objects. Multiplayer markers like spawn points, flag bases and hill beacons are
//! gametype objects and stay out.
use super::{
    super::{
        cache::{Cache, Tag},
        choices,
        material::Material,
        model::{self, Primitive},
        object::Object,
        resource::{Absent, Pages},
        rig,
    },
    METRES_PER_UNIT,
    variant::Variant,
};
use anyhow::{Context, Result, ensure};
use std::collections::{BTreeMap, BTreeSet, btree_map::Entry};

/// A scenario placement block: its rows, their size, its palette of one tag reference per row, and
/// the palette's tag group.
struct Block {
    rows: usize,
    size: usize,
    palette: usize,
    group: &'static str,
}

/// The placement blocks the level draws.
const CRATES: Block = Block {
    rows: 0x600,
    size: 0xD8,
    palette: 0x60C,
    group: "bloc",
};
const SCENERY: Block = Block {
    rows: 0xFC,
    size: 0xDC,
    palette: 0x108,
    group: "scen",
};
const CONTROLS: Block = Block {
    rows: 0x1B0,
    size: 0xDC,
    palette: 0x1BC,
    group: "ctrl",
};
const MACHINES: Block = Block {
    rows: 0x180,
    size: 0xE4,
    palette: 0x18C,
    group: "mach",
};
/// Each row's palette index at `+0x00`, then its placement flags, position in world units,
/// rotation in radians (yaw, pitch, roll), scale and variant name.
const PLACEMENT_FLAGS: usize = 0x04;
const PLACEMENT_POSITION: usize = 0x08;
const PLACEMENT_ROTATION: usize = 0x14;
const PLACEMENT_SCALE: usize = 0x20;
const PLACEMENT_VARIANT: usize = 0x58;
/// A placement the scenario never creates.
const NEVER_PLACED: u32 = 1 << 6;
const PALETTE_SIZE: usize = 0x10;
/// A device row's power and position groups, `-1` for none, and its device flags.
const DEVICE_POWER_GROUP: usize = 0x70;
const DEVICE_POSITION_GROUP: usize = 0x72;
const DEVICE_FLAGS: usize = 0x74;
/// The device flag of a device that starts switched off. A machine that starts off and that no
/// power or position group can switch on keeps its starting position, closed.
const INITIALLY_OFF: u32 = 1 << 1;
/// The scenario's skies, each naming its sky object first and the structure BSPs it is active on
/// as a mask at `+0x2E`.
const SKIES: usize = 0x88;
const SKY_SIZE: usize = 0x30;
const SKY_BSPS: usize = 0x2E;
/// Gametype markers, which the level does not draw.
const MARKERS: &str = r"objects\multi\";
/// The groups of map variant objects the level draws: crates, scenery and controls.
const VARIANT_GROUPS: [&str; 3] = ["bloc", "scen", "ctrl"];
/// A render model's geometry block, and a section's eight vertex buffer slots.
const MODEL_GEOMETRY: usize = 100;
const VERTEX_SLOTS: usize = 8;
/// The vertex buffer type of a stream of three floats per vertex, which a mesh lit by its vertex
/// colors (lighting policy 0) keeps its colors in.
const COLOR_STREAM: u32 = 0x000C_000E;
/// Shader groups drawn without a surface to stand on: decals, water and holograms.
const UNSOLID: [&str; 3] = ["rmd ", "rmw ", "rmhg"];

/// The placed objects' render geometry in world meters, with their own materials, and the
/// triangles of the crates' solid surfaces for collision.
pub struct Objects {
    pub materials: Vec<Material>,
    pub primitives: Vec<Primitive>,
    pub faces: Vec<[[f32; 3]; 3]>,
    /// Each drawn object's path and placement count.
    pub placed: BTreeMap<String, usize>,
    /// Each placed object's path and the primitives it leaves out because their material never
    /// draws. An object left with none is not counted as placed.
    pub hidden: BTreeMap<String, usize>,
    /// Loaded placements whose machine motion and collision lifecycle have no native binding.
    pub motion: BTreeMap<String, usize>,
    /// The skies' render geometry about the origin, in meters and in the order Reach draws it,
    /// with materials among `materials`, and each vertex's color: the light Reach baked into
    /// the sky's vertex color stream. It is never stood on.
    pub sky: Vec<(Primitive, Vec<[f32; 3]>)>,
    /// The objects left out because their render geometry holds no data in the cache.
    pub absent: BTreeSet<String>,
}

/// Whether a surface drawn with `material` is solid: neither blended nor a decal, water or
/// hologram. Glass blends but stays solid, as Reach's glass panels block players and shots.
fn solid(material: Option<&Material>) -> bool {
    material.is_none_or(|material| {
        (material.alpha_mode() != "BLEND" || material.glass())
            && material
                .tag
                .as_ref()
                .is_none_or(|tag| !UNSOLID.contains(&tag.group.as_str()))
    })
}

/// The column-major transform of forward, left and up `axes`, scaled, then moved to `position`.
fn transform(position: [f32; 3], axes: [[f32; 3]; 3], scale: f32) -> [f32; 16] {
    let mut out = rig::IDENTITY;
    for (column, axis) in axes.into_iter().enumerate() {
        for k in 0..3 {
            out[column * 4 + k] = axis[k] * scale;
        }
    }
    out[12..15].copy_from_slice(&position);
    out
}

/// The column-major transform of a scenario placement, scaled, then moved to `position`. Reach
/// turns by yaw about the world's up axis first, then by pitch about its y axis, nose up, then by
/// roll about its x axis, as the map variants that re-place these rows show.
fn placement(position: [f32; 3], [yaw, pitch, roll]: [f32; 3], scale: f32) -> [f32; 16] {
    let (sy, cy) = yaw.sin_cos();
    let (sp, cp) = pitch.sin_cos();
    let (sr, cr) = roll.sin_cos();
    let forward = [cp * cy, cr * sy - sr * sp * cy, sr * sy + cr * sp * cy];
    let left = [-cp * sy, cr * cy + sr * sp * sy, sr * cy - cr * sp * sy];
    let up = [-sp, -sr * cp, cr * cp];
    transform(position, [forward, left, up], scale)
}

/// The render model `tag` draws with `variant`, or its default variant when it has none by that
/// name, when it has a world model.
fn model_of(
    cache: &Cache,
    pages: &mut Pages,
    tag: &Tag,
    variant: &str,
) -> Result<Option<model::Model>> {
    let object = Object::read_appearance(cache, tag)?;
    let Some(render) = object
        .world
        .as_ref()
        .and_then(|branch| branch.model.as_ref())
    else {
        return Ok(None);
    };
    let requested = (!variant.is_empty() && object.variants.iter().any(|v| v.name == variant))
        .then_some(variant);
    let selection = choices(&object, requested, &BTreeMap::new())?;
    model::read_static(cache, pages, render, &selection)
        .with_context(|| format!("Object {}", tag.path))
        .map(Some)
}

/// Each primitive's vertex colors in `model`, the render model `render` draws: the stream of
/// three floats per vertex its section keeps among its vertex buffer slots, read at each vertex's
/// index in its section. A section without one reads as white.
fn vertex_colors(
    cache: &Cache,
    pages: &mut Pages,
    render: &Tag,
    model: &model::Model,
) -> Result<Vec<Vec<[f32; 3]>>> {
    let geometry = model::Geometry::read(cache, render, render.address()? + MODEL_GEOMETRY)?;
    let mut streams = BTreeMap::<usize, Option<Vec<[f32; 3]>>>::new();
    model
        .primitives
        .iter()
        .map(|primitive| {
            let colors = match streams.entry(primitive.section) {
                Entry::Occupied(entry) => entry.into_mut(),
                Entry::Vacant(entry) => entry.insert(section_colors(
                    cache,
                    pages,
                    &geometry,
                    render,
                    primitive.section,
                )?),
            };
            Ok(match colors {
                Some(colors) => primitive
                    .vertices
                    .iter()
                    .map(|vertex| {
                        colors
                            .get(vertex.source as usize)
                            .copied()
                            .filter(|color| color.iter().all(|v| v.is_finite()))
                            .with_context(|| {
                                format!("Vertex color outside the stream of {}", render.path)
                            })
                    })
                    .collect::<Result<Vec<_>>>()?,
                None => vec![[1.; 3]; primitive.vertices.len()],
            })
        })
        .collect()
}

/// The vertex colors section `section` of `geometry`, the geometry of render model `render`,
/// keeps: the first of its vertex buffers that holds a stream of three floats per vertex.
fn section_colors(
    cache: &Cache,
    pages: &mut Pages,
    geometry: &model::Geometry,
    render: &Tag,
    section: usize,
) -> Result<Option<Vec<[f32; 3]>>> {
    for slot in 0..VERTEX_SLOTS {
        let Some(buffer) = geometry.section_buffer(cache, section, slot)? else {
            continue;
        };
        let (count, declaration, data) = geometry.vertex_buffer(cache, pages, buffer)?;
        if declaration != COLOR_STREAM {
            continue;
        }
        ensure!(
            data.len() == count * 12,
            "Vertex color stream of {} section {section} is truncated",
            render.path
        );
        return Ok(Some(
            data.chunks_exact(12)
                .map(|bytes| {
                    std::array::from_fn(|k| {
                        f32::from_le_bytes(bytes[k * 4..k * 4 + 4].try_into().unwrap_or([0; 4]))
                    })
                })
                .collect::<Vec<[f32; 3]>>(),
        ));
    }
    Ok(None)
}

impl Objects {
    /// Adds `model`'s primitives moved by `transform`, naming its materials from `base`, and
    /// its solid triangles as collision when `collides`. A primitive whose material `hidden`
    /// marks as never drawn is left out of both.
    fn place(
        &mut self,
        path: &str,
        model: &model::Model,
        base: usize,
        transform: &[f32; 16],
        collides: bool,
        hidden: &[bool],
    ) -> Result<()> {
        let count = self.primitives.len();
        for source in &model.primitives {
            if source
                .material
                .is_some_and(|material| hidden.get(material).copied().unwrap_or(false))
            {
                *self.hidden.entry(path.to_owned()).or_default() += 1;
                continue;
            }
            let mut primitive = source.clone();
            primitive.material = source.material.map(|material| base + material);
            primitive.instance = Some(format!("{path}#{count}"));
            for vertex in &mut primitive.vertices {
                vertex.position = rig::point(transform, vertex.position, true);
                if let Ok(normal) = rig::normalize(rig::point(transform, vertex.normal, false)) {
                    vertex.normal = normal;
                }
                let tangent = [vertex.tangent[0], vertex.tangent[1], vertex.tangent[2]];
                if let Ok(tangent) = rig::normalize(rig::point(transform, tangent, false)) {
                    vertex.tangent[..3].copy_from_slice(&tangent);
                }
            }
            ensure!(
                primitive.indices.len() % 3 == 0,
                "Object {path} draws a primitive that is not a triangle list"
            );
            if collides
                && solid(
                    source
                        .material
                        .and_then(|material| model.materials.get(material)),
                )
            {
                for face in primitive.indices.chunks_exact(3) {
                    let corner = |i: u32| primitive.vertices[i as usize].position;
                    self.faces
                        .push([corner(face[0]), corner(face[1]), corner(face[2])]);
                }
            }
            self.primitives.push(primitive);
        }
        if self.primitives.len() > count {
            *self.placed.entry(path.to_owned()).or_default() += 1;
        }
        Ok(())
    }
}

/// An object's model, the index of its first material among the level's objects' materials, and
/// which of its materials never draw.
type Loaded = (model::Model, usize, Vec<bool>);

/// The model `tag` draws with model variant `name`, read once per object and variant from
/// `models`, whose materials join `objects` the first time.
fn model_for<'a>(
    cache: &Cache,
    pages: &mut Pages,
    objects: &mut Objects,
    models: &'a mut BTreeMap<(String, String), Option<Loaded>>,
    tag: &Tag,
    name: String,
) -> Result<Option<&'a Loaded>> {
    let key = (tag.path.clone(), name);
    if !models.contains_key(&key) {
        let model = match model_of(cache, pages, tag, &key.1) {
            Ok(Some(model)) => {
                let hidden = model
                    .materials
                    .iter()
                    .map(|material| material.never_drawn(cache, pages))
                    .collect::<Result<Vec<_>>>()?;
                let base = objects.materials.len();
                objects.materials.extend(model.materials.iter().cloned());
                Some((model, base, hidden))
            }
            Ok(None) => None,
            Err(error) if error.downcast_ref::<Absent>().is_some() => {
                objects.absent.insert(tag.path.clone());
                None
            }
            Err(error) => return Err(error),
        };
        models.insert(key.clone(), model);
    }
    Ok(models[&key].as_ref())
}

/// Whether the machine placed by `row` never moves: it starts switched off, and no power or position
/// group can switch it on.
fn fixed_machine(cache: &Cache, row: usize) -> Result<bool> {
    Ok(cache.u32(row + DEVICE_FLAGS)? & INITIALLY_OFF != 0
        && cache.i16(row + DEVICE_POWER_GROUP)? < 0
        && cache.i16(row + DEVICE_POSITION_GROUP)? < 0)
}

/// A placement's origin structure BSP, -1 for none, and the mask of the BSPs it may attach to.
const PLACEMENT_ORIGIN_BSP: usize = 0x40;
const PLACEMENT_ATTACH_BSPS: usize = 0x54;

/// Whether the placement at `row` belongs to a structure BSP in `loaded`: its origin BSP when it
/// names one, else any BSP its attach mask allows, every BSP when the mask is empty. A campaign
/// scenario places each stretch's objects in that stretch's BSPs.
fn in_loaded_bsp(cache: &Cache, row: usize, loaded: u32) -> Result<bool> {
    if let Ok(origin) = u32::try_from(cache.i16(row + PLACEMENT_ORIGIN_BSP)?) {
        return Ok(origin < 32 && (loaded >> origin) & 1 != 0);
    }
    let attach = u32::from(cache.u16(row + PLACEMENT_ATTACH_BSPS)?);
    Ok(attach == 0 || attach & loaded != 0)
}

/// Whether the scenario creates the placement at `row` in the structure BSPs `loaded`: it is not
/// marked never placed, and it belongs to one of them ([`in_loaded_bsp`]). Sound scenery shares the
/// placement layout of the other objects.
pub(super) fn placed_in(cache: &Cache, row: usize, loaded: u32) -> Result<bool> {
    Ok(cache.u32(row + PLACEMENT_FLAGS)? & NEVER_PLACED == 0 && in_loaded_bsp(cache, row, loaded)?)
}

/// Reads the objects the scenario places in the structure BSPs its zone sets `zone_sets` load, and
/// with a map `variant` the variant's Forge objects in place of the scenario's own placements of
/// every object the variant's palette offers.
pub fn read(
    cache: &Cache,
    pages: &mut Pages,
    variant: Option<&Variant>,
    zone_sets: &[usize],
) -> Result<Objects> {
    let scenario = cache.only("scnr")?.address()?;
    let loaded = super::loaded_bsps(cache, zone_sets)?;
    let mut objects = Objects {
        materials: Vec::new(),
        primitives: Vec::new(),
        faces: Vec::new(),
        placed: BTreeMap::new(),
        hidden: BTreeMap::new(),
        motion: BTreeMap::new(),
        sky: Vec::new(),
        absent: BTreeSet::new(),
    };
    let mut models = BTreeMap::new();
    for block in [&CRATES, &SCENERY, &CONTROLS, &MACHINES] {
        let palette = cache
            .block(scenario + block.palette, PALETTE_SIZE)?
            .into_iter()
            .map(|row| cache.reference(row, Some(block.group)))
            .collect::<Result<Vec<_>>>()?;
        for row in cache.block(scenario + block.rows, block.size)? {
            let index = cache.i16(row)?;
            if index < 0 || cache.u32(row + PLACEMENT_FLAGS)? & NEVER_PLACED != 0 {
                continue;
            }
            if !in_loaded_bsp(cache, row, loaded)? {
                continue;
            }
            let Some(tag) = palette.get(usize::try_from(index)?).with_context(|| {
                format!("A {} palette index lies outside its palette", block.group)
            })?
            else {
                continue;
            };
            if block.group == MACHINES.group && !fixed_machine(cache, row)? {
                *objects.motion.entry(tag.path.clone()).or_default() += 1;
                continue;
            }
            if tag.path.starts_with(MARKERS) || variant.is_some_and(|variant| variant.replaces(tag))
            {
                continue;
            }
            let name = cache.string_id(cache.u32(row + PLACEMENT_VARIANT)?)?;
            let Some((model, base, hidden)) =
                model_for(cache, pages, &mut objects, &mut models, tag, name)?
            else {
                continue;
            };
            let scale = match cache.f32(row + PLACEMENT_SCALE)? {
                scale if scale > 0. => scale,
                _ => 1.,
            };
            let position = cache
                .floats::<3>(row + PLACEMENT_POSITION)?
                .map(|value| value * METRES_PER_UNIT);
            let transform = placement(position, cache.floats(row + PLACEMENT_ROTATION)?, scale);
            objects.place(&tag.path, model, *base, &transform, true, hidden)?;
        }
    }
    // A map variant's crates, scenery and controls, at its own positions and axes. Its machines
    // stay out, since a variant keeps no device state that would show they never move.
    for placement in variant.map_or(&[][..], |variant| variant.placements.as_slice()) {
        let tag = &placement.tag;
        if !VARIANT_GROUPS.contains(&tag.group.as_str()) || tag.path.starts_with(MARKERS) {
            continue;
        }
        let Some((model, base, hidden)) = model_for(
            cache,
            pages,
            &mut objects,
            &mut models,
            tag,
            placement.variant.clone(),
        )?
        else {
            continue;
        };
        let transform = transform(placement.position, placement.axes, 1.);
        objects.place(&tag.path, model, *base, &transform, true, hidden)?;
    }
    // Each sky assigned to a loaded structure BSP draws around the level from its origin.
    // An empty mask assigns no BSP. Such rows can be alternate or cinematic sky entries,
    // including another reference to the same sky, so treating zero as all duplicates layers.
    for row in cache.block(scenario + SKIES, SKY_SIZE)? {
        let active = u32::from(cache.u16(row + SKY_BSPS)?);
        if active & loaded == 0 {
            continue;
        }
        let Some(tag) = cache.reference(row, Some("scen"))? else {
            continue;
        };
        let model = match model_of(cache, pages, &tag, "") {
            Ok(model) => model,
            Err(error) if error.downcast_ref::<Absent>().is_some() => {
                objects.absent.insert(tag.path.clone());
                None
            }
            Err(error) => return Err(error),
        };
        if let Some(model) = model {
            let render = Object::read_appearance(cache, &tag)?
                .world
                .and_then(|branch| branch.model)
                .context("Sky has no render model")?;
            let colors = vertex_colors(cache, pages, &render, &model)?;
            let base = objects.materials.len();
            objects.materials.extend(model.materials.iter().cloned());
            let start = objects.primitives.len();
            // Every sky primitive stays, so each keeps its vertex colors.
            objects.place(&tag.path, &model, base, &rig::IDENTITY, false, &[])?;
            let sky = objects.primitives.split_off(start);
            objects.sky.extend(sky.into_iter().zip(colors));
        }
    }
    Ok(objects)
}

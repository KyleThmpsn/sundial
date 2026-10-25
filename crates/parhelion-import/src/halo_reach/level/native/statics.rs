//! The level's pieces as native static meshes, placed as a stock bubble places its static geometry:
//! one static instance group per area whose instance set lists each piece's static mesh with its
//! transform and world bounds. A placement row places one world object however many statics its
//! group draws, where a piece placed as its own prop entity took one world object each, and a
//! connected area's switch then created hundreds at once.
//!
//! A static mesh (`808071A7`) names its mesh data (`80807194`), one technique per draw group,
//! and transparent draws as special meshes. Each surface draws in the depth prepass and shadow
//! stages with the donor's own static depth and shadow techniques, as stock statics do, then in
//! its own stage with the level's surface. The instance set (`8080966D`) holds the transforms,
//! the statics, one instance group per static and a bounds record (`80809671`) with each
//! instance's world box, and the group's preheader (`80806EF4`) names the set.
use super::{Graph, Payload, Reader, put};
use crate::presentation::{append, geometry::StaticStreams};
use anyhow::{Context, Result, ensure};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

const PREHEADER_CLASS: u32 = 0x8080_6EF4;
const INSTANCES_CLASS: u32 = 0x8080_966D;
const MESH_CLASS: u32 = 0x8080_71A7;
const DATA_CLASS: u32 = 0x8080_7194;
const TECHNIQUE_ROW_CLASS: u64 = 0x8080_0014;
const SPECIAL_CLASS: u64 = 0x8080_7193;
const GROUP_CLASS: u64 = 0x8080_719B;
const PART_CLASS: u64 = 0x8080_719A;
const BUFFERS_CLASS: u64 = 0x8080_7199;
const TRANSFORM_CLASS: u64 = 0x8080_71A3;
const STATIC_ROW_CLASS: u64 = 0x8080_967D;
const INSTANCE_GROUP_CLASS: u64 = 0x8080_7190;
const BOUNDS_ROW_CLASS: u64 = 0x8080_9673;

/// The render stages stock static draw groups name: the deferred surface, shadow generation,
/// transparents and the depth prepass.
const SURFACE_STAGE: u8 = 0;
const SHADOW_STAGE: u8 = 3;
const TRANSPARENT_STAGE: u8 = 7;
const DEPTH_STAGE: u8 = 12;
/// The position-only vertex layout of stock static depth and shadow groups, which reads the
/// position stream alone.
const POSITION_LAYOUT: u8 = 120;
/// The surfaces' vertex layout, the converted entity pieces' rigid layout: the same eight-byte
/// position stream and a 24-byte attribute stream with detail coordinates at byte 20.
const SURFACE_LAYOUT: u8 = 139;
/// Detail category 2, which a stock static with one part set draws alone (`814032DA`) and
/// which draws an entity part at every distance.
const EVERY_DISTANCE: u8 = 2;
const TRIANGLE_LIST: u8 = 3;
/// The trailing word of every stock static draw group, and of every stock instance group.
const GROUP_WORD: u16 = 0x0102;
const INSTANCE_GROUP_WORD: u16 = 7;
/// The preheader word of a group drawn through the bubble's precomputed visibility.
const VISIBILITY_WORD: u32 = 1;
/// A special mesh's byte `+5`, 127 in every stock transparent row.
const SPECIAL_BYTE: u8 = 0x7F;

/// The header lengths before each record's first array, as stock records lay them out.
const DATA_HEADER: usize = 0x48;
const MESH_HEADER: usize = 0x90;
const INSTANCES_HEADER: usize = 0xA0;
const BOUNDS_HEADER: usize = 0x10;

/// Stock records the map's statics copy, from the donor's own static instance group: its
/// preheader, instance set and bounds record, its first static mesh and that mesh's data, and
/// the depth prepass and shadow techniques that mesh's position-only groups draw with.
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct Templates {
    pub preheader: u32,
    pub instances: u32,
    pub bounds: u32,
    pub mesh: u32,
    pub data: u32,
    pub depth: u32,
    pub shadow: u32,
}

/// One drawn group of a piece: its surface key, the stage its surface draws in, whether it also
/// draws in the depth prepass and shadow stages, which hold every pixel of its triangles, and
/// whether it is alpha tested, so it draws the depth prepass with its own surface.
pub(super) struct Draw {
    pub key: u32,
    pub stage: u8,
    pub solid: bool,
    pub clipped: bool,
}

/// One instance of the map's statics: its static mesh's symbol, where its geometry's origin sits
/// and its world bounds.
pub(super) struct Instance<'a> {
    pub mesh: &'a str,
    pub position: [f32; 3],
    pub minimum: [f32; 3],
    pub maximum: [f32; 3],
}

/// The bounds-row identifiers of group `name`'s `count` instances. The bubble's visibility record
/// keys each instance by its identifier, so the map writes the same ones there.
pub(super) fn identifiers(name: &str, count: usize) -> Vec<u32> {
    (0..count)
        .map(|index| fnv1a32(&format!("parhelion/statics/{name}/{index}")) | 1)
        .collect()
}

fn fnv1a32(text: &str) -> u32 {
    text.bytes().fold(0x811C_9DC5, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    })
}

fn floats(values: &[f32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

/// Adds the buffer header and data pairs of `streams`, with headers copied from the native model
/// `template`'s mesh at `row`, and returns the index, position and attribute header symbols.
fn buffers(
    native: &mut Reader,
    graph: &mut Graph,
    template: &Payload,
    row: usize,
    streams: &StaticStreams,
    name: &str,
) -> Result<[String; 3]> {
    let mut symbols = Vec::with_capacity(3);
    for (kind, at, bytes) in [
        ("indices", 16, &streams.indices),
        ("positions", 0, &streams.positions),
        ("attributes", 4, &streams.attributes),
    ] {
        // A template mesh may leave a vertex stream empty, as Midtown's do. Both vertex streams'
        // headers are vertex buffer headers whose size and stride are written below, so an empty
        // one takes the other's.
        let mut tag = template.u32(row + at)?;
        if tag == u32::MAX && kind != "indices" {
            tag = template.u32(row + if at == 0 { 4 } else { 0 })?;
        }
        let mut header = native
            .tag(tag, None)
            .with_context(|| format!("Static buffer {tag:08X}"))?
            .0
            .clone();
        if kind == "indices" {
            put(&mut header, 8, &u32::try_from(bytes.len())?.to_le_bytes())?;
            // Byte 1 selects 32-bit indices, and a static's are 16 bits.
            header[1] = 0;
        } else {
            put(&mut header, 0, &u32::try_from(bytes.len())?.to_le_bytes())?;
            let stride: u16 = if kind == "positions" { 8 } else { 24 };
            put(&mut header, 4, &stride.to_le_bytes())?;
        }
        let symbol = format!("{name}-{kind}-header");
        let data = format!("{name}-{kind}-data");
        graph.add(&symbol, tag, &header, Some(&data), vec![])?;
        graph.add(&data, native.reference(tag)?, bytes, Some(&symbol), vec![])?;
        symbols.push(symbol);
    }
    Ok(symbols.try_into().unwrap())
}

/// Adds piece `name`'s static mesh, whose groups `draws` follow `streams.ranges`, and returns its
/// symbol. A transparent group draws as a special mesh, and every other in its surface's stage
/// and, when solid, the depth prepass and shadow stages.
#[allow(clippy::too_many_arguments)]
pub(super) fn mesh(
    native: &mut Reader,
    graph: &mut Graph,
    templates: &Templates,
    model: &Payload,
    row: usize,
    streams: &StaticStreams,
    draws: &[Draw],
    name: &str,
) -> Result<String> {
    ensure!(
        draws.len() == streams.ranges.len() && !draws.is_empty(),
        "Static mesh {name} has {} draws for {} groups",
        draws.len(),
        streams.ranges.len()
    );
    let [indices, positions, attributes] = buffers(native, graph, model, row, streams, name)?;
    let mut parts = Vec::new();
    let mut groups = Vec::new();
    // Each group's technique: a stock technique, or a surface by symbol.
    let mut techniques = Vec::<Result<u32, String>>::new();
    let mut specials = Vec::new();
    let mut special_techniques = Vec::new();
    // Stock statics list their depth groups, then their shadow groups, then their surfaces.
    let mut passes = [Vec::new(), Vec::new(), Vec::new()];
    for (draw, &(first, count)) in draws.iter().zip(&streams.ranges) {
        let surface = format!("material-{:08X}", draw.key);
        if draw.stage == TRANSPARENT_STAGE {
            let mut special = vec![0u8; 32];
            special[0] = TRANSPARENT_STAGE;
            put(&mut special, 2, &u16::from(SURFACE_LAYOUT).to_le_bytes())?;
            special[4] = EVERY_DISTANCE;
            special[5] = SPECIAL_BYTE;
            special[6] = TRIANGLE_LIST;
            for at in [8, 12, 16, 28] {
                put(&mut special, at, &u32::MAX.to_le_bytes())?;
            }
            put(&mut special, 0x14, &first.to_le_bytes())?;
            put(&mut special, 0x18, &count.to_le_bytes())?;
            specials.push(special);
            special_techniques.push(surface);
            continue;
        }
        ensure!(
            draw.stage == SURFACE_STAGE,
            "Static mesh {name} draws stage {}",
            draw.stage
        );
        let part = u16::try_from(parts.len())?;
        let mut part_row = vec![0u8; 12];
        put(&mut part_row, 0, &first.to_le_bytes())?;
        put(&mut part_row, 4, &count.to_le_bytes())?;
        part_row[10] = EVERY_DISTANCE;
        part_row[11] = TRIANGLE_LIST;
        parts.push(part_row);
        if draw.solid {
            passes[0].push((part, DEPTH_STAGE, POSITION_LAYOUT, Ok(templates.depth)));
            passes[1].push((part, SHADOW_STAGE, POSITION_LAYOUT, Ok(templates.shadow)));
        } else if draw.clipped {
            // Stock statics draw every part in the depth prepass, an alpha-tested one with a
            // pixel program of its own. Left out of it, Paradiso's pine branches drew from afar
            // and vanished as the camera neared them, while their solid trunks stayed. The
            // surface's own technique clips the same texels in the prepass as in its stage.
            passes[0].push((part, DEPTH_STAGE, SURFACE_LAYOUT, Err(surface.clone())));
        }
        passes[2].push((part, SURFACE_STAGE, SURFACE_LAYOUT, Err(surface)));
    }
    for (part, stage, layout, technique) in passes.into_iter().flatten() {
        let mut group_row = vec![0u8; 8];
        put(&mut group_row, 0, &part.to_le_bytes())?;
        group_row[2] = stage;
        group_row[4] = layout;
        put(&mut group_row, 6, &GROUP_WORD.to_le_bytes())?;
        groups.push(group_row);
        techniques.push(technique);
    }

    let data_template = native
        .tag(templates.data, Some(DATA_CLASS))
        .context("Static mesh data template")?;
    let mut data = vec![0u8; DATA_HEADER];
    data[0x38..DATA_HEADER].copy_from_slice(
        data_template
            .0
            .get(0x38..DATA_HEADER)
            .context("The donor's static mesh data is too short")?,
    );
    append(&mut data, 0x08, GROUP_CLASS, &groups.concat(), 8)?;
    append(&mut data, 0x18, PART_CLASS, &parts.concat(), 12)?;
    let mut buffer_row = vec![0u8; 16];
    for at in [0, 4, 8] {
        put(&mut buffer_row, at, &u32::MAX.to_le_bytes())?;
    }
    append(&mut data, 0x28, BUFFERS_CLASS, &buffer_row, 16)?;
    let buffer_at = Payload(data.clone()).array(0x28, 16, None)?[0];
    let data_symbol = format!("{name}-data");
    graph.add(
        &data_symbol,
        templates.data,
        &data,
        None,
        [&indices, &positions, &attributes]
            .iter()
            .enumerate()
            .map(|(k, symbol)| json!({"offset": buffer_at + k * 4, "symbol": symbol}))
            .collect(),
    )?;

    let mesh_template = native
        .tag(templates.mesh, Some(MESH_CLASS))
        .context("Static mesh template")?;
    let mut bytes = vec![0u8; MESH_HEADER];
    bytes[0x30..MESH_HEADER].copy_from_slice(
        mesh_template
            .0
            .get(0x30..MESH_HEADER)
            .context("The donor's static mesh is too short")?,
    );
    put(&mut bytes, 0x08, &u32::MAX.to_le_bytes())?;
    put(&mut bytes, 0x0C, &0u32.to_le_bytes())?;
    // Stock statics hold their bounds' middle at +0x50 and a bounding distance at +0x38 to
    // +0x40 and +0x5C, which here is the distance to the box's corner.
    let (low, high) = streams.bounds;
    let middle: [f32; 3] = std::array::from_fn(|k| (low[k] + high[k]) / 2.);
    let reach = (0..3)
        .map(|k| ((high[k] - low[k]) / 2.).powi(2))
        .sum::<f32>()
        .sqrt();
    put(
        &mut bytes,
        0x38,
        &floats(&[reach, reach, reach, 0., 0., 0.]),
    )?;
    put(
        &mut bytes,
        0x50,
        &floats(&[middle[0], middle[1], middle[2], reach]),
    )?;
    put(
        &mut bytes,
        0x60,
        &floats(&[
            streams.center[0],
            streams.center[1],
            streams.center[2],
            streams.scale,
        ]),
    )?;
    put(
        &mut bytes,
        0x70,
        &floats(&[
            streams.uv_scale,
            streams.uv_scale,
            streams.uv_center[0],
            streams.uv_center[1],
        ]),
    )?;
    let technique_rows = techniques
        .iter()
        .flat_map(|technique| match technique {
            Ok(tag) => tag.to_le_bytes(),
            Err(_) => u32::MAX.to_le_bytes(),
        })
        .collect::<Vec<_>>();
    append(&mut bytes, 0x10, TECHNIQUE_ROW_CLASS, &technique_rows, 4)?;
    append(&mut bytes, 0x20, SPECIAL_CLASS, &specials.concat(), 32)?;
    let mut patches = vec![json!({"offset": 0x08, "symbol": data_symbol})];
    if !techniques.is_empty() {
        let rows = Payload(bytes.clone()).array(0x10, 4, None)?;
        for (at, technique) in rows.into_iter().zip(&techniques) {
            if let Err(symbol) = technique {
                patches.push(json!({"offset": at, "symbol": symbol}));
            }
        }
    }
    if !specials.is_empty() {
        let rows = Payload(bytes.clone()).array(0x20, 32, None)?;
        for (at, technique) in rows.into_iter().zip(&special_techniques) {
            for (offset, symbol) in [
                (8, &indices),
                (12, &positions),
                (16, &attributes),
                (28, technique),
            ] {
                patches.push(json!({"offset": at + offset, "symbol": symbol}));
            }
        }
    }
    graph.add(name, templates.mesh, &bytes, None, patches)?;
    Ok(name.to_owned())
}

/// Adds the static instance group `name` placing `instances`, each its own static mesh at its
/// own transform, and returns its preheader's symbol, which a placement row's resource names.
pub(super) fn group(
    native: &mut Reader,
    graph: &mut Graph,
    templates: &Templates,
    instances: &[Instance<'_>],
    name: &str,
) -> Result<String> {
    ensure!(
        !instances.is_empty() && instances.len() <= usize::from(u16::MAX),
        "Static instance group {name} places {} statics",
        instances.len()
    );
    let ids = identifiers(name, instances.len());
    let mut bounds = vec![0u8; BOUNDS_HEADER];
    let mut rows = Vec::with_capacity(instances.len() * 48);
    for (instance, id) in instances.iter().zip(&ids) {
        let [a, b, c] = instance.minimum;
        let [d, e, f] = instance.maximum;
        rows.extend(floats(&[a, b, c, 1., d, e, f, 1.]));
        rows.extend(id.to_le_bytes());
        rows.extend([0; 12]);
    }
    append(&mut bounds, 0x08, BOUNDS_ROW_CLASS, &rows, 48)?;
    let bounds_symbol = format!("{name}-bounds");
    graph.add(&bounds_symbol, templates.bounds, &bounds, None, vec![])?;

    let template = native
        .tag(templates.instances, Some(INSTANCES_CLASS))
        .context("Static instance set template")?;
    let mut bytes = vec![0u8; INSTANCES_HEADER];
    for range in [0x30..0x38, 0x50..0x58] {
        bytes[range.clone()].copy_from_slice(
            template
                .0
                .get(range)
                .context("The donor's static instance set is too short")?,
        );
    }
    put(&mut bytes, 0x18, &u32::MAX.to_le_bytes())?;
    put(&mut bytes, 0x38, &(instances.len() as u64).to_le_bytes())?;
    put(&mut bytes, 0x78, &ids[0].to_le_bytes())?;
    let low: [f32; 3] = std::array::from_fn(|k| {
        instances
            .iter()
            .map(|i| i.minimum[k])
            .fold(f32::INFINITY, f32::min)
    });
    let high: [f32; 3] = std::array::from_fn(|k| {
        instances
            .iter()
            .map(|i| i.maximum[k])
            .fold(f32::NEG_INFINITY, f32::max)
    });
    put(
        &mut bytes,
        0x80,
        &floats(&[low[0], low[1], low[2], 1., high[0], high[1], high[2], 1.]),
    )?;
    let mut transforms = Vec::with_capacity(instances.len() * 48);
    for instance in instances {
        let [x, y, z] = instance.position;
        transforms.extend(floats(&[0., 0., 0., 1., x, y, z, 1., 1., 1.]));
        transforms.extend([0; 8]);
    }
    append(&mut bytes, 0x40, TRANSFORM_CLASS, &transforms, 48)?;
    append(
        &mut bytes,
        0x58,
        STATIC_ROW_CLASS,
        &vec![0xFF; instances.len() * 4],
        4,
    )?;
    let mut groups = Vec::with_capacity(instances.len() * 8);
    for index in 0..instances.len() {
        let index = u16::try_from(index)?;
        for word in [1, index, index, INSTANCE_GROUP_WORD] {
            groups.extend(word.to_le_bytes());
        }
    }
    append(&mut bytes, 0x68, INSTANCE_GROUP_CLASS, &groups, 8)?;
    let mut patches = vec![json!({"offset": 0x18, "symbol": bounds_symbol})];
    for (at, instance) in Payload(bytes.clone())
        .array(0x58, 4, None)?
        .into_iter()
        .zip(instances)
    {
        patches.push(json!({"offset": at, "symbol": instance.mesh}));
    }
    let instances_symbol = format!("{name}-instances");
    graph.add(
        &instances_symbol,
        templates.instances,
        &bytes,
        None,
        patches,
    )?;

    let preheader = native
        .tag(templates.preheader, Some(PREHEADER_CLASS))
        .context("Static preheader template")?;
    let mut bytes = preheader.0.clone();
    put(&mut bytes, 0x08, &u32::MAX.to_le_bytes())?;
    // Word 1 at +0x20 draws the group through the bubble's precomputed visibility: the client
    // looks each instance identifier up in the area's visibility record and draws the instance
    // when the Umbra object its index names is visible. Each area's tome holds one object per
    // instance. Across the Shadowkeep packages no group with word 0 or 0x101 has an instance in
    // such a table. Every converted map builds that tome, a single-area map's as well as each
    // connected area's: drawn with word 0 instead, a converted multiplayer level's statics drew
    // stretched triangles near the camera and took none of Destiny's dynamic lighting.
    put(&mut bytes, 0x20, &VISIBILITY_WORD.to_le_bytes())?;
    graph.add(
        name,
        templates.preheader,
        &bytes,
        None,
        vec![json!({"offset": 0x08, "symbol": instances_symbol})],
    )?;
    Ok(name.to_owned())
}

/// The solid surface keys: opaque surfaces that are not decals, whose every triangle pixel draws,
/// so the depth prepass and shadow stages may draw them whole. An alpha-tested surface casts no
/// shadow, since the shared shadow technique draws every triangle solid: Forge World's 113,424
/// ground cover cards then shadowed the grass they stand in, so sunlit slopes and shores drew in
/// Destiny's shadow beside Reach's lighter baked tree shadows. Reach's lightmaps already hold its
/// foliage's shadows on the structure.
pub(super) fn solid(materials: &BTreeMap<u32, super::Material>) -> BTreeSet<u32> {
    materials
        .iter()
        .filter(|(_, material)| material.alpha_mode() == "OPAQUE" && !material.decal())
        .map(|(&key, _)| key)
        .collect()
}

/// The alpha-tested surface keys that are not decals, which draw the depth prepass with their own
/// surface so it holds only the texels they keep.
pub(super) fn clipped(materials: &BTreeMap<u32, super::Material>) -> BTreeSet<u32> {
    materials
        .iter()
        .filter(|(_, material)| material.alpha_mode() == "MASK" && !material.decal())
        .map(|(&key, _)| key)
        .collect()
}

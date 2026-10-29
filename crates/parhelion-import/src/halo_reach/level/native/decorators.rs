//! A level's decorators as Destiny's own instanced decorators, which stock maps place for their
//! grass, bushes and trees (placement kind `808071AD`). Each decorator record (`80807164`) names
//! one model row (`8080717E`) whose entity model holds every type the record draws, one part group
//! per type, at three levels of detail, and draws each type once per instance of its range
//! ([`Templates`] names the stock records each part copies):
//!
//! - an instance header (`80807170`) naming six constant rows (`8080716B`), the instance vertex
//!   buffer and a CPU copy of its rows (`80807173`)
//! - 16-byte instance rows: position and scale as four 16-bit signed normalized values under
//!   constant rows 0 and 1, a rotation quaternion (x, y, z, w) as four unsigned normalized bytes
//!   under rows 2 and 3, and four more bytes under rows 4 and 5, whose first a stock grass vertex
//!   program reads as the instance's wind bend and whose others it adds to the albedo
//! - a bounds record (`80809671`) of the record's clusters, each a run of one type's instances
//!   with its box and an identifier at row `+0x24`, which the bubble's visibility record keys in
//!   the upper half of its 64-bit key, as Distant Shore's keys 1,462 of its 1,466 decorator
//!   clusters
//! - a level of detail record (`80807180`) with one row per type in each of its five arrays
//!
//! The model draws through stock grass's vertex layout 71: one 32-byte stream of 16-bit signed
//! normalized position, an anchor offset, texture coordinates, normal, a second anchor offset,
//! tangent and four bytes of wind weights, then the instance stream. Its surfaces copy a stock grass
//! technique pair, the deferred surface and the alpha-tested depth prepass, binding the set's
//! texture in place of the stock albedo and opening the alpha test at half, as Reach's decorator
//! program clips. They draw no shadow, as converted alpha-tested statics do not, and as stock
//! grass leaves some of its types out of the shadow stage. Destiny lights them as dynamic
//! surfaces, where Reach multiplied each instance by its baked light.
use super::{Graph, Payload, Reader, Visible, put};
use crate::{
    halo_reach::{cache::Cache, material::Mapping, native, resource::Pages},
    presentation::append,
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

use super::super::decorators::{Instanced, Kind};

const RECORD_CLASS: u32 = 0x8080_7164;
const HEADER_CLASS: u32 = 0x8080_7170;
const CONSTANTS_CLASS: u32 = 0x8080_716B;
const INSTANCES_CLASS: u32 = 0x8080_7173;
const BOUNDS_CLASS: u32 = 0x8080_9671;
const ROW_CLASS: u32 = 0x8080_717E;
const LOD_CLASS: u32 = 0x8080_7180;
const MODEL_CLASS: u32 = 0x8080_73A5;
const TECHNIQUE_CLASS: u32 = 0x8080_71E8;
const TABLE_CLASS: u32 = 0x8080_99D6;
const ENTITY_CLASS: u32 = 0x8080_9C0F;
/// Array row classes: the record's model rows, `u32` lists, instance rows, bounds rows, a model's
/// meshes and parts, and the level of detail record's bytes, `float4` rows, floats, signed bytes
/// and band rows.
const MODEL_ROW_CLASS: u64 = 0x8080_717D;
const INDEX_CLASS: u64 = 0x8080_0007;
const INSTANCE_ROW_CLASS: u64 = 0x8080_7175;
const BOUNDS_ROW_CLASS: u64 = 0x8080_9673;
const MESH_CLASS: u64 = 0x8080_7378;
const PART_CLASS: u64 = 0x8080_737E;
const DETAIL_CLASS: u64 = 0x8080_0005;
const FLOAT4_CLASS: u64 = 0x8080_0090;
const FLOAT_CLASS: u64 = 0x8080_000F;
const SIGNED_CLASS: u64 = 0x8080_0004;
const BANDS_CLASS: u64 = 0x8080_7188;
const TEXTURE_ROW_CLASS: u32 = 0x8080_7211;

/// The decorator record: model rows, each type's first instance and the count after the last,
/// the boundaries of runs of types that share a surface, each cluster's first instance and the
/// count after the last, the instance header, the bounds record and a list over the clusters
/// that counts up from 0 in both stock records. `+0x60` and `+0x90` are copied from the template,
/// and `+0x70` holds the box of every cluster, each corner with w 1.
const RECORD_HEADER: usize = 0xA0;
const RECORD_MODELS: usize = 0x08;
const RECORD_RANGES: usize = 0x18;
const RECORD_RUNS: usize = 0x28;
const RECORD_CLUSTERS: usize = 0x38;
const RECORD_INSTANCES: usize = 0x48;
const RECORD_BOUNDS: usize = 0x4C;
const RECORD_ORDER: usize = 0x50;
const RECORD_COPIED: [std::ops::Range<usize>; 2] = [0x60..0x70, 0x90..0xA0];
const RECORD_BOX: usize = 0x70;
/// The instance header: a 64-bit value the two stock headers hold differently, which each
/// record takes its own of, then the constants, the instance vertex buffer and the CPU rows.
const HEADER_IDENTITY: usize = 0x08;
const HEADER_CONSTANTS: usize = 0x14;
const HEADER_BUFFER: usize = 0x18;
const HEADER_DATA: usize = 0x1C;
const INSTANCES_HEADER: usize = 0x10;
const BOUNDS_HEADER: usize = 0x10;
/// The model row: its entity model, its level of detail record and an optional wind record,
/// which stock grass leaves out.
const ROW_MODEL: usize = 0x08;
const ROW_LOD: usize = 0x10;
const ROW_WIND: usize = 0x14;
/// The level of detail record's arrays, one row per type: the last mesh the type draws with
/// (`u8`, 2 with three meshes and 1 for the two stock grass types that lack a third), four
/// distances in meters, a float, a signed byte and four band bytes. The distances take the order
/// of SpeedTree 7.0's `SLodProfile`: where the transition from the most detailed mesh begins,
/// where the coarsest 3D mesh is sustained, and where a billboard fades in and the 3D geometry
/// is gone. Stock grass, which has no billboard, holds (5, 35, 130, 140), and a stock tree, whose
/// third mesh is a billboard, (80, 180, 290, 300).
const LOD_HEADER: usize = 0x58;
const LOD_DETAILS: usize = 0x08;
const LOD_BANDS: usize = 0x18;
const LOD_FLOATS: usize = 0x28;
const LOD_SIGNED: usize = 0x38;
const LOD_SELECTIONS: usize = 0x48;
/// An entity model's header, its mesh rows and their fields: the vertex and index buffer headers,
/// the parts, 24 render stage part boundaries and 23 stage vertex layouts.
const MODEL_HEADER: usize = 0xA0;
const MODEL_MESHES: usize = 0x10;
const MODEL_SCALE: usize = 0x50;
const MODEL_OFFSET: usize = 0x60;
const MODEL_TEXTURE: usize = 0x70;
const MESH_SIZE: usize = 0x88;
const MESH_VERTICES: usize = 0x00;
const MESH_INDICES: usize = 0x10;
const MESH_PARTS: usize = 0x18;
const MESH_RANGES: usize = 0x28;
const MESH_LAYOUTS: usize = 0x58;
const PART_SIZE: usize = 32;
/// The stages the decorator parts draw in, the deferred surface and the depth prepass, the
/// render stages a stage range spans, and stock grass's vertex layout, which every stage of its
/// model names.
const SURFACE_STAGE: usize = 0;
const DEPTH_STAGE: usize = 12;
const STAGES: usize = 23;
const LAYOUT: u16 = 71;
const TRIANGLE_LIST: u16 = 3;
const VERTEX_STRIDE: usize = 32;
const INSTANCE_STRIDE: usize = 16;
/// The meshes a decorator model holds, one for each distance band, as stock grass's does.
const DETAILS: usize = 3;
/// A technique's pixel stage textures (rows of a slot and a texture) and its constant rows,
/// which the stock grass pixel programs read as `cb0`, and its vertex stage's constant rows.
const PIXEL_TEXTURES: usize = 0x2D0;
const PIXEL_CONSTANTS: usize = 0x318;
const VERTEX_CONSTANTS: usize = 0x98;
/// The vertex constant rows the stock grass programs take their wind from. Even with zero wind
/// weights, row 5's z sways every vertex along the world's y axis by its height, which would
/// wobble pebbles and rocks, so the copies zero them.
const WIND_ROWS: std::ops::Range<usize> = 4..8;
/// Pixel constant rows: the alpha threshold in x, the albedo multiplier, and the normal and
/// detail normal decodes, whose z lanes add to the ambient occlusion.
const ALPHA_ROW: usize = 16;
const ALBEDO_ROW: usize = 18;
const NORMAL_ROW: usize = 21;
const DETAIL_ROW: usize = 23;
/// Reach's decorator pixel program discards texels whose alpha falls below half.
const ALPHA_TEST: f32 = 0.5;
/// The most clusters one record holds, and the side of the first cell its clusters try, in
/// meters. Each area's tome holds one Umbra object per cluster beside its statics, and its object
/// lists hold at most 65,535. Distant Shore's grass clusters span 2 to 10 m.
const MAX_CLUSTERS: usize = 4096;
const CELL: f32 = 8.;
/// The shortest fade end and fade the level of detail records take, in meters, where a set's own
/// are shorter or unreadable. Stock grass fades out over 10 m, the nearest by 70 m.
const MIN_FADE_END: f32 = 40.;
const MIN_FADE: f32 = 5.;
const DEFAULT_BANDS: [f32; 4] = [5., 20., 60., 70.];

/// Stock records the map's decorators copy, from Distant Shore (`pvp_greenhouse_2`, package
/// `0358`), whose second decorator draws 11,520 grass instances of 42 types through one model:
/// its placement table and decorator entity, its record, instance header, constants, instance
/// buffer header, instance rows and bounds, its model row and level of detail record, its grass
/// model, whose first mesh's buffer headers the models copy, and the technique pair of its first
/// type, whose programs read layout 71 and the instance stream.
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct Templates {
    pub table: u32,
    pub entity: u32,
    pub record: u32,
    pub header: u32,
    pub constants: u32,
    pub buffer: u32,
    pub instances: u32,
    pub bounds: u32,
    pub row: u32,
    pub lod: u32,
    pub model: u32,
    pub surface: u32,
    pub depth: u32,
}

impl Templates {
    pub const STOCK: Self = Self {
        table: 0x80EB_0311,
        entity: 0x80C7_0D0D,
        record: 0x80EB_0310,
        header: 0x80EB_02FE,
        constants: 0x80EB_02FA,
        buffer: 0x80EB_02FC,
        instances: 0x80EB_02FD,
        bounds: 0x80EB_02FF,
        row: 0x80EB_030F,
        lod: 0x80EB_030E,
        model: 0x80EB_030D,
        surface: 0x80FE_1883,
        depth: 0x80F7_BAF6,
    };

    /// The stock templates, when `class` finds each tag with its expected class. A buffer
    /// header's class is its data tag, so it need only exist.
    pub fn find(class: impl Fn(u32) -> Option<u32>) -> Result<Self, String> {
        let stock = Self::STOCK;
        for (tag, expected, what) in [
            (stock.table, Some(TABLE_CLASS), "placement table"),
            (stock.entity, Some(ENTITY_CLASS), "entity"),
            (stock.record, Some(RECORD_CLASS), "record"),
            (stock.header, Some(HEADER_CLASS), "instance header"),
            (stock.constants, Some(CONSTANTS_CLASS), "constants"),
            (stock.buffer, None, "instance buffer"),
            (stock.instances, Some(INSTANCES_CLASS), "instance rows"),
            (stock.bounds, Some(BOUNDS_CLASS), "bounds"),
            (stock.row, Some(ROW_CLASS), "model row"),
            (stock.lod, Some(LOD_CLASS), "level of detail record"),
            (stock.model, Some(MODEL_CLASS), "model"),
            (stock.surface, Some(TECHNIQUE_CLASS), "surface technique"),
            (stock.depth, Some(TECHNIQUE_CLASS), "depth technique"),
        ] {
            match class(tag) {
                Some(found) if expected.is_none_or(|expected| expected == found) => {}
                Some(found) => {
                    return Err(format!(
                        "The stock decorator {what} {tag:08X} is class {found:08X}"
                    ));
                }
                None => return Err(format!("The stock decorator {what} {tag:08X} is missing")),
            }
        }
        Ok(stock)
    }

    /// The stock tags a container placing these decorators loads, as Distant Shore's loads its
    /// own: the decorator entity its rows place and the techniques whose programs, textures and
    /// samplers the copies name.
    pub fn loads(&self) -> [u32; 3] {
        [self.entity, self.surface, self.depth]
    }
}

/// The decorator records [`add`] wrote: each area's records by symbol, with their clusters as
/// its tome's Umbra objects, and the report.
pub(super) struct Added {
    pub areas: Vec<(Vec<String>, Vec<Visible>)>,
    pub report: Value,
}

fn floats(values: &[f32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

fn fnv1a64(text: &str) -> u64 {
    text.bytes().fold(0xCBF2_9CE4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01B3)
    })
}

fn snorm16(value: f32) -> [u8; 2] {
    ((value.clamp(-1., 1.) * 32767.).round() as i16).to_le_bytes()
}

fn unorm8(value: f32) -> u8 {
    (value.clamp(0., 1.) * 255.).round() as u8
}

fn unit(v: [f32; 3]) -> Option<[f32; 3]> {
    let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    (length.is_finite() && length > 1e-6).then(|| v.map(|value| value / length))
}

/// A direction perpendicular to unit `n`.
fn perpendicular(n: [f32; 3]) -> [f32; 3] {
    let axis = if n[0].abs() < 0.9 {
        [1., 0., 0.]
    } else {
        [0., 1., 0.]
    };
    let along = axis[0] * n[0] + axis[1] * n[1] + axis[2] * n[2];
    unit(std::array::from_fn(|k| axis[k] - along * n[k])).unwrap_or([0., 0., 1.])
}

/// The four distances of type `kind`'s level of detail row from its set's fade distances: the
/// first mesh until Reach's first model is gone, the coarsest mesh from where Reach's second is
/// gone, and the fourth model's fade as the fade out. A fade that ends nearer than [`MIN_FADE_END`]
/// moves out to it, keeping its width of at least [`MIN_FADE`], and unreadable distances take
/// [`DEFAULT_BANDS`].
fn bands(kind: &Kind) -> [f32; 4] {
    let fades = kind.fades;
    if !fades
        .iter()
        .flatten()
        .all(|value| value.is_finite() && *value >= 0.)
        || fades[3][1] <= 0.
    {
        return DEFAULT_BANDS;
    }
    let near = fades[0][1].max(1.);
    let middle = fades[1][1].max(near);
    let width = (fades[3][1] - fades[3][0]).max(MIN_FADE);
    let end = fades[3][1].max(MIN_FADE_END).max(middle + width);
    let start = (end - width).max(middle);
    [near, middle, start, end]
}

/// Copies stock technique `template`, binding `texture` in its pixel stage's slot 0, opening
/// its alpha test at [`ALPHA_TEST`] and stilling its wind ([`WIND_ROWS`]). A `surface` copy
/// also takes the texture's color unscaled and a flat normal with no ambient occlusion from the
/// stock normal maps it still names.
fn technique(
    native: &mut Reader,
    graph: &mut Graph,
    template: u32,
    texture: &str,
    symbol: &str,
    surface: bool,
) -> Result<()> {
    let payload = native
        .tag(template, Some(TECHNIQUE_CLASS))
        .context("Decorator technique template")?;
    let mut bytes = payload.0.clone();
    let slot = payload
        .array(PIXEL_TEXTURES, 8, Some(TEXTURE_ROW_CLASS))?
        .into_iter()
        .find(|&row| payload.u32(row).ok() == Some(0))
        .context("The decorator technique template binds no texture in slot 0")?;
    put(&mut bytes, slot + 4, &u32::MAX.to_le_bytes())?;
    let rows = payload.array(PIXEL_CONSTANTS, 16, Some(FLOAT4_CLASS as u32))?;
    let needed = if surface { DETAIL_ROW } else { ALPHA_ROW };
    ensure!(
        rows.len() > needed,
        "The decorator technique template holds {} pixel constant rows",
        rows.len()
    );
    put(&mut bytes, rows[ALPHA_ROW], &ALPHA_TEST.to_le_bytes())?;
    let vertex_rows = payload.array(VERTEX_CONSTANTS, 16, Some(FLOAT4_CLASS as u32))?;
    ensure!(
        vertex_rows.len() >= WIND_ROWS.end,
        "The decorator technique template holds {} vertex constant rows",
        vertex_rows.len()
    );
    for row in WIND_ROWS {
        put(&mut bytes, vertex_rows[row], &[0; 16])?;
    }
    if surface {
        put(&mut bytes, rows[ALBEDO_ROW], &floats(&[1.; 4]))?;
        for row in [NORMAL_ROW, DETAIL_ROW] {
            put(&mut bytes, rows[row], &floats(&[0., 0., 1., 0.]))?;
        }
    }
    graph.add(
        symbol,
        template,
        &bytes,
        None,
        vec![json!({"offset": slot + 4, "symbol": texture})],
    )
}

/// One layout 71 vertex of `vertex`, under the model's position and texture coordinate scales:
/// no anchor offsets and no wind weights.
fn vertex(
    vertex: &crate::halo_reach::model::Vertex,
    center: [f32; 3],
    scale: f32,
    uv: ([f32; 2], [f32; 2]),
) -> [u8; VERTEX_STRIDE] {
    let mut out = [0u8; VERTEX_STRIDE];
    for k in 0..3 {
        out[k * 2..k * 2 + 2].copy_from_slice(&snorm16((vertex.position[k] - center[k]) / scale));
    }
    for k in 0..2 {
        out[8 + k * 2..10 + k * 2].copy_from_slice(&snorm16((vertex.uv[k] - uv.0[k]) / uv.1[k]));
    }
    let normal = unit(vertex.normal).unwrap_or([0., 0., 1.]);
    let tangent = unit([vertex.tangent[0], vertex.tangent[1], vertex.tangent[2]])
        .unwrap_or_else(|| perpendicular(normal));
    for k in 0..3 {
        out[12 + k * 2..14 + k * 2].copy_from_slice(&snorm16(normal[k]));
        out[20 + k * 2..22 + k * 2].copy_from_slice(&snorm16(tangent[k]));
    }
    out[26..28].copy_from_slice(&snorm16(if vertex.tangent[3] < 0. { -1. } else { 1. }));
    out
}

/// Adds the buffer header and data pair `name` of `bytes`, the header copied from `template`.
fn buffer(
    native: &mut Reader,
    graph: &mut Graph,
    template: u32,
    bytes: &[u8],
    name: &str,
    index: bool,
) -> Result<String> {
    let mut header = native
        .tag(template, None)
        .with_context(|| format!("Decorator buffer {template:08X}"))?
        .0
        .clone();
    if index {
        put(&mut header, 8, &u32::try_from(bytes.len())?.to_le_bytes())?;
        // Byte 1 selects 32-bit indices, and a decorator's are 16 bits.
        header[1] = 0;
    } else {
        put(&mut header, 0, &u32::try_from(bytes.len())?.to_le_bytes())?;
    }
    let symbol = format!("{name}-header");
    let data = format!("{name}-data");
    graph.add(&symbol, template, &header, Some(&data), vec![])?;
    graph.add(
        &data,
        native.reference(template)?,
        bytes,
        Some(&symbol),
        vec![],
    )?;
    Ok(symbol)
}

/// The surface and depth techniques each kind draws with.
type Techniques = Vec<(String, String)>;

/// One mesh's vertex and index bytes, and each kind's first index and index count.
type Streams = (Vec<u8>, Vec<u8>, Vec<(u32, u32)>);

/// Model `name`'s layout 71 vertices and 16-bit triangle list indices at `detail`, kind after
/// kind, and each kind's first index and index count.
fn detail_streams(
    kinds: &[&Kind],
    detail: usize,
    center: [f32; 3],
    scale: f32,
    uv: ([f32; 2], [f32; 2]),
    name: &str,
) -> Result<Streams> {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let mut ranges = Vec::with_capacity(kinds.len());
    for kind in kinds {
        let first = u32::try_from(indices.len() / 2)?;
        for primitive in &kind.details[detail] {
            let base = vertices.len() / VERTEX_STRIDE;
            for v in &primitive.vertices {
                vertices.extend(vertex(v, center, scale, uv));
            }
            for &corner in &primitive.indices {
                ensure!(
                    (corner as usize) < primitive.vertices.len(),
                    "Decorator model {name} indexes past its vertices"
                );
                indices.extend(u16::try_from(base + corner as usize)?.to_le_bytes());
            }
        }
        let count = u32::try_from(indices.len() / 2)? - first;
        ensure!(
            count > 0 && count.is_multiple_of(3),
            "Decorator kind {} of {} has no triangles at detail {detail}",
            kind.index,
            kind.set.path
        );
        ranges.push((first, count));
    }
    ensure!(
        vertices.len() / VERTEX_STRIDE <= super::MAX_VERTICES,
        "Decorator model {name} holds too many vertices"
    );
    Ok((vertices, indices, ranges))
}

/// Adds the entity model `name` of `kinds`, part group `g` drawing kind `g` with its techniques,
/// and returns each kind's radius: the farthest any of its vertices lies from its origin.
fn model(
    native: &mut Reader,
    graph: &mut Graph,
    templates: &Templates,
    kinds: &[&Kind],
    techniques: &Techniques,
    name: &str,
) -> Result<Vec<f32>> {
    let template = native
        .tag(templates.model, Some(MODEL_CLASS))
        .context("Decorator model template")?;
    let row = *template
        .array(MODEL_MESHES, MESH_SIZE, Some(MESH_CLASS as u32))?
        .first()
        .context("The decorator model template has no mesh")?;
    let (vertex_template, index_template) = (
        template.u32(row + MESH_VERTICES)?,
        template.u32(row + MESH_INDICES)?,
    );
    let stride = native.tag(vertex_template, None)?.u16(4)?;
    ensure!(
        usize::from(stride) == VERTEX_STRIDE,
        "The decorator model template's vertices are {stride} bytes"
    );
    let mut low = [f32::INFINITY; 3];
    let mut high = [f32::NEG_INFINITY; 3];
    let mut uv_low = [f32::INFINITY; 2];
    let mut uv_high = [f32::NEG_INFINITY; 2];
    let mut radii = vec![0f32; kinds.len()];
    for (kind, radius) in kinds.iter().zip(&mut radii) {
        for v in kind.details.iter().flatten().flat_map(|p| &p.vertices) {
            for k in 0..3 {
                low[k] = low[k].min(v.position[k]);
                high[k] = high[k].max(v.position[k]);
            }
            for k in 0..2 {
                uv_low[k] = uv_low[k].min(v.uv[k]);
                uv_high[k] = uv_high[k].max(v.uv[k]);
            }
            *radius = radius.max(
                v.position
                    .iter()
                    .map(|value| value * value)
                    .sum::<f32>()
                    .sqrt(),
            );
        }
    }
    ensure!(
        low.iter()
            .chain(&high)
            .chain(&uv_low)
            .chain(&uv_high)
            .all(|v| v.is_finite()),
        "Decorator model {name} has no finite vertices"
    );
    let center: [f32; 3] = std::array::from_fn(|k| (low[k] + high[k]) / 2.);
    let scale = (0..3).map(|k| (high[k] - low[k]) / 2.).fold(1e-4, f32::max);
    let uv_center: [f32; 2] = std::array::from_fn(|k| (uv_low[k] + uv_high[k]) / 2.);
    let uv_scale: [f32; 2] = std::array::from_fn(|k| ((uv_high[k] - uv_low[k]) / 2.).max(1e-4));
    let mut bytes = template.0[..MODEL_HEADER].to_vec();
    put(&mut bytes, MODEL_SCALE, &floats(&[scale, scale, scale, 0.]))?;
    put(
        &mut bytes,
        MODEL_OFFSET,
        &floats(&[center[0], center[1], center[2], scale]),
    )?;
    put(
        &mut bytes,
        MODEL_TEXTURE,
        &floats(&[uv_scale[0], uv_scale[1], uv_center[0], uv_center[1]]),
    )?;
    let mut patches = Vec::new();
    let mut meshes = Vec::with_capacity(DETAILS);
    // Each part's running index over the model, kind by kind and stage by stage, as stock grass
    // numbers its parts.
    let mut index = 0u16;
    for detail in 0..DETAILS {
        let (vertices, indices, ranges) =
            detail_streams(kinds, detail, center, scale, (uv_center, uv_scale), name)?;
        let mut parts = Vec::with_capacity(kinds.len() * 2);
        let mut part_techniques = Vec::with_capacity(kinds.len() * 2);
        let mut numbers = vec![[0u16; 2]; kinds.len()];
        for slot in &mut numbers {
            *slot = [index, index + 1];
            index += 2;
        }
        for (stage, which) in [(SURFACE_STAGE, 0), (DEPTH_STAGE, 1)] {
            for (group, (&(first, count), techniques)) in ranges.iter().zip(techniques).enumerate()
            {
                let mut part = vec![0u8; PART_SIZE];
                put(&mut part, 0, &u32::MAX.to_le_bytes())?;
                put(&mut part, 4, &u16::MAX.to_le_bytes())?;
                put(&mut part, 6, &TRIANGLE_LIST.to_le_bytes())?;
                put(&mut part, 8, &first.to_le_bytes())?;
                put(&mut part, 12, &count.to_le_bytes())?;
                put(&mut part, 16, &(count / 3).to_le_bytes())?;
                put(&mut part, 20, &u16::try_from(group)?.to_le_bytes())?;
                put(&mut part, 22, &numbers[group][which].to_le_bytes())?;
                part[28] = 0x7F;
                part[29] = 1;
                parts.push((stage, part));
                part_techniques.push(if which == 0 {
                    techniques.0.clone()
                } else {
                    techniques.1.clone()
                });
            }
        }
        let vertices_symbol = buffer(
            native,
            graph,
            vertex_template,
            &vertices,
            &format!("{name}-{detail}-vertices"),
            false,
        )?;
        let indices_symbol = buffer(
            native,
            graph,
            index_template,
            &indices,
            &format!("{name}-{detail}-indices"),
            true,
        )?;
        meshes.push((vertices_symbol, indices_symbol, parts, part_techniques));
    }
    let mut rows = Vec::with_capacity(DETAILS * MESH_SIZE);
    for (_, _, parts, _) in &meshes {
        let mut mesh = template.0[row..row + MESH_SIZE].to_vec();
        put(&mut mesh, MESH_VERTICES, &u32::MAX.to_le_bytes())?;
        put(&mut mesh, MESH_INDICES, &u32::MAX.to_le_bytes())?;
        put(&mut mesh, MESH_PARTS, &[0; 16])?;
        for stage in 0..=STAGES {
            let before = parts.iter().filter(|(drawn, _)| *drawn < stage).count();
            put(
                &mut mesh,
                MESH_RANGES + stage * 2,
                &u16::try_from(before)?.to_le_bytes(),
            )?;
        }
        for stage in 0..STAGES {
            put(&mut mesh, MESH_LAYOUTS + stage * 2, &LAYOUT.to_le_bytes())?;
        }
        rows.extend(mesh);
    }
    append(&mut bytes, MODEL_MESHES, MESH_CLASS, &rows, MESH_SIZE)?;
    let mesh_rows = Payload(bytes.clone()).array(MODEL_MESHES, MESH_SIZE, None)?;
    for (&at, (vertices, indices, parts, part_techniques)) in mesh_rows.iter().zip(&meshes) {
        patches.push(json!({"offset": at + MESH_VERTICES, "symbol": vertices}));
        patches.push(json!({"offset": at + MESH_INDICES, "symbol": indices}));
        let rows = parts
            .iter()
            .flat_map(|(_, part)| part.iter().copied())
            .collect::<Vec<_>>();
        append(&mut bytes, at + MESH_PARTS, PART_CLASS, &rows, PART_SIZE)?;
        let placed = Payload(bytes.clone()).array(at + MESH_PARTS, PART_SIZE, None)?;
        for (&part, technique) in placed.iter().zip(part_techniques) {
            patches.push(json!({"offset": part, "symbol": technique}));
        }
    }
    graph.add(name, templates.model, &bytes, None, patches)?;
    Ok(radii)
}

/// Adds the level of detail record `name`, one row per kind of `kinds`. Each row draws all three
/// meshes over its [`bands`], and takes the template's first row's float and band bytes.
fn lod(
    native: &mut Reader,
    graph: &mut Graph,
    templates: &Templates,
    kinds: &[&Kind],
    name: &str,
) -> Result<Vec<[f32; 4]>> {
    let template = native
        .tag(templates.lod, Some(LOD_CLASS))
        .context("Decorator level of detail template")?;
    let float = template.f32(
        *template
            .array(LOD_FLOATS, 4, Some(FLOAT_CLASS as u32))?
            .first()
            .context("The decorator level of detail template has no rows")?,
    )?;
    let selection = template.bytes::<4>(
        *template
            .array(LOD_SELECTIONS, 4, Some(BANDS_CLASS as u32))?
            .first()
            .context("The decorator level of detail template has no band rows")?,
    )?;
    let bands = kinds.iter().map(|kind| bands(kind)).collect::<Vec<_>>();
    let mut bytes = vec![0u8; LOD_HEADER];
    append(
        &mut bytes,
        LOD_DETAILS,
        DETAIL_CLASS,
        &vec![u8::try_from(DETAILS - 1)?; kinds.len()],
        1,
    )?;
    append(
        &mut bytes,
        LOD_BANDS,
        FLOAT4_CLASS,
        &floats(&bands.concat()),
        16,
    )?;
    append(
        &mut bytes,
        LOD_FLOATS,
        FLOAT_CLASS,
        &floats(&vec![float; kinds.len()]),
        4,
    )?;
    append(
        &mut bytes,
        LOD_SIGNED,
        SIGNED_CLASS,
        &vec![0; kinds.len()],
        1,
    )?;
    append(
        &mut bytes,
        LOD_SELECTIONS,
        BANDS_CLASS,
        &selection.repeat(kinds.len()),
        4,
    )?;
    graph.add(name, templates.lod, &bytes, None, vec![])?;
    Ok(bands)
}

/// One record's instance: its kind's index among the record's kinds and its placement's index.
struct Member {
    kind: usize,
    placement: usize,
}

/// Writes `instanced`, the level's decorators, as Destiny's instanced decorators, and returns
/// each of the level's `areas` records with their clusters as Umbra objects. Instances group by
/// the areas that load them (`membership` of their BSPs), and each group's kinds split into
/// records whose meshes hold at most [`super::MAX_VERTICES`] vertices, so an area places the
/// records of every group it belongs to. A record's clusters are its instances of one kind in one
/// cell of a grid, its cells doubling from [`CELL`] until they number at most [`MAX_CLUSTERS`].
/// Positions move by `offset` into the map's coordinates.
#[allow(clippy::too_many_arguments, clippy::cognitive_complexity)]
pub(super) fn add(
    cache: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    graph: &mut Graph,
    templates: &Templates,
    instanced: Instanced,
    membership: &dyn Fn(u32) -> u64,
    offset: [f32; 3],
    areas: usize,
) -> Result<Added> {
    let Instanced { kinds, placements } = instanced;
    let mut added = Added {
        areas: vec![(Vec::new(), Vec::new()); areas],
        report: json!({"mode": "native", "records": [], "kinds": []}),
    };
    if placements.is_empty() {
        return Ok(added);
    }
    // Each set's techniques, which its kinds share.
    let mut sets = BTreeMap::<String, (String, String)>::new();
    let mut techniques = Vec::with_capacity(kinds.len());
    for kind in &kinds {
        if !sets.contains_key(&kind.set.path) {
            let mapping = Mapping {
                role: "base_map".to_owned(),
                bitmap: Some(kind.texture.clone()),
                transform: [1., 1., 0., 0.],
                frame: 0,
                address: 0,
                filter: 0,
                functions: Vec::new(),
            };
            let texture =
                native::material::texture(cache, pages, native, graph, &mapping, false, true)
                    .with_context(|| format!("Decorator set {} texture", kind.set.path))?;
            let set = sets.len();
            let pair = (
                format!("decorator-surface{set}"),
                format!("decorator-depth{set}"),
            );
            technique(native, graph, templates.surface, &texture, &pair.0, true)?;
            technique(native, graph, templates.depth, &texture, &pair.1, false)?;
            sets.insert(kind.set.path.clone(), pair);
        }
        techniques.push(sets[&kind.set.path].clone());
    }
    // The instances of each group of areas, by kind.
    let mut groups = BTreeMap::<u64, BTreeMap<usize, Vec<usize>>>::new();
    let mut unloaded = 0usize;
    for (index, placement) in placements.iter().enumerate() {
        let mask = membership(placement.bsps);
        if mask == 0 {
            unloaded += 1;
            continue;
        }
        groups
            .entry(mask)
            .or_default()
            .entry(placement.kind)
            .or_default()
            .push(index);
    }
    // Each distinct list of kinds becomes one model, row and level of detail record.
    let mut models = BTreeMap::<Vec<usize>, (String, Vec<f32>)>::new();
    let mut kind_report = BTreeMap::<usize, Value>::new();
    let mut records = Vec::new();
    // Cluster identifiers stay distinct over every record, since an area's visibility record
    // keys the clusters of all the records it loads.
    let mut taken = BTreeSet::new();
    for (mask, by_kind) in groups {
        // Kinds in set order, so a set's kinds sit together and share a run.
        let mut used = by_kind.keys().copied().collect::<Vec<_>>();
        used.sort_by(|&a, &b| {
            kinds[a]
                .set
                .path
                .cmp(&kinds[b].set.path)
                .then(kinds[a].index.cmp(&kinds[b].index))
        });
        let mut chunks = vec![Vec::new()];
        let mut vertices = 0usize;
        for kind in used {
            let count = kinds[kind]
                .details
                .iter()
                .map(|primitives| primitives.iter().map(|p| p.vertices.len()).sum::<usize>())
                .max()
                .unwrap_or(0);
            ensure!(
                count <= super::MAX_VERTICES,
                "Decorator kind {} of {} holds {count} vertices",
                kinds[kind].index,
                kinds[kind].set.path
            );
            if vertices + count > super::MAX_VERTICES {
                chunks.push(Vec::new());
                vertices = 0;
            }
            vertices += count;
            chunks.last_mut().context("Decorator chunk")?.push(kind);
        }
        for chunk in chunks.into_iter().filter(|chunk| !chunk.is_empty()) {
            let record = records.len();
            let name = format!("decorators{record}");
            if !models.contains_key(&chunk) {
                let model_name = format!("decorator-model{}", models.len());
                let chosen = chunk.iter().map(|&kind| &kinds[kind]).collect::<Vec<_>>();
                let chosen_techniques = chunk
                    .iter()
                    .map(|&kind| techniques[kind].clone())
                    .collect::<Vec<_>>();
                let radii = model(
                    native,
                    graph,
                    templates,
                    &chosen,
                    &chosen_techniques,
                    &model_name,
                )?;
                let lod_name = format!("{model_name}-lod");
                let distances = lod(native, graph, templates, &chosen, &lod_name)?;
                let row = native
                    .tag(templates.row, Some(ROW_CLASS))
                    .context("Decorator model row template")?;
                ensure!(
                    row.u32(ROW_WIND)? == u32::MAX,
                    "The decorator model row template names a wind record"
                );
                let mut bytes = row.0.clone();
                put(&mut bytes, ROW_MODEL, &u32::MAX.to_le_bytes())?;
                put(&mut bytes, ROW_LOD, &u32::MAX.to_le_bytes())?;
                let row_name = format!("{model_name}-row");
                graph.add(
                    &row_name,
                    templates.row,
                    &bytes,
                    None,
                    vec![
                        json!({"offset": ROW_MODEL, "symbol": model_name}),
                        json!({"offset": ROW_LOD, "symbol": lod_name}),
                    ],
                )?;
                for (k, &kind) in chunk.iter().enumerate() {
                    kind_report.entry(kind).or_insert_with(|| {
                        json!({
                            "set": kinds[kind].set.path,
                            "type": kinds[kind].index,
                            "fades": kinds[kind].fades,
                            "bands": distances[k],
                            "radius": radii[k],
                            "triangles": kinds[kind].details.iter().map(|primitives| {
                                primitives.iter().map(|p| p.indices.len() / 3).sum::<usize>()
                            }).collect::<Vec<_>>(),
                        })
                    });
                }
                models.insert(chunk.clone(), (row_name, radii));
            }
            let (row_name, radii) = models[&chunk].clone();
            let members = chunk
                .iter()
                .enumerate()
                .flat_map(|(local, kind)| {
                    by_kind[kind].iter().map(move |&placement| Member {
                        kind: local,
                        placement,
                    })
                })
                .collect::<Vec<_>>();
            let position = |member: &Member| -> [f32; 3] {
                let p = placements[member.placement].position;
                std::array::from_fn(|k| p[k] + offset[k])
            };
            // The coarsest grid that keeps the clusters within MAX_CLUSTERS.
            let mut cell = CELL;
            let clustered = loop {
                let mut cells = BTreeMap::<(usize, [i64; 3]), Vec<usize>>::new();
                for (index, member) in members.iter().enumerate() {
                    let p = position(member);
                    cells
                        .entry((member.kind, p.map(|v| (v / cell).floor() as i64)))
                        .or_default()
                        .push(index);
                }
                if cells.len() <= MAX_CLUSTERS || cell > 1e5 {
                    break cells;
                }
                cell *= 2.;
            };
            ensure!(
                clustered.len() <= MAX_CLUSTERS,
                "Decorator record {name} needs {} clusters",
                clustered.len()
            );
            // Instances in cluster order, which keeps each kind's instances together.
            let mut ordered = Vec::with_capacity(members.len());
            let mut cluster_starts = Vec::with_capacity(clustered.len() + 1);
            let mut kind_starts = vec![None; chunk.len()];
            let mut boxes = Vec::with_capacity(clustered.len());
            for ((kind, _), indices) in &clustered {
                kind_starts[*kind].get_or_insert(ordered.len());
                cluster_starts.push(u32::try_from(ordered.len())?);
                let mut low = [f32::INFINITY; 3];
                let mut high = [f32::NEG_INFINITY; 3];
                for &index in indices {
                    let member = &members[index];
                    let p = position(member);
                    let reach = radii[member.kind] * placements[member.placement].scale;
                    for k in 0..3 {
                        low[k] = low[k].min(p[k] - reach);
                        high[k] = high[k].max(p[k] + reach);
                    }
                    ordered.push(index);
                }
                boxes.push((low, high));
            }
            cluster_starts.push(u32::try_from(ordered.len())?);
            let mut kind_ranges = kind_starts
                .iter()
                .map(|start| {
                    start
                        .map(u32::try_from)
                        .transpose()?
                        .context("A decorator kind without instances")
                })
                .collect::<Result<Vec<_>>>()?;
            kind_ranges.push(u32::try_from(ordered.len())?);
            // Runs of kinds sharing a surface, as stock grass merges its types that share one.
            let mut runs = vec![0u32];
            for local in 1..chunk.len() {
                if kinds[chunk[local]].set.path != kinds[chunk[local - 1]].set.path {
                    runs.push(kind_ranges[local]);
                }
            }
            runs.push(u32::try_from(ordered.len())?);
            // The instances' positions and scales over their range, then each row.
            let mut low = [f32::INFINITY; 4];
            let mut high = [f32::NEG_INFINITY; 4];
            for member in &members {
                let p = position(member);
                let values = [p[0], p[1], p[2], placements[member.placement].scale];
                for k in 0..4 {
                    low[k] = low[k].min(values[k]);
                    high[k] = high[k].max(values[k]);
                }
            }
            ensure!(
                low.iter().chain(&high).all(|v| v.is_finite()),
                "Decorator record {name} has a nonfinite instance"
            );
            let middle: [f32; 4] = std::array::from_fn(|k| (low[k] + high[k]) / 2.);
            let half: [f32; 4] = std::array::from_fn(|k| ((high[k] - low[k]) / 2.).max(1e-4));
            let mut rows = Vec::with_capacity(ordered.len() * INSTANCE_STRIDE);
            for &index in &ordered {
                let member = &members[index];
                let placement = &placements[member.placement];
                let p = position(member);
                let values = [p[0], p[1], p[2], placement.scale];
                for k in 0..4 {
                    rows.extend(snorm16((values[k] - middle[k]) / half[k]));
                }
                for value in placement.rotation {
                    rows.push(unorm8((value + 1.) / 2.));
                }
                // No wind bend and no albedo tint.
                rows.extend([0u8; 4]);
            }
            let constants = floats(&[
                half[0], half[1], half[2], half[3], middle[0], middle[1], middle[2], middle[3], 2.,
                2., 2., 2., -1., -1., -1., -1., 1., 0., 0., 0., 0., 0., 0., 0.,
            ]);
            let constants_name = format!("{name}-constants");
            graph.add(
                &constants_name,
                templates.constants,
                &constants,
                None,
                vec![],
            )?;
            let mut data = vec![0u8; INSTANCES_HEADER];
            append(&mut data, 0x08, INSTANCE_ROW_CLASS, &rows, INSTANCE_STRIDE)?;
            let data_name = format!("{name}-instances");
            graph.add(&data_name, templates.instances, &data, None, vec![])?;
            let buffer_template = native.tag(templates.buffer, None)?;
            ensure!(
                usize::from(buffer_template.u16(4)?) == INSTANCE_STRIDE,
                "The decorator instance buffer template's rows are not 16 bytes"
            );
            let buffer_name = buffer(
                native,
                graph,
                templates.buffer,
                &rows,
                &format!("{name}-buffer"),
                false,
            )?;
            let header_template = native
                .tag(templates.header, Some(HEADER_CLASS))
                .context("Decorator instance header template")?;
            let mut header = header_template.0.clone();
            put(
                &mut header,
                HEADER_IDENTITY,
                &fnv1a64(&format!("parhelion/decorators/{name}")).to_le_bytes(),
            )?;
            for at in [HEADER_CONSTANTS, HEADER_BUFFER, HEADER_DATA] {
                put(&mut header, at, &u32::MAX.to_le_bytes())?;
            }
            let header_name = format!("{name}-header");
            graph.add(
                &header_name,
                templates.header,
                &header,
                None,
                vec![
                    json!({"offset": HEADER_CONSTANTS, "symbol": constants_name}),
                    json!({"offset": HEADER_BUFFER, "symbol": buffer_name}),
                    json!({"offset": HEADER_DATA, "symbol": data_name}),
                ],
            )?;
            let identifiers = super::statics::distinct(
                &format!("parhelion/decorators/{name}"),
                boxes.len(),
                &mut taken,
            );
            let mut bounds_rows = Vec::with_capacity(boxes.len() * 48);
            for (&(low, high), id) in boxes.iter().zip(&identifiers) {
                bounds_rows.extend(floats(&[
                    low[0], low[1], low[2], 1., high[0], high[1], high[2], 1.,
                ]));
                bounds_rows.extend(0u32.to_le_bytes());
                bounds_rows.extend(id.to_le_bytes());
                bounds_rows.extend([0; 8]);
            }
            let mut bounds = vec![0u8; BOUNDS_HEADER];
            append(&mut bounds, 0x08, BOUNDS_ROW_CLASS, &bounds_rows, 48)?;
            let bounds_name = format!("{name}-bounds");
            graph.add(&bounds_name, templates.bounds, &bounds, None, vec![])?;
            let record_template = native
                .tag(templates.record, Some(RECORD_CLASS))
                .context("Decorator record template")?;
            let mut bytes = vec![0u8; RECORD_HEADER];
            for range in RECORD_COPIED {
                bytes[range.clone()].copy_from_slice(
                    record_template
                        .0
                        .get(range)
                        .context("The decorator record template is too short")?,
                );
            }
            let mut every_low = [f32::INFINITY; 3];
            let mut every_high = [f32::NEG_INFINITY; 3];
            for (low, high) in &boxes {
                for k in 0..3 {
                    every_low[k] = every_low[k].min(low[k]);
                    every_high[k] = every_high[k].max(high[k]);
                }
            }
            put(
                &mut bytes,
                RECORD_BOX,
                &floats(&[
                    every_low[0],
                    every_low[1],
                    every_low[2],
                    1.,
                    every_high[0],
                    every_high[1],
                    every_high[2],
                    1.,
                ]),
            )?;
            put(&mut bytes, RECORD_INSTANCES, &u32::MAX.to_le_bytes())?;
            put(&mut bytes, RECORD_BOUNDS, &u32::MAX.to_le_bytes())?;
            let list = |values: &[u32]| {
                values
                    .iter()
                    .flat_map(|v| v.to_le_bytes())
                    .collect::<Vec<_>>()
            };
            append(
                &mut bytes,
                RECORD_MODELS,
                MODEL_ROW_CLASS,
                &u32::MAX.to_le_bytes(),
                4,
            )?;
            append(
                &mut bytes,
                RECORD_RANGES,
                INDEX_CLASS,
                &list(&kind_ranges),
                4,
            )?;
            append(&mut bytes, RECORD_RUNS, INDEX_CLASS, &list(&runs), 4)?;
            append(
                &mut bytes,
                RECORD_CLUSTERS,
                INDEX_CLASS,
                &list(&cluster_starts),
                4,
            )?;
            let order = (0..u32::try_from(boxes.len())?).collect::<Vec<_>>();
            append(&mut bytes, RECORD_ORDER, INDEX_CLASS, &list(&order), 4)?;
            let model_row = *Payload(bytes.clone())
                .array(RECORD_MODELS, 4, None)?
                .first()
                .context("Decorator record model row")?;
            graph.add(
                &name,
                templates.record,
                &bytes,
                None,
                vec![
                    json!({"offset": model_row, "symbol": row_name}),
                    json!({"offset": RECORD_INSTANCES, "symbol": header_name}),
                    json!({"offset": RECORD_BOUNDS, "symbol": bounds_name}),
                ],
            )?;
            let visible = boxes
                .iter()
                .zip(&identifiers)
                .map(|(&(minimum, maximum), &id)| Visible {
                    id,
                    minimum,
                    maximum,
                    upper: true,
                })
                .collect::<Vec<_>>();
            for (area, (symbols, objects)) in added.areas.iter_mut().enumerate() {
                if mask & 1u64 << area != 0 {
                    symbols.push(name.clone());
                    objects.extend(visible.iter().cloned());
                }
            }
            records.push(json!({
                "record": name,
                "areas": mask,
                "model": row_name,
                "kinds": chunk.iter().map(|&kind| json!([kinds[kind].set.path, kinds[kind].index])).collect::<Vec<_>>(),
                "instances": ordered.len(),
                "clusters": boxes.len(),
                "cell": cell,
                "box": [every_low, every_high],
            }));
        }
    }
    added.report = json!({
        "mode": "native",
        "records": records,
        "models": models.len(),
        "kinds": kind_report.into_values().collect::<Vec<_>>(),
        "instances": placements.len() - unloaded,
        "unloaded": unloaded,
        "sets": sets.keys().collect::<Vec<_>>(),
    });
    Ok(added)
}

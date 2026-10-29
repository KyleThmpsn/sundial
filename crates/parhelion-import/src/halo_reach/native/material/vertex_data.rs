//! Lighting and water coordinates transported through lossless half bits and vertex texture loads,
//! and the fixed-point lightmap coordinates a static vertex's detail lanes carry.
use super::*;
use std::collections::HashMap;

const WIDTH: usize = 2048;
const PER_ROW: usize = WIDTH / 4;
// Positive finite half bit patterns, including zero. The negative second component uses
// bits 1 and above so even record zero retains the table marker.
const ADDRESS_RADIX: usize = 0x7c00;
const MAX_HEIGHT: usize = 16384;

/// Lightmap coordinates ride in two half lanes as fixed point in their bits: the least normal half's
/// bits, `0x400`, plus the coordinate's magnitude times `COORDINATE_STEPS`, up to `0x7BFF`, with
/// the sign bit for a negative coordinate. Every pattern is a finite normal half, so the input
/// assembler's and a texture load's conversions to float keep it, and `f32tof16` recovers it
/// without relying on subnormals. A step is 1/30,719 of the atlas's side. Plain half floats step by
/// 2^-11 from 0.5 to 1, 3.5 texels on a side of 7,118, where Reach's own coordinates step by 0.016
/// texels of their image. A positive second lane also keeps a vertex out of the record path, which
/// marks its records with a negative one.
pub(in crate::halo_reach) const COORDINATE_STEPS: f32 = 30_719.;
const COORDINATE_FIRST: u16 = 0x400;

/// The half lane bits of lightmap coordinate `value`, from -1 to 1. A negative value keeps at least
/// one step, so its sign survives the decode.
pub(in crate::halo_reach) fn coordinate_bits(value: f32) -> u16 {
    let steps = (value.abs().min(1.) * COORDINATE_STEPS + 0.5) as u16;
    if value < 0. {
        0x8000 | (COORDINATE_FIRST + steps.max(1))
    } else {
        COORDINATE_FIRST + steps
    }
}

/// HLSL decoding the float2 `lanes`, read from [`coordinate_bits`] halves, back to coordinates.
/// A lane that holds zero decodes to zero.
pub(super) fn coordinates(lanes: &str) -> String {
    format!(
        "sign({lanes})*max(float2(f32tof16(abs({lanes})))-{COORDINATE_FIRST},0)/{COORDINATE_STEPS:.1}"
    )
}

pub(in crate::halo_reach) struct Table {
    pub symbol: String,
    /// Each vertex's detail lane bits, which name its record.
    pub coordinates: HashMap<(u32, u32), [u16; 2]>,
    pub records: usize,
    pub height: usize,
}

pub(in crate::halo_reach) fn add(
    native: &mut Reader,
    graph: &mut Graph,
    primitives: &[crate::halo_reach::model::Primitive],
    peak: f32,
    lightmap: impl Fn(u32, &crate::halo_reach::model::Vertex) -> [f32; 2],
) -> Result<Option<Table>> {
    let count = primitives
        .iter()
        .flat_map(|primitive| &primitive.vertices)
        .filter(|vertex| vertex.lighting.is_some() || vertex.water_uv.is_some())
        .count();
    if count == 0 {
        return Ok(None);
    }
    ensure!(
        peak.is_finite() && peak > 0.,
        "Vertex lighting scale is invalid"
    );
    let mut bytes = Vec::new();
    let mut records = HashMap::new();
    let mut coordinates = HashMap::new();
    for (primitive, part) in primitives.iter().enumerate() {
        for (vertex, source) in part.vertices.iter().enumerate() {
            if source.lighting.is_none() && source.water_uv.is_none() {
                continue;
            }
            let mut lighting = source.lighting.unwrap_or([[0.; 4]; 3]);
            // Keep radiance linear through raster interpolation. Pixel lightmaps instead decode
            // their logarithmic encoding before shading.
            for row in &mut lighting[..2] {
                for channel in &mut row[..3] {
                    *channel /= peak;
                }
            }
            lighting[2][3] = f32::from(
                u8::from(source.lighting.is_some()) | (u8::from(source.water_uv.is_some()) << 1),
            );
            let water = source.water_uv.unwrap_or([0.; 2]);
            let lightmap = lightmap(u32::try_from(primitive)?, source).map(coordinate_bits);
            let mut record = [0u8; 32];
            for (channel, value) in lighting.into_iter().flatten().chain(water).enumerate() {
                let value = half::f16::from_f32(value);
                ensure!(
                    value.is_finite(),
                    "Vertex data exceeds the finite half-float range"
                );
                record[channel * 2..channel * 2 + 2].copy_from_slice(&value.to_le_bytes());
            }
            // The last two lanes hold the lightmap coordinates' fixed-point bits.
            for (k, bits) in lightmap.into_iter().enumerate() {
                record[28 + k * 2..30 + k * 2].copy_from_slice(&bits.to_le_bytes());
            }
            let next = records.len();
            let index = *records.entry(record).or_insert_with(|| {
                bytes.extend_from_slice(&record);
                next
            });
            ensure!(
                index < PER_ROW * MAX_HEIGHT,
                "Vertex data exceeds the 2048 by 16384 texture capacity"
            );
            // These bits are half values, not rounded integer coordinates. The shader recovers
            // them before deriving a texel address, including subnormal halves.
            coordinates.insert(
                (u32::try_from(primitive)?, u32::try_from(vertex)?),
                [
                    (index % ADDRESS_RADIX) as u16,
                    0x8000 | (index / ADDRESS_RADIX + 1) as u16,
                ],
            );
        }
    }
    let height = records.len().div_ceil(PER_ROW);
    bytes.resize(WIDTH * height * 8, 0);
    let symbol = "reach-vertex-data".to_owned();
    let template = 0x80ec31bc;
    let mut header = native.tag(template, None)?.0.clone();
    put(&mut header, 0, &u32::try_from(bytes.len())?.to_le_bytes())?;
    put(&mut header, 4, &RGBA16_FLOAT.to_le_bytes())?;
    put(&mut header, 14, &u16::try_from(WIDTH)?.to_le_bytes())?;
    put(&mut header, 16, &u16::try_from(height)?.to_le_bytes())?;
    put(&mut header, 18, &1u16.to_le_bytes())?;
    put(&mut header, 20, &1u16.to_le_bytes())?;
    header[23] = 1;
    put(&mut header, 36, &u32::MAX.to_le_bytes())?;
    crate::tiger::texture::resident(&mut header, bytes.len())?;
    let data = format!("{symbol}-data");
    graph.add(&symbol, template, &header, Some(&data), vec![])?;
    graph.add(
        &data,
        native.reference(template)?,
        &bytes,
        Some(&symbol),
        vec![],
    )?;
    Ok(Some(Table {
        symbol,
        coordinates,
        records: records.len(),
        height,
    }))
}

pub(super) const DECLARATION: &str = "Texture2D<float4> VertexData:register(t0);";
/// Loads a record's rows into the lighting outputs, its water coordinates into `o9` and its
/// lightmap coordinate bits into `o3.zw`, which the static vertex program then decodes with the
/// attribute stream's own ([`coordinates`]).
pub(super) const READ: &str = "o6=0;o7=0;o8=0;if(detail.y<0){uint record=f32tof16(detail.x)+(f32tof16(-detail.y)-1)*31744;int2 at=int2((record%512)*4,record/512);o6=VertexData.Load(int3(at,0));o7=VertexData.Load(int3(at+int2(1,0),0));o8=VertexData.Load(int3(at+int2(2,0),0));float4 extra=VertexData.Load(int3(at+int2(3,0),0));uint flags=(uint)o8.w;o8.w=flags&1;o3.zw=extra.zw;if((flags&2)!=0){o9.xy=extra.xy;o9.w=1;}}";

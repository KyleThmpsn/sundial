//! Lighting and water coordinates transported through lossless half bits and vertex texture loads.
use super::*;
use std::collections::HashMap;

const WIDTH: usize = 2048;
const PER_ROW: usize = WIDTH / 4;
// Positive finite half bit patterns, including zero. The negative second component uses
// bits 1 and above so even record zero retains the table marker.
const ADDRESS_RADIX: usize = 0x7c00;
const MAX_HEIGHT: usize = 16384;

pub(in crate::halo_reach) struct Table {
    pub symbol: String,
    pub coordinates: HashMap<(u32, u32), [f32; 2]>,
    pub records: usize,
    pub height: usize,
}

pub(in crate::halo_reach) fn add(
    native: &mut Reader,
    graph: &mut Graph,
    primitives: &[super::super::model::Primitive],
    peak: f32,
    lightmap: impl Fn(u32, &super::super::model::Vertex) -> [f32; 2],
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
            // their square-root encoding before shading.
            for row in &mut lighting[..2] {
                for channel in &mut row[..3] {
                    *channel /= peak;
                }
            }
            lighting[2][3] = f32::from(
                u8::from(source.lighting.is_some()) | (u8::from(source.water_uv.is_some()) << 1),
            );
            let water = source.water_uv.unwrap_or([0.; 2]);
            let lightmap = lightmap(u32::try_from(primitive)?, source);
            let extra = [water[0], water[1], lightmap[0], lightmap[1]];
            let mut record = [0u8; 32];
            for (channel, value) in lighting.into_iter().flatten().chain(extra).enumerate() {
                let value = half::f16::from_f32(value);
                ensure!(
                    value.is_finite(),
                    "Vertex data exceeds the finite half-float range"
                );
                record[channel * 2..channel * 2 + 2].copy_from_slice(&value.to_le_bytes());
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
            // These floats are exact half values, not rounded integer coordinates. The shader
            // recovers their bits before deriving a texel address, including subnormal halves.
            coordinates.insert(
                (u32::try_from(primitive)?, u32::try_from(vertex)?),
                [
                    half::f16::from_bits((index % ADDRESS_RADIX) as u16).to_f32(),
                    -half::f16::from_bits((index / ADDRESS_RADIX + 1) as u16).to_f32(),
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
pub(super) const READ: &str = "o6=0;o7=0;o8=0;if(detail.y<0){uint record=f32tof16(detail.x)+(f32tof16(-detail.y)-1)*31744;int2 at=int2((record%512)*4,record/512);o6=VertexData.Load(int3(at,0));o7=VertexData.Load(int3(at+int2(1,0),0));o8=VertexData.Load(int3(at+int2(2,0),0));float4 extra=VertexData.Load(int3(at+int2(3,0),0));uint flags=(uint)o8.w;o8.w=flags&1;o3.zw=extra.zw;if((flags&2)!=0){o9.xy=extra.xy;o9.w=1;}}";

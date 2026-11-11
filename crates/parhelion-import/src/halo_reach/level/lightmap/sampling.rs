//! Original compressed lightmap samples and the separate constant-probe path.
use super::{Lighting, Placement, Texel};
use anyhow::{Result, ensure};
use serde::Serialize;
use serde_json::{Value, json};

const PROBES_PER_ROW: usize = 512;
const PROBE_WIDTH: usize = PROBES_PER_ROW * 3;

pub(super) struct Image {
    pub source: String,
    pub constant: f32,
    pub intensity: [Vec<u8>; 3],
    pub mask: Vec<u8>,
}

#[derive(Serialize)]
struct Region {
    source: String,
    origin: [usize; 2],
    extent: [usize; 2],
    constant: f32,
}

/// Source blocks remain unchanged, while probes use three linear half4 rows per record.
pub(in crate::halo_reach) struct Sampling {
    pub intensity: [Vec<u8>; 3],
    pub mask: Vec<u8>,
    pub probes: Vec<u8>,
    pub probe_extent: [usize; 2],
    regions: Vec<Region>,
    extent: [usize; 2],
    probe_row: usize,
    probe_count: usize,
    peak: f32,
}

impl Sampling {
    pub(super) fn pack(
        extent: [usize; 2],
        images: &[Option<Lighting>],
        placements: &[Option<Placement>],
        probes: &[Vec<Texel>],
        probe_row: usize,
        peak: f32,
    ) -> Result<Self> {
        let [width, height] = extent;
        ensure!(
            width.is_multiple_of(4) && height.is_multiple_of(4) && images.len() == placements.len(),
            "Source lightmap atlas is not block aligned"
        );
        let mut intensity = std::array::from_fn(|_| vec![0; width * height]);
        let mut mask = vec![0; width * height];
        let mut regions = Vec::new();
        for (image, placement) in images.iter().zip(placements) {
            let (Some(image), Some(placement)) = (image, placement) else {
                ensure!(
                    image.is_none() && placement.is_none(),
                    "Lightmap placement differs"
                );
                continue;
            };
            let Placement {
                column,
                row,
                width: w,
                height: h,
            } = *placement;
            ensure!(
                column.is_multiple_of(4)
                    && row.is_multiple_of(4)
                    && w.is_multiple_of(4)
                    && h.is_multiple_of(4)
                    && column + w <= width
                    && row + h <= height,
                "Source lightmap image exceeds its block atlas"
            );
            let source = &image.source;
            for (input, output) in source
                .intensity
                .iter()
                .chain([&source.mask])
                .zip(intensity.iter_mut().chain([&mut mask]))
            {
                ensure!(input.len() == w * h, "Source lightmap block count differs");
                for y in 0..h / 4 {
                    let from = y * w * 4;
                    let to = ((row / 4 + y) * (width / 4) + column / 4) * 16;
                    output[to..to + w * 4].copy_from_slice(&input[from..from + w * 4]);
                }
            }
            regions.push(Region {
                source: source.source.clone(),
                origin: [column, row],
                extent: [w, h],
                constant: source.constant,
            });
        }
        let probe_count = probes.iter().map(Vec::len).sum::<usize>();
        let probe_height = probe_count.div_ceil(PROBES_PER_ROW).max(1);
        ensure!(
            probe_height <= 16_384,
            "Lightmap probes exceed the native texture extent"
        );
        let mut data = vec![0; PROBE_WIDTH * probe_height * 8];
        for (index, probe) in probes.iter().flatten().enumerate() {
            let values = [
                [
                    probe.lobe[0] / peak,
                    probe.lobe[1] / peak,
                    probe.lobe[2] / peak,
                    f32::from(probe.bandwidth) / 255.,
                ],
                [
                    probe.ambient[0] / peak,
                    probe.ambient[1] / peak,
                    probe.ambient[2] / peak,
                    f32::from(probe.sun) / 255.,
                ],
                [
                    f32::from(probe.direction[0]) / 127.5 - 1.,
                    f32::from(probe.direction[1]) / 127.5 - 1.,
                    f32::from(probe.direction[2]) / 127.5 - 1.,
                    0.,
                ],
            ];
            for (channel, value) in values.into_iter().flatten().enumerate() {
                let value = half::f16::from_f32(value);
                ensure!(
                    value.is_finite(),
                    "Lightmap probe exceeds the finite half-float range"
                );
                let at = index * 24 + channel * 2;
                data[at..at + 2].copy_from_slice(&value.to_le_bytes());
            }
        }
        Ok(Self {
            intensity,
            mask,
            probes: data,
            probe_extent: [PROBE_WIDTH, probe_height],
            regions,
            extent,
            probe_row,
            probe_count,
            peak,
        })
    }

    pub fn report(&self) -> Value {
        json!({
            "images": self.regions,
            "extent": self.extent,
            "peak": self.peak,
            "probe_row": self.probe_row,
            "probe_count": self.probe_count,
            "probe_extent": self.probe_extent,
            "method": "Original BC3 and BC5 samples are filtered before VMF decoding. Constant probes use linear half-float records."
        })
    }

    /// Decode the original sampled values. Region constants stay float32, and the image bounds
    /// clamp its sampling footprint so the atlas does not blend neighboring BSPs or neutral cells.
    /// The half-texel selection margin covers the transported coordinate's fixed-point rounding.
    pub fn function(&self, scale: f32) -> String {
        let [width, height] = self.extent;
        let mut select = String::new();
        for (index, region) in self.regions.iter().enumerate() {
            let [x, y] = region.origin;
            let [w, h] = region.extent;
            select.push_str(&format!(
                "{}if(all(pixel>=float2({:.1},{:.1}))&&all(pixel<=float2({:.1},{:.1}))){{factor={:.9e};bounds=float4({:.1},{:.1},{:.1},{:.1});}}",
                if index == 0 { "" } else { "else " },
                x as f32 - 0.5, y as f32 - 0.5,
                (x + w) as f32 + 0.5, (y + h) as f32 + 0.5,
                region.constant / self.peak,
                x as f32 + 0.5, y as f32 + 0.5,
                (x + w) as f32 - 0.5, (y + h) as f32 - 0.5,
            ));
        }
        format!(
            r#"
void reachLightmap(float2 coordinates,out float4 lobe,out float4 ambient,out float3 direction){{
float2 pixel=abs(coordinates)*float2({width},{height});
float factor=0;float4 bounds=0;
{select}
if(factor>0){{
float2 uv=clamp(pixel,bounds.xy,bounds.zw)/float2({width},{height});
float4 a=LightIntensity0.SampleLevel(ClampSampler,uv,0);
float4 b=LightIntensity1.SampleLevel(ClampSampler,uv,0);
float4 c=LightIntensity2.SampleLevel(ClampSampler,uv,0);
float2 mask=LightMask.SampleLevel(ClampSampler,uv,0).xy;
float3 axis=float3(a.a,b.a,c.a)*2-1;float bandwidth=length(axis);
factor*=exp(-6.238325*mask.x);
lobe=float4((a.rgb+b.rgb*2-1)*factor,bandwidth);
ambient=float4(c.rgb*(0.25*factor),mask.y);
direction=bandwidth>0.00001?axis/bandwidth:float3(0,0,1);
return;
}}
lobe=0;ambient=float4({neutral:.9e},{neutral:.9e},{neutral:.9e},1);direction=float3(0,0,1);
if(pixel.y>={probe_row}&&pixel.y<{probe_end}){{
uint column=(uint)max(floor(pixel.x/3),0);uint row=(uint)floor((pixel.y-{probe_row})/3);
uint index=row*{columns}+column;
if(column<{columns}&&index<{probe_count}){{
int2 at=int2((index%{per_row})*3,index/{per_row});
lobe=LightProbes.Load(int3(at,0));ambient=LightProbes.Load(int3(at+int2(1,0),0));
float3 axis=LightProbes.Load(int3(at+int2(2,0),0)).xyz;
direction=normalize(axis+float3(0,0,0.00001));
}}
}}
}}
"#,
            neutral = 1. / scale,
            probe_row = self.probe_row,
            probe_end = self.probe_row + self.probe_count.div_ceil((width / 3).max(1)) * 3,
            columns = (width / 3).max(1),
            probe_count = self.probe_count,
            per_row = PROBES_PER_ROW,
        )
    }
}

//! Spatial baked colour in Shadowkeep's native cubemap modulation grid.
use crate::{
    halo_reach::native::lighting,
    presentation::{Graph, put},
    tiger::reader::Reader,
};
use anyhow::{Result, ensure};

const TEMPLATE: u32 = 0x80EC_454F;
const AMPLITUDE: f32 = 255.;
const CELL: f32 = 8.;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Grid {
    pub texture: String,
    pub dimensions: [u32; 3],
    pub strength: f32,
}

/// RGB has unit luminance. Magnitude stays exclusively in native surface/object visibility.
fn chroma(color: [f32; 3]) -> Option<[f32; 3]> {
    let light = lighting::luminance(color);
    (color.iter().all(|v| v.is_finite() && *v >= 0.) && light > 1e-6)
        .then(|| color.map(|v| v / light))
}

pub(super) struct Colors {
    samples: Vec<([f32; 3], [f32; 3])>,
    /// Each cube channel is normalized to this reference. The grid then multiplies it by
    /// 1..256. Channel-specific references keep the RGBA8 quantization proportional to range.
    pub baseline: [f32; 3],
}

impl Colors {
    pub fn new(samples: &[([f32; 3], [f32; 3])]) -> Self {
        let samples = samples
            .iter()
            .filter_map(|&(point, color)| {
                point.iter().all(|v| v.is_finite()).then_some(())?;
                Some((point, chroma(color)?))
            })
            .collect::<Vec<_>>();
        let mut maximum = [1f32; 3];
        for (_, color) in &samples {
            for k in 0..3 {
                maximum[k] = maximum[k].max(color[k]);
            }
        }
        Self {
            samples,
            baseline: maximum.map(|v| v / (1. + AMPLITUDE)),
        }
    }

    /// The nearest four floor samples inside the volume. Without an interior sample, use the
    /// nearest available floor colour. Cube bounds limit interpolation across unrelated rooms.
    fn sample(&self, point: [f32; 3], selected: &[usize]) -> [f32; 3] {
        let mut nearest = selected
            .iter()
            .map(|&i| {
                let distance = (0..3)
                    .map(|k| (self.samples[i].0[k] - point[k]).powi(2))
                    .sum::<f32>();
                (distance, i)
            })
            .collect::<Vec<_>>();
        if nearest.is_empty() {
            nearest.extend(self.samples.iter().enumerate().map(|(i, (position, _))| {
                (
                    (0..3)
                        .map(|k| (position[k] - point[k]).powi(2))
                        .sum::<f32>(),
                    i,
                )
            }));
            nearest.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
            return nearest.first().map_or([1.; 3], |&(_, i)| self.samples[i].1);
        }
        nearest.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let mut color = [0.; 3];
        let mut total = 0.;
        for &(distance, i) in nearest.iter().take(4) {
            let weight = 1. / distance.max(0.25);
            for (channel, sample) in color.iter_mut().zip(self.samples[i].1) {
                *channel += sample * weight;
            }
            total += weight;
        }
        color.map(|v| v / total)
    }

    pub fn write(
        &self,
        native: &mut Reader,
        graph: &mut Graph,
        symbol: &str,
        center: [f32; 3],
        extents: [f32; 3],
    ) -> Result<Grid> {
        ensure!(
            center.iter().all(|v| v.is_finite())
                && extents.iter().all(|v| v.is_finite() && *v > 0.),
            "Invalid cubemap grid bounds"
        );
        let dimensions = std::array::from_fn::<u32, 3, _>(|k| {
            ((2. * extents[k] / CELL).ceil() as u32).clamp(2, if k == 2 { 8 } else { 16 })
        });
        let selected = self
            .samples
            .iter()
            .enumerate()
            .filter(|(_, (point, _))| {
                (0..3).all(|k| (point[k] - center[k]).abs() <= extents[k] + 2.)
            })
            .map(|(i, _)| i)
            .collect::<Vec<_>>();
        let mut pixels = Vec::new();
        for z in 0..dimensions[2] {
            for y in 0..dimensions[1] {
                for x in 0..dimensions[0] {
                    let index = [x, y, z];
                    let point = std::array::from_fn(|k| {
                        center[k]
                            + ((index[k] as f32 + 0.5) / dimensions[k] as f32 * 2. - 1.)
                                * extents[k]
                    });
                    let color = self.sample(point, &selected);
                    for (channel, baseline) in color.into_iter().zip(self.baseline) {
                        pixels.push(
                            (((channel / baseline - 1.) / AMPLITUDE).clamp(0., 1.) * 255.).round()
                                as u8,
                        );
                    }
                    // Coverage belongs to volume geometry, not the source light's magnitude.
                    pixels.push(255);
                }
            }
        }
        let mut header = native.tag(TEMPLATE, None)?.0.clone();
        ensure!(header.len() == 40, "Unexpected native grid texture header");
        put(&mut header, 0, &u32::try_from(pixels.len())?.to_le_bytes())?;
        put(&mut header, 4, &28u32.to_le_bytes())?;
        for (k, size) in dimensions.iter().enumerate() {
            put(
                &mut header,
                14 + 2 * k,
                &u16::try_from(*size)?.to_le_bytes(),
            )?;
        }
        put(&mut header, 20, &1u16.to_le_bytes())?;
        header[23] = 1;
        put(&mut header, 36, &u32::MAX.to_le_bytes())?;
        crate::tiger::texture::resident(&mut header, pixels.len())?;
        let data = format!("{symbol}-data");
        graph.add(symbol, TEMPLATE, &header, Some(&data), vec![])?;
        graph.add(
            &data,
            native.reference(TEMPLATE)?,
            &pixels,
            Some(symbol),
            vec![],
        )?;
        Ok(Grid {
            texture: symbol.to_owned(),
            dimensions,
            strength: AMPLITUDE,
        })
    }
}

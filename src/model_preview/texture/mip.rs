//! Authored 2D levels and implicit footprints for recovered native gear bindings.
use super::*;

#[derive(Clone, Copy, Default)]
pub(in crate::model_preview) struct Footprint {
    pub dx: [f32; 2],
    pub dy: [f32; 2],
}

impl Footprint {
    pub fn scaled(self, scale: [f32; 2]) -> Self {
        Self {
            dx: std::array::from_fn(|i| self.dx[i] * scale[i]),
            dy: std::array::from_fn(|i| self.dy[i] * scale[i]),
        }
    }

    pub fn triangle(points: [[f32; 3]; 3], uv: [[f32; 2]; 3]) -> Self {
        let x = [points[1][0] - points[0][0], points[2][0] - points[0][0]];
        let y = [points[1][1] - points[0][1], points[2][1] - points[0][1]];
        let area = x[0] * y[1] - x[1] * y[0];
        if !area.is_finite() || area.abs() < 0.0001 {
            return Self::default();
        }
        let axis = |i| [uv[1][i] - uv[0][i], uv[2][i] - uv[0][i]];
        Self {
            dx: std::array::from_fn(|i| (axis(i)[0] * y[1] - axis(i)[1] * y[0]) / area),
            dy: std::array::from_fn(|i| (axis(i)[1] * x[0] - axis(i)[0] * x[1]) / area),
        }
    }

    fn taps(self, size: [usize; 2], anisotropy: u32) -> (f32, [f32; 2], usize) {
        let dx = std::array::from_fn(|i| self.dx[i] * size[i] as f32);
        let dy = std::array::from_fn(|i| self.dy[i] * size[i] as f32);
        let length = |v: [f32; 2]| v[0].hypot(v[1]);
        if anisotropy <= 1 {
            return (length(dx).max(length(dy)), [0.0; 2], 1);
        }
        // Principal axes of the texel footprint. Native driver tap placement can differ.
        let a = dx[0] * dx[0] + dy[0] * dy[0];
        let b = dx[0] * dx[1] + dy[0] * dy[1];
        let d = dx[1] * dx[1] + dy[1] * dy[1];
        let delta = (a - d).hypot(2.0 * b);
        let major = ((a + d + delta) * 0.5).max(0.0).sqrt();
        let minor = ((a + d - delta) * 0.5)
            .max(0.0)
            .sqrt()
            .max(major / anisotropy as f32);
        let taps = (major / minor.max(1e-8))
            .ceil()
            .clamp(1.0, anisotropy as f32) as usize;
        let angle = 0.5 * (2.0 * b).atan2(a - d);
        (
            minor,
            [
                angle.cos() * major / size[0] as f32,
                angle.sin() * major / size[1] as f32,
            ],
            taps,
        )
    }
}

fn length(format: u32, [width, height]: [usize; 2]) -> Result<usize, String> {
    Ok(match format {
        26 | 28 | 29 | 35 | 87 | 88 | 91 | 93 => width * height * 4,
        10 => width * height * 8,
        61 => width * height,
        71 | 72 | 80 => width.div_ceil(4) * height.div_ceil(4) * 8,
        74 | 75 | 77 | 78 | 83 | 98 | 99 => width.div_ceil(4) * height.div_ceil(4) * 16,
        _ => return Err(format!("Unsupported texture mip format {format}")),
    })
}

pub(super) fn decode(
    tag: u32,
    header: &[u8],
    bytes: &[u8],
    source: [usize; 2],
) -> Result<Texture, String> {
    let format = u32_at(header, 4)?;
    let declared = usize::from(*header.get(23).ok_or("Truncated texture mip count")?);
    let count = declared.max(1);
    if count > source[0].max(source[1]).ilog2() as usize + 1 {
        return Err("Invalid texture mip count".into());
    }
    let mut levels = Vec::new();
    let mut size = source;
    let mut offset = 0;
    for _ in 0..count {
        let end = offset + length(format, size)?;
        let pixels = bytes
            .get(offset..end)
            .ok_or("Texture mip is missing from the payload")?;
        if size[0] * size[1] <= MAX_PIXELS {
            levels.push(Texture {
                tag,
                size,
                rgba: super::decode(pixels, format, size[0], size[1])?,
                linear: float::decode(pixels, format, size[0], size[1])?,
                mips: None,
            });
        }
        offset = end;
        size = size.map(|v| (v / 2).max(1));
    }
    if levels.is_empty() {
        return Err("Preview mip is missing from the texture payload".into());
    }
    let mut base = levels.remove(0);
    base.mips = (declared > 0).then_some(levels);
    Ok(base)
}

impl Texture {
    pub(in crate::model_preview) fn level(&self, level: usize) -> &Self {
        if level == 0 {
            self
        } else {
            self.mips
                .as_ref()
                .and_then(|m| m.get(level - 1))
                .unwrap_or(self)
        }
    }

    pub(in crate::model_preview) fn sample_implicit(
        &self,
        uv: [f32; 2],
        sampler: &Sampler,
        footprint: Footprint,
        color: bool,
    ) -> [f32; 4] {
        let Some(filter) = sampler.filter else {
            return self.sample_filtered(uv, sampler, color);
        };
        let (radius, direction, taps) = footprint.taps(self.size, sampler.anisotropy);
        if !radius.is_finite() || direction.iter().any(|v| !v.is_finite()) {
            return self.sample_filtered(uv, sampler, color);
        }
        let requested = radius.max(1e-20).log2() + sampler.mip_bias;
        let lod = requested.clamp(sampler.lod[0], sampler.lod[1]);
        let bilinear = if lod <= 0.0 {
            filter & 4 != 0
        } else {
            filter & 16 != 0
        };
        let level = lod.clamp(0.0, self.mips.as_ref().map_or(0, Vec::len) as f32);
        let (low, high, blend) = if filter & 1 != 0 {
            (level.floor() as usize, level.ceil() as usize, level.fract())
        } else {
            (level.round() as usize, level.round() as usize, 0.0)
        };
        let mut result = [0.0; 4];
        for tap in 0..taps {
            let position = (tap as f32 + 0.5) / taps as f32 - 0.5;
            let uv = std::array::from_fn(|i| uv[i] + direction[i] * position);
            let a = self.level(low).sample_level(uv, sampler, color, bilinear);
            let b = self.level(high).sample_level(uv, sampler, color, bilinear);
            for i in 0..4 {
                result[i] += (a[i] * (1.0 - blend) + b[i] * blend) / taps as f32;
            }
        }
        result
    }

    /// Composed plates have no native payload for their complete canvas mip chain.
    pub(super) fn generate_mips(&mut self, color: bool) {
        let mut levels = Vec::new();
        while levels.last().unwrap_or(self).size != [1, 1] {
            levels.push(reduce(levels.last().unwrap_or(self), color));
        }
        self.mips = Some(levels);
    }
}

fn reduce(source: &Texture, color: bool) -> Texture {
    let size = source.size.map(|v| (v / 2).max(1));
    let mut result = Texture {
        tag: source.tag,
        size,
        rgba: Vec::with_capacity(size[0] * size[1] * 4),
        linear: source
            .linear
            .as_ref()
            .map(|_| Vec::with_capacity(size[0] * size[1])),
        mips: None,
    };
    for y in 0..size[1] {
        for x in 0..size[0] {
            let value = average(source, [x * 2, y * 2], color);
            if let Some(pixels) = &mut result.linear {
                pixels.push(value);
            }
            result
                .rgba
                .extend(inspection(value, color && source.linear.is_none()));
        }
    }
    result
}

fn average(source: &Texture, origin: [usize; 2], color: bool) -> [f32; 4] {
    static LINEAR: std::sync::OnceLock<[f32; 256]> = std::sync::OnceLock::new();
    let table = LINEAR.get_or_init(|| std::array::from_fn(|i| shader::linear(i as f32 / 255.0)));
    let mut result = [0.0; 4];
    for delta in [[0, 0], [1, 0], [0, 1], [1, 1]] {
        let [x, y] = std::array::from_fn(|i| (origin[i] + delta[i]).min(source.size[i] - 1));
        let at = y * source.size[0] + x;
        let pixel = source.linear.as_ref().map_or_else(
            || {
                std::array::from_fn(|lane| {
                    let value = source.rgba[at * 4 + lane];
                    if color && lane < 3 {
                        table[usize::from(value)]
                    } else {
                        f32::from(value) / 255.0
                    }
                })
            },
            |pixels| pixels[at],
        );
        for lane in 0..4 {
            result[lane] += pixel[lane] * 0.25;
        }
    }
    result
}

fn inspection(value: [f32; 4], color: bool) -> [u8; 4] {
    std::array::from_fn(|lane| {
        let value = value[lane];
        let value = if color && lane < 3 {
            if value <= 0.0031308 {
                value * 12.92
            } else {
                1.055 * value.powf(1.0 / 2.4) - 0.055
            }
        } else {
            value
        };
        (value.clamp(0.0, 1.0) * 255.0).round() as u8
    })
}

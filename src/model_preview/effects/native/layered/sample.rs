use super::*;
use texture::{AddressMode, Sampler};

#[derive(Clone, Copy)]
struct Filtering {
    color: bool,
    linear: bool,
}

impl Layered {
    pub(in super::super) fn lod(self, dx: [f32; 4], dy: [f32; 4]) -> f32 {
        let radius = |d: [f32; 4]| {
            (0..if self.volume { 3 } else { 2 })
                .map(|i| (d[i] * self.size[i] as f32).powi(2))
                .sum::<f32>()
                .sqrt()
        };
        radius(dx).max(radius(dy)).max(1e-20).log2()
    }

    pub(in super::super) fn sample(
        self,
        texture: &texture::Texture,
        uv: [f32; 4],
        sampler: &Sampler,
        sampling: evaluate::Sampling,
        offset: [i32; 3],
        color: bool,
    ) -> [f32; 4] {
        let (raw, direction, taps) = match sampling {
            evaluate::Sampling::Level(level) => (level, [0.0; 2], 1),
            evaluate::Sampling::Gradients { dx, dy } if !self.volume => {
                let (radius, direction, taps) = texture::Footprint {
                    dx: [dx[0], dx[1]],
                    dy: [dy[0], dy[1]],
                }
                .taps([self.size[0], self.size[1]], sampler.anisotropy);
                (radius.max(1e-20).log2(), direction, taps)
            }
            evaluate::Sampling::Gradients { dx, dy } => (self.lod(dx, dy), [0.0; 2], 1),
            evaluate::Sampling::Implicit => (0.0, [0.0; 2], 1),
        };
        let clamped = if raw.is_finite() {
            (raw + sampler.mip_bias).clamp(sampler.lod[0], sampler.lod[1])
        } else {
            0.0
        };
        let lod = clamped.clamp(0.0, self.levels as f32 - 1.0);
        let filter = sampler.filter.unwrap_or(21);
        let linear = filter & if clamped <= 0.0 { 4 } else { 16 } != 0;
        let (low, high, mix) = if filter & 1 != 0 {
            (lod.floor() as usize, lod.ceil() as usize, lod.fract())
        } else {
            (
                (lod + 0.5).floor() as usize,
                (lod + 0.5).floor() as usize,
                0.0,
            )
        };
        let mut value = [0.0; 4];
        for tap in 0..taps {
            let mut at = uv;
            let shift = (tap as f32 + 0.5) / taps as f32 - 0.5;
            for i in 0..2 {
                at[i] += direction[i] * shift;
            }
            let filtering = Filtering { color, linear };
            let a = self.sample_level(texture, at, low, sampler, offset, filtering);
            let b = self.sample_level(texture, at, high, sampler, offset, filtering);
            for i in 0..4 {
                value[i] += (a[i] + (b[i] - a[i]) * mix) / taps as f32;
            }
        }
        value
    }

    fn sample_level(
        self,
        texture: &texture::Texture,
        uv: [f32; 4],
        level: usize,
        sampler: &Sampler,
        offset: [i32; 3],
        filtering: Filtering,
    ) -> [f32; 4] {
        let border = sampler.border.map(|v| v / 255.0);
        if uv[..3].iter().any(|v| !v.is_finite()) {
            return border;
        }
        let size = self.level(level);
        let modes = [sampler.u, sampler.v, sampler.w];
        let p: [f32; 3] = std::array::from_fn(|i| {
            if i == 2 && !self.volume {
                uv[2].round_ties_even().clamp(0.0, size[2] as f32 - 1.0)
            } else {
                position(uv[i] + offset[i] as f32 / size[i] as f32, size[i], modes[i])
            }
        });
        let first = p.map(|v| v.floor() as i32);
        let t = p.map(|v| v - v.floor());
        let fetch = |delta: [i32; 3]| {
            let mut at = [0i32; 3];
            for i in 0..3 {
                let value = first[i] + delta[i];
                let Some(value) = address(
                    value,
                    size[i],
                    if i == 2 && !self.volume {
                        AddressMode::Clamp
                    } else {
                        modes[i]
                    },
                ) else {
                    return border;
                };
                at[i] = value;
            }
            self.texel(texture, level, at, filtering.color)
                .unwrap_or(border)
        };
        if !filtering.linear {
            return fetch(t.map(|v| i32::from(v >= 0.5)));
        }
        let mix =
            |a: [f32; 4], b: [f32; 4], t: f32| std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t);
        let slice = |z| {
            mix(
                mix(fetch([0, 0, z]), fetch([1, 0, z]), t[0]),
                mix(fetch([0, 1, z]), fetch([1, 1, z]), t[0]),
                t[1],
            )
        };
        if self.volume {
            mix(slice(0), slice(1), t[2])
        } else {
            slice(0)
        }
    }
}

fn position(uv: f32, size: usize, mode: AddressMode) -> f32 {
    let uv = match mode {
        AddressMode::Wrap => uv.rem_euclid(1.0),
        AddressMode::Mirror => uv.rem_euclid(2.0),
        AddressMode::Clamp => uv.clamp(0.0, 1.0),
        AddressMode::Border => uv.clamp(-1.0, 2.0),
        AddressMode::MirrorOnce => uv.abs().clamp(0.0, 1.0),
    };
    uv * size as f32 - 0.5
}

fn address(value: i32, size: usize, mode: AddressMode) -> Option<i32> {
    let size = size as i32;
    Some(match mode {
        AddressMode::Wrap => value.rem_euclid(size),
        AddressMode::Mirror => {
            let at = value.rem_euclid(size * 2);
            if at < size { at } else { size * 2 - 1 - at }
        }
        AddressMode::Clamp | AddressMode::MirrorOnce => value.clamp(0, size - 1),
        AddressMode::Border if (0..size).contains(&value) => value,
        AddressMode::Border => return None,
    })
}

pub(in super::super) const GLSL: &str = include_str!("sample.glsl");

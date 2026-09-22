//! Gear-material preview with evaluated native dye expressions and studio lighting.
//! Layout: Bungie-net/api/wiki/3D-Content-Documentation (2019).
//! Mask packing and blend/remap reference: TiredHobgoblin/Destiny-Collada-Generator,
//! Resources/template.shader. Detail UV scale: MontagueM/Charm EntityModel.cs.
use super::{Model, texture::Texture};
pub(super) mod normal;
use crate::weapon_dyes::material::Surface;

#[derive(Clone, Copy)]
pub(super) struct Dye {
    pub surface: Surface,
    pub detail: Option<usize>,
    pub transform: [f32; 4],
    pub normal: Option<usize>,
    pub normal_transform: [f32; 4],
}

pub(super) fn dyes(model: &Model, seconds: f32) -> [Option<Dye>; 6] {
    let mut dyes = model.dyes;
    for (slot, animation) in &model.dye_animations {
        let Ok(frame) = animation.at(seconds) else {
            continue;
        };
        for i in 0..2 {
            if let Some(dye) = &mut dyes[slot * 2 + i] {
                dye.surface = frame.surfaces[i];
                dye.transform = frame.detail_transform;
                dye.normal_transform = frame.normal_transform;
            }
        }
    }
    dyes
}

#[derive(Default)]
pub(super) struct Bindings<'a> {
    pub albedo: Option<&'a Texture>,
    /// Gearstack whose blue channel is coverage for an alpha-clipped part.
    pub clip: Option<&'a Texture>,
    /// Flat emissive colour that replaces every material term.
    pub constant: Option<[f32; 3]>,
    gearstack: Option<&'a Texture>,
    dye: Option<&'a Dye>,
    detail: Option<&'a Texture>,
    normal: Option<&'a Texture>,
    detail_normal: Option<&'a Texture>,
}

impl<'a> Bindings<'a> {
    pub fn new(model: &'a Model, triangle: usize, dyes: &'a [Option<Dye>; 6]) -> Self {
        let dye = model
            .triangle_dyes
            .get(triangle)
            .and_then(|&i| dyes.get(i as usize))
            .and_then(Option::as_ref);
        let texture = |indices: &[Option<usize>]| {
            indices
                .get(triangle)
                .copied()
                .flatten()
                .and_then(|i| model.textures.get(i))
        };
        let gearstack = texture(&model.triangle_gearstacks);
        Self {
            albedo: texture(&model.triangle_textures),
            clip: gearstack.filter(|_| model.triangle_clip.get(triangle).copied().unwrap_or(false)),
            constant: model.triangle_constant.get(triangle).copied().flatten(),
            gearstack,
            normal: texture(&model.triangle_normals),
            detail_normal: dye
                .and_then(|d| d.normal)
                .and_then(|i| model.textures.get(i)),
            detail: dye
                .and_then(|d| d.detail)
                .and_then(|i| model.textures.get(i)),
            dye,
        }
    }

    /// Only the clip texture, for styles that draw no material.
    pub fn clip_only(&self) -> Self {
        Self {
            clip: self.clip,
            ..Default::default()
        }
    }

    /// Whether an alpha-clipped part covers this texel. The template shader's rule:
    /// `saturate(gstack.b * 7.96875)` is the coverage, clipped at one half.
    pub fn covers(&self, uv: [f32; 2]) -> bool {
        self.clip
            .is_none_or(|texture| texture.sample_rgba(uv)[2] * 7.96875 / 255.0 >= 0.5)
    }

    pub fn tint(&self) -> Option<[f32; 3]> {
        self.dye.map(|d| d.surface.albedo)
    }

    pub fn shade(
        &self,
        uv: [f32; 2],
        geometric: [f32; 3],
        basis: Option<normal::Basis>,
    ) -> Option<[u8; 3]> {
        let base = self.albedo?.sample(uv).map(|v| linear(v / 255.0));
        let mask = self.gearstack.map(|t| t.sample_rgba(uv));
        let mut material = match (self.dye, mask) {
            (Some(dye), Some(mask)) => {
                let detail_uv =
                    std::array::from_fn(|i| uv[i] * 5.0 * dye.transform[i] + dye.transform[i + 2]);
                evaluate(
                    base,
                    mask,
                    self.detail.map(|t| t.sample_rgba(detail_uv)),
                    &dye.surface,
                )
            }
            _ if self.normal.is_some() => Sample {
                albedo: base,
                roughness: 0.6,
                metal: 0.0,
                ao: 1.0,
                emission: [0.0; 3],
            },
            _ => return None,
        };
        let mut normal = geometric;
        if let (Some(texture), Some(basis)) = (self.normal, basis) {
            let sampled = texture.sample_rgba(uv).map(|v| v / 255.0);
            let mut xy = [sampled[0], sampled[1]];
            material.ao *= sampled[2];
            if let (Some(dye), Some(detail), Some(mask)) = (self.dye, self.detail_normal, mask)
                && mask[3] >= 40.0
            {
                let intact = saturate(remap(saturate((mask[3] - 48.0) / 207.0), dye.surface.wear));
                let strength =
                    mix(dye.surface.worn_params[1], dye.surface.params[1], intact).clamp(0.0, 4.0);
                let uv = std::array::from_fn(|i| {
                    uv[i] * 5.0 * dye.normal_transform[i] + dye.normal_transform[i + 2]
                });
                let detail = detail.sample_rgba(uv).map(|v| v / 255.0);
                xy = std::array::from_fn(|i| {
                    let blended = if xy[i] < 0.5 {
                        2.0 * xy[i] * detail[i]
                    } else {
                        1.0 - 2.0 * (1.0 - xy[i]) * (1.0 - detail[i])
                    };
                    mix(xy[i], blended, strength)
                });
                material.ao *= mix(1.0, detail[2], strength.min(1.0));
            }
            normal = basis.apply(xy);
        }
        Some(light(material, normal).map(encode))
    }
}

struct Sample {
    albedo: [f32; 3],
    roughness: f32,
    metal: f32,
    ao: f32,
    emission: [f32; 3],
}

fn evaluate(base: [f32; 3], mask: [f32; 4], detail: Option<[f32; 4]>, dye: &Surface) -> Sample {
    let [ao, smoothness, emission, alpha] = mask;
    let mut sample = Sample {
        albedo: base,
        roughness: 1.0 - smoothness / 255.0,
        metal: saturate(alpha / 32.0),
        ao: saturate(ao / 255.0),
        emission: base.map(|v| v * saturate((emission - 40.0) / 215.0)),
    };
    // Values below 40 retain the original surface, including its encoded metalness.
    if alpha < 40.0 {
        return sample;
    }
    // High alpha is intact paint. Low dyeable alpha exposes the worn material.
    let intact = saturate(remap(saturate((alpha - 48.0) / 207.0), dye.wear));
    let params: [f32; 4] = std::array::from_fn(|i| mix(dye.worn_params[i], dye.params[i], intact));
    sample.albedo =
        std::array::from_fn(|i| overlay(base[i], mix(dye.worn_albedo[i], dye.albedo[i], intact)));
    let mut smoothness = smoothness / 255.0;
    if let Some(detail) = detail {
        sample.albedo = std::array::from_fn(|i| {
            mix(
                sample.albedo[i],
                overlay(linear(detail[i] / 255.0), sample.albedo[i]),
                saturate(params[0]),
            )
        });
        smoothness = mix(
            smoothness,
            overlay(smoothness, detail[3] / 255.0),
            saturate(params[2]),
        );
    }
    sample.roughness = 1.0
        - saturate(mix(
            remap(smoothness, dye.worn_roughness),
            remap(smoothness, dye.roughness),
            intact,
        ));
    sample.metal = saturate(params[3]);
    sample.emission = dye
        .emissive
        .map(|v| v * saturate((emission - 40.0) / 215.0));
    sample
}

fn remap(value: f32, map: [f32; 4]) -> f32 {
    let end = map[2] + map[3];
    (value * map[1] + map[0]).clamp(map[2].min(end), map[2].max(end))
}

fn overlay(base: f32, blend: f32) -> f32 {
    blend * saturate(base * 4.0) + saturate(base - 0.25)
}

fn mix(a: f32, b: f32, amount: f32) -> f32 {
    a + (b - a) * amount
}
fn saturate(value: f32) -> f32 {
    value.clamp(0.0, 1.0)
}

fn light(sample: Sample, mut n: [f32; 3]) -> [f32; 3] {
    // Two-sided studio lighting with a broad fill and a roughness-dependent highlight.
    // Kept independent of native game environments and exposure.
    if n[2] > 0.0 {
        n = n.map(|v| -v);
    }
    let diffuse = (n[0] * -0.35 + n[1] * -0.55 + n[2] * -0.76).max(0.0);
    let half = (n[0] * -0.187 + n[1] * -0.293 + n[2] * -0.937).max(0.0);
    let roughness = sample.roughness.clamp(0.06, 1.0);
    let exponent = (2.0 / roughness.powi(2) - 2.0).clamp(1.0, 512.0);
    let highlight = half.powf(exponent) * (1.2 - 0.8 * roughness);
    let fresnel = (1.0 - n[2].abs()).powi(5);
    // Occlusion darkens the fill and reflected environment, not the key light.
    std::array::from_fn(|i| {
        let base = sample.albedo[i];
        let specular = mix(0.04, base, sample.metal);
        let diffuse = base * (1.0 - sample.metal) * (0.30 * sample.ao + 0.70 * diffuse);
        let reflection =
            specular * (0.32 * sample.ao + highlight * 2.0) + fresnel * 0.18 * sample.ao;
        diffuse + reflection + sample.emission[i]
    })
}

const TABLE: usize = 1024;

fn table(f: fn(f32) -> f32) -> [f32; TABLE + 1] {
    std::array::from_fn(|i| f(i as f32 / TABLE as f32))
}

/// Linear interpolation over a table of `f` sampled at 1/1024 steps on 0..=1.
fn lookup(table: &[f32; TABLE + 1], value: f32) -> f32 {
    let scaled = saturate(value) * TABLE as f32;
    let index = (scaled as usize).min(TABLE - 1);
    let fraction = scaled - index as f32;
    table[index] + (table[index + 1] - table[index]) * fraction
}

fn linear_exact(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}
fn encode_exact(value: f32) -> f32 {
    if value <= 0.0031308 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    }
}

pub(super) fn linear(value: f32) -> f32 {
    static LINEAR: std::sync::OnceLock<[f32; TABLE + 1]> = std::sync::OnceLock::new();
    lookup(LINEAR.get_or_init(|| table(linear_exact)), value)
}
pub(super) fn encode(value: f32) -> u8 {
    static SRGB: std::sync::OnceLock<[f32; TABLE + 1]> = std::sync::OnceLock::new();
    (lookup(SRGB.get_or_init(|| table(encode_exact)), value) * 255.0).round() as u8
}

#[cfg(test)]
mod tests;

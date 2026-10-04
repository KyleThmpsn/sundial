//! Gear-material preview with evaluated native dye expressions and studio lighting.
//! Layout: Bungie-net/api/wiki/3D-Content-Documentation (2019).
//! Mask packing and blend/remap reference: TiredHobgoblin/Destiny-Collada-Generator,
//! Resources/template.shader. Detail UV scale: MontagueM/Charm EntityModel.cs.
use super::{
    Model,
    texture::{AddressMode, Sampler, Texture},
};
pub(super) mod normal;
use crate::dyes::material::{Frame, Surface, apply_writes, properties};

#[derive(Clone, Copy)]
pub(crate) struct Dye {
    pub surface: Surface,
    pub detail: Option<usize>,
    pub transform: [f32; 4],
    pub normal: Option<usize>,
    pub normal_transform: [f32; 4],
    /// The dye's 27 material vectors, which an editor's unbuilt values are written into.
    pub vectors: [[f32; 4]; 27],
}

pub(super) fn dyes(model: &Model, seconds: f32) -> [Option<Dye>; 6] {
    let overrides = model
        .surface_overrides
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    dyes_with_overrides(model, seconds, &overrides)
}

pub(super) fn dyes_with_overrides(
    model: &Model,
    seconds: f32,
    overrides: &[super::SurfaceOverride],
) -> [Option<Dye>; 6] {
    let mut dyes = model.dyes;
    // Gather sparse edits for each channel. Its animation applies them in native emission order.
    // Both of a channel's surfaces share one dye, so its writes gather from both.
    let writes = |channel: usize| -> Vec<(usize, usize, f32)> {
        overrides
            .iter()
            .filter(|surface| surface.slot / 2 == channel)
            .flat_map(|surface| surface.writes.iter().copied())
            .collect()
    };
    let show = |dyes: &mut [Option<Dye>; 6], channel: usize, frame: &Frame| {
        for i in 0..2 {
            if let Some(dye) = &mut dyes[channel * 2 + i] {
                dye.surface = frame.surfaces[i];
                dye.transform = frame.detail_transform;
                dye.normal_transform = frame.normal_transform;
                dye.vectors = frame.vectors;
            }
        }
    };
    for channel in 0..3 {
        let writes = writes(channel);
        if writes.is_empty()
            || model
                .dye_animations
                .iter()
                .any(|(slot, _)| *slot == channel)
        {
            continue;
        }
        let Some(mut vectors) = dyes[channel * 2]
            .or(dyes[channel * 2 + 1])
            .map(|dye| dye.vectors)
        else {
            continue;
        };
        apply_writes(&mut vectors, &writes);
        show(&mut dyes, channel, &properties(&vectors));
    }
    for (slot, animation) in &model.dye_animations {
        let writes = writes(*slot);
        let Ok(frame) = animation.at_edited(seconds, |vectors| apply_writes(vectors, &writes))
        else {
            continue;
        };
        show(&mut dyes, *slot, &frame);
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
    pub(in crate::model_preview) gearstack: Option<&'a Texture>,
    pub(in crate::model_preview) dye: Option<&'a Dye>,
    pub(in crate::model_preview) detail: Option<&'a Texture>,
    pub(in crate::model_preview) normal: Option<&'a Texture>,
    pub(in crate::model_preview) detail_normal: Option<&'a Texture>,
    iridescence: Option<&'a Texture>,
    dye_map: Option<(&'a Texture, super::texture::DyeMap, u8)>,
}

impl<'a> Bindings<'a> {
    /// The transparent gear programs output color and authored smoothness together.
    pub(super) fn effect_base(&self, uv: [f32; 2], detail_uv: [f32; 2]) -> [f32; 4] {
        let base = self.albedo.map_or([0.0; 4], |t| t.sample_color(uv));
        let mask = self.gearstack.map_or([0.0; 4], |t| t.sample_rgba(uv));
        let raw = mask[1] / 255.0;
        let Some(dye) = self.dye.filter(|_| mask[3] >= 40.0) else {
            return [base[0], base[1], base[2], raw];
        };
        let detail_uv =
            std::array::from_fn(|i| detail_uv[i] * dye.transform[i] + dye.transform[i + 2]);
        let detail = self
            .detail
            .map_or([0.25, 0.25, 0.25, 0.25], |t| t.sample_color(detail_uv));
        let map = |value: f32, m: [f32; 4]| saturate(m[2] + m[3] * saturate(m[0] + m[1] * value));
        let surface = |color: [f32; 3], params: [f32; 4], rough: [f32; 4]| {
            let mapped = map(raw, rough);
            let color: [f32; 3] = std::array::from_fn(|i| {
                mix(color[i], saturate(overlay(detail[i], color[i])), params[0])
            });
            [
                overlay(base[0], color[0]),
                overlay(base[1], color[1]),
                overlay(base[2], color[2]),
                mix(mapped, map(overlay(mapped, detail[3]), rough), params[2]),
            ]
        };
        let s = &dye.surface;
        let intact = map(saturate((mask[3] - 48.0) / 207.0), s.wear);
        let a = surface(s.worn_albedo, s.worn_params.map(saturate), s.worn_roughness);
        let b = surface(s.albedo, s.params, s.roughness);
        std::array::from_fn(|i| mix(a[i], b[i], intact))
    }

    pub(super) fn effect_plate(&self, uv: [f32; 2]) -> [f32; 4] {
        self.albedo.map_or([0.0; 4], |t| t.sample_color(uv))
    }

    pub(super) fn effect_mask(&self, uv: [f32; 2]) -> [f32; 4] {
        self.gearstack
            .map_or([0.0; 4], |t| t.sample_rgba(uv).map(|v| v / 255.0))
    }
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
            iridescence: model.iridescence.as_ref(),
            dye_map: model
                .triangle_dye_maps
                .get(triangle)
                .copied()
                .flatten()
                .and_then(|map| {
                    Some((
                        model.textures.get(map.texture)?,
                        map,
                        *model.triangle_dyes.get(triangle)?,
                    ))
                }),
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
        self.dye_map.is_none_or(|(texture, map, slot)| {
            super::texture::DyeMap::slot(texture.sample_rgba(map.uv(uv))) == slot
        }) && self
            .clip
            .is_none_or(|texture| texture.sample_rgba(uv)[2] * 7.96875 / 255.0 >= 0.5)
    }

    pub fn tint(&self) -> Option<[f32; 3]> {
        self.dye.map(|d| d.surface.albedo)
    }

    /// Continuous coverage for an alpha-clipped part, before the one-half clip `covers` applies.
    pub fn coverage(&self, uv: [f32; 2]) -> Option<f32> {
        if self.dye_map.is_some() && !self.covers(uv) {
            return Some(0.0);
        }
        self.clip
            .map(|texture| saturate(texture.sample_rgba(uv)[2] * 7.96875 / 255.0))
            .or_else(|| self.dye_map.map(|_| 1.0))
    }

    /// The unlit material at one point of the plate. The preview lights this, and an export
    /// bakes it, so the two share one evaluation and cannot drift apart.
    pub fn texel(&self, uv: [f32; 2]) -> Option<Texel> {
        let color = self.albedo?.sample_color(uv);
        let base = [color[0], color[1], color[2]];
        let mask = self.gearstack.map(|t| t.sample_rgba(uv));
        let surface = match (self.dye, mask) {
            (Some(dye), Some(mask)) => {
                let detail_uv =
                    std::array::from_fn(|i| uv[i] * 5.0 * dye.transform[i] + dye.transform[i + 2]);
                evaluate(
                    base,
                    mask,
                    self.detail.map(|t| t.sample_color(detail_uv)),
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
        let normal = self.normal.map(|texture| {
            let sampled = texture.sample_rgba(uv).map(|v| v / 255.0);
            let mut packed = [sampled[0], sampled[1]];
            // The map's blue channel is occlusion, not the normal's third axis.
            let mut occlusion = [sampled[2], 1.0];
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
                packed = std::array::from_fn(|i| {
                    let blended = if packed[i] < 0.5 {
                        2.0 * packed[i] * detail[i]
                    } else {
                        1.0 - 2.0 * (1.0 - packed[i]) * (1.0 - detail[i])
                    };
                    mix(packed[i], blended, strength)
                });
                occlusion[1] = mix(1.0, detail[2], strength.min(1.0));
            }
            NormalTexel { packed, occlusion }
        });
        Some(Texel { surface, normal })
    }

    pub fn shade(
        &self,
        uv: [f32; 2],
        geometric: [f32; 3],
        basis: Option<normal::Basis>,
        scene: super::render::Scene,
    ) -> Option<[u8; 3]> {
        let Texel {
            mut surface,
            normal: map,
        } = self.texel(uv)?;
        let mut normal = geometric;
        if let (Some(map), Some(basis)) = (map, basis) {
            // Applied one after the other, as they always were, so the preview stays exact.
            surface.ao *= map.occlusion[0];
            surface.ao *= map.occlusion[1];
            normal = basis.apply(map.packed);
        }
        let tint = self.apply_iridescence(uv, normal, &mut surface);
        Some(light(surface, normal, scene, tint).map(encode))
    }

    /// The GPU preview's iridescence: the lookup row the dye names, read along the row by view
    /// angle and weighted by how dark the dye is. An even row tints the colour and makes it
    /// metal. An odd row tints the highlight, which this returns.
    fn apply_iridescence(&self, uv: [f32; 2], normal: [f32; 3], surface: &mut Sample) -> [f32; 3] {
        let (Some(dye), Some(gearstack), Some(lookup)) =
            (self.dye, self.gearstack, self.iridescence)
        else {
            return [1.0; 3];
        };
        let id = dye.surface.iridescence;
        let alpha = gearstack.sample_rgba(uv)[3];
        if id < 0.0 || alpha < 40.0 {
            return [1.0; 3];
        }
        let intact = saturate(remap(saturate((alpha - 48.0) / 207.0), dye.surface.wear));
        let color: [f32; 3] =
            std::array::from_fn(|i| mix(dye.surface.worn_albedo[i], dye.surface.albedo[i], intact));
        let clamped = Sampler {
            u: AddressMode::Clamp,
            v: AddressMode::Clamp,
            border: [0.0; 4],
        };
        let rows = lookup.size[1].max(1) as f32;
        let sample = lookup
            .sample_with_sampler([saturate(normal[2].abs()), (id + 0.5) / rows], &clamped)
            .map(|value| value / 255.0);
        // Rows past the authored ones hold a magenta placeholder the game skips.
        if sample[0] > 0.98 && sample[1] < 0.02 && sample[2] > 0.98 {
            return [1.0; 3];
        }
        let strength = 1.0 - (0.2126 * color[0] + 0.7152 * color[1] + 0.0722 * color[2]);
        if id.rem_euclid(2.0) < 0.5 {
            surface.albedo = std::array::from_fn(|i| mix(surface.albedo[i], sample[i], strength));
            surface.metal = mix(surface.metal, 1.0, strength);
            [1.0; 3]
        } else {
            std::array::from_fn(|i| mix(1.0, sample[i], strength))
        }
    }
}

/// The unlit material at one texel, with the normal map's contribution kept apart because
/// the preview can only apply it where a triangle has a usable tangent basis.
pub(super) struct Texel {
    pub surface: Sample,
    pub normal: Option<NormalTexel>,
}

/// The normal map's packed tangent-space X and Y, after any detail blending, in the native UV
/// convention. The occlusion factors it carries are applied in this order.
pub(super) struct NormalTexel {
    pub packed: [f32; 2],
    pub occlusion: [f32; 2],
}

/// Linear colour terms and the scalar channels a PBR material carries.
pub(super) struct Sample {
    pub albedo: [f32; 3],
    pub roughness: f32,
    pub metal: f32,
    pub ao: f32,
    pub emission: [f32; 3],
}

/// Color inputs are filtered linear light, mask channels retain their native 0..255 packing.
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
                overlay(detail[i], sample.albedo[i]),
                saturate(params[0]),
            )
        });
        smoothness = mix(
            smoothness,
            overlay(smoothness, detail[3]),
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

/// `tint` colours the highlight, as an odd iridescence row does.
fn light(sample: Sample, mut n: [f32; 3], scene: super::render::Scene, tint: [f32; 3]) -> [f32; 3] {
    // Two-sided studio lighting with a broad fill and a roughness-dependent highlight.
    // Kept independent of native game environments; the viewer's own exposure is applied last.
    if n[2] > 0.0 {
        n = n.map(|v| -v);
    }
    let key = scene.raster_light();
    // The highlight tracks the key, halfway between it and the fixed head-on view direction.
    let half_vector = {
        let sum = [key[0], key[1], key[2] - 1.0];
        let length = sum.iter().map(|v| v * v).sum::<f32>().sqrt().max(0.0001);
        sum.map(|v| v / length)
    };
    let diffuse = (n[0] * key[0] + n[1] * key[1] + n[2] * key[2]).max(0.0);
    let half = (n[0] * half_vector[0] + n[1] * half_vector[1] + n[2] * half_vector[2]).max(0.0);
    let roughness = sample.roughness.clamp(0.06, 1.0);
    // The key light has a size, so even a mirror finish shows a highlight rather than a point.
    let glint = roughness.max(GLINT_ROUGHNESS);
    let exponent = (2.0 / glint.powi(2) - 2.0).clamp(1.0, 512.0);
    let highlight = half.powf(exponent) * (1.2 - 0.8 * glint);
    let fresnel = (1.0 - n[2].abs()).powi(5);
    let surroundings = studio(n, key, roughness);
    // Occlusion darkens the fill and reflected environment, not the key light.
    std::array::from_fn(|i| {
        let base = sample.albedo[i];
        let specular = mix(0.04, base, sample.metal);
        let diffuse = base * (1.0 - sample.metal) * (scene.fill * sample.ao + scene.key * diffuse);
        let reflection = tint[i] * specular * (surroundings * sample.ao + highlight * 2.0)
            + fresnel * 0.18 * sample.ao;
        (diffuse + reflection + sample.emission[i]) * scene.exposure
    })
}

/// The least roughness the key light's highlight shows with, standing for the light's size.
const GLINT_ROUGHNESS: f32 = 0.2;
/// What a rough surface reflects: the flat level the preview reflected everywhere before the
/// studio, so rough finishes and paint look as they did.
const FLAT_SURROUNDINGS: f32 = 0.32;

/// The studio a surface reflects, seen head-on: brighter above than below, a softbox where the key
/// light hangs, and soft clouds. Smooth finishes see it sharply, which is what makes metal read as
/// metal and shows its smoothness and normal detail. Roughness blurs it back to the flat level.
/// `n` faces the viewer (negative z) and `key` is the raster light, whose y points down. The GPU
/// preview's `studio` mirrors this exactly.
fn studio(n: [f32; 3], key: [f32; 3], roughness: f32) -> f32 {
    let facing = -n[2];
    let reflected = [
        2.0 * facing * n[0],
        2.0 * facing * n[1],
        2.0 * facing * n[2] + 1.0,
    ];
    let height = -reflected[1];
    let gradient = mix(0.08, 0.6, saturate(0.5 + 0.6 * height));
    let toward_key = reflected[0] * key[0] + reflected[1] * key[1] + reflected[2] * key[2];
    let softbox = 1.4 * (-(1.0 - toward_key) / 0.06).exp();
    let clouds = (value_noise(reflected[0] * 3.0 + 7.0, -height * 3.0 + 7.0) - 0.5) * 0.204;
    let sharp = (gradient + softbox + clouds).max(0.0);
    mix(sharp, FLAT_SURROUNDINGS, saturate(roughness * 2.5))
}

/// Smoothly interpolated lattice noise from 0 to 1. The GPU preview's `valueNoise` mirrors it.
fn value_noise(x: f32, y: f32) -> f32 {
    let hash = |x: i32, y: i32| {
        let mut value = (x as u32).wrapping_mul(0x8DA6_B343)
            ^ (y as u32).wrapping_mul(0xD816_3841)
            ^ 0x2C1B_3C6D;
        value ^= value >> 13;
        value = value.wrapping_mul(0x5BD1_E995);
        value ^= value >> 15;
        (value & 0xFFFF) as f32 / 65_535.0
    };
    let (left, top) = (x.floor(), y.floor());
    let smooth = |t: f32| t * t * (3.0 - 2.0 * t);
    let (fx, fy) = (smooth(x - left), smooth(y - top));
    let (column, row) = (left as i32, top as i32);
    mix(
        mix(hash(column, row), hash(column + 1, row), fx),
        mix(hash(column, row + 1), hash(column + 1, row + 1), fx),
        fy,
    )
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
    (encoded(value) * 255.0).round() as u8
}

pub(super) fn encoded(value: f32) -> f32 {
    static SRGB: std::sync::OnceLock<[f32; TABLE + 1]> = std::sync::OnceLock::new();
    lookup(SRGB.get_or_init(|| table(encode_exact)), value)
}

#[cfg(test)]
mod tests;

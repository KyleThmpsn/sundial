//! CPU study of one packaged additive particle pixel shader. Runtime spawn and
//! engine constant buffers still have to be recovered before native playback.
use super::{
    Model, assets,
    texture::{Sampler, Texture},
};

pub(super) struct State<'a> {
    distortion: &'a Texture,
    mask: &'a Texture,
    ramp: &'a Texture,
    distortion_sampler: &'a Sampler,
    mask_sampler: &'a Sampler,
    ramp_sampler: &'a Sampler,
    transforms: [[f32; 4]; 3],
    ramp_control: [f32; 4],
    intensity: [f32; 4],
    visible: bool,
}

pub(super) fn prepare(model: &Model, seconds: f32) -> Option<State<'_>> {
    let [particle] = model.assets.particles.as_slice() else {
        return None;
    };
    if particle.pixel_kind != Some(assets::PixelKind::DualMaskRamp)
        || !model.particle_geometry
        || !model.particle_sources.is_empty()
    {
        return None;
    }
    let texture = |slot| {
        particle
            .material_textures
            .iter()
            .find(|(index, _)| *index == slot)
            .map(|(_, texture)| texture)
    };
    let [distortion_sampler, mask_sampler, ramp_sampler, ..] =
        particle.material_samplers.as_slice()
    else {
        return None;
    };
    let program = particle.program.as_ref()?;
    let lifetime = program.lifetime_default()?;
    let age = (seconds / lifetime).max(0.0);
    let mut registers = assets::Registers::new(program);
    registers.set(1, 3, [0.0, 0.0, 0.0, age]).ok()?;
    registers.set(1, 6, [0.0, 0.0, 0.0, 0.5]).ok()?;
    registers.set(1, 7, [0.5, 0.0, 0.0, 0.0]).ok()?;
    program.evaluate_section(4, &mut registers).ok()?;
    Some(State {
        distortion: texture(0)?,
        mask: texture(1)?,
        ramp: texture(2)?,
        distortion_sampler,
        mask_sampler,
        ramp_sampler,
        transforms: [
            registers.get(1, 0)?,
            registers.get(1, 1)?,
            registers.get(1, 2)?,
        ],
        ramp_control: registers.get(1, 5)?,
        intensity: registers.get(1, 6)?,
        visible: age < 0.999,
    })
}

impl State<'_> {
    pub(super) fn sample(&self, uv: [f32; 2], exposure: f32) -> [f32; 3] {
        if !self.visible {
            return [0.0; 3];
        }
        let [second, distortion, first] = self.transforms;
        let distortion_uv = [
            uv[0] * distortion[0] + distortion[2],
            uv[1] * distortion[1] + distortion[3],
        ];
        let noise = self
            .distortion
            .sample_with_sampler(distortion_uv, self.distortion_sampler);
        let first_uv = [
            uv[0] * first[0] + first[2] + noise[0] / 255.0 * self.ramp_control[3],
            uv[1] * first[1] + first[3] + (noise[1] / 255.0 - 0.2) * self.ramp_control[3],
        ];
        let second_uv = [uv[0] * second[0] + second[2], uv[1] * second[1] + second[3]];
        let a = self.mask.sample_with_sampler(first_uv, self.mask_sampler)[0] / 255.0;
        let b = self.mask.sample_with_sampler(second_uv, self.mask_sampler)[1] / 255.0;
        let ramp_u = (a * b * self.ramp_control[0] + self.ramp_control[1]).clamp(0.0, 1.0);
        let color = self
            .ramp
            .sample_with_sampler([ramp_u, 0.0], self.ramp_sampler);
        let edge = (1.0 - (uv[0] - 0.5).abs() * 2.222_222).max(0.0).powi(2);
        let strength = edge * self.intensity[1] * self.intensity[2] * exposure * 0.02;
        [
            color[0] * strength,
            color[1] * strength,
            color[2] * strength,
        ]
    }
}

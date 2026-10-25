//! Recovered particle instance and pixel contracts in the software studio view.
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

pub(super) struct Batch<'a> {
    pub states: Vec<State<'a>>,
    instances: Option<&'a [super::particles::simulation::Instance]>,
}

impl Batch<'_> {
    /// One compact record per visible or retired instance. Mesh vertices stay on the GPU.
    pub(super) fn gpu_instances(&self) -> Vec<[[f32; 4]; 7]> {
        self.states
            .iter()
            .enumerate()
            .map(|(index, state)| {
                [
                    state.transforms[0],
                    state.transforms[1],
                    state.transforms[2],
                    self.instances
                        .map_or([0.0; 4], |instances| instances[index].attributes[4]),
                    state.ramp_control,
                    state.intensity,
                    [
                        f32::from(u8::from(state.visible)),
                        f32::from(u8::from(self.instances.is_some())),
                        0.0,
                        0.0,
                    ],
                ]
            })
            .collect()
    }

    pub(super) fn position(&self, instance: usize, point: [f32; 3]) -> [f32; 3] {
        let Some(instances) = self.instances else {
            return point;
        };
        let [x, y, z, angle] = instances[instance].attributes[4];
        let (sin, cos) = angle.sin_cos();
        [
            x + point[0] * 0.1,
            y + (cos * point[1] - sin * point[2]) * 0.1,
            z + (sin * point[1] + cos * point[2]) * 0.1,
        ]
    }
}

pub(super) fn prepare(model: &Model, seconds: f32) -> Option<Batch<'_>> {
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
    let distortion = texture(0)?;
    let mask = texture(1)?;
    let ramp = texture(2)?;
    let state = |attributes: [[f32; 4]; 7]| State {
        distortion,
        mask,
        ramp,
        distortion_sampler,
        mask_sampler,
        ramp_sampler,
        transforms: [attributes[0], attributes[1], attributes[2]],
        ramp_control: attributes[5],
        intensity: attributes[6],
        visible: attributes[3][3] <= 0.999 && attributes[6][2] >= 0.000001,
    };
    if let Some(simulation) = model.particle_simulation() {
        let instances = simulation.at(seconds);
        return Some(Batch {
            states: instances
                .iter()
                .map(|item| state(item.attributes))
                .collect(),
            instances: Some(instances),
        });
    }
    let program = particle.program.as_ref()?;
    let lifetime = program.lifetime_default()?;
    let age = (seconds / lifetime).max(0.0);
    let mut registers = assets::Registers::new(program);
    registers.set(1, 3, [0.0, 0.0, 0.0, age]).ok()?;
    registers.set(1, 6, [0.0, 0.0, 0.0, 0.5]).ok()?;
    registers.set(1, 7, [0.5, 0.0, 0.0, 0.0]).ok()?;
    program.evaluate_section(4, &mut registers).ok()?;
    let attributes = std::array::from_fn(|index| registers.get(1, index as u8).unwrap());
    let mut material = state(attributes);
    material.visible &= age <= 0.999;
    Some(Batch {
        states: vec![material],
        instances: None,
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
        // The native sample writes destination Y using resource X. Both UVs read red.
        let b = self.mask.sample_with_sampler(second_uv, self.mask_sampler)[0] / 255.0;
        let ramp_u = (a * b * self.ramp_control[0] + self.ramp_control[1]).clamp(0.0, 1.0);
        let color = self
            .ramp
            .sample_with_sampler([ramp_u, 0.0], self.ramp_sampler);
        let edge = (1.0 - (uv[0] - 0.5).abs() * 2.222_222).max(0.0).powi(2);
        let coverage = (edge * self.intensity[2]).clamp(0.0, 1.0);
        let strength = coverage * self.intensity[1] * exposure * 0.02;
        [
            color[0] * strength,
            color[1] * strength,
            color[2] * strength,
        ]
    }
}

use super::*;

#[derive(Clone, Copy)]
pub(in crate::model_preview) struct Depth<'a> {
    pub values: &'a [f32],
    pub size: [usize; 2],
    pub top: usize,
    pub scale: f32,
}

impl Depth<'_> {
    fn at(self, x: i32, y: i32) -> f32 {
        if x < 0 || x >= self.size[0] as i32 || y < self.top as i32 || y >= self.size[1] as i32 {
            return f32::INFINITY;
        }
        self.values
            .get((y as usize - self.top) * self.size[0] + x as usize)
            .copied()
            .unwrap_or(f32::INFINITY)
    }
}

#[derive(Clone, Copy)]
pub(in crate::model_preview) struct Pixel<'a> {
    pub varyings: Varyings,
    pub dx: Varyings,
    pub dy: Varyings,
    pub screen: [f32; 3],
    pub direction: [f32; 3],
    pub distance: f32,
    pub front: bool,
    pub depth: Depth<'a>,
    pub exposure: f32,
}

struct Context<'a, 'b> {
    model: &'a Model,
    material: &'a Material,
    native: &'a Native,
    constants: &'a Frame,
    gear: &'a shader::Bindings<'b>,
    pixel: Pixel<'a>,
}

impl Context<'_, '_> {
    fn texture(&self, role: Role) -> Option<&texture::Texture> {
        match role {
            Role::Texture(index) => self.model.textures.get(index),
            Role::Albedo => self.gear.albedo,
            Role::Normal => self.gear.normal,
            Role::Gear => self.gear.gearstack,
            Role::Detail => self.gear.detail,
            Role::DetailNormal => self.gear.detail_normal,
            Role::Depth | Role::Mask | Role::Scene => None,
        }
    }

    fn input_value(&self, register: usize, varying: &Varyings) -> [u32; 4] {
        if let Some(semantic) = self
            .native
            .pixel
            .inputs
            .iter()
            .find(|s| s.register == register)
        {
            match semantic.system {
                1 => {
                    return [self.pixel.screen[0], self.pixel.screen[1], 0.0, 1.0]
                        .map(f32::to_bits);
                }
                9 => return [if self.pixel.front { u32::MAX } else { 0 }; 4],
                _ => {
                    return varying
                        .get(semantic.index as usize)
                        .copied()
                        .unwrap_or([0.0; 4])
                        .map(f32::to_bits);
                }
            }
        }
        varying
            .get(register)
            .copied()
            .unwrap_or([0.0; 4])
            .map(f32::to_bits)
    }
}

impl evaluate::Context for Context<'_, '_> {
    fn size(&self, resource: usize, mip: u32) -> [u32; 4] {
        let Some(binding) = self
            .native
            .bindings
            .iter()
            .find(|b| !b.vertex && b.slot == resource)
        else {
            return [0; 4];
        };
        if matches!(binding.role, Role::Scene) {
            return if binding.slot == 15 { [0; 4] } else { [1; 4] };
        }
        self.texture(binding.role).map_or([0; 4], |texture| {
            image::Image { texture, binding }.size(mip)
        })
    }

    fn input(&self, register: usize) -> [u32; 4] {
        if register == 15
            && let Some(t) = self.native.opaque_uv
        {
            let uv = std::array::from_fn(|i| (self.pixel.varyings[3][i] - t[i + 2]) / t[i]);
            let detail = [self.pixel.varyings[3][2], self.pixel.varyings[3][3]];
            let rgb = self.gear.base_color(uv, detail);
            return [rgb[0], rgb[1], rgb[2], 1.0].map(f32::to_bits);
        }
        self.input_value(register, &self.pixel.varyings)
    }
    fn constant(&self, buffer: usize, index: usize) -> [u32; 4] {
        let p = &self.pixel;
        let value = match (buffer, index) {
            (0, i) => self.constants.get(i).copied().unwrap_or([0.0; 4]),
            (5..=7, i) => self
                .gear
                .dye
                .and_then(|d| d.vectors.get(i))
                .copied()
                .unwrap_or([0.0; 4]),
            (2, 0) => [0.0, 1.0, 0.0, 0.0],
            (13, 1) => [1.0; 4],
            (12, 6) => [p.direction[0], p.direction[1], p.direction[2], 0.0],
            (12, 7) => [
                p.varyings[4][0] + p.direction[0] * p.distance,
                p.varyings[4][1] + p.direction[1] * p.distance,
                p.varyings[4][2] + p.direction[2] * p.distance,
                1.0,
            ],
            (12, 12) => [
                p.depth.size[0] as f32,
                p.depth.size[1] as f32,
                1.0 / p.depth.size[0] as f32,
                1.0 / p.depth.size[1] as f32,
            ],
            _ => [0.0; 4],
        };
        value.map(f32::to_bits)
    }
    fn sample(
        &self,
        resource: usize,
        sampler: usize,
        uv: [f32; 4],
        sampling: evaluate::Sampling,
        offset: [i32; 3],
    ) -> [u32; 4] {
        let Some(binding) = self
            .native
            .bindings
            .iter()
            .find(|b| !b.vertex && b.slot == resource)
        else {
            return [0; 4];
        };
        let Some(texture) = self.texture(binding.role) else {
            return match binding.role {
                Role::Normal | Role::DetailNormal => [0.5f32, 0.5, 1.0, 1.0].map(f32::to_bits),
                Role::Detail => [0.25f32; 4].map(f32::to_bits),
                Role::Scene if binding.slot == 16 => [1.0f32; 4].map(f32::to_bits),
                _ => [0; 4],
            };
        };
        image::Image { texture, binding }.sample(
            uv,
            sampler
                .checked_sub(1)
                .and_then(|i| self.material.samplers.get(i)),
            sampling,
            offset,
        )
    }

    fn load(&self, resource: usize, position: [i32; 4], offset: [i32; 3]) -> [u32; 4] {
        let Some(binding) = self
            .native
            .bindings
            .iter()
            .find(|b| !b.vertex && b.slot == resource)
        else {
            return [0; 4];
        };
        let depth = self.pixel.depth.at(
            position[0].saturating_add(offset[0]),
            position[1].saturating_add(offset[1]),
        );
        match binding.role {
            Role::Mask => [if depth.is_finite() { 8 } else { 0 }; 4],
            Role::Depth => {
                let gap = ((depth - self.pixel.screen[2]) / self.pixel.depth.scale).clamp(0.0, 1e6);
                [(self.pixel.distance + gap).max(1e-6).recip().to_bits(); 4]
            }
            _ => self.texture(binding.role).map_or([0; 4], |texture| {
                image::Image { texture, binding }.load(position, offset)
            }),
        }
    }

    fn lod(&self, resource: usize, coordinates: [f32; 4]) -> [f32; 4] {
        let Some(cube) = self
            .native
            .bindings
            .iter()
            .find(|b| !b.vertex && b.slot == resource)
            .and_then(|b| b.cube)
        else {
            return [0.0; 4];
        };
        let neighbour = |vertical: bool| {
            let axis = usize::from(vertical);
            let offset = if self.pixel.screen[axis].floor() as i32 & 1 == 0 {
                1.0
            } else {
                -1.0
            };
            let mut pixel = self.pixel;
            let delta = if vertical { pixel.dy } else { pixel.dx };
            for (varying, delta) in pixel.varyings.iter_mut().zip(delta) {
                for (value, delta) in varying.iter_mut().zip(delta) {
                    *value += delta * offset;
                }
            }
            pixel.screen[axis] += offset;
            let other = Context {
                model: self.model,
                material: self.material,
                native: self.native,
                constants: self.constants,
                gear: self.gear,
                pixel,
            };
            let value = self
                .native
                .pixel
                .lod_coordinates(&other)
                .unwrap_or(coordinates);
            std::array::from_fn(|i| (value[i] - coordinates[i]) * offset)
        };
        cube.lod(
            [coordinates[0], coordinates[1], coordinates[2]],
            neighbour(false),
            neighbour(true),
        )
    }
    fn derivative(&self, input: &program::Operand, y: bool) -> [f32; 4] {
        let register = input.indices[0].base as usize;
        let values = if y { &self.pixel.dy } else { &self.pixel.dx };
        let raw = self.input_value(register, values).map(f32::from_bits);
        let actual = self
            .input_value(register, &self.pixel.varyings)
            .map(f32::from_bits);
        std::array::from_fn(|i| {
            let value = raw[input.lanes[i]];
            match input.modifier {
                1 => -value,
                2 => value * actual[input.lanes[i]].signum(),
                3 => -value * actual[input.lanes[i]].signum(),
                _ => value,
            }
        })
    }
}

pub(in crate::model_preview) fn sample(
    model: &Model,
    material: &Material,
    constants: &Frame,
    gear: &shader::Bindings<'_>,
    pixel: Pixel<'_>,
) -> [f32; 4] {
    sample_with_ambient(model, material, constants, gear, pixel).0
}

pub(in crate::model_preview) fn sample_with_ambient(
    model: &Model,
    material: &Material,
    constants: &Frame,
    gear: &shader::Bindings<'_>,
    pixel: Pixel<'_>,
) -> ([f32; 4], Option<f32>) {
    let Some(native) = &material.native else {
        return ([0.0; 4], None);
    };
    let exposure = if native.opaque() { 1.0 } else { pixel.exposure };
    let Some(outputs) = outputs(model, material, constants, gear, pixel) else {
        return ([0.0; 4], None);
    };
    let mut output = outputs[0];
    for value in &mut output[..3] {
        *value = (*value * exposure).max(0.0);
    }
    output[3] = if native.opaque() {
        if native.intensity {
            super::decode_intensity(outputs[2][1])
        } else {
            1.0
        }
    } else {
        output[3].clamp(0.0, 1.0)
    };
    if output.iter().any(|v| !v.is_finite()) {
        ([0.0; 4], None)
    } else {
        let ambient = native
            .ambient_power
            .filter(|_| outputs[2][1].is_finite() && outputs[2][3].is_finite())
            .map(|power| super::ambient_visibility(outputs[2][1], outputs[2][3], power));
        (output, ambient)
    }
}

pub(super) fn outputs(
    model: &Model,
    material: &Material,
    constants: &Frame,
    gear: &shader::Bindings<'_>,
    pixel: Pixel<'_>,
) -> Option<[[f32; 4]; 16]> {
    let native = material.native.as_ref()?;
    let gear = gear.with_native(native, constants);
    native.pixel.evaluate(&Context {
        model,
        material,
        native,
        constants,
        gear: &gear,
        pixel,
    })
}

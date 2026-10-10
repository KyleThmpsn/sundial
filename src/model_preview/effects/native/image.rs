//! The same explicit image reads serve vertex and pixel stages.
use super::*;

pub(super) struct Image<'a> {
    pub texture: &'a texture::Texture,
    pub binding: &'a Binding,
}

impl Image<'_> {
    pub fn size(&self, mip: u32) -> [u32; 4] {
        if let Some(cube) = self.binding.cube {
            let edge = if mip < cube.levels as u32 {
                (cube.edge >> mip).max(1) as u32
            } else {
                0
            };
            [edge, edge, 0, cube.levels as u32]
        } else if let Some(image) = self.binding.layered {
            image.dimensions(mip)
        } else {
            let levels = 1 + self.texture.mips.as_ref().map_or(0, Vec::len);
            let size = if mip < levels as u32 {
                self.texture.size.map(|v| (v >> mip).max(1) as u32)
            } else {
                [0; 2]
            };
            [size[0], size[1], 0, levels as u32]
        }
    }

    pub fn sample(
        &self,
        uv: [f32; 4],
        sampler: Option<&texture::Sampler>,
        sampling: evaluate::Sampling,
        offset: [i32; 3],
    ) -> [u32; 4] {
        let texture = self.texture;
        let color = self.binding.color;
        let value = if let Some(cube) = self.binding.cube {
            let direction = [uv[0], uv[1], uv[2]];
            let lod = match sampling {
                evaluate::Sampling::Implicit => 0.0,
                evaluate::Sampling::Level(lod) => lod,
                evaluate::Sampling::Gradients { dx, dy } => {
                    let lod = cube.lod(direction, [dx[0], dx[1], dx[2]], [dy[0], dy[1], dy[2]])[1];
                    sampler
                        .filter(|s| s.filter.is_some())
                        .map_or(lod, |s| (lod + s.mip_bias).clamp(s.lod[0], s.lod[1]))
                }
            };
            cube.sample(texture, direction, lod, color)
        } else {
            let Some(sampler) = sampler else {
                return [0; 4];
            };
            if let Some(image) = self.binding.layered {
                image.sample(texture, uv, sampler, sampling, offset, color)
            } else {
                let value = match sampling {
                    evaluate::Sampling::Gradients { dx, dy } => texture.sample_grad(
                        [uv[0], uv[1]],
                        sampler,
                        texture::Footprint {
                            dx: [dx[0], dx[1]],
                            dy: [dy[0], dy[1]],
                        },
                        color,
                        [offset[0], offset[1]],
                    ),
                    evaluate::Sampling::Level(lod) => texture.sample_explicit(
                        [uv[0], uv[1]],
                        sampler,
                        lod,
                        color,
                        [offset[0], offset[1]],
                    ),
                    evaluate::Sampling::Implicit => texture.sample_explicit(
                        [uv[0], uv[1]],
                        sampler,
                        0.0,
                        color,
                        [offset[0], offset[1]],
                    ),
                };
                if color {
                    value
                } else {
                    value.map(|v| v / 255.0)
                }
            }
        };
        value.map(f32::to_bits)
    }

    pub fn load(&self, position: [i32; 4], offset: [i32; 3]) -> [u32; 4] {
        if let Some(image) = self.binding.layered {
            return image
                .load_texel(self.texture, position, offset, self.binding.color)
                .map(f32::to_bits);
        }
        if self.binding.cube.is_some() || position[3] < 0 {
            return [0; 4];
        }
        let texture = if position[3] == 0 {
            Some(self.texture)
        } else {
            self.texture
                .mips
                .as_ref()
                .and_then(|levels| levels.get(position[3] as usize - 1))
        };
        let Some(texture) = texture else {
            return [0; 4];
        };
        let p = [
            position[0].saturating_add(offset[0]),
            position[1].saturating_add(offset[1]),
        ];
        if (0..2).any(|i| p[i] < 0 || p[i] as usize >= texture.size[i]) {
            return [0; 4];
        }
        layered::texel(texture, p.map(|v| v as usize), self.binding.color).map(f32::to_bits)
    }
}

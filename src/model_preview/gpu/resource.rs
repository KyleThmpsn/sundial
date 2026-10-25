//! Explicit ownership for auxiliary passes on the painter's current context.
use super::*;

#[derive(Default)]
pub(super) struct Objects {
    buffers: Vec<glow::Buffer>,
    textures: Vec<glow::Texture>,
    arrays: Vec<glow::VertexArray>,
    programs: Vec<glow::Program>,
    framebuffers: Vec<glow::Framebuffer>,
    samplers: Vec<glow::Sampler>,
}

#[derive(Clone, Copy)]
pub(super) struct Table {
    pub buffer: glow::Buffer,
    pub texture: glow::Texture,
}

impl Objects {
    pub unsafe fn sampler(
        &mut self,
        gl: &glow::Context,
        source: &crate::model_preview::texture::Sampler,
    ) -> Result<glow::Sampler, String> {
        use crate::model_preview::texture::AddressMode;
        // SAFETY: the sampler belongs to this context. These base-level studies always use
        // bilinear reads. Mirror-once coordinates are folded in the caller's shader.
        unsafe {
            let object = gl.create_sampler()?;
            self.samplers.push(object);
            let mode = |value| match value {
                AddressMode::Wrap => glow::REPEAT,
                AddressMode::Mirror => glow::MIRRORED_REPEAT,
                AddressMode::Clamp | AddressMode::MirrorOnce => glow::CLAMP_TO_EDGE,
                AddressMode::Border => glow::CLAMP_TO_BORDER,
            } as i32;
            gl.sampler_parameter_i32(object, glow::TEXTURE_WRAP_S, mode(source.u));
            gl.sampler_parameter_i32(object, glow::TEXTURE_WRAP_T, mode(source.v));
            gl.sampler_parameter_i32(object, glow::TEXTURE_MIN_FILTER, glow::LINEAR as i32);
            gl.sampler_parameter_i32(object, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
            gl.sampler_parameter_f32_slice(
                object,
                glow::TEXTURE_BORDER_COLOR,
                &source.border.map(|v| v / 255.0),
            );
            Ok(object)
        }
    }

    pub unsafe fn image(
        &mut self,
        gl: &glow::Context,
        source: &crate::model_preview::texture::Texture,
        color: bool,
    ) -> Result<glow::Texture, String> {
        // SAFETY: checked dimensions and source lengths match the base-level upload.
        unsafe {
            let maximum = gl.get_parameter_i32(glow::MAX_TEXTURE_SIZE).max(0) as usize;
            if source.size.iter().any(|&axis| axis == 0 || axis > maximum) {
                return Err("A preview texture exceeds the GPU's image limit.".into());
            }
            let pixels = source.size[0]
                .checked_mul(source.size[1])
                .ok_or("Texture size overflow.")?;
            if source.linear.as_ref().is_some_and(|v| v.len() != pixels)
                || source.rgba.len() != pixels * 4
            {
                return Err("A preview texture has an incomplete pixel buffer.".into());
            }
            let object = self.texture(gl)?;
            gl.bind_texture(glow::TEXTURE_2D, Some(object));
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                if source.linear.is_some() {
                    glow::RGBA32F
                } else if color {
                    glow::SRGB8_ALPHA8
                } else {
                    glow::RGBA8
                } as i32,
                source.size[0] as i32,
                source.size[1] as i32,
                0,
                glow::RGBA,
                if source.linear.is_some() {
                    glow::FLOAT
                } else {
                    glow::UNSIGNED_BYTE
                },
                glow::PixelUnpackData::Slice(Some(
                    source
                        .linear
                        .as_ref()
                        .map_or(source.rgba.as_slice(), |values| {
                            bytes_of(values.as_flattened())
                        }),
                )),
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MIN_FILTER,
                glow::LINEAR as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MAG_FILTER,
                glow::LINEAR as i32,
            );
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::REPEAT as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::REPEAT as i32);
            if gl.get_error() != glow::NO_ERROR {
                return Err("The GPU could not upload a preview texture.".into());
            }
            Ok(object)
        }
    }
    pub unsafe fn buffer(&mut self, gl: &glow::Context) -> Result<glow::Buffer, String> {
        // SAFETY: the caller owns the current GL context throughout construction and deletion.
        let object = unsafe { gl.create_buffer()? };
        self.buffers.push(object);
        Ok(object)
    }
    pub unsafe fn texture(&mut self, gl: &glow::Context) -> Result<glow::Texture, String> {
        // SAFETY: same context ownership as buffer.
        let object = unsafe { gl.create_texture()? };
        self.textures.push(object);
        Ok(object)
    }
    pub unsafe fn array(&mut self, gl: &glow::Context) -> Result<glow::VertexArray, String> {
        // SAFETY: same context ownership as buffer.
        let object = unsafe { gl.create_vertex_array()? };
        self.arrays.push(object);
        Ok(object)
    }
    pub unsafe fn framebuffer(&mut self, gl: &glow::Context) -> Result<glow::Framebuffer, String> {
        // SAFETY: same context ownership as buffer.
        let object = unsafe { gl.create_framebuffer()? };
        self.framebuffers.push(object);
        Ok(object)
    }
    pub fn program(&mut self, program: glow::Program) -> glow::Program {
        self.programs.push(program);
        program
    }
    pub unsafe fn table(
        &mut self,
        gl: &glow::Context,
        rows: &[[f32; 4]],
        count: usize,
    ) -> Result<Table, String> {
        let count = count.max(rows.len()).max(1);
        // SAFETY: checked lengths fit GL's signed byte count and texture-buffer capacity.
        unsafe {
            if count > gl.get_parameter_i32(glow::MAX_TEXTURE_BUFFER_SIZE).max(0) as usize {
                return Err("The GPU's texture buffer limit is too small for this model.".into());
            }
            let size = count
                .checked_mul(16)
                .and_then(|n| i32::try_from(n).ok())
                .ok_or("The GPU buffer exceeds the supported size.")?;
            let buffer = self.buffer(gl)?;
            gl.bind_buffer(glow::TEXTURE_BUFFER, Some(buffer));
            gl.buffer_data_size(glow::TEXTURE_BUFFER, size, glow::DYNAMIC_COPY);
            if !rows.is_empty() {
                gl.buffer_sub_data_u8_slice(glow::TEXTURE_BUFFER, 0, bytes_of(rows.as_flattened()));
            }
            let texture = self.texture(gl)?;
            gl.bind_texture(glow::TEXTURE_BUFFER, Some(texture));
            gl.tex_buffer(glow::TEXTURE_BUFFER, glow::RGBA32F, Some(buffer));
            if gl.get_error() != glow::NO_ERROR {
                return Err("The GPU could not allocate a preview buffer.".into());
            }
            Ok(Table { buffer, texture })
        }
    }
    pub unsafe fn delete(self, gl: &glow::Context) {
        // SAFETY: every handle was created on this context and is deleted exactly once.
        unsafe {
            for object in self.framebuffers {
                gl.delete_framebuffer(object);
            }
            for object in self.programs {
                gl.delete_program(object);
            }
            for object in self.arrays {
                gl.delete_vertex_array(object);
            }
            for object in self.samplers {
                gl.delete_sampler(object);
            }
            for object in self.textures {
                gl.delete_texture(object);
            }
            for object in self.buffers {
                gl.delete_buffer(object);
            }
        }
    }
}

impl Table {
    pub unsafe fn bind(self, gl: &glow::Context, program: glow::Program, name: &str, unit: u32) {
        // SAFETY: the table belongs to this context and the named program is active.
        unsafe {
            gl.active_texture(glow::TEXTURE0 + unit);
            gl.bind_sampler(unit, None);
            gl.bind_texture(glow::TEXTURE_BUFFER, Some(self.texture));
            gl.uniform_1_i32(gl.get_uniform_location(program, name).as_ref(), unit as i32);
        }
    }
}

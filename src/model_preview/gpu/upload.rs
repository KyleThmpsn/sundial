//! CPU preparation runs off the paint thread. GL transfers yield between small chunks.
use super::*;
use crate::model_preview::{effects, texture::AddressMode};
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub(super) struct Prepared {
    model: Arc<Model>,
    positions: Vec<[f32; 3]>,
    attributes: Vec<f32>,
    groups: Vec<Group>,
    order: Vec<u32>,
    ordering: order::Cache,
    framing: [([f32; 3], f32); 2],
    hide_emitter: bool,
    roles: Vec<[bool; 2]>,
    source_indices: Vec<u32>,
    deformation: Option<deform::Prepared>,
}

impl Prepared {
    fn new(held: Arc<Model>) -> Self {
        let model: &Model = &held;
        let hide_light = model.has_surface_mesh();
        let hide_emitter = model.has_object_mesh();
        let framing = [Style::Textured, Style::Solid].map(|style| bounds(model, style));

        let mut order: Vec<u32> = (0..model.triangles.len() as u32)
            .filter(|&index| {
                !hide_light
                    || !model
                        .triangle_light
                        .get(index as usize)
                        .copied()
                        .unwrap_or(false)
            })
            .collect();
        let key_of = |triangle: usize| Key {
            emitter: model
                .triangle_emitter
                .get(triangle)
                .copied()
                .unwrap_or(false),
            cutoff: model
                .triangle_cutoff
                .get(triangle)
                .copied()
                .flatten()
                .map(f32::to_bits),
            native_detail: model
                .triangle_detail_uv
                .get(triangle)
                .copied()
                .unwrap_or(false),
            effect: effects::index(model, triangle),
            albedo: model.triangle_textures.get(triangle).copied().flatten(),
            gearstack: model.triangle_gearstacks.get(triangle).copied().flatten(),
            normal: model.triangle_normals.get(triangle).copied().flatten(),
            slot: model
                .triangle_dyes
                .get(triangle)
                .copied()
                .unwrap_or(u8::MAX),
            clip: model.triangle_clip.get(triangle).copied().unwrap_or(false),
            dye_map: model.triangle_dye_maps.get(triangle).copied().flatten(),
            constant: model
                .triangle_constant
                .get(triangle)
                .copied()
                .flatten()
                .map(|c| c.map(f32::to_bits)),
        };
        order.sort_by_key(|&t| (effects::transparent(model, t as usize), key_of(t as usize)));
        let mut groups: Vec<Group> = Vec::new();
        for (position, &triangle) in order.iter().enumerate() {
            let key = key_of(triangle as usize);
            match groups.last_mut() {
                Some(group) if group.key == key => group.count += 3,
                _ => groups.push(Group {
                    key,
                    indexed: false,
                    first: position as i32 * 3,
                    count: 3,
                }),
            }
        }

        let positions = expand_positions(&order, model, &model.vertices);
        let attributes = expand_attributes(
            &order,
            model,
            &model.vertices,
            &model.normals,
            &model.tangents,
        );

        let roles = texture_roles(model);
        let ordering = order::Cache::new(model, &groups, &order, framing[0].0, hide_emitter);
        let source_indices = order
            .iter()
            .flat_map(|&triangle| {
                let [a, b, c] = model.triangles[triangle as usize];
                [a, b, c, b, c, a, c, a, b]
            })
            .collect();
        let deformation = deform::Prepared::new(model);

        Self {
            model: held,
            positions,
            attributes,
            groups,
            order,
            ordering,
            framing,
            hide_emitter,
            roles,
            source_indices,
            deformation,
        }
    }
}

#[derive(Default)]
pub(super) struct Preparation {
    pending: Option<mpsc::Receiver<Prepared>>,
}
impl Preparation {
    pub fn poll(&mut self, model: &Arc<Model>) -> Option<Prepared> {
        if let Some(receiver) = &self.pending {
            match receiver.try_recv() {
                Ok(prepared) => {
                    self.pending = None;
                    if Arc::ptr_eq(&prepared.model, model) {
                        return Some(prepared);
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => self.pending = None,
                Err(mpsc::TryRecvError::Empty) => return None,
            }
        }
        let (sender, receiver) = mpsc::channel();
        self.pending = Some(receiver);
        let model = model.clone();
        std::thread::spawn(move || {
            let _ = sender.send(Prepared::new(model));
        });
        None
    }
}

pub(super) struct Pending {
    positions: Vec<[f32; 3]>,
    attributes: Vec<f32>,
    offsets: [usize; 2],
    roles: Vec<[bool; 2]>,
    cursor: usize,
    row: usize,
    level: usize,
}

pub(super) unsafe fn begin(gl: &glow::Context, prepared: Prepared) -> Result<Uploaded, String> {
    let Prepared {
        model,
        positions,
        attributes,
        groups,
        order,
        ordering,
        framing,
        hide_emitter,
        roles,
        source_indices,
        deformation,
    } = prepared;
    // SAFETY: the paint callback supplies its current context. These objects remain owned
    // by Uploaded and all buffers are allocated to the exact prepared slice lengths.
    unsafe {
        let deformation = deformation
            .map(|prepared| deform::Pipeline::new(gl, prepared, model.vertices.len()))
            .transpose()?;
        let vao = gl.create_vertex_array().expect("vertex array");
        gl.bind_vertex_array(Some(vao));
        let position_buffer = gl.create_buffer().expect("buffer");
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(position_buffer));
        gl.buffer_data_size(
            glow::ARRAY_BUFFER,
            std::mem::size_of_val(positions.as_slice()) as i32,
            glow::DYNAMIC_DRAW,
        );
        gl.enable_vertex_attrib_array(0);
        gl.vertex_attrib_pointer_f32(0, 3, glow::FLOAT, false, 12, 0);
        let attribute_buffer = gl.create_buffer().expect("buffer");
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(attribute_buffer));
        gl.buffer_data_size(
            glow::ARRAY_BUFFER,
            std::mem::size_of_val(attributes.as_slice()) as i32,
            glow::DYNAMIC_DRAW,
        );
        gl.enable_vertex_attrib_array(1);
        gl.vertex_attrib_pointer_f32(1, 3, glow::FLOAT, false, 64, 0);
        gl.enable_vertex_attrib_array(2);
        gl.vertex_attrib_pointer_f32(2, 2, glow::FLOAT, false, 64, 12);
        gl.enable_vertex_attrib_array(3);
        gl.vertex_attrib_pointer_f32(3, 2, glow::FLOAT, false, 64, 20);
        gl.enable_vertex_attrib_array(4);
        gl.vertex_attrib_pointer_f32(4, 4, glow::FLOAT, false, 64, 28);
        gl.enable_vertex_attrib_array(5);
        gl.vertex_attrib_pointer_f32(5, 4, glow::FLOAT, false, 64, 44);
        gl.enable_vertex_attrib_array(6);
        gl.vertex_attrib_pointer_f32(6, 1, glow::FLOAT, false, 64, 60);
        let source_buffer = gl.create_buffer().expect("source index buffer");
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(source_buffer));
        let bytes = std::slice::from_raw_parts(
            source_indices.as_ptr().cast::<u8>(),
            std::mem::size_of_val(source_indices.as_slice()),
        );
        gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes, glow::STATIC_DRAW);
        gl.enable_vertex_attrib_array(7);
        gl.vertex_attrib_pointer_i32(7, 3, glow::UNSIGNED_INT, 12, 0);
        let index_buffer = gl.create_buffer().expect("element buffer");
        gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(index_buffer));
        gl.bind_vertex_array(None);
        let lookup = model.iridescence.as_ref().map(|texture| {
            let handle = gl.create_texture().expect("texture");
            gl.bind_texture(glow::TEXTURE_2D, Some(handle));
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                if texture.linear.is_some() {
                    glow::RGBA32F
                } else {
                    glow::RGBA8
                } as i32,
                texture.size[0] as i32,
                texture.size[1] as i32,
                0,
                glow::RGBA,
                if texture.linear.is_some() {
                    glow::FLOAT
                } else {
                    glow::UNSIGNED_BYTE
                },
                glow::PixelUnpackData::Slice(Some(texture.linear.as_ref().map_or_else(
                    || texture.rgba.as_slice(),
                    |pixels| bytes_of(pixels.as_flattened()),
                ))),
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
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_S,
                glow::CLAMP_TO_EDGE as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_T,
                glow::CLAMP_TO_EDGE as i32,
            );
            handle
        });
        gl.bind_texture(glow::TEXTURE_2D, None);

        let samplers = model
            .effects
            .iter()
            .map(|effect| {
                effect
                    .samplers
                    .iter()
                    .map(|source| {
                        let sampler = gl.create_sampler().expect("effect sampler");
                        let mode = |mode| match mode {
                            AddressMode::Wrap => glow::REPEAT,
                            AddressMode::Mirror => glow::MIRRORED_REPEAT,
                            AddressMode::Clamp => glow::CLAMP_TO_EDGE,
                            AddressMode::Border => glow::CLAMP_TO_BORDER,
                            AddressMode::MirrorOnce => 0x8743,
                        } as i32;
                        gl.sampler_parameter_i32(sampler, glow::TEXTURE_WRAP_S, mode(source.u));
                        gl.sampler_parameter_i32(sampler, glow::TEXTURE_WRAP_T, mode(source.v));
                        configure_sampler(
                            gl,
                            sampler,
                            source,
                            effect.sampling().is_some() || effect.native.is_some(),
                        );
                        gl.sampler_parameter_f32_slice(
                            sampler,
                            glow::TEXTURE_BORDER_COLOR,
                            &source.border.map(|v| v / 255.0),
                        );
                        sampler
                    })
                    .collect()
            })
            .collect();

        let textures = vec![[None; 2]; model.textures.len()];
        Ok(Uploaded {
            model,
            vao,
            positions: position_buffer,
            attributes: attribute_buffer,
            indices: index_buffer,
            source_indices: source_buffer,
            deformation,
            particles: None,
            pose: None,
            ordering,
            textures,
            lookup,
            groups,
            order,
            framing,
            hide_emitter,
            samplers,
            pending: Some(Pending {
                positions,
                attributes,
                offsets: [0; 2],
                roles,
                cursor: 0,
                row: 0,
                level: 0,
            }),
        })
    }
}

impl Uploaded {
    pub(super) unsafe fn advance(&mut self, gl: &glow::Context) -> bool {
        let Some(pending) = &mut self.pending else {
            return true;
        };
        let started = Instant::now();
        // SAFETY: this is called only by draw on the object's current GL context. Offsets
        // and transfer lengths are bounded by their allocated source and destination.
        unsafe {
            loop {
                if !transfer_buffer(gl, pending, [self.positions, self.attributes]) {
                    if pending.cursor >= pending.roles.len() * 2 {
                        self.pending = None;
                        return true;
                    }
                    let (index, role) = (pending.cursor / 2, pending.cursor % 2);
                    if pending.roles[index][role] {
                        transfer_texture(
                            gl,
                            pending,
                            &self.model.textures[index],
                            &mut self.textures[index][role],
                            role,
                        );
                    } else {
                        pending.cursor += 1;
                    }
                }
                if started.elapsed() >= Duration::from_millis(4) {
                    return false;
                }
            }
        }
    }
}

unsafe fn transfer_buffer(
    gl: &glow::Context,
    pending: &mut Pending,
    buffers: [glow::Buffer; 2],
) -> bool {
    // SAFETY: buffers were allocated for these prepared slices, with bounded offsets.
    unsafe {
        for (index, buffer, data) in [
            (0, buffers[0], bytes_of(pending.positions.as_flattened())),
            (1, buffers[1], bytes_of(&pending.attributes)),
        ] {
            let offset = pending.offsets[index];
            if offset < data.len() {
                let end = (offset + 1024 * 1024).min(data.len());
                gl.bind_buffer(glow::ARRAY_BUFFER, Some(buffer));
                gl.buffer_sub_data_u8_slice(glow::ARRAY_BUFFER, offset as i32, &data[offset..end]);
                pending.offsets[index] = end;
                return true;
            }
        }
        false
    }
}

unsafe fn transfer_texture(
    gl: &glow::Context,
    pending: &mut Pending,
    texture: &crate::model_preview::texture::Texture,
    handle: &mut Option<glow::Texture>,
    role: usize,
) {
    // SAFETY: the upload owns the handle in the current context, and each row is bounded.
    unsafe {
        let image = texture.level(pending.level);
        transfer_image(gl, handle, image, pending.level, &mut pending.row, role);
        if pending.row == image.size[1] {
            pending.row = 0;
            if pending.level < texture.mips.as_ref().map_or(0, Vec::len) {
                pending.level += 1;
            } else {
                finish_texture(gl, texture);
                pending.cursor += 1;
                pending.level = 0;
            }
        }
        gl.bind_texture(glow::TEXTURE_2D, None);
    }
}

unsafe fn finish_texture(gl: &glow::Context, texture: &crate::model_preview::texture::Texture) {
    // SAFETY: transfer_texture has bound and fully uploaded this image and its levels.
    unsafe {
        if let Some(levels) = &texture.mips {
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MAX_LEVEL,
                levels.len() as i32,
            );
        } else {
            gl.generate_mipmap(glow::TEXTURE_2D);
        }
        for (key, value) in [
            (glow::TEXTURE_MIN_FILTER, glow::LINEAR_MIPMAP_LINEAR),
            (glow::TEXTURE_MAG_FILTER, glow::LINEAR),
            (glow::TEXTURE_WRAP_S, glow::REPEAT),
            (glow::TEXTURE_WRAP_T, glow::REPEAT),
        ] {
            gl.tex_parameter_i32(glow::TEXTURE_2D, key, value as i32);
        }
        if gl
            .supported_extensions()
            .contains("GL_EXT_texture_filter_anisotropic")
        {
            let maximum = gl.get_parameter_f32(0x84FF).max(1.0);
            gl.tex_parameter_f32(glow::TEXTURE_2D, 0x84FE, 8.0f32.min(maximum));
        }
    }
}

unsafe fn configure_sampler(
    gl: &glow::Context,
    handle: glow::Sampler,
    source: &crate::model_preview::texture::Sampler,
    native: bool,
) {
    // SAFETY: the upload owns this sampler in the painter's current context.
    unsafe {
        let filter = source.filter.unwrap_or(0x15);
        let min = if native {
            match (filter & 16 != 0, filter & 1 != 0) {
                (false, false) => glow::NEAREST_MIPMAP_NEAREST,
                (false, true) => glow::NEAREST_MIPMAP_LINEAR,
                (true, false) => glow::LINEAR_MIPMAP_NEAREST,
                (true, true) => glow::LINEAR_MIPMAP_LINEAR,
            }
        } else {
            glow::LINEAR
        };
        let mag = if native && filter & 4 == 0 {
            glow::NEAREST
        } else {
            glow::LINEAR
        };
        gl.sampler_parameter_i32(handle, glow::TEXTURE_MIN_FILTER, min as i32);
        gl.sampler_parameter_i32(handle, glow::TEXTURE_MAG_FILTER, mag as i32);
        if native {
            // Isotropic native reads apply the bias explicitly in samplePlate. The sampler's
            // bias must be zero because GL also adds it to an explicit textureLod lookup.
            gl.sampler_parameter_f32(handle, glow::TEXTURE_LOD_BIAS, 0.0);
            gl.sampler_parameter_f32(handle, glow::TEXTURE_MIN_LOD, source.lod[0]);
            gl.sampler_parameter_f32(handle, glow::TEXTURE_MAX_LOD, source.lod[1]);
            if gl
                .supported_extensions()
                .contains("GL_EXT_texture_filter_anisotropic")
            {
                let maximum = gl.get_parameter_f32(0x84FF).max(1.0);
                gl.sampler_parameter_f32(handle, 0x84FE, (source.anisotropy as f32).min(maximum));
            }
        }
    }
}

unsafe fn transfer_image(
    gl: &glow::Context,
    handle: &mut Option<glow::Texture>,
    image: &crate::model_preview::texture::Texture,
    level: usize,
    row: &mut usize,
    role: usize,
) {
    // SAFETY: the upload owns the texture and image, and rows are bounded by this mip.
    unsafe {
        if level == 0 && *row == 0 {
            *handle = Some(gl.create_texture().expect("texture"));
        }
        gl.bind_texture(glow::TEXTURE_2D, *handle);
        let format = if image.linear.is_some() {
            glow::FLOAT
        } else {
            glow::UNSIGNED_BYTE
        };
        if *row == 0 {
            let internal = if image.linear.is_some() {
                glow::RGBA32F
            } else if role == 1 {
                glow::SRGB8_ALPHA8
            } else {
                glow::RGBA8
            };
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                level as i32,
                internal as i32,
                image.size[0] as i32,
                image.size[1] as i32,
                0,
                glow::RGBA,
                format,
                glow::PixelUnpackData::Slice(None),
            );
        }
        let end = (*row + 256).min(image.size[1]);
        let stride = image.size[0] * 4;
        let data = image.linear.as_ref().map_or_else(
            || &image.rgba[*row * stride..end * stride],
            |pixels| bytes_of(pixels[*row * image.size[0]..end * image.size[0]].as_flattened()),
        );
        gl.tex_sub_image_2d(
            glow::TEXTURE_2D,
            level as i32,
            0,
            *row as i32,
            image.size[0] as i32,
            (end - *row) as i32,
            glow::RGBA,
            format,
            glow::PixelUnpackData::Slice(Some(data)),
        );
        *row = end;
    }
}

fn texture_roles(model: &Model) -> Vec<[bool; 2]> {
    // Colour plates and dye detail colour are sRGB; masks and normals are linear.
    let mut roles = vec![[false; 2]; model.textures.len()];
    for source in &model.particle_sources {
        if let Some(role) = roles.get_mut(source.texture) {
            *role = [true, true];
        }
        if let Some(index) = source.gradient
            && let Some(role) = roles.get_mut(index)
        {
            role[1] = true;
        }
    }
    for map in model.triangle_dye_maps.iter().flatten() {
        roles[map.texture][0] = true;
    }
    for index in model.triangle_textures.iter().flatten() {
        roles[*index][1] = true;
    }
    for index in model.dyes.iter().flatten().filter_map(|dye| dye.detail) {
        roles[index][1] = true;
    }
    for index in model
        .triangle_gearstacks
        .iter()
        .chain(model.triangle_normals.iter())
        .flatten()
    {
        roles[*index][0] = true;
    }
    for index in model.dyes.iter().flatten().filter_map(|dye| dye.normal) {
        roles[index][0] = true;
    }
    for effect in &model.effects {
        if let Some(native) = &effect.native {
            for binding in &native.bindings {
                if let super::super::effects::native::Role::Texture(index) = binding.role
                    && let Some(roles) = roles.get_mut(index)
                {
                    roles[usize::from(binding.color)] = true;
                }
            }
        }
        for (slot, index) in effect.textures.iter().enumerate() {
            if let Some(index) = index {
                roles[*index][usize::from(effect.color[slot])] = true;
            }
        }
    }

    roles
}

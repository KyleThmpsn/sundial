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
    center: [f32; 3],
    radius: f32,
    roles: Vec<[bool; 2]>,
}

impl Prepared {
    fn new(held: Arc<Model>) -> Self {
        let model: &Model = &held;
        let hide_light = model.has_surface_mesh();
        let (center, radius) = bounds(model, hide_light);

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
        order.sort_by_key(|&t| key_of(t as usize));
        let mut groups: Vec<Group> = Vec::new();
        for (position, &triangle) in order.iter().enumerate() {
            let key = key_of(triangle as usize);
            match groups.last_mut() {
                Some(group) if group.key == key => group.count += 3,
                _ => groups.push(Group {
                    key,
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

        Self {
            model: held,
            positions,
            attributes,
            groups,
            order,
            center,
            radius,
            roles,
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
}

pub(super) unsafe fn begin(gl: &glow::Context, prepared: Prepared) -> Uploaded {
    let Prepared {
        model,
        positions,
        attributes,
        groups,
        order,
        center,
        radius,
        roles,
    } = prepared;
    // SAFETY: the paint callback supplies its current context. These objects remain owned
    // by Uploaded and all buffers are allocated to the exact prepared slice lengths.
    unsafe {
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
        gl.vertex_attrib_pointer_f32(1, 3, glow::FLOAT, false, 60, 0);
        gl.enable_vertex_attrib_array(2);
        gl.vertex_attrib_pointer_f32(2, 2, glow::FLOAT, false, 60, 12);
        gl.enable_vertex_attrib_array(3);
        gl.vertex_attrib_pointer_f32(3, 2, glow::FLOAT, false, 60, 20);
        gl.enable_vertex_attrib_array(4);
        gl.vertex_attrib_pointer_f32(4, 4, glow::FLOAT, false, 60, 28);
        gl.enable_vertex_attrib_array(5);
        gl.vertex_attrib_pointer_f32(5, 4, glow::FLOAT, false, 60, 44);
        gl.bind_vertex_array(None);
        let lookup = model.iridescence.as_ref().map(|texture| {
            let handle = gl.create_texture().expect("texture");
            gl.bind_texture(glow::TEXTURE_2D, Some(handle));
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA8 as i32,
                texture.size[0] as i32,
                texture.size[1] as i32,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(Some(&texture.rgba)),
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
                        gl.sampler_parameter_i32(
                            sampler,
                            glow::TEXTURE_MIN_FILTER,
                            glow::LINEAR as i32,
                        );
                        gl.sampler_parameter_i32(
                            sampler,
                            glow::TEXTURE_MAG_FILTER,
                            glow::LINEAR as i32,
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
        Uploaded {
            model,
            vao,
            positions: position_buffer,
            attributes: attribute_buffer,
            posed: false,
            textures,
            lookup,
            groups,
            order,
            center,
            radius,
            samplers,
            pending: Some(Pending {
                positions,
                attributes,
                offsets: [0; 2],
                roles,
                cursor: 0,
                row: 0,
            }),
        }
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
                let mut transferred = false;
                for (index, buffer, data) in [
                    (
                        0,
                        self.positions,
                        bytes_of(pending.positions.as_flattened()),
                    ),
                    (1, self.attributes, bytes_of(&pending.attributes)),
                ] {
                    let offset = pending.offsets[index];
                    if offset < data.len() {
                        let end = (offset + 1024 * 1024).min(data.len());
                        gl.bind_buffer(glow::ARRAY_BUFFER, Some(buffer));
                        gl.buffer_sub_data_u8_slice(
                            glow::ARRAY_BUFFER,
                            offset as i32,
                            &data[offset..end],
                        );
                        pending.offsets[index] = end;
                        transferred = true;
                        break;
                    }
                }
                if !transferred {
                    if pending.cursor >= pending.roles.len() * 2 {
                        self.pending = None;
                        return true;
                    }
                    let (index, role) = (pending.cursor / 2, pending.cursor % 2);
                    if !pending.roles[index][role] {
                        pending.cursor += 1;
                        continue;
                    }
                    let texture = &self.model.textures[index];
                    if pending.row == 0 {
                        let handle = gl.create_texture().expect("texture");
                        self.textures[index][role] = Some(handle);
                        gl.bind_texture(glow::TEXTURE_2D, Some(handle));
                        gl.tex_image_2d(
                            glow::TEXTURE_2D,
                            0,
                            if role == 1 {
                                glow::SRGB8_ALPHA8
                            } else {
                                glow::RGBA8
                            } as i32,
                            texture.size[0] as i32,
                            texture.size[1] as i32,
                            0,
                            glow::RGBA,
                            glow::UNSIGNED_BYTE,
                            glow::PixelUnpackData::Slice(None),
                        );
                    } else {
                        gl.bind_texture(glow::TEXTURE_2D, self.textures[index][role]);
                    }
                    let end = (pending.row + 256).min(texture.size[1]);
                    let stride = texture.size[0] * 4;
                    gl.tex_sub_image_2d(
                        glow::TEXTURE_2D,
                        0,
                        0,
                        pending.row as i32,
                        texture.size[0] as i32,
                        (end - pending.row) as i32,
                        glow::RGBA,
                        glow::UNSIGNED_BYTE,
                        glow::PixelUnpackData::Slice(Some(
                            &texture.rgba[pending.row * stride..end * stride],
                        )),
                    );
                    pending.row = end;
                    if end == texture.size[1] {
                        gl.generate_mipmap(glow::TEXTURE_2D);
                        gl.tex_parameter_i32(
                            glow::TEXTURE_2D,
                            glow::TEXTURE_MIN_FILTER,
                            glow::LINEAR_MIPMAP_LINEAR as i32,
                        );
                        gl.tex_parameter_i32(
                            glow::TEXTURE_2D,
                            glow::TEXTURE_MAG_FILTER,
                            glow::LINEAR as i32,
                        );
                        gl.tex_parameter_i32(
                            glow::TEXTURE_2D,
                            glow::TEXTURE_WRAP_S,
                            glow::REPEAT as i32,
                        );
                        gl.tex_parameter_i32(
                            glow::TEXTURE_2D,
                            glow::TEXTURE_WRAP_T,
                            glow::REPEAT as i32,
                        );
                        if gl
                            .supported_extensions()
                            .contains("GL_EXT_texture_filter_anisotropic")
                        {
                            gl.tex_parameter_f32(glow::TEXTURE_2D, 0x84FE, 8.0);
                        }
                        pending.cursor += 1;
                        pending.row = 0;
                    }
                    gl.bind_texture(glow::TEXTURE_2D, None);
                }
                if started.elapsed() >= Duration::from_millis(4) {
                    return false;
                }
            }
        }
    }
}

fn texture_roles(model: &Model) -> Vec<[bool; 2]> {
    // Colour plates and dye detail colour are sRGB; masks and normals are linear.
    let mut roles = vec![[false; 2]; model.textures.len()];
    for map in model.triangle_dye_maps.iter().flatten() {
        roles[map.texture][0] = true;
    }
    for index in model.triangle_textures.iter().flatten() {
        roles[*index][1] = true;
    }
    for dye in model.dyes.iter().flatten() {
        if let Some(index) = dye.detail {
            roles[index][1] = true;
        }
    }
    for index in model
        .triangle_gearstacks
        .iter()
        .chain(model.triangle_normals.iter())
        .flatten()
    {
        roles[*index][0] = true;
    }
    for dye in model.dyes.iter().flatten() {
        if let Some(index) = dye.normal {
            roles[index][0] = true;
        }
    }
    for effect in &model.effects {
        if let Some(native) = &effect.native {
            for binding in &native.bindings {
                if let super::super::effects::native::Role::Texture(index) = binding.role {
                    if let Some(roles) = roles.get_mut(index) {
                        roles[usize::from(binding.color)] = true;
                    }
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

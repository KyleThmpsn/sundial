//! Bounded MRT material passes. UV planning and file encoding stay on the export worker.
use super::*;
use crate::model_preview::export::bake::{Layout, Painted, Request};
use std::collections::{BTreeMap, BTreeSet};

const ROWS: usize = 128;

pub(super) struct Renderer {
    objects: resource::Objects,
    program: glow::Program,
    vao: glow::VertexArray,
    framebuffer: glow::Framebuffer,
    surfaces: resource::Table,
    owner: glow::Texture,
    textures: BTreeMap<(usize, bool), glow::Texture>,
    request: Request,
    row: usize,
    painted: Painted,
}

impl Renderer {
    pub unsafe fn new(gl: &glow::Context, model: &Model, request: Request) -> Result<Self, String> {
        let mut objects = resource::Objects::default();
        // SAFETY: construction and all subsequent passes use the painter's current context.
        let result = unsafe { Self::create(gl, model, request, &mut objects) };
        match result {
            Ok(mut renderer) => {
                renderer.objects = objects;
                Ok(renderer)
            }
            Err(error) => {
                // SAFETY: this is still the construction context, including partial allocations.
                unsafe { objects.delete(gl) };
                Err(error)
            }
        }
    }

    unsafe fn create(
        gl: &glow::Context,
        model: &Model,
        request: Request,
        objects: &mut resource::Objects,
    ) -> Result<Self, String> {
        // SAFETY: dimensions and indices are validated before GL uploads and texture access.
        unsafe {
            let [width, height] = request.size;
            let maximum = gl.get_parameter_i32(glow::MAX_TEXTURE_SIZE).max(0) as usize;
            if width == 0 || height == 0 || width > maximum || height > maximum {
                return Err("This material exceeds the GPU's image limit.".into());
            }
            let count = width.checked_mul(height).ok_or("Material size overflow.")?;
            let program = objects.program(
                link(gl, VERTEX, include_str!("bake/shader.frag"))
                    .ok_or("Could not initialize the GPU material baker.")?,
            );
            let vao = objects.array(gl)?;
            let framebuffer = objects.framebuffer(gl)?;
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(framebuffer));
            for index in 0..4 {
                let texture = objects.texture(gl)?;
                gl.bind_texture(glow::TEXTURE_2D, Some(texture));
                gl.tex_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    glow::RGBA32F as i32,
                    width as i32,
                    height.min(ROWS) as i32,
                    0,
                    glow::RGBA,
                    glow::FLOAT,
                    glow::PixelUnpackData::Slice(None),
                );
                gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_MIN_FILTER,
                    glow::NEAREST as i32,
                );
                gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_MAG_FILTER,
                    glow::NEAREST as i32,
                );
                gl.framebuffer_texture_2d(
                    glow::FRAMEBUFFER,
                    glow::COLOR_ATTACHMENT0 + index,
                    glow::TEXTURE_2D,
                    Some(texture),
                    0,
                );
            }
            gl.draw_buffers(&[
                glow::COLOR_ATTACHMENT0,
                glow::COLOR_ATTACHMENT1,
                glow::COLOR_ATTACHMENT2,
                glow::COLOR_ATTACHMENT3,
            ]);
            if gl.check_framebuffer_status(glow::FRAMEBUFFER) != glow::FRAMEBUFFER_COMPLETE {
                return Err("Could not allocate the GPU material bake targets.".into());
            }
            let owner = objects.texture(gl)?;
            gl.bind_texture(glow::TEXTURE_2D, Some(owner));
            let (owner_size, owners) = match &request.layout {
                Layout::Plate(owners) => {
                    if owners.len() != count
                        || owners
                            .iter()
                            .any(|&v| v == 0 || v as usize > request.surfaces.len())
                    {
                        return Err("The bake ownership map is incomplete.".into());
                    }
                    (request.size, owners.as_slice())
                }
                Layout::Charts { cell } => {
                    if *cell < 16 || width % cell != 0 || height % cell != 0 {
                        return Err("The bake chart dimensions are invalid.".into());
                    }
                    ([1, 1], &[0_u32][..])
                }
            };
            let owner_bytes: Vec<u8> = owners
                .iter()
                .flat_map(|value| value.to_ne_bytes())
                .collect();
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::R32UI as i32,
                owner_size[0] as i32,
                owner_size[1] as i32,
                0,
                glow::RED_INTEGER,
                glow::UNSIGNED_INT,
                glow::PixelUnpackData::Slice(Some(&owner_bytes)),
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MIN_FILTER,
                glow::NEAREST as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MAG_FILTER,
                glow::NEAREST as i32,
            );
            let mut rows = Vec::with_capacity(request.surfaces.len() * 6);
            for surface in &request.surfaces {
                let indices = model
                    .triangles
                    .get(surface.triangle)
                    .ok_or("A bake triangle is missing.")?;
                rows.push([
                    f32::from(surface.dye),
                    f32::from(u8::from(surface.clip)),
                    f32::from(u8::from(surface.detail.is_some())),
                    surface.cutoff,
                ]);
                for m in surface.detail.unwrap_or([[0.0; 3]; 2]) {
                    rows.push([m[0], m[1], m[2], 0.0]);
                }
                for &index in indices {
                    let uv = model
                        .uvs
                        .get(index as usize)
                        .ok_or("A bake texture coordinate is missing.")?;
                    let detail = model
                        .detail_uvs
                        .get(index as usize)
                        .copied()
                        .unwrap_or([0.0; 2]);
                    rows.push([uv[0], uv[1], detail[0], detail[1]]);
                }
            }
            let surfaces = objects.table(gl, &rows, rows.len())?;
            let plate = request.plate;
            let mut roles = BTreeSet::from([(plate.albedo, true)]);
            roles.extend(
                [
                    plate.gearstack,
                    plate.normal,
                    plate.dye_map.map(|map| map.texture),
                ]
                .into_iter()
                .flatten()
                .map(|index| (index, false)),
            );
            let slots: BTreeSet<_> = request.surfaces.iter().map(|surface| surface.dye).collect();
            for dye in slots
                .into_iter()
                .filter_map(|slot| request.dyes.get(slot as usize).and_then(Option::as_ref))
            {
                roles.extend(dye.detail.map(|index| (index, true)));
                roles.extend(dye.normal.map(|index| (index, false)));
            }
            let mut textures = BTreeMap::new();
            for key in roles {
                let texture = model
                    .textures
                    .get(key.0)
                    .ok_or("A baked material texture is missing.")?;
                textures.insert(key, objects.image(gl, texture, key.1)?);
            }
            let painted = Painted {
                color: vec![0; count * 4],
                channels: vec![255; count * 3],
                normal: vec![0; count * 3],
                emission: vec![[0.0; 3]; count],
            };
            Ok(Self {
                objects: resource::Objects::default(),
                program,
                vao,
                framebuffer,
                surfaces,
                owner,
                textures,
                request,
                row: 0,
                painted,
            })
        }
    }

    pub unsafe fn step(&mut self, gl: &glow::Context) -> Result<bool, String> {
        // SAFETY: the renderer owns the bound MRT attachments and readback buffers.
        unsafe {
            let [width, height] = self.request.size;
            let rows = (height - self.row).min(ROWS);
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.framebuffer));
            gl.viewport(0, 0, width as i32, rows as i32);
            gl.disable(glow::SCISSOR_TEST);
            gl.disable(glow::DEPTH_TEST);
            gl.disable(glow::CULL_FACE);
            gl.disable(glow::BLEND);
            gl.disable(glow::FRAMEBUFFER_SRGB);
            gl.disable(glow::RASTERIZER_DISCARD);
            gl.polygon_mode(glow::FRONT_AND_BACK, glow::FILL);
            gl.color_mask(true, true, true, true);
            for index in 0..4 {
                gl.clear_buffer_f32_slice(
                    glow::COLOR,
                    index,
                    if index == 1 { &[1.0; 4] } else { &[0.0; 4] },
                );
            }
            gl.use_program(Some(self.program));
            gl.bind_vertex_array(Some(self.vao));
            gl.uniform_2_i32(
                gl.get_uniform_location(self.program, "uSize").as_ref(),
                width as i32,
                height as i32,
            );
            self.int(gl, "uRow", self.row as i32);
            self.int(gl, "uCount", self.request.surfaces.len() as i32);
            self.int(
                gl,
                "uCell",
                match self.request.layout {
                    Layout::Plate(_) => 0,
                    Layout::Charts { cell } => cell as i32,
                },
            );
            self.surfaces.bind(gl, self.program, "uSurfaces", 6);
            gl.active_texture(glow::TEXTURE7);
            gl.bind_sampler(7, None);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.owner));
            self.int(gl, "uOwner", 7);
            let slots: BTreeSet<_> = self.request.surfaces.iter().map(|s| s.dye).collect();
            for slot in slots {
                self.bind(gl, slot);
                gl.draw_arrays(glow::TRIANGLES, 0, 3);
            }
            gl.pixel_store_i32(glow::PACK_ALIGNMENT, 1);
            let mut values = vec![0.0_f32; width * rows * 4];
            for attachment in 0..4 {
                gl.read_buffer(glow::COLOR_ATTACHMENT0 + attachment);
                gl.read_pixels(
                    0,
                    0,
                    width as i32,
                    rows as i32,
                    glow::RGBA,
                    glow::FLOAT,
                    glow::PixelPackData::Slice(Some(float_bytes_mut(&mut values))),
                );
                for (local, pixel) in values.chunks_exact(4).enumerate() {
                    let at = self.row * width + local;
                    match attachment {
                        0 => self.painted.color[at * 4..at * 4 + 4].copy_from_slice(&[
                            shader::encode(pixel[0]),
                            shader::encode(pixel[1]),
                            shader::encode(pixel[2]),
                            unorm(pixel[3]),
                        ]),
                        1 => self.painted.channels[at * 3..at * 3 + 3].copy_from_slice(&[
                            unorm(pixel[0]),
                            unorm(pixel[1]),
                            unorm(pixel[2]),
                        ]),
                        2 => self.painted.normal[at * 3..at * 3 + 3].copy_from_slice(&[
                            unorm(pixel[0]),
                            unorm(pixel[1]),
                            unorm(pixel[2]),
                        ]),
                        _ => self.painted.emission[at] = [pixel[0], pixel[1], pixel[2]],
                    }
                }
            }
            if gl.get_error() != glow::NO_ERROR {
                return Err("The GPU material bake failed during drawing or readback.".into());
            }
            self.row += rows;
            Ok(self.row == height)
        }
    }

    unsafe fn bind(&self, gl: &glow::Context, slot: u8) {
        // SAFETY: uniforms and texture handles all belong to this renderer's current program.
        unsafe {
            let plate = self.request.plate;
            let dye = self
                .request
                .dyes
                .get(slot as usize)
                .and_then(Option::as_ref);
            self.int(gl, "uSlot", slot as i32);
            let sources = [
                Some(plate.albedo),
                plate.gearstack,
                plate.normal,
                dye.and_then(|d| d.detail),
                dye.and_then(|d| d.normal),
                plate.dye_map.map(|m| m.texture),
            ];
            for (unit, (source, name)) in sources
                .into_iter()
                .zip([
                    "Albedo",
                    "Gear",
                    "Normal",
                    "Detail",
                    "DetailNormal",
                    "DyeMap",
                ])
                .enumerate()
            {
                gl.active_texture(glow::TEXTURE0 + unit as u32);
                gl.bind_sampler(unit as u32, None);
                gl.bind_texture(
                    glow::TEXTURE_2D,
                    source.and_then(|index| {
                        self.textures.get(&(index, matches!(unit, 0 | 3))).copied()
                    }),
                );
                self.int(gl, &format!("u{name}"), unit as i32);
                self.int(gl, &format!("uHas{name}"), i32::from(source.is_some()));
            }
            self.int(gl, "uHasDye", i32::from(dye.is_some()));
            self.int(gl, "uSkipNormal", i32::from(plate.no_basis));
            self.int(gl, "uHasPaint", i32::from(plate.paint.is_some()));
            self.vector(
                gl,
                "uPaint",
                &plate
                    .paint
                    .map(|v| v.map(f32::from_bits))
                    .unwrap_or([0.0; 2]),
            );
            self.int(gl, "uHasGain", i32::from(plate.base_gain.is_some()));
            self.vector(
                gl,
                "uGain",
                &plate
                    .base_gain
                    .map(|v| v.map(f32::from_bits))
                    .unwrap_or([1.0; 3]),
            );
            self.int(gl, "uHasMetal", i32::from(plate.base_metal.is_some()));
            self.vector(
                gl,
                "uMetal",
                &[plate.base_metal.map(f32::from_bits).unwrap_or(0.0)],
            );
            let channel = slot as usize / 2;
            let legacy = plate.legacy_normal.map(|v| v.map(f32::from_bits));
            let detail = dye.map_or([2.0, -1.0, 0.0, 0.0], |d| d.vectors[2]);
            let decode = plate
                .normal_decode
                .map(|v| {
                    [
                        v[0].map(f32::from_bits),
                        v.get(channel + 1)
                            .copied()
                            .unwrap_or([2.0f32.to_bits(), (-1.0f32).to_bits()])
                            .map(f32::from_bits),
                    ]
                })
                .or_else(|| legacy.map(|v| [[v[0], v[1]], [detail[0], detail[1]]]));
            self.int(gl, "uHasDecode", i32::from(decode.is_some()));
            self.vector(
                gl,
                "uDecode",
                decode.unwrap_or([[2.0, -1.0]; 2]).as_flattened(),
            );
            self.int(gl, "uLegacy", i32::from(legacy.is_some()));
            self.vector(
                gl,
                "uLegacyOffsets",
                &[legacy.map_or(0.0, |v| v[2]), detail[2]],
            );
            let grain = plate
                .grain
                .and_then(|v| v.get(channel).copied())
                .map(f32::from_bits);
            self.int(gl, "uHasGrain", i32::from(grain.is_some()));
            self.vector(gl, "uGrain", &[grain.unwrap_or(0.0)]);
            if let Some(map) = plate.dye_map {
                self.vector(gl, "uMapTransform", &map.transform.map(f32::from_bits));
            }
            if let Some(dye) = dye {
                let surface = &dye.surface;
                for (name, value) in [
                    ("uDyeAlbedo", surface.albedo),
                    ("uDyeWorn", surface.worn_albedo),
                    ("uEmissive", surface.emissive),
                ] {
                    self.vector(gl, name, &value);
                }
                for (name, value) in [
                    ("uParams", surface.params),
                    ("uWornParams", surface.worn_params),
                    ("uRough", surface.roughness),
                    ("uWornRough", surface.worn_roughness),
                    ("uWear", surface.wear),
                    ("uDetailTransform", dye.transform),
                    ("uNormalTransform", dye.normal_transform),
                ] {
                    self.vector(gl, name, &value);
                }
            }
        }
    }

    unsafe fn int(&self, gl: &glow::Context, name: &str, value: i32) {
        // SAFETY: program is active and locations belong to it.
        unsafe { gl.uniform_1_i32(gl.get_uniform_location(self.program, name).as_ref(), value) }
    }
    unsafe fn vector(&self, gl: &glow::Context, name: &str, value: &[f32]) {
        // SAFETY: each caller supplies the vector size declared in this shader.
        unsafe {
            let location = gl.get_uniform_location(self.program, name);
            match value {
                [x] => gl.uniform_1_f32(location.as_ref(), *x),
                [x, y] => gl.uniform_2_f32(location.as_ref(), *x, *y),
                [x, y, z] => gl.uniform_3_f32(location.as_ref(), *x, *y, *z),
                [x, y, z, w] => gl.uniform_4_f32(location.as_ref(), *x, *y, *z, *w),
                _ => unreachable!("material uniform arity"),
            }
        }
    }
    pub unsafe fn finish(self, gl: &glow::Context) -> Painted {
        // SAFETY: completion and cancellation run on the painter's context.
        unsafe { self.objects.delete(gl) };
        self.painted
    }
}

fn unorm(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}
fn float_bytes_mut(values: &mut [f32]) -> &mut [u8] {
    // SAFETY: every float bit pattern is valid and the byte view has the exact allocation length.
    unsafe {
        std::slice::from_raw_parts_mut(values.as_mut_ptr().cast(), std::mem::size_of_val(values))
    }
}

const VERTEX: &str = r#"#version 330 core
void main(){vec2 p=vec2(float((gl_VertexID<<1)&2),float(gl_VertexID&2));gl_Position=vec4(p*2.0-1.0,0.0,1.0);}
"#;

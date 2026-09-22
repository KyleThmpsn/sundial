//! OpenGL preview through egui's glow painter: native resolution, mipmapped plates and
//! multisampled edges. The fragment shader is a port of `shader.rs`; the CPU rasterizer
//! stays as the fallback for tests and non-GL backends.
use super::{
    Model,
    render::{Camera, Style},
    shader,
};
use eframe::{
    egui,
    glow::{self, HasContext},
};
use std::{
    num::NonZeroU32,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

static AVAILABLE: AtomicBool = AtomicBool::new(false);

/// Set once at start-up from `CreationContext::gl`.
pub fn set_available(available: bool) {
    AVAILABLE.store(available, Ordering::Relaxed);
}

pub fn available() -> bool {
    AVAILABLE.load(Ordering::Relaxed)
}

/// What one frame draws.
pub(crate) struct Frame {
    pub model: Arc<Model>,
    pub camera: Camera,
    pub style: Style,
    pub seconds: f32,
    /// Skinned positions for this instant; `None` draws the bind pose with smooth normals.
    pub positions: Option<Arc<Vec<[f32; 3]>>>,
}

/// GL objects shared by every frame of one preview. Touched only on the paint thread.
#[derive(Clone, Default)]
pub(crate) struct Shared(Arc<Mutex<State>>);

impl Shared {
    pub fn paint(&self, ui: &egui::Ui, rect: egui::Rect, frame: Frame) {
        let state = self.0.clone();
        let callback = eframe::egui_glow::CallbackFn::new(move |info, painter| {
            if let Ok(mut state) = state.lock() {
                // SAFETY: the painter hands us its own current context on the paint thread.
                unsafe { state.draw(painter.gl(), &info, &frame) };
            }
        });
        ui.painter().add(egui::Shape::Callback(egui::PaintCallback {
            rect,
            callback: Arc::new(callback),
        }));
    }
}

#[derive(Default)]
struct State {
    program: Option<(glow::Program, Uniforms)>,
    model: Option<Uploaded>,
    target: Option<Target>,
}

struct Uniforms {
    center: Option<glow::UniformLocation>,
    rotate: Option<glow::UniformLocation>,
    scale: Option<glow::UniformLocation>,
    depth_scale: Option<glow::UniformLocation>,
    style: Option<glow::UniformLocation>,
    flat: Option<glow::UniformLocation>,
    samplers: [Option<glow::UniformLocation>; 6],
    has: [Option<glow::UniformLocation>; 9],
    constant: Option<glow::UniformLocation>,
    iridescence_id: Option<glow::UniformLocation>,
    dye_albedo: Option<glow::UniformLocation>,
    dye_worn: Option<glow::UniformLocation>,
    emissive: Option<glow::UniformLocation>,
    params: Option<glow::UniformLocation>,
    worn_params: Option<glow::UniformLocation>,
    rough: Option<glow::UniformLocation>,
    worn_rough: Option<glow::UniformLocation>,
    wear: Option<glow::UniformLocation>,
    detail_transform: Option<glow::UniformLocation>,
    normal_transform: Option<glow::UniformLocation>,
}

struct Uploaded {
    key: usize,
    vao: glow::VertexArray,
    positions: glow::Buffer,
    attributes: glow::Buffer,
    textures: Vec<glow::Texture>,
    lookup: Option<glow::Texture>,
    groups: Vec<Group>,
    /// Triangle indices in draw order, for expanding skinned positions.
    order: Vec<u32>,
    center: [f32; 3],
    radius: f32,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Key {
    /// First so emissive panels sort after the surfaces they sit on.
    constant: Option<[u32; 3]>,
    albedo: Option<usize>,
    gearstack: Option<usize>,
    normal: Option<usize>,
    slot: u8,
    clip: bool,
}

struct Group {
    key: Key,
    first: i32,
    count: i32,
}

struct Target {
    fbo: glow::Framebuffer,
    color: glow::Renderbuffer,
    depth: glow::Renderbuffer,
    size: [i32; 2],
}

const BACKGROUND: [f32; 3] = [24.0 / 255.0, 28.0 / 255.0, 35.0 / 255.0];

impl State {
    #[expect(
        clippy::cognitive_complexity,
        reason = "One GL pass: upload, animate, bind, draw groups, blit"
    )]
    unsafe fn draw(&mut self, gl: &glow::Context, info: &egui::PaintCallbackInfo, frame: &Frame) {
        // SAFETY: egui hands us its live GL context inside the paint callback; every object
        // used here was created on this context and is deleted through `Uploaded::delete`.
        unsafe {
            let viewport = info.viewport_in_pixels();
            let size = [viewport.width_px.max(1), viewport.height_px.max(1)];
            // Read before any of our own bindings; the target is created below.
            let previous = gl.get_parameter_i32(glow::DRAW_FRAMEBUFFER_BINDING);
            let previous = NonZeroU32::new(previous as u32).map(glow::NativeFramebuffer);
            if self.program.is_none() {
                self.program = compile(gl);
            }
            let Some((program, uniforms)) = self.program.as_ref() else {
                return;
            };
            let key = Arc::as_ptr(&frame.model) as usize;
            if self.model.as_ref().is_some_and(|m| m.key != key) {
                if let Some(old) = self.model.take() {
                    old.delete(gl);
                }
            }
            if self.model.is_none() {
                self.model = Some(upload(gl, &frame.model, key));
            }
            let Some(uploaded) = self.model.as_ref() else {
                return;
            };
            if let Some(positions) = &frame.positions {
                let expanded = expand_positions(&uploaded.order, &frame.model, positions);
                gl.bind_buffer(glow::ARRAY_BUFFER, Some(uploaded.positions));
                gl.buffer_sub_data_u8_slice(glow::ARRAY_BUFFER, 0, bytes_of(&expanded));
            } else if !uploaded.order.is_empty() && frame.model.animation.is_some() {
                let expanded =
                    expand_positions(&uploaded.order, &frame.model, &frame.model.vertices);
                gl.bind_buffer(glow::ARRAY_BUFFER, Some(uploaded.positions));
                gl.buffer_sub_data_u8_slice(glow::ARRAY_BUFFER, 0, bytes_of(&expanded));
            }
            if self.target.as_ref().is_none_or(|t| t.size != size) {
                if let Some(old) = self.target.take() {
                    old.delete(gl);
                }
                self.target = Target::new(gl, size);
            }
            let Some(target) = self.target.as_ref() else {
                return;
            };

            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(target.fbo));
            gl.viewport(0, 0, size[0], size[1]);
            gl.disable(glow::SCISSOR_TEST);
            gl.disable(glow::BLEND);
            gl.disable(glow::CULL_FACE);
            gl.enable(glow::DEPTH_TEST);
            // Equal passes so emissive panels coincident with a surface win by draw order.
            gl.depth_func(glow::LEQUAL);
            gl.depth_mask(true);
            gl.clear_color(BACKGROUND[0], BACKGROUND[1], BACKGROUND[2], 1.0);
            gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);

            gl.use_program(Some(*program));
            gl.bind_vertex_array(Some(uploaded.vao));
            let (sy, cy) = frame.camera.yaw.sin_cos();
            let (sp, cp) = frame.camera.pitch.sin_cos();
            // Same rotation as the CPU rasterizer, with y up instead of down.
            let rotate = [
                cy,
                sp * sy,
                cp * sy, // column 0
                -sy,
                sp * cy,
                cp * cy, // column 1
                0.0,
                cp,
                -sp, // column 2
            ];
            let scale = size[0].min(size[1]) as f32 * 0.43 * frame.camera.zoom / uploaded.radius;
            gl.uniform_3_f32(
                uniforms.center.as_ref(),
                uploaded.center[0],
                uploaded.center[1],
                uploaded.center[2],
            );
            gl.uniform_matrix_3_f32_slice(uniforms.rotate.as_ref(), false, &rotate);
            gl.uniform_2_f32(
                uniforms.scale.as_ref(),
                2.0 * scale / size[0] as f32,
                2.0 * scale / size[1] as f32,
            );
            gl.uniform_1_f32(uniforms.depth_scale.as_ref(), 1.0 / uploaded.radius);
            gl.uniform_1_i32(
                uniforms.style.as_ref(),
                match frame.style {
                    Style::Textured => 0,
                    Style::Solid => 1,
                    Style::Wireframe => 2,
                },
            );
            let flat = frame.positions.is_some() || frame.model.normals.is_empty();
            gl.uniform_1_i32(uniforms.flat.as_ref(), i32::from(flat));
            for (unit, location) in uniforms.samplers.iter().enumerate() {
                gl.uniform_1_i32(location.as_ref(), unit as i32);
            }
            if frame.style == Style::Wireframe {
                gl.polygon_mode(glow::FRONT_AND_BACK, glow::LINE);
            }
            let dyes = shader::dyes(&frame.model, frame.seconds);
            for group in &uploaded.groups {
                let dye = dyes
                    .get(usize::from(group.key.slot))
                    .and_then(Option::as_ref);
                let textures = [
                    group.key.albedo,
                    group.key.gearstack,
                    group.key.normal,
                    dye.and_then(|d| d.detail),
                    dye.and_then(|d| d.normal),
                ];
                for (unit, texture) in textures.iter().enumerate() {
                    gl.active_texture(glow::TEXTURE0 + unit as u32);
                    gl.bind_texture(
                        glow::TEXTURE_2D,
                        texture.and_then(|i| uploaded.textures.get(i).copied()),
                    );
                }
                let has = [
                    textures[0].is_some(),
                    textures[1].is_some(),
                    textures[2].is_some(),
                    textures[3].is_some(),
                    textures[4].is_some(),
                    dye.is_some(),
                    group.key.clip,
                    uploaded.lookup.is_some(),
                    group.key.constant.is_some(),
                ];
                if let Some(constant) = group.key.constant.map(|c| c.map(f32::from_bits)) {
                    gl.uniform_3_f32(
                        uniforms.constant.as_ref(),
                        constant[0],
                        constant[1],
                        constant[2],
                    );
                }
                gl.active_texture(glow::TEXTURE5);
                gl.bind_texture(glow::TEXTURE_2D, uploaded.lookup);
                for (location, value) in uniforms.has.iter().zip(has) {
                    gl.uniform_1_i32(location.as_ref(), i32::from(value));
                }
                if let Some(dye) = dye {
                    let s = &dye.surface;
                    gl.uniform_3_f32(
                        uniforms.dye_albedo.as_ref(),
                        s.albedo[0],
                        s.albedo[1],
                        s.albedo[2],
                    );
                    gl.uniform_3_f32(
                        uniforms.dye_worn.as_ref(),
                        s.worn_albedo[0],
                        s.worn_albedo[1],
                        s.worn_albedo[2],
                    );
                    gl.uniform_3_f32(
                        uniforms.emissive.as_ref(),
                        s.emissive[0],
                        s.emissive[1],
                        s.emissive[2],
                    );
                    gl.uniform_4_f32_slice(uniforms.params.as_ref(), &s.params);
                    gl.uniform_4_f32_slice(uniforms.worn_params.as_ref(), &s.worn_params);
                    gl.uniform_4_f32_slice(uniforms.rough.as_ref(), &s.roughness);
                    gl.uniform_4_f32_slice(uniforms.worn_rough.as_ref(), &s.worn_roughness);
                    gl.uniform_4_f32_slice(uniforms.wear.as_ref(), &s.wear);
                    gl.uniform_4_f32_slice(uniforms.detail_transform.as_ref(), &dye.transform);
                    gl.uniform_4_f32_slice(
                        uniforms.normal_transform.as_ref(),
                        &dye.normal_transform,
                    );
                    gl.uniform_1_f32(uniforms.iridescence_id.as_ref(), s.iridescence);
                }
                gl.draw_arrays(glow::TRIANGLES, group.first, group.count);
            }
            gl.polygon_mode(glow::FRONT_AND_BACK, glow::FILL);
            gl.bind_vertex_array(None);
            gl.use_program(None);
            gl.disable(glow::DEPTH_TEST);
            gl.active_texture(glow::TEXTURE0);

            // Resolve into egui's framebuffer, clipped by the scissor the painter left set.
            gl.enable(glow::SCISSOR_TEST);
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, Some(target.fbo));
            gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, previous);
            gl.blit_framebuffer(
                0,
                0,
                size[0],
                size[1],
                viewport.left_px,
                viewport.from_bottom_px,
                viewport.left_px + size[0],
                viewport.from_bottom_px + size[1],
                glow::COLOR_BUFFER_BIT,
                glow::NEAREST,
            );
            gl.bind_framebuffer(glow::FRAMEBUFFER, previous);
        }
    }
}

fn bytes_of(values: &[[f32; 3]]) -> &[u8] {
    // SAFETY: `[f32; 3]` is plain data with no padding.
    unsafe { std::slice::from_raw_parts(values.as_ptr().cast::<u8>(), values.len() * 12) }
}

fn expand_positions(order: &[u32], model: &Model, positions: &[[f32; 3]]) -> Vec<[f32; 3]> {
    let mut out = Vec::with_capacity(order.len() * 3);
    for &triangle in order {
        for vertex in model.triangles[triangle as usize] {
            out.push(positions.get(vertex as usize).copied().unwrap_or_default());
        }
    }
    out
}

unsafe fn upload(gl: &glow::Context, model: &Model, key: usize) -> Uploaded {
    // SAFETY: called from the paint callback with the live context; buffers are sized from
    // the slices uploaded and stay owned by the returned `Uploaded`.
    unsafe {
        let mut low = [f32::INFINITY; 3];
        let mut high = [f32::NEG_INFINITY; 3];
        for index in model.triangles.iter().flatten() {
            let vertex = model.vertices[*index as usize];
            for axis in 0..3 {
                low[axis] = low[axis].min(vertex[axis]);
                high[axis] = high[axis].max(vertex[axis]);
            }
        }
        let center: [f32; 3] = std::array::from_fn(|axis| (low[axis] + high[axis]) * 0.5);
        let radius = (0..3)
            .map(|axis| (high[axis] - low[axis]).powi(2))
            .sum::<f32>()
            .sqrt()
            .max(0.0001)
            * 0.5;

        let mut order: Vec<u32> = (0..model.triangles.len() as u32).collect();
        let key_of = |triangle: usize| Key {
            albedo: model.triangle_textures.get(triangle).copied().flatten(),
            gearstack: model.triangle_gearstacks.get(triangle).copied().flatten(),
            normal: model.triangle_normals.get(triangle).copied().flatten(),
            slot: model.triangle_dyes.get(triangle).copied().unwrap_or(0),
            clip: model.triangle_clip.get(triangle).copied().unwrap_or(false),
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
        let mut attributes: Vec<f32> = Vec::with_capacity(order.len() * 15);
        for &triangle in &order {
            let corners = model.triangles[triangle as usize];
            let flat = {
                let [a, b, c] = corners.map(|v| model.vertices[v as usize]);
                let ab: [f32; 3] = std::array::from_fn(|i| b[i] - a[i]);
                let ac: [f32; 3] = std::array::from_fn(|i| c[i] - a[i]);
                [
                    ab[1] * ac[2] - ab[2] * ac[1],
                    ab[2] * ac[0] - ab[0] * ac[2],
                    ab[0] * ac[1] - ab[1] * ac[0],
                ]
            };
            for vertex in corners {
                let normal = model.normals.get(vertex as usize).copied().unwrap_or(flat);
                let uv = model.uvs.get(vertex as usize).copied().unwrap_or_default();
                attributes.extend_from_slice(&[normal[0], normal[1], normal[2], uv[0], uv[1]]);
            }
        }

        let vao = gl.create_vertex_array().expect("vertex array");
        gl.bind_vertex_array(Some(vao));
        let position_buffer = gl.create_buffer().expect("buffer");
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(position_buffer));
        gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes_of(&positions), glow::DYNAMIC_DRAW);
        gl.enable_vertex_attrib_array(0);
        gl.vertex_attrib_pointer_f32(0, 3, glow::FLOAT, false, 12, 0);
        let attribute_buffer = gl.create_buffer().expect("buffer");
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(attribute_buffer));
        // `f32` slices are plain data.
        let attribute_bytes =
            std::slice::from_raw_parts(attributes.as_ptr().cast::<u8>(), attributes.len() * 4);
        gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, attribute_bytes, glow::STATIC_DRAW);
        gl.enable_vertex_attrib_array(1);
        gl.vertex_attrib_pointer_f32(1, 3, glow::FLOAT, false, 20, 0);
        gl.enable_vertex_attrib_array(2);
        gl.vertex_attrib_pointer_f32(2, 2, glow::FLOAT, false, 20, 12);
        gl.bind_vertex_array(None);

        // Colour plates and dye detail colour are sRGB; masks and normals are linear.
        let mut srgb = vec![false; model.textures.len()];
        for index in model.triangle_textures.iter().flatten() {
            srgb[*index] = true;
        }
        for dye in model.dyes.iter().flatten() {
            if let Some(index) = dye.detail {
                srgb[index] = true;
            }
        }
        for index in model
            .triangle_gearstacks
            .iter()
            .chain(model.triangle_normals.iter())
            .flatten()
        {
            srgb[*index] = false;
        }
        for dye in model.dyes.iter().flatten() {
            if let Some(index) = dye.normal {
                srgb[index] = false;
            }
        }
        let anisotropic = gl
            .supported_extensions()
            .contains("GL_EXT_texture_filter_anisotropic");
        let textures = model
            .textures
            .iter()
            .zip(srgb)
            .map(|(texture, srgb)| {
                let handle = gl.create_texture().expect("texture");
                gl.bind_texture(glow::TEXTURE_2D, Some(handle));
                gl.tex_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    if srgb {
                        glow::SRGB8_ALPHA8
                    } else {
                        glow::RGBA8
                    } as i32,
                    texture.size[0] as i32,
                    texture.size[1] as i32,
                    0,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(Some(&texture.rgba)),
                );
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
                gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::REPEAT as i32);
                gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::REPEAT as i32);
                if anisotropic {
                    const MAX_ANISOTROPY: u32 = 0x84FE;
                    gl.tex_parameter_f32(glow::TEXTURE_2D, MAX_ANISOTROPY, 8.0);
                }
                handle
            })
            .collect();
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

        Uploaded {
            key,
            vao,
            positions: position_buffer,
            attributes: attribute_buffer,
            textures,
            lookup,
            groups,
            order,
            center,
            radius,
        }
    }
}

impl Uploaded {
    unsafe fn delete(self, gl: &glow::Context) {
        // SAFETY: the objects were created on this context by `upload` and are dropped here.
        unsafe {
            gl.delete_vertex_array(self.vao);
            gl.delete_buffer(self.positions);
            gl.delete_buffer(self.attributes);
            for texture in self.textures {
                gl.delete_texture(texture);
            }
            if let Some(lookup) = self.lookup {
                gl.delete_texture(lookup);
            }
        }
    }
}

impl Target {
    unsafe fn new(gl: &glow::Context, size: [i32; 2]) -> Option<Self> {
        // SAFETY: live context from the paint callback; a failed attachment deletes what it made.
        unsafe {
            for samples in [4, 0] {
                let fbo = gl.create_framebuffer().ok()?;
                let color = gl.create_renderbuffer().ok()?;
                let depth = gl.create_renderbuffer().ok()?;
                gl.bind_renderbuffer(glow::RENDERBUFFER, Some(color));
                gl.renderbuffer_storage_multisample(
                    glow::RENDERBUFFER,
                    samples,
                    glow::RGBA8,
                    size[0],
                    size[1],
                );
                gl.bind_renderbuffer(glow::RENDERBUFFER, Some(depth));
                gl.renderbuffer_storage_multisample(
                    glow::RENDERBUFFER,
                    samples,
                    glow::DEPTH_COMPONENT24,
                    size[0],
                    size[1],
                );
                gl.bind_renderbuffer(glow::RENDERBUFFER, None);
                gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
                gl.framebuffer_renderbuffer(
                    glow::FRAMEBUFFER,
                    glow::COLOR_ATTACHMENT0,
                    glow::RENDERBUFFER,
                    Some(color),
                );
                gl.framebuffer_renderbuffer(
                    glow::FRAMEBUFFER,
                    glow::DEPTH_ATTACHMENT,
                    glow::RENDERBUFFER,
                    Some(depth),
                );
                let complete =
                    gl.check_framebuffer_status(glow::FRAMEBUFFER) == glow::FRAMEBUFFER_COMPLETE;
                if complete {
                    return Some(Self {
                        fbo,
                        color,
                        depth,
                        size,
                    });
                }
                gl.delete_framebuffer(fbo);
                gl.delete_renderbuffer(color);
                gl.delete_renderbuffer(depth);
            }
            None
        }
    }

    unsafe fn delete(self, gl: &glow::Context) {
        // SAFETY: the framebuffer and renderbuffers were created on this context by `new`.
        unsafe {
            gl.delete_framebuffer(self.fbo);
            gl.delete_renderbuffer(self.color);
            gl.delete_renderbuffer(self.depth);
        }
    }
}

unsafe fn compile(gl: &glow::Context) -> Option<(glow::Program, Uniforms)> {
    // SAFETY: live context; shaders are deleted after linking and the program on failure.
    unsafe {
        let program = gl.create_program().ok()?;
        for (kind, source) in [
            (glow::VERTEX_SHADER, VERTEX),
            (glow::FRAGMENT_SHADER, FRAGMENT),
        ] {
            let shader = gl.create_shader(kind).ok()?;
            gl.shader_source(shader, source);
            gl.compile_shader(shader);
            if !gl.get_shader_compile_status(shader) {
                eprintln!(
                    "Model preview shader failed to compile: {}",
                    gl.get_shader_info_log(shader)
                );
                gl.delete_shader(shader);
                gl.delete_program(program);
                return None;
            }
            gl.attach_shader(program, shader);
            gl.delete_shader(shader);
        }
        gl.link_program(program);
        if !gl.get_program_link_status(program) {
            eprintln!(
                "Model preview shader failed to link: {}",
                gl.get_program_info_log(program)
            );
            gl.delete_program(program);
            return None;
        }
        let location = |name: &str| gl.get_uniform_location(program, name);
        let uniforms = Uniforms {
            center: location("uCenter"),
            rotate: location("uRotate"),
            scale: location("uScale"),
            depth_scale: location("uDepthScale"),
            style: location("uStyle"),
            flat: location("uFlat"),
            samplers: [
                location("uAlbedo"),
                location("uGear"),
                location("uNormal"),
                location("uDetail"),
                location("uDetailNormal"),
                location("uIridescence"),
            ],
            has: [
                location("uHasAlbedo"),
                location("uHasGear"),
                location("uHasNormal"),
                location("uHasDetail"),
                location("uHasDetailNormal"),
                location("uHasDye"),
                location("uClip"),
                location("uHasIridescence"),
                location("uHasConstant"),
            ],
            constant: location("uConstant"),
            iridescence_id: location("uIridescenceId"),
            dye_albedo: location("uDyeAlbedo"),
            dye_worn: location("uDyeWorn"),
            emissive: location("uEmissive"),
            params: location("uParams"),
            worn_params: location("uWornParams"),
            rough: location("uRough"),
            worn_rough: location("uWornRough"),
            wear: location("uWear"),
            detail_transform: location("uDetailTransform"),
            normal_transform: location("uNormalTransform"),
        };
        Some((program, uniforms))
    }
}

const VERTEX: &str = r#"#version 330 core
layout(location = 0) in vec3 aPosition;
layout(location = 1) in vec3 aNormal;
layout(location = 2) in vec2 aUv;
uniform vec3 uCenter;
uniform mat3 uRotate;
uniform vec2 uScale;
uniform float uDepthScale;
out vec3 vView;
out vec3 vNormal;
out vec2 vUv;
void main() {
    vec3 p = uRotate * (aPosition - uCenter);
    vView = p;
    vNormal = uRotate * aNormal;
    vUv = aUv;
    gl_Position = vec4(p.x * uScale.x, p.y * uScale.y, p.z * uDepthScale, 1.0);
}
"#;

const FRAGMENT: &str = r#"#version 330 core
in vec3 vView;
in vec3 vNormal;
in vec2 vUv;
uniform sampler2D uAlbedo;
uniform sampler2D uGear;
uniform sampler2D uNormal;
uniform sampler2D uDetail;
uniform sampler2D uDetailNormal;
uniform sampler2D uIridescence;
uniform int uHasAlbedo, uHasGear, uHasNormal, uHasDetail, uHasDetailNormal, uHasDye, uClip, uHasIridescence, uHasConstant;
uniform vec3 uConstant;
uniform float uIridescenceId;
uniform int uStyle;
uniform int uFlat;
uniform vec3 uDyeAlbedo, uDyeWorn, uEmissive;
uniform vec4 uParams, uWornParams, uRough, uWornRough, uWear, uDetailTransform, uNormalTransform;
out vec4 fragColor;

const vec3 KEY = vec3(-0.35, 0.55, -0.76);
const vec3 HALF = vec3(-0.187, 0.293, -0.937);

float sat(float v) { return clamp(v, 0.0, 1.0); }
float remap(float v, vec4 m) {
    float e = m.z + m.w;
    return clamp(v * m.y + m.x, min(m.z, e), max(m.z, e));
}
float overlay(float base, float blend) { return blend * sat(base * 4.0) + sat(base - 0.25); }
vec3 overlay3(vec3 base, vec3 blend) {
    return vec3(overlay(base.r, blend.r), overlay(base.g, blend.g), overlay(base.b, blend.b));
}
vec3 encode(vec3 v) {
    v = clamp(v, 0.0, 1.0);
    vec3 lo = v * 12.92;
    vec3 hi = 1.055 * pow(v, vec3(1.0 / 2.4)) - 0.055;
    return mix(lo, hi, step(vec3(0.0031308), v));
}

vec3 light(vec3 albedo, float rough, float metal, float ao, vec3 emission, vec3 n, vec3 tint) {
    if (n.z > 0.0) n = -n;
    float diffuse = max(dot(n, KEY), 0.0);
    float half_ = max(dot(n, HALF), 0.0);
    float r = clamp(rough, 0.06, 1.0);
    float exponent = clamp(2.0 / (r * r) - 2.0, 1.0, 512.0);
    float highlight = pow(half_, exponent) * (1.2 - 0.8 * r);
    float fresnel = pow(1.0 - abs(n.z), 5.0);
    vec3 specular = mix(vec3(0.04), albedo, metal);
    vec3 diff = albedo * (1.0 - metal) * (0.30 * ao + 0.70 * diffuse);
    vec3 refl = tint * specular * (0.32 * ao + highlight * 2.0) + fresnel * 0.18 * ao;
    return diff + refl + emission;
}

void main() {
    vec2 uv = vUv;
    if (uClip == 1 && uHasGear == 1 && texture(uGear, uv).b * 7.96875 < 0.5) discard;
    vec3 dp1 = dFdx(vView);
    vec3 dp2 = dFdy(vView);
    vec3 face = normalize(cross(dp1, dp2));
    vec3 n = uFlat == 1 ? face : normalize(vNormal);
    if (uStyle == 2) {
        fragColor = vec4(180.0 / 255.0, 215.0 / 255.0, 245.0 / 255.0, 1.0);
        return;
    }
    if (n.z > 0.0) n = -n;
    float lighting = 0.30 + 0.70 * min(abs(dot(n, KEY)), 1.0);
    if (uStyle == 0 && uHasConstant == 1) {
        // Panel art lives in the colour plate: alpha cuts the segments, colour tints them.
        vec4 base = uHasAlbedo == 1 ? texture(uAlbedo, uv) : vec4(1.0);
        if (base.a < 0.5) discard;
        fragColor = vec4(encode(uConstant * base.rgb), 1.0);
        return;
    }
    if (uStyle == 1 || (uHasAlbedo == 0 && uHasDye == 0)) {
        fragColor = vec4(vec3(205.0, 216.0, 230.0) / 255.0 * lighting, 1.0);
        return;
    }
    if (uHasAlbedo == 0) {
        fragColor = vec4(encode(uDyeAlbedo * lighting), 1.0);
        return;
    }
    vec3 base = texture(uAlbedo, uv).rgb;
    vec4 mask = texture(uGear, uv) * 255.0;
    vec3 albedo = base;
    float rough = 0.6;
    float metal = 0.0;
    float ao = 1.0;
    vec3 emission = vec3(0.0);
    vec3 dyeColor = vec3(1.0);
    bool painted = false;
    bool dyed = uHasDye == 1 && uHasGear == 1;
    if (dyed) {
        rough = 1.0 - mask.g / 255.0;
        metal = sat(mask.a / 32.0);
        ao = sat(mask.r / 255.0);
        emission = base * sat((mask.b - 40.0) / 215.0);
        if (mask.a >= 40.0) {
            float intact = sat(remap(sat((mask.a - 48.0) / 207.0), uWear));
            vec4 params = mix(uWornParams, uParams, intact);
            dyeColor = mix(uDyeWorn, uDyeAlbedo, intact);
            painted = true;
            albedo = overlay3(base, dyeColor);
            float smoothness = mask.g / 255.0;
            if (uHasDetail == 1) {
                vec4 detail = texture(uDetail, uv * 5.0 * uDetailTransform.xy + uDetailTransform.zw);
                albedo = mix(albedo, overlay3(detail.rgb, albedo), sat(params.x));
                smoothness = mix(smoothness, overlay(smoothness, detail.a), sat(params.z));
            }
            rough = 1.0 - sat(mix(remap(smoothness, uWornRough), remap(smoothness, uRough), intact));
            metal = sat(params.w);
            emission = uEmissive * sat((mask.b - 40.0) / 215.0);
        }
    } else if (uHasNormal == 0) {
        // No material information: the CPU path lights the colour texture directly.
        fragColor = vec4(encode(base) * lighting, 1.0);
        return;
    }
    if (uHasNormal == 1) {
        vec2 duv1 = dFdx(uv);
        vec2 duv2 = dFdy(uv);
        float det = duv1.x * duv2.y - duv2.x * duv1.y;
        if (abs(det) > 1e-12) {
            vec3 t = normalize((dp1 * duv2.y - dp2 * duv1.y) / det);
            vec3 b = normalize((dp2 * duv1.x - dp1 * duv2.x) / det);
            vec3 nn = n;
            if (dot(nn, face) < 0.0) nn = -nn;
            t = normalize(t - nn * dot(t, nn));
            vec3 bb = cross(nn, t);
            if (dot(bb, b) < 0.0) bb = -bb;
            vec4 sampled = texture(uNormal, uv);
            vec2 xy = sampled.rg;
            ao *= sampled.b;
            if (dyed && uHasDetailNormal == 1 && mask.a >= 40.0) {
                float intact = sat(remap(sat((mask.a - 48.0) / 207.0), uWear));
                float strength = clamp(mix(uWornParams.y, uParams.y, intact), 0.0, 4.0);
                vec4 detail = texture(uDetailNormal, uv * 5.0 * uNormalTransform.xy + uNormalTransform.zw);
                vec2 blended = mix(2.0 * xy * detail.rg, 1.0 - 2.0 * (1.0 - xy) * (1.0 - detail.rg), step(0.5, xy));
                xy = mix(xy, blended, strength);
                ao *= mix(1.0, detail.b, min(strength, 1.0));
            }
            vec2 p = clamp(xy * 2.0 - 1.0, -1.0, 1.0);
            float z = sqrt(max(1.0 - dot(p, p), 0.0));
            n = normalize(t * p.x + bb * p.y + nn * z);
        }
    }
    vec3 tint = vec3(1.0);
    if (painted && uHasIridescence == 1 && uIridescenceId >= 0.0) {
        float nDotV = sat(abs(n.z));
        // Row per id, top down. Unused rows hold a magenta placeholder.
        vec4 iri = texture(uIridescence, vec2(nDotV, (uIridescenceId + 0.5) / 128.0));
        bool placeholder = iri.r > 0.98 && iri.g < 0.02 && iri.b > 0.98;
        float strength = placeholder ? 0.0 : 1.0 - dot(dyeColor, vec3(0.2126, 0.7152, 0.0722));
        if (placeholder) {
        } else if (mod(uIridescenceId, 2.0) < 0.5) {
            albedo = mix(albedo, iri.rgb, strength);
            metal = mix(metal, 1.0, strength);
        } else {
            tint = mix(vec3(1.0), iri.rgb, strength);
        }
    }
    fragColor = vec4(encode(light(albedo, rough, metal, ao, emission, n, tint)), 1.0);
}
"#;

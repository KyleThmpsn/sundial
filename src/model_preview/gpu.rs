//! OpenGL preview through egui's glow painter: native resolution, mipmapped plates and
//! multisampled edges. The fragment shader is a port of `shader.rs`; the CPU rasterizer
//! stays as the fallback for tests and non-GL backends.
use super::{
    Model,
    render::{Camera, Scene, Style},
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

mod draw;
mod resolve;
mod upload;
#[cfg(all(test, windows))]
mod verification;

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
    pub scene: Scene,
    pub style: Style,
    pub seconds: f32,
    /// Skinned positions and normals for this instant. `None` draws the bind pose.
    pub pose: Option<Arc<super::animation::Deformed>>,
}

/// GL objects shared by every frame of one preview. Touched only on the paint thread.
#[derive(Clone, Default)]
pub(crate) struct Shared(Arc<Mutex<State>>);

impl Shared {
    pub fn fallback(&self, model: &Arc<Model>) -> Option<String> {
        self.0.lock().ok().and_then(|state| {
            state
                .fallback
                .as_ref()
                .filter(|(source, _)| Arc::ptr_eq(source, model))
                .map(|(_, reason)| reason.clone())
        })
    }
    pub fn paint(&self, ui: &egui::Ui, rect: egui::Rect, frame: Frame) {
        let state = self.0.clone();
        let (repaint, viewport) = (ui.ctx().clone(), ui.ctx().viewport_id());
        let callback = eframe::egui_glow::CallbackFn::new(move |info, painter| {
            if let Ok(mut state) = state.lock() {
                // SAFETY: the painter hands us its own current context on the paint thread.
                if !unsafe { state.draw(painter.gl(), &info, &frame) } {
                    repaint
                        .request_repaint_after_for(std::time::Duration::from_millis(16), viewport);
                }
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
    fallback: Option<(Arc<Model>, String)>,
    program: Option<(glow::Program, Uniforms)>,
    program_model: Option<Arc<Model>>,
    model: Option<Uploaded>,
    target: Option<Target>,
    preparation: upload::Preparation,
}

struct Uniforms {
    center: Option<glow::UniformLocation>,
    rotate: Option<glow::UniformLocation>,
    scale: Option<glow::UniformLocation>,
    depth_scale: Option<glow::UniformLocation>,
    style: Option<glow::UniformLocation>,
    flat: Option<glow::UniformLocation>,
    pan: Option<glow::UniformLocation>,
    key_dir: Option<glow::UniformLocation>,
    half_dir: Option<glow::UniformLocation>,
    key: Option<glow::UniformLocation>,
    fill: Option<glow::UniformLocation>,
    exposure: Option<glow::UniformLocation>,
    samplers: [Option<glow::UniformLocation>; 6],
    has: [Option<glow::UniformLocation>; 9],
    dye_map: Option<glow::UniformLocation>,
    has_dye_map: Option<glow::UniformLocation>,
    map_transform: Option<glow::UniformLocation>,
    map_slot: Option<glow::UniformLocation>,
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
    native_detail: Option<glow::UniformLocation>,
    cutoff: Option<glow::UniformLocation>,
    normal_transform: Option<glow::UniformLocation>,
    has_legacy_normal: Option<glow::UniformLocation>,
    legacy_normal: Option<glow::UniformLocation>,
    legacy_detail: Option<glow::UniformLocation>,
    effect: Option<glow::UniformLocation>,
    effect_constants: Option<glow::UniformLocation>,
    effect_textures: [Option<glow::UniformLocation>; 3],
    scene_depth: Option<glow::UniformLocation>,
    native_index: Option<glow::UniformLocation>,
    native_quaternion: Option<glow::UniformLocation>,
    native_vertex_constants: Option<glow::UniformLocation>,
    native_dye: Option<glow::UniformLocation>,
    native_direction: Option<glow::UniformLocation>,
    native_distance: Option<glow::UniformLocation>,
    native_present: Option<glow::UniformLocation>,
}

struct Uploaded {
    /// The model these buffers hold. Kept alive so no later model can take its address and
    /// be drawn with this upload's triangle order.
    model: Arc<Model>,
    vao: glow::VertexArray,
    positions: glow::Buffer,
    attributes: glow::Buffer,
    posed: bool,
    /// One image may serve both data and color roles. Keep a separate upload for each role.
    textures: Vec<[Option<glow::Texture>; 2]>,
    lookup: Option<glow::Texture>,
    groups: Vec<Group>,
    /// Triangle indices in draw order, for expanding skinned positions.
    order: Vec<u32>,
    /// Textured object framing followed by stored mesh inspection framing.
    framing: [([f32; 3], f32); 2],
    hide_emitter: bool,
    samplers: Vec<Vec<glow::Sampler>>,
    pending: Option<upload::Pending>,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Key {
    /// Native transparents follow every opaque surface, including emissive panels.
    effect: Option<usize>,
    /// Keep particle and object draws distinct even when their materials match.
    emitter: bool,
    /// Emissive panels sort after the surfaces they sit on.
    constant: Option<[u32; 3]>,
    albedo: Option<usize>,
    gearstack: Option<usize>,
    normal: Option<usize>,
    slot: u8,
    clip: bool,
    dye_map: Option<super::texture::DyeMap>,
    native_detail: bool,
    cutoff: Option<u32>,
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
    scene_depth: glow::Texture,
    scene_fbo: glow::Framebuffer,
    resolve: resolve::Resolve,
}

impl State {
    unsafe fn prepare(&mut self, gl: &glow::Context, frame: &Frame) -> bool {
        // SAFETY: called only from draw with its live context. Uploads and deletes use
        // that same context and preparation itself runs without GL on a worker.
        unsafe {
            if self
                .fallback
                .as_ref()
                .is_some_and(|(model, _)| Arc::ptr_eq(model, &frame.model))
            {
                return false;
            }
            self.fallback = None;
            let maximum = gl.get_parameter_i32(glow::MAX_TEXTURE_SIZE).max(0) as usize;
            if frame
                .model
                .textures
                .iter()
                .chain(frame.model.iridescence.iter())
                .any(|texture| texture.size.iter().any(|&axis| axis > maximum))
            {
                self.fallback = Some((
                    frame.model.clone(),
                    format!(
                        "Software rendering preserves textures above this GPU's {maximum}-pixel limit."
                    ),
                ));
                return false;
            }
            if self
                .program_model
                .as_ref()
                .is_some_and(|model| !Arc::ptr_eq(model, &frame.model))
            {
                if let Some((program, _)) = self.program.take() {
                    gl.delete_program(program);
                }
            }
            if self.program.is_none() {
                self.program = compile(gl, &frame.model);
                self.program_model = Some(frame.model.clone());
            }
            if self.program.is_none() {
                self.fallback = Some((frame.model.clone(), "Software rendering is active because this model's GPU shader could not initialize.".into()));
                return false;
            }
            if self
                .model
                .as_ref()
                .is_some_and(|m| !Arc::ptr_eq(&m.model, &frame.model))
            {
                if let Some(old) = self.model.take() {
                    old.delete(gl);
                }
            }
            if self.model.is_none() {
                let Some(prepared) = self.preparation.poll(&frame.model) else {
                    return false;
                };
                self.model = Some(upload::begin(gl, prepared));
            }
            let Some(uploaded) = self.model.as_mut() else {
                return false;
            };
            uploaded.advance(gl)
        }
    }

    unsafe fn draw(
        &mut self,
        gl: &glow::Context,
        info: &egui::PaintCallbackInfo,
        frame: &Frame,
    ) -> bool {
        // SAFETY: egui hands us its live GL context inside the paint callback; every object
        // used here was created on this context and is deleted through `Uploaded::delete`.
        unsafe {
            let viewport = info.viewport_in_pixels();
            let size = [viewport.width_px.max(1), viewport.height_px.max(1)];
            // Read before any of our own bindings; the target is created below.
            let previous = gl.get_parameter_i32(glow::DRAW_FRAMEBUFFER_BINDING);
            let previous = NonZeroU32::new(previous as u32).map(glow::NativeFramebuffer);
            if !self.prepare(gl, frame) {
                return false;
            }
            let (program, uniforms) = self.program.as_ref().expect("prepared program");
            let uploaded = self.model.as_mut().expect("prepared model");
            if frame.pose.is_some() || uploaded.posed {
                let positions = frame
                    .pose
                    .as_ref()
                    .map_or(frame.model.vertices.as_slice(), |pose| {
                        pose.positions.as_slice()
                    });
                let normals = frame
                    .pose
                    .as_ref()
                    .map_or(frame.model.normals.as_slice(), |pose| {
                        pose.normals.as_slice()
                    });
                let expanded = expand_positions(&uploaded.order, &frame.model, positions);
                gl.bind_buffer(glow::ARRAY_BUFFER, Some(uploaded.positions));
                gl.buffer_sub_data_u8_slice(
                    glow::ARRAY_BUFFER,
                    0,
                    bytes_of(expanded.as_flattened()),
                );
                let attributes = expand_attributes(
                    &uploaded.order,
                    &frame.model,
                    positions,
                    normals,
                    frame
                        .pose
                        .as_ref()
                        .map_or(frame.model.tangents.as_slice(), |p| p.tangents.as_slice()),
                );
                gl.bind_buffer(glow::ARRAY_BUFFER, Some(uploaded.attributes));
                gl.buffer_sub_data_u8_slice(glow::ARRAY_BUFFER, 0, bytes_of(&attributes));
            }
            uploaded.posed = frame.pose.is_some();
            if self.target.as_ref().is_none_or(|t| t.size != size) {
                if let Some(old) = self.target.take() {
                    old.delete(gl);
                }
                self.target = Target::new(gl, size);
                gl.bind_framebuffer(glow::FRAMEBUFFER, previous);
            }
            let Some(target) = self.target.as_ref() else {
                self.fallback = Some((frame.model.clone(), "Software rendering is active because the GPU could not initialize a float color target.".into()));
                return false;
            };

            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(target.fbo));
            gl.viewport(0, 0, size[0], size[1]);
            gl.disable(glow::SCISSOR_TEST);
            gl.disable(glow::BLEND);
            gl.disable(glow::FRAMEBUFFER_SRGB);
            gl.disable(glow::CULL_FACE);
            gl.enable(glow::DEPTH_TEST);
            // Equal passes so emissive panels coincident with a surface win by draw order.
            gl.depth_func(glow::LEQUAL);
            gl.depth_mask(true);
            let background = frame
                .scene
                .background
                .map(|v| shader::linear(f32::from(v) / 255.0));
            gl.clear_color(background[0], background[1], background[2], 1.0);
            gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);

            gl.use_program(Some(*program));
            gl.bind_vertex_array(Some(uploaded.vao));
            let (bind_center, radius) =
                uploaded.framing[usize::from(frame.style != Style::Textured)];
            let (sy, cy) = frame.camera.yaw.sin_cos();
            let (sp, cp) = frame.camera.pitch.sin_cos();
            gl.uniform_3_f32(uniforms.native_direction.as_ref(), -cp * sy, -cp * cy, sp);
            gl.uniform_1_f32(uniforms.native_distance.as_ref(), (radius * 4.0).max(1.0));
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
            let scale =
                size[0].min(size[1]) as f32 * super::render::RADIUS_SCALE * frame.camera.zoom
                    / radius;
            let center = frame
                .pose
                .as_ref()
                .map_or(bind_center, |pose| pose.framing_center(bind_center));
            gl.uniform_3_f32(uniforms.center.as_ref(), center[0], center[1], center[2]);
            gl.uniform_matrix_3_f32_slice(uniforms.rotate.as_ref(), false, &rotate);
            gl.uniform_2_f32(
                uniforms.scale.as_ref(),
                2.0 * scale / size[0] as f32,
                2.0 * scale / size[1] as f32,
            );
            // Keep the stable bind-pose screen scale, but include every posed vertex in
            // the depth range. Bone motion can extend far beyond the stored bounding sphere.
            let depth_radius = frame.pose.as_ref().map_or(radius, |pose| {
                pose.positions.iter().fold(radius, |radius, point| {
                    let [x, y, z] =
                        std::array::from_fn::<_, 3, _>(|axis| point[axis] - center[axis]);
                    radius.max((cp * (sy * x + cy * y) - sp * z).abs())
                })
            });
            gl.uniform_1_f32(uniforms.depth_scale.as_ref(), (1.0 - 1e-4) / depth_radius);
            // Pan is a fraction of the viewport; clip space spans two units and points up.
            gl.uniform_2_f32(
                uniforms.pan.as_ref(),
                frame.camera.pan[0] * 2.0,
                -frame.camera.pan[1] * 2.0,
            );
            let key = frame.scene.light;
            // The highlight sits halfway between the key and the fixed head-on view direction.
            let half = {
                let sum = [key[0], key[1], key[2] - 1.0];
                let length = sum.iter().map(|v| v * v).sum::<f32>().sqrt().max(0.0001);
                sum.map(|v| v / length)
            };
            gl.uniform_3_f32(uniforms.key_dir.as_ref(), key[0], key[1], key[2]);
            gl.uniform_3_f32(uniforms.half_dir.as_ref(), half[0], half[1], half[2]);
            gl.uniform_1_f32(uniforms.key.as_ref(), frame.scene.key);
            gl.uniform_1_f32(uniforms.fill.as_ref(), frame.scene.fill);
            gl.uniform_1_f32(uniforms.exposure.as_ref(), frame.scene.exposure);
            gl.uniform_1_i32(
                uniforms.style.as_ref(),
                match frame.style {
                    Style::Textured => 0,
                    Style::Solid => 1,
                    Style::Wireframe => 2,
                },
            );
            let flat = frame
                .pose
                .as_ref()
                .map_or(frame.model.normals.is_empty(), |pose| {
                    pose.normals.is_empty()
                });
            gl.uniform_1_i32(uniforms.flat.as_ref(), i32::from(flat));
            for (unit, location) in uniforms.samplers.iter().enumerate() {
                gl.uniform_1_i32(location.as_ref(), unit as i32);
            }
            if frame.style == Style::Wireframe {
                gl.polygon_mode(glow::FRONT_AND_BACK, glow::LINE);
            }
            draw::groups(gl, uniforms, uploaded, frame, target);
            gl.polygon_mode(glow::FRONT_AND_BACK, glow::FILL);
            gl.disable(glow::FRAMEBUFFER_SRGB);
            gl.disable(glow::BLEND);
            gl.depth_mask(true);
            for unit in 0..10 {
                gl.bind_sampler(unit, None);
            }
            gl.bind_vertex_array(None);
            gl.use_program(None);
            gl.disable(glow::DEPTH_TEST);
            gl.active_texture(glow::TEXTURE0);

            target.resolve.paint(
                gl,
                target,
                previous,
                [viewport.left_px, viewport.from_bottom_px],
            );
            true
        }
    }
}

fn bytes_of(values: &[f32]) -> &[u8] {
    // SAFETY: Every initialized f32 has exactly four bytes and no padding.
    unsafe {
        std::slice::from_raw_parts(values.as_ptr().cast::<u8>(), std::mem::size_of_val(values))
    }
}

fn expand_attributes(
    order: &[u32],
    model: &Model,
    positions: &[[f32; 3]],
    normals: &[[f32; 3]],
    tangents: &[[f32; 4]],
) -> Vec<f32> {
    let mut attributes = Vec::with_capacity(order.len() * 48);
    for &triangle in order {
        let corners = model.triangles[triangle as usize];
        let [a, b, c] = corners.map(|v| positions.get(v as usize).copied().unwrap_or_default());
        let ab: [f32; 3] = std::array::from_fn(|i| b[i] - a[i]);
        let ac: [f32; 3] = std::array::from_fn(|i| c[i] - a[i]);
        let flat = [
            ab[1] * ac[2] - ab[2] * ac[1],
            ab[2] * ac[0] - ab[0] * ac[2],
            ab[0] * ac[1] - ab[1] * ac[0],
        ];
        let native = super::effects::index(model, triangle as usize)
            .and_then(|index| model.effects[index].native.as_ref())
            .is_some();
        let stored_basis = corners.iter().all(|&v| {
            normals
                .get(v as usize)
                .zip(tangents.get(v as usize))
                .and_then(|(&n, &t)| shader::normal::Basis::stored(n, t))
                .is_some()
        });
        for vertex in corners {
            let mut normal = normals.get(vertex as usize).copied().unwrap_or(flat);
            if native || stored_basis {
                normal = shader::normal::normalize(normal)
                    .or_else(|| shader::normal::normalize(flat))
                    .unwrap_or([0.0, 0.0, 1.0]);
            }
            let uv = model.uvs.get(vertex as usize).copied().unwrap_or_default();
            let detail = model.detail_uvs.get(vertex as usize).copied().unwrap_or(uv);
            attributes.extend_from_slice(&[
                normal[0], normal[1], normal[2], uv[0], uv[1], detail[0], detail[1],
            ]);
            let tangent = super::effects::native::tangent(tangents, vertex as usize, normal);
            attributes.extend_from_slice(&tangent);
            attributes.extend_from_slice(
                &model
                    .colors
                    .get(vertex as usize)
                    .copied()
                    .unwrap_or([1.0; 4]),
            );
            attributes.push(f32::from(u8::from(stored_basis)));
        }
    }
    attributes
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

/// Center and half diagonal of the triangles drawn, leaving out light volumes when hidden.
fn bounds(model: &Model, style: Style) -> ([f32; 3], f32) {
    let (low, high) = super::render::drawn_bounds(model, style);
    let center: [f32; 3] = std::array::from_fn(|axis| (low[axis] + high[axis]) * 0.5);
    let radius = (0..3)
        .map(|axis| (high[axis] - low[axis]).powi(2))
        .sum::<f32>()
        .sqrt()
        .max(0.0001)
        * 0.5;
    (center, radius)
}

impl Uploaded {
    unsafe fn delete(self, gl: &glow::Context) {
        // SAFETY: the objects were created on this context by `upload` and are dropped here.
        unsafe {
            gl.delete_vertex_array(self.vao);
            gl.delete_buffer(self.positions);
            gl.delete_buffer(self.attributes);
            for sampler in self.samplers.into_iter().flatten() {
                gl.delete_sampler(sampler);
            }
            for texture in self.textures.into_iter().flatten().flatten() {
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
                let color = match gl.create_renderbuffer() {
                    Ok(value) => value,
                    Err(_) => {
                        gl.delete_framebuffer(fbo);
                        return None;
                    }
                };
                let depth = match gl.create_renderbuffer() {
                    Ok(value) => value,
                    Err(_) => {
                        gl.delete_renderbuffer(color);
                        gl.delete_framebuffer(fbo);
                        return None;
                    }
                };
                gl.bind_renderbuffer(glow::RENDERBUFFER, Some(color));
                gl.renderbuffer_storage_multisample(
                    glow::RENDERBUFFER,
                    samples,
                    glow::RGBA32F,
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
                    let scene_depth = match gl.create_texture() {
                        Ok(value) => value,
                        Err(_) => {
                            gl.delete_renderbuffer(depth);
                            gl.delete_renderbuffer(color);
                            gl.delete_framebuffer(fbo);
                            return None;
                        }
                    };
                    gl.bind_texture(glow::TEXTURE_2D, Some(scene_depth));
                    gl.tex_image_2d(
                        glow::TEXTURE_2D,
                        0,
                        glow::DEPTH_COMPONENT24 as i32,
                        size[0],
                        size[1],
                        0,
                        glow::DEPTH_COMPONENT,
                        glow::UNSIGNED_INT,
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
                    gl.bind_texture(glow::TEXTURE_2D, None);
                    let scene_fbo = match gl.create_framebuffer() {
                        Ok(value) => value,
                        Err(_) => {
                            gl.delete_texture(scene_depth);
                            gl.delete_renderbuffer(depth);
                            gl.delete_renderbuffer(color);
                            gl.delete_framebuffer(fbo);
                            return None;
                        }
                    };
                    gl.bind_framebuffer(glow::FRAMEBUFFER, Some(scene_fbo));
                    gl.framebuffer_texture_2d(
                        glow::FRAMEBUFFER,
                        glow::DEPTH_ATTACHMENT,
                        glow::TEXTURE_2D,
                        Some(scene_depth),
                        0,
                    );
                    gl.draw_buffer(glow::NONE);
                    gl.read_buffer(glow::NONE);
                    if gl.check_framebuffer_status(glow::FRAMEBUFFER) != glow::FRAMEBUFFER_COMPLETE
                    {
                        gl.delete_framebuffer(scene_fbo);
                        gl.delete_texture(scene_depth);
                        gl.delete_framebuffer(fbo);
                        gl.delete_renderbuffer(color);
                        gl.delete_renderbuffer(depth);
                        continue;
                    }
                    let Some(resolve) = resolve::Resolve::new(gl, size) else {
                        gl.delete_framebuffer(scene_fbo);
                        gl.delete_texture(scene_depth);
                        gl.delete_framebuffer(fbo);
                        gl.delete_renderbuffer(color);
                        gl.delete_renderbuffer(depth);
                        return None;
                    };
                    return Some(Self {
                        fbo,
                        color,
                        depth,
                        size,
                        scene_depth,
                        scene_fbo,
                        resolve,
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
            gl.delete_framebuffer(self.scene_fbo);
            gl.delete_texture(self.scene_depth);
            self.resolve.delete(gl);
        }
    }
}

unsafe fn link(gl: &glow::Context, vertex: &str, fragment: &str) -> Option<glow::Program> {
    // SAFETY: shaders and the program are created and released on the supplied context.
    unsafe {
        let program = gl.create_program().ok()?;
        for (kind, source) in [
            (glow::VERTEX_SHADER, vertex),
            (glow::FRAGMENT_SHADER, fragment),
        ] {
            let shader = match gl.create_shader(kind) {
                Ok(value) => value,
                Err(_) => {
                    gl.delete_program(program);
                    return None;
                }
            };
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
        Some(program)
    }
}

unsafe fn compile(gl: &glow::Context, model: &Model) -> Option<(glow::Program, Uniforms)> {
    // SAFETY: live context; shaders are deleted after linking and the program on failure.
    unsafe {
        let fragment = FRAGMENT.replace(
            "// EFFECT FUNCTIONS",
            &format!(
                "{}\n{}",
                include_str!("effects/shader.glsl"),
                super::effects::native::source(model, false)
            ),
        );
        let vertex = VERTEX.replace(
            "// NATIVE VERTEX FUNCTIONS",
            &super::effects::native::source(model, true),
        );
        let program = link(gl, &vertex, &fragment)?;
        let location = |name: &str| gl.get_uniform_location(program, name);
        let uniforms = Uniforms {
            center: location("uCenter"),
            rotate: location("uRotate"),
            scale: location("uScale"),
            depth_scale: location("uDepthScale"),
            style: location("uStyle"),
            pan: location("uPan"),
            key_dir: location("uKeyDir"),
            half_dir: location("uHalfDir"),
            key: location("uKey"),
            fill: location("uFill"),
            exposure: location("uExposure"),
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
            dye_map: location("uDyeMap"),
            has_dye_map: location("uHasDyeMap"),
            map_transform: location("uMapTransform"),
            map_slot: location("uMapSlot"),
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
            native_detail: location("uNativeDetail"),
            cutoff: location("uCutoff"),
            normal_transform: location("uNormalTransform"),
            has_legacy_normal: location("uHasLegacyNormal"),
            legacy_normal: location("uLegacyNormal"),
            legacy_detail: location("uLegacyDetail"),
            effect: location("uEffect"),
            effect_constants: location("uEffectConstants[0]"),
            effect_textures: [
                location("uEffectTexture0"),
                location("uEffectTexture1"),
                location("uEffectTexture2"),
            ],
            scene_depth: location("uSceneDepth"),
            native_index: location("uNativeIndex"),
            native_quaternion: location("uNativeQuaternion"),
            native_vertex_constants: location("uNativeVertexConstants[0]"),
            native_dye: location("uNativeDye[0]"),
            native_direction: location("uNativeDirection"),
            native_distance: location("uNativeDistance"),
            native_present: location("uNativePresent[0]"),
        };
        Some((program, uniforms))
    }
}

const VERTEX: &str = r#"#version 330 core
layout(location = 0) in vec3 aPosition;
layout(location = 1) in vec3 aNormal;
layout(location = 2) in vec2 aUv;
layout(location = 3) in vec2 aDetailUv;
layout(location = 4) in vec4 aTangent;
layout(location = 5) in vec4 aColor;
layout(location = 6) in float aBasisValid;
uniform vec3 uCenter;
uniform mat3 uRotate;
uniform vec2 uScale;
uniform vec2 uPan;
uniform float uDepthScale;
out vec3 vView;
out vec3 vNormal;
out vec2 vUv;
out vec2 vDetailUv;
out vec4 vNative[9];
flat out float vBasisValid;
// NATIVE VERTEX FUNCTIONS
void main() {
    vec3 position=aPosition,normal=aNormal;
    nativeVertex(position,normal);
    vec3 p = uRotate * (position - uCenter);
    vView = p;
    vNormal = uRotate * normal;
    vBasisValid = aBasisValid;
    vUv = aUv;
    vDetailUv = aDetailUv;
    gl_Position = vec4(p.x * uScale.x + uPan.x, p.y * uScale.y + uPan.y, p.z * uDepthScale, 1.0);
}
"#;

const FRAGMENT: &str = r#"#version 330 core
in vec3 vView;
in vec3 vNormal;
in vec2 vUv;
in vec4 vNative[9];
flat in float vBasisValid;
uniform mat3 uRotate;
uniform sampler2D uAlbedo;
uniform sampler2D uGear;
uniform sampler2D uNormal;
uniform sampler2D uDetail;
uniform sampler2D uDetailNormal;
uniform sampler2D uIridescence;
uniform sampler2D uDyeMap;
uniform int uHasDyeMap, uMapSlot;
uniform int uNativeDetail;
uniform float uCutoff;
uniform vec4 uMapTransform;
uniform int uHasAlbedo, uHasGear, uHasNormal, uHasDetail, uHasDetailNormal, uHasDye, uClip, uHasIridescence, uHasConstant;
uniform vec3 uConstant;
uniform float uIridescenceId;
uniform int uStyle;
uniform int uFlat;
uniform vec3 uDyeAlbedo, uDyeWorn, uEmissive;
uniform vec4 uParams, uWornParams, uRough, uWornRough, uWear, uDetailTransform, uNormalTransform;
uniform int uHasLegacyNormal;
uniform vec3 uLegacyNormal, uLegacyDetail;
out vec4 fragColor;

uniform vec3 uKeyDir, uHalfDir;
uniform float uKey, uFill, uExposure;

float sat(float v) { return clamp(v, 0.0, 1.0); }
float remap(float v, vec4 m) {
    return sat(m.z + m.w * sat(v * m.y + m.x));
}
float overlay(float base, float blend) { return blend * sat(base * 4.0) + sat(base - 0.25); }
vec3 overlay3(vec3 base, vec3 blend) {
    return vec3(overlay(base.r, blend.r), overlay(base.g, blend.g), overlay(base.b, blend.b));
}
vec3 legacyColor(vec3 base, vec4 detail, vec3 dye, float strength) {
    vec3 detailed = clamp(overlay3(detail.rgb,dye),0.0,1.0);
    return overlay3(base,mix(dye,detailed,strength));
}
float paintRemap(float value, vec4 map) { return sat(map.z + map.w * sat(map.x + map.y * value)); }
vec3 paintColor(vec3 base, vec4 detail, vec3 dye, float strength) {
    vec3 detailed = clamp(overlay3(detail.rgb, dye), 0.0, 1.0);
    return clamp(overlay3(base, mix(dye, detailed, sat(strength))), 0.0, 1.0);
}
float paintSmooth(float raw, float detail, vec4 map, float strength) {
    return mix(paintRemap(raw, map), paintRemap(sat(overlay(raw, detail)), map), sat(strength));
}
vec3 linear(vec3 v) {
    v = clamp(v, 0.0, 1.0);
    vec3 lo = v / 12.92;
    vec3 hi = pow((v + 0.055) / 1.055, vec3(2.4));
    return mix(lo, hi, step(vec3(0.04045), v));
}

// Mirrors shader.rs `value_noise`.
float hashCell(int x, int y) {
    uint v = uint(x) * 0x8DA6B343u ^ uint(y) * 0xD8163841u ^ 0x2C1B3C6Du;
    v ^= v >> 13u;
    v *= 0x5BD1E995u;
    v ^= v >> 15u;
    return float(v & 0xFFFFu) / 65535.0;
}
float valueNoise(vec2 p) {
    vec2 cell = floor(p);
    vec2 f = p - cell;
    f = f * f * (3.0 - 2.0 * f);
    int cx = int(cell.x);
    int cy = int(cell.y);
    return mix(mix(hashCell(cx, cy), hashCell(cx + 1, cy), f.x),
               mix(hashCell(cx, cy + 1), hashCell(cx + 1, cy + 1), f.x), f.y);
}

// Mirrors shader.rs `studio`. View space points y up, where the CPU raster points it down.
float studio(vec3 n, float rough) {
    float facing = -n.z;
    vec3 r = vec3(2.0 * facing * n.x, 2.0 * facing * n.y, 2.0 * facing * n.z + 1.0);
    float height = r.y;
    float gradient = mix(0.08, 0.6, sat(0.5 + 0.6 * height));
    float softbox = 1.4 * exp(-(1.0 - dot(r, uKeyDir)) / 0.06);
    float clouds = (valueNoise(vec2(r.x * 3.0 + 7.0, -height * 3.0 + 7.0)) - 0.5) * 0.204;
    float sharp = max(gradient + softbox + clouds, 0.0);
    return mix(sharp, 0.32, sat(rough * 2.5));
}

vec3 light(vec3 albedo, float rough, float metal, float ao, vec3 emission, vec3 n, vec3 tint) {
    if (n.z > 0.0) n = -n;
    float diffuse = max(dot(n, uKeyDir), 0.0);
    float half_ = max(dot(n, uHalfDir), 0.0);
    float r = clamp(rough, 0.06, 1.0);
    float glint = max(r, 0.2);
    float exponent = clamp(2.0 / (glint * glint) - 2.0, 1.0, 512.0);
    float highlight = pow(half_, exponent) * (1.2 - 0.8 * glint);
    float fresnel = pow(1.0 - abs(n.z), 5.0);
    float surroundings = studio(n, r);
    vec3 specular = mix(vec3(0.04), albedo, metal);
    vec3 diff = albedo * (1.0 - metal) * (uFill * ao + uKey * diffuse);
    vec3 refl = tint * specular * (surroundings * ao + highlight * 2.0) + fresnel * 0.18 * ao;
    return (diff + refl + emission) * uExposure;
}

// EFFECT FUNCTIONS
void main() {
    if(uStyle==0 && uNativeIndex>=0 && uEffect>=0){fragColor=nativePixel(vec3(0.0));return;}
    vec2 uv = vUv;
    if (uHasDyeMap == 1 && uStyle == 0) {
        vec3 map = texture(uDyeMap, uv * uMapTransform.xy + uMapTransform.zw).rgb;
        float third = map.g - map.b < (1.2 / 255.0) ? map.g : map.b;
        int bank = third >= 0.5 ? 2 : (map.g >= 0.5 ? 1 : 0);
        int slot = bank * 2 + (map.r >= 0.5 ? 1 : 0);
        if (slot != uMapSlot) discard;
    }
    if (uClip == 1 && uHasGear == 1 && texture(uGear, uv).b * 7.96875 < uCutoff) discard;
    vec3 dp1 = dFdx(vView);
    vec3 dp2 = dFdy(vView);
    vec3 face = normalize(cross(dp1, dp2));
    bool flatNormal = uFlat == 1 || dot(vNormal, vNormal) < 1e-12;
    vec3 n = flatNormal ? face : normalize(vNormal);
    if (flatNormal && n.z > 0.0) n = -n;
    if (uStyle == 0 && uEffect >= 0) {
        fragColor = effectColor(uv, n);
        return;
    }
    if (uStyle == 2) {
        fragColor = vec4(linear(vec3(180.0, 215.0, 245.0) / 255.0), 1.0);
        return;
    }
    float lighting = uFill + uKey * min(abs(dot(n, uKeyDir)), 1.0);
    if (uStyle == 0 && uHasConstant == 1) {
        // Panel art lives in the colour plate: alpha cuts the segments, colour tints them.
        vec4 base = uHasAlbedo == 1 ? texture(uAlbedo, uv) : vec4(1.0);
        if (base.a < 0.5) discard;
        fragColor = vec4(uConstant * base.rgb * uExposure, 1.0);
        return;
    }
    if (uStyle == 1 || (uHasAlbedo == 0 && uHasDye == 0)) {
        fragColor = vec4(linear(vec3(205.0, 216.0, 230.0) / 255.0 * lighting * uExposure), 1.0);
        return;
    }
    if (uHasAlbedo == 0) {
        fragColor = vec4(uDyeAlbedo * lighting * uExposure, 1.0);
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
    bool nativePaint = uNativeIndex>=0 && uEffect<0 && nHasPaint();
    bool legacyNormal = uHasLegacyNormal == 1 && uEffect < 0;
    vec2 nativeSmooth = nativePaint ? nPaintSmooth() : vec2(0.0);
    if (dyed) {
        rough = nativePaint ? 1.0 - nativeSmooth.y : 1.0 - mask.g / 255.0;
        metal = sat(mask.a / 32.0);
        ao = sat(mask.r / 255.0);
        emission = base * sat((mask.b - 40.0) / 215.0);
        if (mask.a >= 40.0) {
            float wear = sat((mask.a - 48.0) / 207.0);
            float intact = nativePaint ? paintRemap(wear, uWear) : sat(remap(wear, uWear));
            vec4 params = nativePaint ? mix(clamp(uWornParams,0.0,1.0),clamp(uParams,0.0,1.0),intact) : mix(uWornParams, uParams, intact);
            dyeColor = mix(uDyeWorn, uDyeAlbedo, intact);
            painted = true;
            albedo = overlay3(base, dyeColor);
            float smoothness = mask.g / 255.0;
            vec4 detail = vec4(0.25);
            if (uHasDetail == 1) {
                vec2 detailUv = uNativeDetail == 1 ? vNative[3].zw : uv * 5.0;
                detail = texture(uDetail, detailUv * uDetailTransform.xy + uDetailTransform.zw);
                smoothness = mix(smoothness, overlay(smoothness, detail.a), sat(params.z));
            }
            albedo = mix(legacyColor(base,detail,uDyeWorn,sat(uWornParams.x)),legacyColor(base,detail,uDyeAlbedo,uParams.x),intact);
            rough = 1.0 - sat(mix(remap(smoothness, uWornRough), remap(smoothness, uRough), intact));
            if (nativePaint) {
                albedo = mix(paintColor(base,detail,uDyeWorn,uWornParams.x),paintColor(base,detail,uDyeAlbedo,uParams.x),intact);
                rough = 1.0 - mix(paintSmooth(nativeSmooth.x,detail.a,uWornRough,uWornParams.z),paintSmooth(nativeSmooth.x,detail.a,uRough,uParams.z),intact);
            }
            metal = sat(params.w);
            emission = uEmissive * sat((mask.b - 40.0) / 215.0);
            if (uNativeIndex<0 && !nativePaint && all(equal(emission,vec3(0.0)))) {
                float peak = max(albedo.r,max(albedo.g,albedo.b));
                albedo *= 1.0-sat(peak-1.0);
                albedo /= max(1.0,max(albedo.r,max(albedo.g,albedo.b)));
            }
        }
    } else if (uHasNormal == 0 && !(nativePaint && uHasGear == 1)) {
        // No material information: the CPU path lights the colour texture directly.
        fragColor = vec4(base * lighting * uExposure, 1.0);
        return;
    }
    if (nativePaint && uHasGear == 1 && mask.a < 40.0) {
        rough = 1.0 - nativeSmooth.y;
        if (nHasBaseMetal()) metal = nBaseMetal();
    }
    if (nativePaint && dyed && uHasDetailNormal == 1 && mask.a >= 40.0 && nHasNormalGrain()) {
        float intact = paintRemap(sat((mask.a - 48.0) / 207.0),uWear);
        float strength = mix(sat(uWornParams.y),sat(uParams.y),intact);
        vec2 detailUv = uNativeDetail == 1 ? vNative[3].zw : uv * 5.0;
        float blue = texture(uDetailNormal,detailUv*uNormalTransform.xy+uNormalTransform.zw).b;
        float limit = mix(1.0,sat(blue+nNormalGrain()),strength);
        rough = max(rough,1.0-limit);
    }
    if (legacyNormal && uHasNormal == 1) {
        float limit = sat(texture(uNormal,uv).b+uLegacyNormal.z);
        if (dyed && uHasDetailNormal == 1 && !isnan(uLegacyDetail.z) && !isinf(uLegacyDetail.z)) {
            float intact = paintRemap(sat((mask.a-48.0)/207.0),uWear);
            float strength = mix(sat(uWornParams.y),uParams.y,intact);
            vec2 detailUv = uNativeDetail == 1 ? vNative[3].zw : uv*5.0;
            float blue = texture(uDetailNormal,detailUv*uNormalTransform.xy+uNormalTransform.zw).b;
            limit = min(limit,mix(1.0,sat(blue+uLegacyDetail.z),strength));
        }
        rough = max(rough,1.0-limit);
    }
    if (uHasNormal == 1) {
        vec2 duv1 = dFdx(uv);
        vec2 duv2 = dFdy(uv);
        float det = duv1.x * duv2.y - duv2.x * duv1.y;
        vec3 nn = vBasisValid > 0.5 ? normalize(uRotate*vNative[0].xyz) : n;
        vec3 tangent = uRotate*vNative[1].xyz;
        vec3 bitangent = uRotate*vNative[2].xyz;
        vec3 storedT = tangent - nn * dot(tangent,nn);
        bool storedBasis = vBasisValid > 0.5 && dot(storedT,storedT)>1e-12 && dot(bitangent,bitangent)>1e-12;
        if (storedBasis || abs(det) > 1e-12) {
            vec3 t = storedBasis ? normalize(storedT) : normalize((dp1 * duv2.y - dp2 * duv1.y) / det);
            vec3 b = storedBasis ? bitangent : normalize((dp2 * duv1.x - dp1 * duv2.x) / det);
            if (!storedBasis) nn = n;
            t = normalize(t - nn * dot(t, nn));
            vec3 bb = cross(nn, t);
            if (dot(bb, b) < 0.0) bb = -bb;
            vec4 sampled = texture(uNormal, uv);
            vec2 xy = sampled.rg;
            bool decodedNormal = legacyNormal || (uNativeIndex >= 0 && uEffect < 0 && nHasDecodedNormal());
            vec4 decode = legacyNormal ? vec4(uLegacyNormal.xy,uLegacyDetail.xy) : nNormalDecode();
            if (decodedNormal) xy = xy * decode.x + decode.y;
            else ao *= sampled.b;
            if (dyed && uHasDetailNormal == 1 && mask.a >= 40.0) {
                float wear = sat((mask.a - 48.0) / 207.0);
                float intact = nativePaint ? paintRemap(wear,uWear) : sat(remap(wear,uWear));
                float strength = decodedNormal ? mix(sat(uWornParams.y), legacyNormal ? uParams.y : sat(uParams.y), intact) : clamp(mix(uWornParams.y, uParams.y, intact), 0.0, 4.0);
                vec2 detailUv = uNativeDetail == 1 ? vNative[3].zw : uv * 5.0;
                vec4 detail = texture(uDetailNormal, detailUv * uNormalTransform.xy + uNormalTransform.zw);
                vec2 blended = mix(2.0 * xy * detail.rg, 1.0 - 2.0 * (1.0 - xy) * (1.0 - detail.rg), step(0.5, xy));
                if (decodedNormal) {
                    if (!(any(isnan(decode.zw)) || any(isinf(decode.zw)))) xy += strength * (detail.rg * decode.z + decode.w);
                }
                else {
                    xy = mix(xy, blended, strength);
                    ao *= mix(1.0, detail.b, min(strength, 1.0));
                }
            }
            vec2 p = decodedNormal ? xy : clamp(xy * 2.0 - 1.0, -1.0, 1.0);
            float z = sqrt(max(1.0 - dot(p, p), 0.0));
            n = normalize(t * p.x + bb * p.y + nn * z);
        }
    }
    if(uNativeIndex>=0 && uEffect<0)albedo=nativePixel(albedo).rgb;
    vec3 tint = vec3(1.0);
    if (painted && uHasIridescence == 1 && uIridescenceId >= 0.0) {
        float nDotV = sat(abs(n.z));
        // Row per id, top down. Unused rows hold a magenta placeholder.
        float rows = float(textureSize(uIridescence, 0).y);
        vec4 iri = texture(uIridescence, vec2(nDotV, (uIridescenceId + 0.5) / rows));
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
    fragColor = vec4(light(albedo, rough, metal, ao, emission, n, tint), 1.0);
}
"#;

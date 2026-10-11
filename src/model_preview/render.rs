//! Small depth-buffered geometry preview with a stable scale that follows stored root motion.
use super::Model;
use eframe::egui::{Color32, ColorImage};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Style {
    #[default]
    Textured,
    Solid,
    Wireframe,
}
impl Style {
    pub fn label(self) -> &'static str {
        match self {
            Self::Textured => "Textured",
            Self::Solid => "Solid",
            Self::Wireframe => "Wireframe",
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) struct Camera {
    pub yaw: f32,
    pub pitch: f32,
    pub zoom: f32,
    /// Screen-space offset as a fraction of the viewport, so panning survives a resize.
    pub pan: [f32; 2],
}
impl Default for Camera {
    fn default() -> Self {
        Self {
            yaw: -0.65,
            pitch: 0.25,
            zoom: 1.0,
            pan: [0.0, 0.0],
        }
    }
}

/// How far the orbit may tip before the pole degenerates.
pub(crate) const MAX_PITCH: f32 = std::f32::consts::FRAC_PI_2;

/// Viewer-adjustable scene settings. Exposure is calibrated for the default film curve,
/// while particle studies require an explicit choice.
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct Scene {
    pub background: [u8; 3],
    /// Key light direction in view space, in the gear shader's convention.
    pub light: [f32; 3],
    pub key: f32,
    pub fill: f32,
    pub exposure: f32,
    pub filmic: bool,
    pub bloom: bool,
    /// Explicit diagnostic study. Native particle spawning and motion are unavailable.
    pub particle_study: bool,
}
impl Default for Scene {
    fn default() -> Self {
        Self {
            background: [24, 28, 35],
            light: [-0.35, 0.55, -0.76],
            key: 0.70,
            fill: 0.30,
            exposure: 0.4,
            filmic: true,
            bloom: true,
            particle_study: false,
        }
    }
}
impl Scene {
    #[cfg(test)]
    pub(crate) fn unit_exposure() -> Self {
        Self {
            exposure: 1.0,
            ..Self::default()
        }
    }
    #[cfg(test)]
    pub(crate) fn unprocessed() -> Self {
        Self {
            filmic: false,
            bloom: false,
            exposure: 1.0,
            ..Self::default()
        }
    }
    /// The rasterizer projects with screen y downward, so the key tips the opposite way here.
    pub(crate) fn raster_light(self) -> [f32; 3] {
        [self.light[0], -self.light[1], self.light[2]]
    }
}

#[cfg(test)]
pub(crate) fn image(model: &Model, camera: Camera, size: [usize; 2]) -> ColorImage {
    frame(
        model,
        camera,
        Scene::unprocessed(),
        size,
        None,
        Style::Textured,
        0.0,
        None,
        16,
        false,
    )
}

pub(crate) fn animated_image(
    model: &Model,
    camera: Camera,
    scene: Scene,
    size: [usize; 2],
    seconds: f32,
) -> ColorImage {
    let pose = model.pose(seconds);
    frame(
        model,
        camera,
        scene,
        size,
        pose.as_ref(),
        Style::Textured,
        seconds,
        None,
        16,
        false,
    )
}

pub(crate) fn styled_image(
    model: &Model,
    camera: Camera,
    scene: Scene,
    size: [usize; 2],
    seconds: f32,
    style: Style,
) -> ColorImage {
    if style == Style::Textured {
        return animated_image(model, camera, scene, size, seconds);
    }
    let pose = model.pose(seconds);
    frame(
        model,
        camera,
        scene,
        size,
        pose.as_ref(),
        style,
        seconds,
        None,
        16,
        false,
    )
}

/// Background interactive rendering retains one request's edits and leaves CPU capacity
/// available for authoring, package operations and other previews.
#[allow(clippy::too_many_arguments)]
pub(crate) fn preview_image(
    model: &Model,
    camera: Camera,
    scene: Scene,
    size: [usize; 2],
    seconds: f32,
    style: Style,
    overrides: &[super::SurfaceOverride],
) -> ColorImage {
    let pose = model.pose(seconds);
    let workers =
        std::thread::available_parallelism().map_or(1, |n| n.get().saturating_sub(2).clamp(1, 4));
    frame(
        model,
        camera,
        scene,
        size,
        pose.as_ref(),
        style,
        seconds,
        Some(overrides),
        workers,
        false,
    )
}

/// Native surface coverage on a transparent background for inventory artwork.
pub(crate) fn transparent_image(model: &Model, camera: Camera, size: usize) -> ColorImage {
    let scene = Scene {
        background: [0; 3],
        bloom: false,
        ..Scene::default()
    };
    let pose = model.pose(0.0);
    frame(
        model,
        camera,
        scene,
        [size; 2],
        pose.as_ref(),
        Style::Textured,
        0.0,
        None,
        16,
        true,
    )
}

/// One projected triangle with everything the rasterizer needs, so each band of rows can
/// draw it without repeating the setup.
struct Prepared {
    index: usize,
    emitter: bool,
    instance: usize,
    points: [[f32; 3]; 3],
    uvs: [[f32; 2]; 3],
    detail_uvs: [[f32; 2]; 3],
    normals: [[f32; 3]; 3],
    basis: Option<[super::shader::normal::Basis; 3]>,
    native: Option<Box<super::effects::native::Triangle>>,
    min_y: usize,
    max_y: usize,
}

fn tangent_frames(
    triangle: [u32; 3],
    normals: &[[f32; 3]],
    tangents: &[[f32; 4]],
    native: Option<&super::effects::native::Triangle>,
    rotate: impl Fn([f32; 3]) -> [f32; 3],
) -> Option<[super::shader::normal::Basis; 3]> {
    use super::shader::normal::Basis;
    let [Some(a), Some(b), Some(c)] =
        triangle.map(|v| Basis::stored(*normals.get(v as usize)?, *tangents.get(v as usize)?))
    else {
        return None;
    };
    let frames = if let Some(native) = native {
        let [Some(a), Some(b), Some(c)] = native.values.map(|v| {
            Basis::vectors(
                [v[0][0], v[0][1], v[0][2]],
                [v[1][0], v[1][1], v[1][2]],
                [v[2][0], v[2][1], v[2][2]],
            )
        }) else {
            return None;
        };
        [a, b, c]
    } else {
        [a, b, c]
    };
    Some(frames.map(|basis| basis.map(&rotate)))
}

/// The default framing draws the bounding sphere's radius at this share of the frame's shorter
/// side, at zoom 1.
pub(crate) const RADIUS_SCALE: f32 = 0.43;

/// Rest-pose framing excludes light volumes when surfaces exist and uninstanced particle
/// meshes in a composed textured view. Unsupported transparent materials cannot draw and
/// do not affect textured framing. When opaque surfaces exist, transparent effect
/// volumes also stay outside the fit. Inspection modes retain their geometry. Models
/// without triangles use their particle sources. Empty when there is neither.
pub(crate) fn drawn_bounds(model: &Model, style: Style) -> ([f32; 3], [f32; 3]) {
    surface_bounds(model, style, false)
}

/// Artwork must fit transparent surfaces such as blades as well as opaque geometry.
pub(crate) fn artwork_bounds(model: &Model) -> ([f32; 3], [f32; 3]) {
    surface_bounds(model, Style::Textured, true)
}

fn surface_bounds(model: &Model, style: Style, include_effects: bool) -> ([f32; 3], [f32; 3]) {
    let hide_light = model.has_surface_mesh();
    let hide_emitter = style == Style::Textured && model.has_object_mesh();
    let hide_effect = !include_effects
        && style == Style::Textured
        && (0..model.triangles.len()).any(|index| {
            !super::effects::transparent(model, index)
                && !model.triangle_light.get(index).copied().unwrap_or(false)
                && !model.triangle_emitter.get(index).copied().unwrap_or(false)
        });
    let mut low = [f32::INFINITY; 3];
    let mut high = [f32::NEG_INFINITY; 3];
    for (index, triangle) in model.triangles.iter().enumerate() {
        if hide_light && model.triangle_light.get(index).copied().unwrap_or(false)
            || hide_emitter && model.triangle_emitter.get(index).copied().unwrap_or(false)
            || hide_effect && super::effects::transparent(model, index)
            || style == Style::Textured
                && super::effects::index(model, index).is_some_and(|effect| {
                    let material = &model.effects[effect];
                    material.kind == super::effects::Kind::Unavailable && !material.opaque()
                })
        {
            continue;
        }
        for &vertex in triangle {
            let vertex = model.vertices[vertex as usize];
            for axis in 0..3 {
                low[axis] = low[axis].min(vertex[axis]);
                high[axis] = high[axis].max(vertex[axis]);
            }
        }
    }
    if model.triangles.is_empty() {
        for source in &model.particle_sources {
            for axis in 0..3 {
                low[axis] = low[axis].min(source.position[axis]);
                high[axis] = high[axis].max(source.position[axis]);
            }
        }
    }
    (low, high)
}

/// Half the diagonal of `bounds`, the sphere the default framing fits.
fn framing_radius(model: &Model, (low, high): ([f32; 3], [f32; 3])) -> f32 {
    (0..3)
        .map(|axis| (high[axis] - low[axis]).powi(2))
        .sum::<f32>()
        .sqrt()
        .max(if model.triangles.is_empty() {
            1.0
        } else {
            0.0001
        })
        * 0.5
}

/// Points of a `size` frame per model unit, the scale `frame` draws with at `camera`'s zoom.
pub(crate) fn screen_scale(
    model: &Model,
    bounds: ([f32; 3], [f32; 3]),
    camera: Camera,
    size: [f32; 2],
) -> f32 {
    size[0].min(size[1]) * RADIUS_SCALE * camera.zoom / framing_radius(model, bounds)
}

/// Where a model-space point lands in a `size` frame that `frame` draws with `camera` and no
/// pose: across from the left, down from the top, and toward the viewer.
pub(crate) fn project(
    model: &Model,
    bounds: ([f32; 3], [f32; 3]),
    camera: Camera,
    size: [f32; 2],
    point: [f32; 3],
) -> [f32; 3] {
    let (low, high) = bounds;
    let scale = screen_scale(model, bounds, camera, size);
    let [x, y, z]: [f32; 3] =
        std::array::from_fn(|axis| point[axis] - (low[axis] + high[axis]) * 0.5);
    let (sy, cy) = camera.yaw.sin_cos();
    let (sp, cp) = camera.pitch.sin_cos();
    let forward = sy * x + cy * y;
    [
        size[0] * (0.5 + camera.pan[0]) + (cy * x - sy * y) * scale,
        size[1] * (0.5 + camera.pan[1]) - (sp * forward + cp * z) * scale,
        (cp * forward - sp * z) * scale,
    ]
}

/// The model-space directions that point right and up on screen at `camera`'s angle.
pub(crate) fn screen_axes(camera: Camera) -> ([f32; 3], [f32; 3]) {
    let (sy, cy) = camera.yaw.sin_cos();
    let (sp, cp) = camera.pitch.sin_cos();
    // A snapped view's trigonometry leaves residues like 6e-17 where an axis is exactly zero.
    let clean = |v: [f32; 3]| v.map(|c| if c.abs() < 1e-6 { 0.0 } else { c });
    (clean([cy, -sy, 0.0]), clean([sp * sy, sp * cy, cp]))
}

/// How much closer than the default framing a `size` frame can come while the outline of
/// `bounds`, seen from `camera`'s angle, fills `fill` of the frame on its tighter axis. The
/// default fits the box's circumscribed sphere into the frame's shorter side, which leaves a long
/// or flat model small, most of all in a wide frame. Never less than 1.
pub(crate) fn fitted_zoom(
    model: &Model,
    bounds: ([f32; 3], [f32; 3]),
    camera: Camera,
    size: [f32; 2],
    fill: f32,
) -> f32 {
    let (low, high) = bounds;
    if (0..3).any(|axis| low[axis] > high[axis]) {
        return 1.0;
    }
    let half: [f32; 3] = std::array::from_fn(|axis| (high[axis] - low[axis]) * 0.5);
    let (sy, cy) = camera.yaw.sin_cos();
    let (sp, cp) = camera.pitch.sin_cos();
    // The box is centered in the frame, so the farthest corner on each axis gives half its
    // outline there.
    let (mut across, mut up) = (0.0_f32, 0.0_f32);
    for corner in 0..8_usize {
        let [x, y, z]: [f32; 3] = std::array::from_fn(|axis| {
            if (corner >> axis) & 1 == 0 {
                -half[axis]
            } else {
                half[axis]
            }
        });
        let forward = sy * x + cy * y;
        across = across.max((cy * x - sy * y).abs());
        up = up.max((sp * forward + cp * z).abs());
    }
    let unit = size[0].min(size[1]) * RADIUS_SCALE / framing_radius(model, bounds);
    let zoom = (fill * size[0] / (2.0 * across * unit).max(f32::EPSILON))
        .min(fill * size[1] / (2.0 * up * unit).max(f32::EPSILON));
    zoom.max(1.0)
}

#[allow(clippy::too_many_arguments)]
fn frame(
    model: &Model,
    camera: Camera,
    scene: Scene,
    size: [usize; 2],
    pose: Option<&super::animation::Deformed>,
    style: Style,
    seconds: f32,
    overrides: Option<&[super::SurfaceOverride]>,
    workers: usize,
    transparent_background: bool,
) -> ColorImage {
    let vertices = pose.map_or(model.vertices.as_slice(), |p| p.positions.as_slice());
    let source_normals = pose.map_or(model.normals.as_slice(), |p| p.normals.as_slice());
    let source_tangents = pose.map_or(model.tangents.as_slice(), |p| p.tangents.as_slice());
    // The interactive path already picks a small size; the ceiling only bounds an export.
    let [width, height] = size.map(|v| v.clamp(1, 4096));
    let background = Color32::from_rgb(
        scene.background[0],
        scene.background[1],
        scene.background[2],
    );
    let mut image = ColorImage::filled(
        [width, height],
        if transparent_background {
            Color32::TRANSPARENT
        } else {
            background
        },
    );
    if model.triangles.is_empty() && model.particle_sources.is_empty() {
        return image;
    }
    let hide_light = model.has_surface_mesh();
    let material_study =
        style == Style::Textured && scene.particle_study && model.has_particle_material_study();
    let hide_emitter = style == Style::Textured && model.has_object_mesh() && !material_study;
    let bounds = if transparent_background {
        artwork_bounds(model)
    } else {
        drawn_bounds(model, if material_study { Style::Solid } else { style })
    };
    let (low, high) = bounds;
    let bind_center = std::array::from_fn(|axis| (low[axis] + high[axis]) * 0.5);
    let center = pose.map_or(bind_center, |p| p.framing_center(bind_center));
    let radius = framing_radius(model, bounds);
    let scale = width.min(height) as f32 * RADIUS_SCALE * camera.zoom / radius;
    let (sy, cy) = camera.yaw.sin_cos();
    let (sp, cp) = camera.pitch.sin_cos();
    let project_point = |point: [f32; 3]| {
        let [x, y, z] = std::array::from_fn::<_, 3, _>(|axis| point[axis] - center[axis]);
        let horizontal = cy * x - sy * y;
        let forward = sy * x + cy * y;
        [
            width as f32 * (0.5 + camera.pan[0]) + horizontal * scale,
            height as f32 * (0.5 + camera.pan[1]) - (sp * forward + cp * z) * scale,
            (cp * forward - sp * z) * scale,
        ]
    };
    let projected = vertices
        .iter()
        .copied()
        .map(project_point)
        .collect::<Vec<_>>();
    let dyes = overrides.map_or_else(
        || super::shader::dyes(model, seconds),
        |overrides| super::shader::dyes_with_overrides(model, seconds, overrides),
    );
    let rotate = |[x, y, z]: [f32; 3]| {
        let forward = sy * x + cy * y;
        [
            cy * x - sy * y,
            -(sp * forward + cp * z),
            cp * forward - sp * z,
        ]
    };
    let normals: Vec<[f32; 3]> = source_normals.iter().copied().map(rotate).collect();
    let effect_frames = super::effects::frames(model, seconds);
    let vertex_frames: Vec<_> = model
        .effects
        .iter()
        .map(|e| e.native.as_ref().and_then(|n| n.vertex_frame(seconds)))
        .collect();
    let particle_material = (style == Style::Textured && scene.particle_study)
        .then(|| super::particle_material::prepare(model, seconds))
        .flatten();
    let mut prepared: Vec<Prepared> = model
        .triangles
        .iter()
        .enumerate()
        .flat_map(|(index, triangle)| {
            let count = if model.triangle_emitter.get(index).copied().unwrap_or(false) {
                particle_material
                    .as_ref()
                    .map_or(1, |batch| batch.states.len())
            } else {
                1
            };
            (0..count).map(move |instance| (index, triangle, instance))
        })
        .filter_map(|(index, triangle, instance)| {
            if hide_light && model.triangle_light.get(index).copied().unwrap_or(false) {
                return None;
            }
            if style == Style::Textured
                && (hide_emitter || scene.particle_study && !model.particle_sources.is_empty())
                && model.triangle_emitter.get(index).copied().unwrap_or(false)
            {
                return None;
            }
            let mut points = triangle.map(|v| projected[v as usize]);
            let mut native_triangle = None;
            let emitter = model.triangle_emitter.get(index).copied().unwrap_or(false);
            if style == Style::Textured
                && !(emitter && particle_material.is_some())
                && let Some(effect) = super::effects::index(model, index)
                && let Some(native) = &model.effects[effect].native
            {
                let constants = vertex_frames[effect].as_ref()?;
                let corners = triangle.map(|v| vertices[v as usize]);
                let ab: [f32; 3] = std::array::from_fn(|i| corners[1][i] - corners[0][i]);
                let ac: [f32; 3] = std::array::from_fn(|i| corners[2][i] - corners[0][i]);
                let flat = super::shader::normal::normalize([
                    ab[1] * ac[2] - ab[2] * ac[1],
                    ab[2] * ac[0] - ab[0] * ac[2],
                    ab[0] * ac[1] - ab[1] * ac[0],
                ])
                .unwrap_or([0.0, 0.0, 1.0]);
                let mut values = [[[0.0; 4]; 9]; 3];
                for (corner, &vertex) in triangle.iter().enumerate() {
                    let vertex = vertex as usize;
                    let normal = source_normals
                        .get(vertex)
                        .copied()
                        .and_then(super::shader::normal::normalize)
                        .unwrap_or(flat);
                    let uv = model.uvs.get(vertex).copied().unwrap_or_default();
                    let input = super::effects::native::Input {
                        position: vertices[vertex],
                        normal,
                        tangent: super::effects::native::tangent(source_tangents, vertex, normal),
                        color: model.colors.get(vertex).copied().unwrap_or([1.0; 4]),
                        uv,
                        detail_uv: model.detail_uvs.get(vertex).copied().unwrap_or(uv),
                    };
                    values[corner] = native.vertex(model, &input, constants)?;
                }
                points = values.map(|v| project_point([v[4][0], v[4][1], v[4][2]]));
                native_triangle = Some(Box::new(super::effects::native::Triangle::new(
                    values, points,
                )));
            }
            if emitter && let Some(batch) = &particle_material {
                points =
                    triangle.map(|v| project_point(batch.position(instance, vertices[v as usize])));
            }
            let (min_y, max_y) = points
                .iter()
                .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), p| {
                    (lo.min(p[1]), hi.max(p[1]))
                });
            if !min_y.is_finite() || !max_y.is_finite() {
                return None;
            }
            Some(Prepared {
                index,
                emitter,
                instance,
                points,
                uvs: triangle.map(|v| model.uvs.get(v as usize).copied().unwrap_or_default()),
                detail_uvs: triangle.map(|v| {
                    model
                        .detail_uvs
                        .get(v as usize)
                        .copied()
                        .unwrap_or_else(|| model.uvs.get(v as usize).copied().unwrap_or_default())
                }),
                normals: triangle.map(|v| normals.get(v as usize).copied().unwrap_or([0.0; 3])),
                basis: tangent_frames(
                    *triangle,
                    source_normals,
                    source_tangents,
                    native_triangle.as_deref(),
                    rotate,
                ),
                native: native_triangle,
                min_y: min_y.floor().clamp(0.0, height as f32) as usize,
                max_y: max_y.ceil().clamp(0.0, height as f32) as usize,
            })
        })
        .collect();
    if style == Style::Textured {
        prepared.sort_by(|a, b| {
            let a_effect = super::effects::transparent(model, a.index);
            let b_effect = super::effects::transparent(model, b.index);
            a_effect.cmp(&b_effect).then_with(|| {
                if a_effect {
                    b.points
                        .iter()
                        .map(|p| p[2])
                        .sum::<f32>()
                        .total_cmp(&a.points.iter().map(|p| p[2]).sum::<f32>())
                } else {
                    model
                        .triangle_constant
                        .get(a.index)
                        .copied()
                        .flatten()
                        .is_some()
                        .cmp(
                            &model
                                .triangle_constant
                                .get(b.index)
                                .copied()
                                .flatten()
                                .is_some(),
                        )
                }
            })
        });
    }
    // Shading is per pixel and CPU-bound, so the rows the model covers are split into bands
    // drawn on every core. Each band owns its pixels and depth and only visits triangles
    // that reach it.
    let sprite_study =
        style == Style::Textured && scene.particle_study && !model.particle_sources.is_empty();
    let full_frame = style == Style::Textured && scene.bloom || sprite_study;
    let first = if full_frame {
        0
    } else {
        prepared.iter().map(|t| t.min_y).min().unwrap_or(0)
    };
    let last = if full_frame {
        height
    } else {
        prepared.iter().map(|t| t.max_y).max().unwrap_or(0)
    };
    if last <= first {
        return image;
    }
    let bands = std::thread::available_parallelism().map_or(1, |n| n.get().min(workers));
    let rows = (last - first).div_ceil(bands).max(1);
    let mut depth = vec![f32::INFINITY; width * (last - first)];
    let background = super::output::background(scene, style);
    let mut linear_pixels = vec![background; width * (last - first)];
    let mut coverage = vec![0.0f32; width * (last - first)];
    let has_effects = style == Style::Textured
        && (0..model.triangles.len()).any(|i| super::effects::transparent(model, i));
    let mut opaque_depth = Vec::new();
    for transparent in [false, true] {
        if transparent && !has_effects {
            break;
        }
        let native_depth = transparent.then_some(super::effects::native::Depth {
            values: &opaque_depth,
            size: [width, height],
            top: first,
            scale,
        });
        std::thread::scope(|scope| {
            for (band, ((pixels, depth), coverage)) in linear_pixels
                .chunks_mut(rows * width)
                .zip(depth.chunks_mut(rows * width))
                .zip(coverage.chunks_mut(rows * width))
                .enumerate()
            {
                let (prepared, dyes, particle_material, effect_frames) =
                    (&prepared, &dyes, particle_material.as_ref(), &effect_frames);
                let top = first + band * rows;
                let bottom = (top + rows).min(last);
                scope.spawn(move || {
                    for triangle in prepared {
                        let is_effect = style == Style::Textured
                            && super::effects::transparent(model, triangle.index);
                        if is_effect != transparent
                            || triangle.max_y <= top
                            || triangle.min_y >= bottom
                        {
                            continue;
                        }
                        let bindings = super::shader::Bindings::new(model, triangle.index, dyes);
                        raster(
                            Band {
                                pixels,
                                depth,
                                coverage,
                                width,
                                top,
                                bottom,
                                scene,
                                camera,
                                scale,
                                native_depth,
                                view_direction: [-cp * sy, -cp * cy, sp],
                                view_distance: (radius * 4.0).max(1.0),
                            },
                            triangle,
                            style,
                            if style == Style::Textured {
                                bindings
                            } else {
                                bindings.clip_only()
                            },
                            particle_material
                                .filter(|_| triangle.emitter)
                                .and_then(|batch| batch.states.get(triangle.instance)),
                            if style == Style::Textured {
                                super::effects::index(model, triangle.index).map(|index| {
                                    (model, &model.effects[index], effect_frames[index].as_ref())
                                })
                            } else {
                                None
                            },
                        );
                    }
                });
            }
        });
        if !transparent && has_effects {
            opaque_depth.clone_from(&depth);
        }
    }
    if sprite_study {
        draw_particles(
            &mut linear_pixels,
            [width, height],
            model,
            camera,
            scene,
            seconds,
            center,
            scale,
        );
    }
    finish_pixels(
        &mut image.pixels[first * width..last * width],
        &mut linear_pixels,
        &coverage,
        [width, last - first],
        scene,
        style,
        transparent_background,
    );
    image
}

fn finish_pixels(
    pixels: &mut [Color32],
    linear_pixels: &mut [[f32; 3]],
    coverage: &[f32],
    size: [usize; 2],
    scene: Scene,
    style: Style,
    transparent_background: bool,
) {
    if transparent_background {
        for (pixel, alpha) in linear_pixels.iter_mut().zip(coverage) {
            if *alpha > 0.0 {
                *pixel = pixel.map(|value| value / alpha);
            }
        }
    }
    super::output::apply(linear_pixels, size, scene, style);
    for ((pixel, linear), coverage) in pixels.iter_mut().zip(linear_pixels).zip(coverage) {
        let mut rgb = linear.map(super::shader::encode);
        *pixel = if transparent_background {
            // Tiny inventory artwork needs readable energy surfaces over a bright rarity
            // plate. This export increases coverage and chroma without changing materials
            // or opaque model details. Zero coverage stays transparent.
            let alpha = coverage.clamp(0.0, 1.0);
            let peak = f32::from(*rgb.iter().max().unwrap());
            rgb = rgb.map(|v| {
                (peak - (peak - f32::from(v)) * (1.0 + 0.75 * (1.0 - alpha)))
                    .round()
                    .clamp(0.0, 255.0) as u8
            });
            Color32::from_rgba_unmultiplied(
                rgb[0],
                rgb[1],
                rgb[2],
                ((1.0 - (1.0 - alpha).powi(6)) * 255.0).round() as u8,
            )
        } else {
            Color32::from_rgb(rgb[0], rgb[1], rgb[2])
        };
    }
}

/// Explicit sprite study using synthetic placement and drift. This does not evaluate
/// native emitter activation, spawning, attachment or motion.
#[allow(clippy::too_many_arguments)]
fn draw_particles(
    pixels: &mut [[f32; 3]],
    [width, height]: [usize; 2],
    model: &Model,
    camera: Camera,
    scene: Scene,
    seconds: f32,
    center: [f32; 3],
    scale: f32,
) {
    let ramp_sampler = super::texture::Sampler {
        u: super::texture::AddressMode::Clamp,
        v: super::texture::AddressMode::Clamp,
        ..Default::default()
    };
    let (sy, cy) = camera.yaw.sin_cos();
    let (sp, cp) = camera.pitch.sin_cos();
    for source in &model.particle_sources {
        let Some(texture) = model.textures.get(source.texture) else {
            continue;
        };
        let age = (seconds / source.period + source.phase).rem_euclid(1.0);
        let strength = (1.0 - age).powf(1.4) * 0.7 * scene.exposure;
        let position: [f32; 3] = std::array::from_fn(|axis| {
            source.position[axis] + source.drift[axis] * age - center[axis]
        });
        let forward = sy * position[0] + cy * position[1];
        let x =
            width as f32 * (0.5 + camera.pan[0]) + (cy * position[0] - sy * position[1]) * scale;
        let y = height as f32 * (0.5 + camera.pan[1]) - (sp * forward + cp * position[2]) * scale;
        let drift_forward = sy * source.drift[0] + cy * source.drift[1];
        let drift_x = cy * source.drift[0] - sy * source.drift[1];
        let drift_y = -(sp * drift_forward + cp * source.drift[2]);
        let angle = if drift_x.abs() + drift_y.abs() > 0.0001 {
            drift_y.atan2(drift_x)
        } else {
            -0.4
        };
        let (sin, cos) = angle.sin_cos();
        // The upper bound never drops under the lower one: an image a few pixels tall, which
        // a squeezed viewport produces, would otherwise make the clamp panic.
        let widest = (width.min(height) as f32 * 0.6).max(3.0);
        let sprite_width = (source.width * scale * (0.7 + age * 0.5)).clamp(3.0, widest);
        let aspect = texture.size[0] as f32 / texture.size[1].max(1) as f32;
        let sprite_height = sprite_width / aspect.clamp(1.0, 16.0);
        let radius = (sprite_width + sprite_height) * 0.5;
        let left = (x - radius).floor().max(0.0) as usize;
        let right = (x + radius).ceil().min(width as f32) as usize;
        let top = (y - radius).floor().max(0.0) as usize;
        let bottom = (y + radius).ceil().min(height as f32) as usize;
        for row in top..bottom {
            for column in left..right {
                let dx = column as f32 + 0.5 - x;
                let dy = row as f32 + 0.5 - y;
                let u = (cos * dx + sin * dy) / sprite_width + 0.5;
                let v = (-sin * dx + cos * dy) / sprite_height + 0.5;
                if !(0.0..1.0).contains(&u) || !(0.0..1.0).contains(&v) {
                    continue;
                }
                let rgb = texture.sample_color([u, v]);
                let color = source
                    .gradient
                    .and_then(|index| model.textures.get(index))
                    .map_or_else(
                        || rgb,
                        |gradient| {
                            // Ramp position is mask data, while the ramp itself is color.
                            let position = texture.sample_rgba([u, v])[0] / 255.0;
                            gradient.sample_material([position, 0.0], &ramp_sampler, true)
                        },
                    );
                let opacity = strength * rgb[3];
                let index = row * width + column;
                for (base, color) in pixels[index].iter_mut().zip(color) {
                    *base += color * opacity;
                }
            }
        }
    }
}

/// The rows one thread draws.
struct Band<'a> {
    pixels: &'a mut [[f32; 3]],
    depth: &'a mut [f32],
    coverage: &'a mut [f32],
    width: usize,
    top: usize,
    bottom: usize,
    scene: Scene,
    camera: Camera,
    scale: f32,
    native_depth: Option<super::effects::native::Depth<'a>>,
    view_direction: [f32; 3],
    view_distance: f32,
}

fn edge(a: [f32; 3], b: [f32; 3], x: f32, y: f32) -> f64 {
    // Shared edges must make the same coverage decision in both triangles.
    (f64::from(b[0]) - f64::from(a[0])) * (f64::from(y) - f64::from(a[1]))
        - (f64::from(b[1]) - f64::from(a[1])) * (f64::from(x) - f64::from(a[0]))
}

struct EffectSample<'a> {
    barycentric: [f32; 2],
    flat: [f32; 3],
    gap: f32,
    exposure: f32,
    native: Option<super::effects::native::Pixel<'a>>,
}

fn effect_color(
    triangle: &Prepared,
    material: &super::shader::Bindings<'_>,
    (model, effect, frame): (
        &Model,
        &super::effects::Material,
        Option<&super::effects::Frame>,
    ),
    sample: EffectSample<'_>,
) -> [f32; 4] {
    let EffectSample {
        barycentric: [b, c],
        flat,
        gap,
        exposure,
        native,
    } = sample;
    let Some(frame) = frame else {
        return [0.0; 4];
    };
    if let Some(pixel) = native {
        return super::effects::native::sample(model, effect, frame, material, pixel);
    }
    let normals = triangle.normals;
    let uv = std::array::from_fn(|i| {
        triangle.uvs[0][i]
            + b * (triangle.uvs[1][i] - triangle.uvs[0][i])
            + c * (triangle.uvs[2][i] - triangle.uvs[0][i])
    });
    let normal = super::shader::normal::normalize(std::array::from_fn(|i| {
        normals[0][i] + b * (normals[1][i] - normals[0][i]) + c * (normals[2][i] - normals[0][i])
    }))
    .unwrap_or(flat);
    let detail_uv = std::array::from_fn(|i| {
        triangle.detail_uvs[0][i]
            + b * (triangle.detail_uvs[1][i] - triangle.detail_uvs[0][i])
            + c * (triangle.detail_uvs[2][i] - triangle.detail_uvs[0][i])
    });
    super::effects::sample(
        model,
        effect,
        frame,
        material,
        super::effects::Pixel {
            uv,
            detail_uv,
            facing: normal[2],
            gap,
            exposure,
        },
    )
}

fn raster(
    band: Band<'_>,
    triangle: &Prepared,
    style: Style,
    material: super::shader::Bindings<'_>,
    particle_material: Option<&super::particle_material::State<'_>>,
    effect: Option<(
        &super::Model,
        &super::effects::Material,
        Option<&super::effects::Frame>,
    )>,
) {
    let material = effect
        .and_then(|(_, effect, frame)| Some((effect, frame?)))
        .map_or(material, |(effect, frame)| {
            material.with_material(effect, frame)
        });
    let p = triangle.points;
    let material = if style == Style::Textured {
        material.with_footprints(
            [triangle.uvs, triangle.detail_uvs]
                .map(|uv| super::texture::Footprint::triangle(p, uv)),
        )
    } else {
        material
    };
    let uvs = triangle.uvs;
    let normals = triangle.normals;
    let width = band.width;
    let signed_area = edge(p[0], p[1], p[2][0], p[2][1]);
    let area = signed_area as f32;
    if !area.is_finite() || area.abs() < 0.0001 {
        return;
    }
    let min_x = p
        .iter()
        .map(|v| v[0])
        .fold(f32::INFINITY, f32::min)
        .floor()
        .clamp(0.0, width as f32) as usize;
    let max_x = p
        .iter()
        .map(|v| v[0])
        .fold(f32::NEG_INFINITY, f32::max)
        .ceil()
        .clamp(0.0, width as f32) as usize;
    let min_y = triangle.min_y.max(band.top);
    let max_y = triangle.max_y.min(band.bottom);
    let a: [f32; 3] = std::array::from_fn(|axis| p[1][axis] - p[0][axis]);
    let b: [f32; 3] = std::array::from_fn(|axis| p[2][axis] - p[0][axis]);
    let n = [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ];
    let length = n.iter().map(|v| v * v).sum::<f32>().sqrt().max(0.0001);
    let basis = super::shader::normal::Basis::triangle(p, uvs);
    let key = band.scene.raster_light();
    let exposure = band.scene.exposure;
    let edge_lengths = [(1, 2), (2, 0), (0, 1)].map(|(a, b)| {
        ((p[a][0] - p[b][0]).powi(2) + (p[a][1] - p[b][1]).powi(2))
            .sqrt()
            .max(0.0001)
    });
    let inclusive = [(1, 2), (2, 0), (0, 1)].map(|(a, b)| {
        let (a, b) = if area > 0.0 { (a, b) } else { (b, a) };
        p[b][1] < p[a][1] || (p[b][1] == p[a][1] && p[b][0] > p[a][0])
    });
    for y in min_y..max_y {
        for x in min_x..max_x {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let edges = [
                edge(p[1], p[2], px, py),
                edge(p[2], p[0], px, py),
                edge(p[0], p[1], px, py),
            ];
            // The top-left rule also prevents additive materials from drawing seams twice.
            if edges.into_iter().zip(inclusive).any(|(edge, inclusive)| {
                edge * signed_area.signum() < 0.0 || (edge == 0.0 && !inclusive)
            }) {
                continue;
            }
            let [a, b, c] = edges.map(|edge| (edge / signed_area) as f32);
            let z = p[0][2] + b * (p[1][2] - p[0][2]) + c * (p[2][2] - p[0][2]);
            let index = (y - band.top) * width + x;
            // Emissive panels share their surface's depth and are drawn later, so they pass on
            // equality.
            let covered = if material.constant.is_some() || effect.is_some() {
                z > band.depth[index]
            } else {
                z >= band.depth[index]
            };
            if covered {
                continue;
            }
            let uv: [f32; 2] = std::array::from_fn(|axis| {
                uvs[0][axis] + b * (uvs[1][axis] - uvs[0][axis]) + c * (uvs[2][axis] - uvs[0][axis])
            });
            if let Some(effect) = effect.filter(|(_, material, _)| !material.opaque()) {
                let gap = ((band.depth[index] - z) / band.scale).clamp(0.0, 1e6);
                let color = if effect.1.decal() {
                    let mut material = material;
                    opaque_color(
                        triangle,
                        &mut material,
                        Some(effect),
                        &band,
                        [b, c],
                        [px, py, z],
                    )
                    .unwrap_or([0.0; 4])
                } else {
                    effect_color(
                        triangle,
                        &material,
                        effect,
                        EffectSample {
                            barycentric: [b, c],
                            flat: n.map(|v| v / length),
                            gap,
                            exposure,
                            native: triangle.native.as_ref().zip(band.native_depth).map(
                                |(native, depth)| super::effects::native::Pixel {
                                    varyings: native.at(b, c),
                                    dx: native.dx,
                                    dy: native.dy,
                                    screen: [px, py, z],
                                    direction: band.view_direction,
                                    distance: band.view_distance,
                                    front: area < 0.0,
                                    depth,
                                    exposure,
                                },
                            ),
                        },
                    )
                };
                let base = band.pixels[index];
                band.pixels[index] = std::array::from_fn(|i| base[i] * (1.0 - color[3]) + color[i]);
                // Additive light has zero blend alpha. Give exported artwork
                // enough coverage to retain its emitted RGB on any background.
                let alpha =
                    color[3].max(color[..3].iter().copied().fold(0.0f32, f32::max).min(1.0));
                band.coverage[index] = band.coverage[index] * (1.0 - alpha) + alpha;
                continue;
            }
            if let Some(particle) = particle_material {
                let color = particle.sample(uv, exposure);
                let base = band.pixels[index];
                band.pixels[index] = std::array::from_fn(|i| base[i] + color[i] / 255.0);
                let alpha = color[..3]
                    .iter()
                    .map(|v| *v / 255.0)
                    .fold(0.0f32, f32::max)
                    .clamp(0.0, 1.0);
                band.coverage[index] = band.coverage[index] * (1.0 - alpha) + alpha;
                continue;
            }
            if !material.covers(uv) {
                continue;
            }
            let panel = material.constant.map(|_| {
                material
                    .albedo
                    .map_or([1.0; 4], |texture| texture.sample_color(uv))
            });
            if panel.is_some_and(|base| base[3] < 0.5) {
                continue;
            }
            let smooth = std::array::from_fn(|i| {
                normals[0][i]
                    + b * (normals[1][i] - normals[0][i])
                    + c * (normals[2][i] - normals[0][i])
            });
            let geometric = super::shader::normal::normalize(smooth)
                .unwrap_or_else(|| n.map(|v| if n[2] > 0.0 { -v / length } else { v / length }));
            let basis = triangle
                .basis
                .and_then(|frames| super::shader::normal::Basis::at(frames, b, c))
                .or_else(|| basis.map(|basis| basis.with_normal(geometric)));
            let normal = basis.map_or(geometric, |basis| basis.normal);
            let lighting = band.scene.fill
                + band.scene.key * super::shader::normal::dot(geometric, key).abs().min(1.0);
            {
                band.depth[index] = z;
                band.coverage[index] = 1.0;
                band.pixels[index] = if style == Style::Wireframe {
                    let near_edge = [a, b, c]
                        .iter()
                        .zip(edge_lengths)
                        .any(|(weight, length)| weight * area.abs() / length <= 0.8);
                    if near_edge {
                        [180.0, 215.0, 245.0].map(|v| super::shader::linear(v / 255.0))
                    } else {
                        band.scene
                            .background
                            .map(|v| super::shader::linear(f32::from(v) / 255.0))
                    }
                } else {
                    let detail = std::array::from_fn(|i| {
                        triangle.detail_uvs[0][i]
                            + b * (triangle.detail_uvs[1][i] - triangle.detail_uvs[0][i])
                            + c * (triangle.detail_uvs[2][i] - triangle.detail_uvs[0][i])
                    });
                    let mut material = material;
                    opaque_color(triangle, &mut material, effect, &band, [b, c], [px, py, z])
                        .map(|rgba| [rgba[0], rgba[1], rgba[2]])
                        .unwrap_or_else(|| {
                            surface_color(
                                &material,
                                [uv, detail],
                                normal,
                                basis,
                                band.scene,
                                panel,
                                lighting,
                            )
                        })
                };
            }
        }
    }
}

fn opaque_color(
    triangle: &Prepared,
    material: &mut super::shader::Bindings<'_>,
    effect: Option<(
        &super::Model,
        &super::effects::Material,
        Option<&super::effects::Frame>,
    )>,
    band: &Band<'_>,
    barycentric: [f32; 2],
    screen: [f32; 3],
) -> Option<[f32; 4]> {
    let (model, native, Some(constants)) =
        effect.filter(|(_, material, _)| material.opaque() || material.decal())?
    else {
        return None;
    };
    let varying = triangle.native.as_ref()?;
    let pixel = super::effects::native::Pixel {
        varyings: varying.at(barycentric[0], barycentric[1]),
        dx: varying.dx,
        dy: varying.dy,
        screen,
        direction: band.view_direction,
        distance: band.view_distance,
        front: edge(
            triangle.points[0],
            triangle.points[1],
            triangle.points[2][0],
            triangle.points[2][1],
        ) < 0.0,
        depth: super::effects::native::Depth {
            values: &[],
            size: [band.width, band.bottom],
            top: band.top,
            scale: band.scale,
        },
        exposure: band.scene.exposure,
    };
    if let Some(surface) =
        super::effects::native::sample_surface(model, native, constants, material, pixel)
    {
        let (right, up) = screen_axes(band.camera);
        let normal = [
            super::shader::normal::dot(surface.normal, right),
            -super::shader::normal::dot(surface.normal, up),
            -super::shader::normal::dot(surface.normal, band.view_direction),
        ];
        let color =
            super::shader::shade_surface(surface.surface, normal, band.scene, surface.ambient)
                .map(|v| v * surface.coverage);
        return Some([color[0], color[1], color[2], surface.coverage]);
    }
    let (color, ambient) =
        super::effects::native::sample_with_ambient(model, native, constants, material, pixel);
    material.ambient_override = ambient;
    material.color_override = Some([color[0], color[1], color[2]]);
    material.emission_override = native
        .native
        .as_ref()
        .filter(|n| n.intensity)
        .map(|_| [color[0], color[1], color[2]].map(|v| v * color[3]));
    None
}

fn surface_color(
    material: &super::shader::Bindings<'_>,
    coordinates: [[f32; 2]; 2],
    normal: [f32; 3],
    basis: Option<super::shader::normal::Basis>,
    scene: Scene,
    panel: Option<[f32; 4]>,
    lighting: f32,
) -> [f32; 3] {
    let [uv, detail] = coordinates;
    let exposure = scene.exposure;
    let tint = material.tint();
    if let Some(constant) = material.constant {
        // Panel art lives in the colour plate: alpha cuts the segments, colour
        // tints them, and the constant supplies the glow.
        let base = panel.unwrap_or([1.0; 4]);
        std::array::from_fn(|i| constant[i] * base[i] * exposure)
    } else if let Some(texture) = material.albedo {
        material
            .shade_linear(uv, detail, normal, basis, scene)
            .unwrap_or_else(|| {
                let base = texture.sample_color(uv);
                std::array::from_fn(|i| base[i] * lighting * exposure)
            })
    } else if let Some(tint) = tint {
        tint.map(|v| v * lighting * exposure)
    } else {
        [205.0, 216.0, 230.0]
            .map(|v| super::shader::linear((v * lighting * exposure / 255.0).clamp(0.0, 1.0)))
    }
}

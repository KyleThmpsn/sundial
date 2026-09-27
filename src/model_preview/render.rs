//! Small depth-buffered geometry preview with fixed bind-pose framing.
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

/// Viewer-adjustable scene settings. The defaults reproduce the approved rig exactly, so an
/// untouched preview renders bit-for-bit as it did before these controls existed.
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct Scene {
    pub background: [u8; 3],
    /// Key light direction in view space, in the gear shader's convention.
    pub light: [f32; 3],
    pub key: f32,
    pub fill: f32,
    pub exposure: f32,
}
impl Default for Scene {
    fn default() -> Self {
        Self {
            background: [24, 28, 35],
            light: [-0.35, 0.55, -0.76],
            key: 0.70,
            fill: 0.30,
            exposure: 1.0,
        }
    }
}
impl Scene {
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
        Scene::default(),
        size,
        &model.vertices,
        Style::Textured,
        0.0,
    )
}

pub(crate) fn animated_image(
    model: &Model,
    camera: Camera,
    scene: Scene,
    size: [usize; 2],
    seconds: f32,
) -> ColorImage {
    let vertices = model
        .animation
        .as_ref()
        .map(|a| a.vertices(model, seconds.rem_euclid(a.duration())));
    frame(
        model,
        camera,
        scene,
        size,
        vertices.as_deref().unwrap_or(&model.vertices),
        Style::Textured,
        seconds,
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
    let vertices = model
        .animation
        .as_ref()
        .map(|a| a.vertices(model, seconds.rem_euclid(a.duration())));
    frame(
        model,
        camera,
        scene,
        size,
        vertices.as_deref().unwrap_or(&model.vertices),
        style,
        seconds,
    )
}

/// One projected triangle with everything the rasterizer needs, so each band of rows can
/// draw it without repeating the setup.
struct Prepared {
    index: usize,
    emitter: bool,
    points: [[f32; 3]; 3],
    uvs: [[f32; 2]; 3],
    normals: [[f32; 3]; 3],
    min_y: usize,
    max_y: usize,
}

fn frame(
    model: &Model,
    camera: Camera,
    scene: Scene,
    size: [usize; 2],
    vertices: &[[f32; 3]],
    style: Style,
    seconds: f32,
) -> ColorImage {
    // The interactive path already picks a small size; the ceiling only bounds an export.
    let [width, height] = size.map(|v| v.clamp(1, 4096));
    let background = Color32::from_rgb(
        scene.background[0],
        scene.background[1],
        scene.background[2],
    );
    let mut image = ColorImage::new([width, height], background);
    if model.triangles.is_empty() && model.particle_sources.is_empty() {
        return image;
    }
    let hide_light = model.has_surface_mesh();
    let mut low = [f32::INFINITY; 3];
    let mut high = [f32::NEG_INFINITY; 3];
    for (index, triangle) in model.triangles.iter().enumerate() {
        if hide_light && model.triangle_light.get(index).copied().unwrap_or(false) {
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
    let center: [f32; 3] = std::array::from_fn(|axis| (low[axis] + high[axis]) * 0.5);
    let radius = (0..3)
        .map(|axis| (high[axis] - low[axis]).powi(2))
        .sum::<f32>()
        .sqrt()
        .max(if model.triangles.is_empty() {
            1.0
        } else {
            0.0001
        })
        * 0.5;
    let scale = width.min(height) as f32 * 0.43 * camera.zoom / radius;
    let (sy, cy) = camera.yaw.sin_cos();
    let (sp, cp) = camera.pitch.sin_cos();
    let projected = vertices
        .iter()
        .map(|point| {
            let [x, y, z] = std::array::from_fn::<_, 3, _>(|axis| point[axis] - center[axis]);
            let horizontal = cy * x - sy * y;
            let forward = sy * x + cy * y;
            [
                width as f32 * (0.5 + camera.pan[0]) + horizontal * scale,
                height as f32 * (0.5 + camera.pan[1]) - (sp * forward + cp * z) * scale,
                (cp * forward - sp * z) * scale,
            ]
        })
        .collect::<Vec<_>>();
    let dyes = super::shader::dyes(model, seconds);
    let normals: Vec<[f32; 3]> = if model.animation.is_none() {
        model
            .normals
            .iter()
            .map(|&[x, y, z]| {
                let forward = sy * x + cy * y;
                [
                    cy * x - sy * y,
                    -(sp * forward + cp * z),
                    cp * forward - sp * z,
                ]
            })
            .collect()
    } else {
        Vec::new()
    };
    let prepared: Vec<Prepared> = model
        .triangles
        .iter()
        .enumerate()
        .filter_map(|(index, triangle)| {
            if hide_light && model.triangle_light.get(index).copied().unwrap_or(false) {
                return None;
            }
            if style == Style::Textured
                && !model.particle_sources.is_empty()
                && model.triangle_emitter.get(index).copied().unwrap_or(false)
            {
                return None;
            }
            let points = triangle.map(|v| projected[v as usize]);
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
                emitter: model.triangle_emitter.get(index).copied().unwrap_or(false),
                points,
                uvs: triangle.map(|v| model.uvs.get(v as usize).copied().unwrap_or_default()),
                normals: triangle.map(|v| normals.get(v as usize).copied().unwrap_or([0.0; 3])),
                min_y: min_y.floor().clamp(0.0, height as f32) as usize,
                max_y: max_y.ceil().clamp(0.0, height as f32) as usize,
            })
        })
        .collect();
    // Shading is per pixel and CPU-bound, so the rows the model covers are split into bands
    // drawn on every core. Each band owns its pixels and depth and only visits triangles
    // that reach it.
    let first = prepared.iter().map(|t| t.min_y).min().unwrap_or(0);
    let last = prepared.iter().map(|t| t.max_y).max().unwrap_or(0);
    if last <= first {
        if style == Style::Textured {
            draw_particles(&mut image, model, camera, scene, seconds, center, scale);
        }
        return image;
    }
    let bands = std::thread::available_parallelism().map_or(1, |n| n.get().min(16));
    let rows = (last - first).div_ceil(bands).max(1);
    let mut depth = vec![f32::INFINITY; width * (last - first)];
    let particle_material = (style == Style::Textured)
        .then(|| super::particle_material::prepare(model, seconds))
        .flatten();
    std::thread::scope(|scope| {
        for (band, (pixels, depth)) in image.pixels[first * width..last * width]
            .chunks_mut(rows * width)
            .zip(depth.chunks_mut(rows * width))
            .enumerate()
        {
            let (prepared, dyes, particle_material) =
                (&prepared, &dyes, particle_material.as_ref());
            let top = first + band * rows;
            let bottom = (top + rows).min(last);
            scope.spawn(move || {
                for triangle in prepared {
                    if triangle.max_y <= top || triangle.min_y >= bottom {
                        continue;
                    }
                    let bindings = super::shader::Bindings::new(model, triangle.index, dyes);
                    raster(
                        Band {
                            pixels,
                            depth,
                            width,
                            top,
                            bottom,
                            scene,
                        },
                        triangle,
                        style,
                        if style == Style::Textured {
                            bindings
                        } else {
                            bindings.clip_only()
                        },
                        particle_material.filter(|_| triangle.emitter),
                    );
                }
            });
        }
    });
    if style == Style::Textured {
        draw_particles(&mut image, model, camera, scene, seconds, center, scale);
    }
    image
}

/// Screen-facing sprites make an effect's packaged image visible while the native parameter
/// program remains unevaluated. The emitter mesh supplies origins and a rough outward drift.
fn draw_particles(
    image: &mut ColorImage,
    model: &Model,
    camera: Camera,
    scene: Scene,
    seconds: f32,
    center: [f32; 3],
    scale: f32,
) {
    let [width, height] = image.size;
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
        let sprite_width =
            (source.width * scale * (0.7 + age * 0.5)).clamp(3.0, width.min(height) as f32 * 0.6);
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
                let rgb = texture.sample_rgba([u, v]);
                let color = source
                    .gradient
                    .and_then(|index| model.textures.get(index))
                    .map_or(rgb, |gradient| gradient.sample_ramp(rgb[0] / 255.0));
                let opacity = strength * rgb[3] / 255.0;
                let index = row * width + column;
                let previous = image.pixels[index];
                let base = [previous.r(), previous.g(), previous.b()];
                let mixed = std::array::from_fn::<_, 3, _>(|lane| {
                    (f32::from(base[lane]) + color[lane] * opacity).clamp(0.0, 255.0) as u8
                });
                image.pixels[index] = Color32::from_rgb(mixed[0], mixed[1], mixed[2]);
            }
        }
    }
}

/// The rows one thread draws.
struct Band<'a> {
    pixels: &'a mut [Color32],
    depth: &'a mut [f32],
    width: usize,
    top: usize,
    bottom: usize,
    scene: Scene,
}

fn edge(a: [f32; 3], b: [f32; 3], x: f32, y: f32) -> f32 {
    (b[0] - a[0]) * (y - a[1]) - (b[1] - a[1]) * (x - a[0])
}

fn raster(
    band: Band<'_>,
    triangle: &Prepared,
    style: Style,
    material: super::shader::Bindings<'_>,
    particle_material: Option<&super::particle_material::State<'_>>,
) {
    let p = triangle.points;
    let uvs = triangle.uvs;
    let normals = triangle.normals;
    let width = band.width;
    let tint = material.tint();
    let area = edge(p[0], p[1], p[2][0], p[2][1]);
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
    let lighting = band.scene.fill
        + band.scene.key
            * ((n[0] * key[0] + n[1] * key[1] + n[2] * key[2]) / length)
                .abs()
                .min(1.0);
    let exposure = band.scene.exposure;
    let color = Color32::from_rgb(
        (205.0 * lighting * exposure).min(255.0) as u8,
        (216.0 * lighting * exposure).min(255.0) as u8,
        (230.0 * lighting * exposure).min(255.0) as u8,
    );
    let edge_lengths = [(1, 2), (2, 0), (0, 1)].map(|(a, b)| {
        ((p[a][0] - p[b][0]).powi(2) + (p[a][1] - p[b][1]).powi(2))
            .sqrt()
            .max(0.0001)
    });
    for y in min_y..max_y {
        for x in min_x..max_x {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let a = edge(p[1], p[2], px, py) / area;
            let b = edge(p[2], p[0], px, py) / area;
            let c = 1.0 - a - b;
            if a < 0.0 || b < 0.0 || c < 0.0 {
                continue;
            }
            let z = a * p[0][2] + b * p[1][2] + c * p[2][2];
            let index = (y - band.top) * width + x;
            // Emissive panels share their surface's depth and are drawn later, so they pass on
            // equality.
            let covered = if material.constant.is_some() {
                z > band.depth[index]
            } else {
                z >= band.depth[index]
            };
            if covered {
                continue;
            }
            let uv: [f32; 2] =
                std::array::from_fn(|axis| a * uvs[0][axis] + b * uvs[1][axis] + c * uvs[2][axis]);
            if let Some(particle) = particle_material {
                let color = particle.sample(uv, exposure);
                let base = band.pixels[index];
                band.pixels[index] = Color32::from_rgb(
                    (f32::from(base.r()) + color[0]).clamp(0.0, 255.0) as u8,
                    (f32::from(base.g()) + color[1]).clamp(0.0, 255.0) as u8,
                    (f32::from(base.b()) + color[2]).clamp(0.0, 255.0) as u8,
                );
                continue;
            }
            if !material.covers(uv) {
                continue;
            }
            {
                band.depth[index] = z;
                band.pixels[index] = if style == Style::Wireframe {
                    let near_edge = [a, b, c]
                        .iter()
                        .zip(edge_lengths)
                        .any(|(weight, length)| weight * area.abs() / length <= 0.8);
                    if near_edge {
                        Color32::from_rgb(180, 215, 245)
                    } else {
                        Color32::from_rgb(
                            band.scene.background[0],
                            band.scene.background[1],
                            band.scene.background[2],
                        )
                    }
                } else if let Some(constant) = material.constant {
                    // Panel art lives in the colour plate: alpha cuts the segments, colour
                    // tints them, and the constant supplies the glow.
                    let base = material
                        .albedo
                        .map_or([255.0; 4], |texture| texture.sample_rgba(uv));
                    if base[3] < 128.0 {
                        continue;
                    }
                    let rgb: [u8; 3] = std::array::from_fn(|i| {
                        super::shader::encode(constant[i] * super::shader::linear(base[i] / 255.0))
                    });
                    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
                } else if let Some(texture) = material.albedo {
                    let smooth = std::array::from_fn(|i| {
                        a * normals[0][i] + b * normals[1][i] + c * normals[2][i]
                    });
                    let basis = basis.map(|basis| basis.with_normal(smooth));
                    let normal = basis.map_or_else(|| n.map(|v| v / length), |b| b.normal);
                    let rgb = material
                        .shade(uv, normal, basis, band.scene)
                        .unwrap_or_else(|| shade(texture.sample(uv), tint, lighting, exposure));
                    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
                } else if tint.is_some() {
                    let rgb = shade([255.0; 3], tint, lighting, exposure);
                    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
                } else {
                    color
                };
            }
        }
    }
}

fn shade(rgb: [f32; 3], tint: Option<[f32; 3]>, lighting: f32, exposure: f32) -> [u8; 3] {
    let lighting = lighting * exposure;
    let Some(tint) = tint else {
        return rgb.map(|v| (v * lighting).min(255.0) as u8);
    };
    std::array::from_fn(|i| {
        let s = (rgb[i] / 255.0).clamp(0.0, 1.0);
        let linear = if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        };
        let v = (linear * tint[i] * lighting).clamp(0.0, 1.0);
        let s = if v <= 0.0031308 {
            v * 12.92
        } else {
            1.055 * v.powf(1.0 / 2.4) - 0.055
        };
        (s * 255.0).round() as u8
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dye_tints_are_linear_and_leave_untinted_textures_unchanged() {
        assert_eq!(
            shade([255.0; 3], Some([0.25, 0.5, 1.0]), 1.0, 1.0),
            [137, 188, 255]
        );
        assert_eq!(shade([80.0, 120.0, 200.0], None, 0.5, 1.0), [40, 60, 100]);
        assert_eq!(shade([255.0; 3], Some([0.0; 3]), 1.0, 1.0), [0; 3]);
    }
}

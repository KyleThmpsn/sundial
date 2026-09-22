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
}
impl Default for Camera {
    fn default() -> Self {
        Self {
            yaw: -0.65,
            pitch: 0.25,
            zoom: 1.0,
        }
    }
}

#[cfg(test)]
pub(crate) fn image(model: &Model, camera: Camera, size: [usize; 2]) -> ColorImage {
    frame(model, camera, size, &model.vertices, Style::Textured, 0.0)
}

pub(crate) fn animated_image(
    model: &Model,
    camera: Camera,
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
        size,
        vertices.as_deref().unwrap_or(&model.vertices),
        Style::Textured,
        seconds,
    )
}

pub(crate) fn styled_image(
    model: &Model,
    camera: Camera,
    size: [usize; 2],
    seconds: f32,
    style: Style,
) -> ColorImage {
    if style == Style::Textured {
        return animated_image(model, camera, size, seconds);
    }
    let vertices = model
        .animation
        .as_ref()
        .map(|a| a.vertices(model, seconds.rem_euclid(a.duration())));
    frame(
        model,
        camera,
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
    points: [[f32; 3]; 3],
    uvs: [[f32; 2]; 3],
    normals: [[f32; 3]; 3],
    min_y: usize,
    max_y: usize,
}

fn frame(
    model: &Model,
    camera: Camera,
    size: [usize; 2],
    vertices: &[[f32; 3]],
    style: Style,
    seconds: f32,
) -> ColorImage {
    let [width, height] = size.map(|v| v.clamp(1, 640));
    let mut image = ColorImage::new([width, height], Color32::from_rgb(24, 28, 35));
    if model.triangles.is_empty() {
        return image;
    }
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
                width as f32 * 0.5 + horizontal * scale,
                height as f32 * 0.5 - (sp * forward + cp * z) * scale,
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
        return image;
    }
    let bands = std::thread::available_parallelism().map_or(1, |n| n.get().min(16));
    let rows = (last - first).div_ceil(bands).max(1);
    let mut depth = vec![f32::INFINITY; width * (last - first)];
    std::thread::scope(|scope| {
        for (band, (pixels, depth)) in image.pixels[first * width..last * width]
            .chunks_mut(rows * width)
            .zip(depth.chunks_mut(rows * width))
            .enumerate()
        {
            let (prepared, dyes) = (&prepared, &dyes);
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
                        },
                        triangle,
                        style,
                        if style == Style::Textured {
                            bindings
                        } else {
                            bindings.clip_only()
                        },
                    );
                }
            });
        }
    });
    image
}

/// The rows one thread draws.
struct Band<'a> {
    pixels: &'a mut [Color32],
    depth: &'a mut [f32],
    width: usize,
    top: usize,
    bottom: usize,
}

fn edge(a: [f32; 3], b: [f32; 3], x: f32, y: f32) -> f32 {
    (b[0] - a[0]) * (y - a[1]) - (b[1] - a[1]) * (x - a[0])
}

fn raster(
    band: Band<'_>,
    triangle: &Prepared,
    style: Style,
    material: super::shader::Bindings<'_>,
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
    let lighting = 0.30
        + 0.70
            * ((n[0] * -0.35 + n[1] * -0.55 + n[2] * -0.76) / length)
                .abs()
                .min(1.0);
    let color = Color32::from_rgb(
        (205.0 * lighting) as u8,
        (216.0 * lighting) as u8,
        (230.0 * lighting) as u8,
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
                        Color32::from_rgb(24, 28, 35)
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
                        .shade(uv, normal, basis)
                        .unwrap_or_else(|| shade(texture.sample(uv), tint, lighting));
                    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
                } else if tint.is_some() {
                    let rgb = shade([255.0; 3], tint, lighting);
                    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
                } else {
                    color
                };
            }
        }
    }
}

fn shade(rgb: [f32; 3], tint: Option<[f32; 3]>, lighting: f32) -> [u8; 3] {
    let Some(tint) = tint else {
        return rgb.map(|v| (v * lighting) as u8);
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
            shade([255.0; 3], Some([0.25, 0.5, 1.0]), 1.0),
            [137, 188, 255]
        );
        assert_eq!(shade([80.0, 120.0, 200.0], None, 0.5), [40, 60, 100]);
        assert_eq!(shade([255.0; 3], Some([0.0; 3]), 1.0), [0; 3]);
    }
}

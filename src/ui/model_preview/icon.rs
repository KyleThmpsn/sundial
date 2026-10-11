//! Render actual native geometry as a transparent inventory artwork layer.
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Pose {
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
}

/// Placement of the complete model fit within the final artwork square.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Frame {
    /// Relative to the complete-model fit. Larger values deliberately crop at the edges.
    pub zoom: f32,
    /// Fractions of the output edge, positive toward the right and bottom.
    pub offset: [f32; 2],
}

impl Default for Frame {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            offset: [0.0; 2],
        }
    }
}

pub struct Artwork {
    /// Straight-alpha sRGB, suitable for the primary layer of a native icon.
    pub rgba: Vec<u8>,
    pub size: usize,
    pub vertices: usize,
    pub triangles: usize,
    pub bounds: ([f32; 3], [f32; 3]),
}

/// Render a packaged model or entity with native materials and fit its real coverage.
/// Package loading and software rendering are blocking work for a background job.
pub fn object(packages: &Path, entity: u32, pose: Pose, size: usize) -> Result<Artwork, String> {
    object_in_frame(packages, entity, pose, Frame::default(), size)
}

/// Render native geometry with explicit final placement, including intentional edge cropping.
pub fn object_in_frame(
    packages: &Path,
    entity: u32,
    pose: Pose,
    frame: Frame,
    size: usize,
) -> Result<Artwork, String> {
    validate(pose, frame, size)?;
    let model =
        model_preview::load_reported(packages, entity, &model_preview::Load::default(), None)?;
    draw(&model, pose, frame, size)
}

/// Render an unbuilt, pinned import through the same decoder as packaged objects.
pub fn local(
    packages: &Path,
    appearance: &LocalAppearance,
    pose: Pose,
    frame: Frame,
    size: usize,
) -> Result<Artwork, String> {
    validate(pose, frame, size)?;
    let model = appearance.load(packages, &model_preview::Load::default(), None, None)?;
    draw(&model, pose, frame, size)
}

fn validate(pose: Pose, frame: Frame, size: usize) -> Result<(), String> {
    if !(64..=2048).contains(&size)
        || [pose.yaw, pose.pitch, pose.roll]
            .iter()
            .any(|v| !v.is_finite())
        || pose.pitch.abs() > render::MAX_PITCH
    {
        return Err("Invalid icon size or camera pose".into());
    }
    if !frame.zoom.is_finite()
        || !(0.25..=4.0).contains(&frame.zoom)
        || frame.offset.iter().any(|v| !v.is_finite() || v.abs() > 1.0)
    {
        return Err("Invalid icon framing".into());
    }
    Ok(())
}

fn draw(model: &Model, pose: Pose, frame: Frame, size: usize) -> Result<Artwork, String> {
    let bounds = render::artwork_bounds(model);
    let working = size.max(768);
    let mut camera = render::Camera {
        yaw: pose.yaw,
        pitch: pose.pitch,
        ..Default::default()
    };
    camera.zoom = render::fitted_zoom(model, bounds, camera, [working as f32; 2], 0.70);
    let image = render::transparent_image(model, camera, working);
    let rgba = fit(&image, pose.roll, frame, size)?;
    Ok(Artwork {
        rgba,
        size,
        vertices: model.vertices.len(),
        triangles: model.triangles.len(),
        bounds,
    })
}

fn fit(image: &egui::ColorImage, angle: f32, frame: Frame, size: usize) -> Result<Vec<u8>, String> {
    let [width, height] = image.size;
    let (sin, cos) = angle.sin_cos();
    let rotate = |x: f32, y: f32| [cos * x - sin * y, sin * x + cos * y];
    let mut low = [f32::INFINITY; 2];
    let mut high = [f32::NEG_INFINITY; 2];
    for (i, pixel) in image.pixels.iter().enumerate().filter(|(_, p)| p.a() > 0) {
        let x = (i % width) as f32 - width as f32 * 0.5;
        let y = (i / width) as f32 - height as f32 * 0.5;
        if i % width == 0 || i % width == width - 1 || i / width == 0 || i / width == height - 1 {
            return Err("The model extends beyond the icon render frame".into());
        }
        let _ = pixel;
        for dx in [-0.5, 0.5] {
            for dy in [-0.5, 0.5] {
                let point = rotate(x + dx, y + dy);
                for axis in 0..2 {
                    low[axis] = low[axis].min(point[axis]);
                    high[axis] = high[axis].max(point[axis]);
                }
            }
        }
    }
    if !low[0].is_finite() {
        return Err("The selected model has no visible icon surface".into());
    }
    let center = [(low[0] + high[0]) * 0.5, (low[1] + high[1]) * 0.5];
    let scale = size as f32 * 0.90 * frame.zoom / (high[0] - low[0]).max(high[1] - low[1]).max(1.0);
    let destination = frame.offset.map(|offset| size as f32 * (0.5 + offset));
    let mut rgba = Vec::with_capacity(size * size * 4);
    for y in 0..size {
        for x in 0..size {
            let point = [
                (x as f32 + 0.5 - destination[0]) / scale + center[0],
                (y as f32 + 0.5 - destination[1]) / scale + center[1],
            ];
            let source = [
                cos * point[0] + sin * point[1] + width as f32 * 0.5,
                -sin * point[0] + cos * point[1] + height as f32 * 0.5,
            ];
            rgba.extend(sample(image, source));
        }
    }
    Ok(rgba)
}

fn sample(image: &egui::ColorImage, [x, y]: [f32; 2]) -> [u8; 4] {
    let mut value = [0.0f32; 4];
    for row in 0..2 {
        for column in 0..2 {
            let px = x.floor() as isize + column;
            let py = y.floor() as isize + row;
            if px < 0 || py < 0 || px as usize >= image.size[0] || py as usize >= image.size[1] {
                continue;
            }
            let weight = (if column == 0 {
                1.0 - x.fract()
            } else {
                x.fract()
            }) * (if row == 0 { 1.0 - y.fract() } else { y.fract() });
            let color =
                image.pixels[py as usize * image.size[0] + px as usize].to_srgba_unmultiplied();
            let alpha = f32::from(color[3]) / 255.0;
            for axis in 0..3 {
                value[axis] += f32::from(color[axis]) * alpha * weight;
            }
            value[3] += alpha * weight;
        }
    }
    if value[3] <= 0.0 {
        return [0; 4];
    }
    [
        (value[0] / value[3]).round().clamp(0.0, 255.0) as u8,
        (value[1] / value[3]).round().clamp(0.0, 255.0) as u8,
        (value[2] / value[3]).round().clamp(0.0, 255.0) as u8,
        (value[3] * 255.0).round().clamp(0.0, 255.0) as u8,
    ]
}

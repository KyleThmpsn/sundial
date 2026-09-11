//! Saved image placement and background shared by previews and package output.
use image::{Rgba, RgbaImage};
use serde::{Deserialize, Serialize};

pub(crate) const UNITS: u16 = 10_000;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Fit {
    #[default]
    Contain,
    Cover,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Background {
    #[default]
    Transparent,
    Sunrise,
    Solid {
        color: [u8; 3],
    },
    Gradient {
        start: [u8; 3],
        end: [u8; 3],
        angle: u16,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Composition {
    /// Source rectangle in ten-thousandths, before orientation and placement.
    pub crop: [u16; 4],
    pub fit: Fit,
    pub scale: u16,
    pub offset: [i16; 2],
    pub rotation: u8,
    pub flip_horizontal: bool,
    pub flip_vertical: bool,
    pub background: Background,
}

impl Default for Composition {
    fn default() -> Self {
        Self {
            crop: [0, 0, UNITS, UNITS],
            fit: Fit::Contain,
            scale: 100,
            offset: [0, 0],
            rotation: 0,
            flip_horizontal: false,
            flip_vertical: false,
            background: Background::Transparent,
        }
    }
}

impl Composition {
    pub(crate) fn validate(&self) -> Result<(), String> {
        let [x, y, w, h] = self.crop;
        if w == 0
            || h == 0
            || u32::from(x) + u32::from(w) > u32::from(UNITS)
            || u32::from(y) + u32::from(h) > u32::from(UNITS)
        {
            return Err("The crop must be a nonempty rectangle inside the image.".into());
        }
        if !(10..=400).contains(&self.scale)
            || self.offset.iter().any(|v| !(-100..=100).contains(v))
            || self.rotation > 3
        {
            return Err("Artwork placement is outside the supported range.".into());
        }
        if matches!(self.background, Background::Gradient { angle, .. } if angle > 360) {
            return Err("Gradient direction must be between 0 and 360 degrees.".into());
        }
        Ok(())
    }

    pub(crate) fn source(&self, image: &RgbaImage) -> RgbaImage {
        let [x, y, w, h] = self.crop;
        let edge = |value: u16, size: u32| u32::from(value) * size / u32::from(UNITS);
        let left = edge(x, image.width()).min(image.width() - 1);
        let top = edge(y, image.height()).min(image.height() - 1);
        let width = edge(x + w, image.width()).saturating_sub(left).max(1);
        let height = edge(y + h, image.height()).saturating_sub(top).max(1);
        let cropped = image::imageops::crop_imm(image, left, top, width, height).to_image();
        let mut oriented = match self.rotation {
            1 => image::imageops::rotate90(&cropped),
            2 => image::imageops::rotate180(&cropped),
            3 => image::imageops::rotate270(&cropped),
            _ => cropped,
        };
        if self.flip_horizontal {
            image::imageops::flip_horizontal_in_place(&mut oriented);
        }
        if self.flip_vertical {
            image::imageops::flip_vertical_in_place(&mut oriented);
        }
        oriented
    }

    pub(crate) fn render(&self, image: &RgbaImage, width: u32, height: u32) -> RgbaImage {
        let source = self.source(image);
        let sx = width as f32 / source.width() as f32;
        let sy = height as f32 / source.height() as f32;
        let scale = match self.fit {
            Fit::Contain => sx.min(sy),
            Fit::Cover => sx.max(sy),
        } * f32::from(self.scale)
            / 100.0;
        let left = (width as f32 - source.width() as f32 * scale) * 0.5
            + f32::from(self.offset[0]) * width as f32 / 100.0;
        let top = (height as f32 - source.height() as f32 * scale) * 0.5
            + f32::from(self.offset[1]) * height as f32 / 100.0;
        RgbaImage::from_fn(width, height, |x, y| {
            let mut background = self.background.pixel(x, y, width, height);
            let px = (x as f32 + 0.5 - left) / scale;
            let py = (y as f32 + 0.5 - top) / scale;
            if px >= 0.0 && py >= 0.0 && px < source.width() as f32 && py < source.height() as f32 {
                let foreground = sample(&source, px - 0.5, py - 0.5);
                image::Pixel::blend(&mut background, &foreground);
            }
            background
        })
    }
}

impl Background {
    fn pixel(&self, x: u32, y: u32, width: u32, height: u32) -> Rgba<u8> {
        match self {
            Self::Transparent => Rgba([0; 4]),
            Self::Sunrise => crate::badge_icon::card_background(x, y, width, height),
            Self::Solid { color } => Rgba([color[0], color[1], color[2], 255]),
            Self::Gradient { start, end, angle } => {
                let radians = f32::from(*angle).to_radians();
                let (sin, cos) = radians.sin_cos();
                let px = x as f32 / width.saturating_sub(1).max(1) as f32 - 0.5;
                let py = y as f32 / height.saturating_sub(1).max(1) as f32 - 0.5;
                let t = ((px * cos + py * sin) / (cos.abs() + sin.abs()) + 0.5).clamp(0.0, 1.0);
                let mut result = [255; 4];
                for i in 0..3 {
                    result[i] =
                        (f32::from(start[i]) * (1.0 - t) + f32::from(end[i]) * t).round() as u8;
                }
                Rgba(result)
            }
        }
    }
}

// Sample the visible canvas directly so a very wide image in Fill mode never
// allocates an enormous intermediate. Interpolate premultiplied color to keep
// hidden RGB in transparent PNGs from bleeding into the result.
fn sample(image: &RgbaImage, x: f32, y: f32) -> Rgba<u8> {
    let x = x.clamp(0.0, (image.width() - 1) as f32);
    let y = y.clamp(0.0, (image.height() - 1) as f32);
    let left = x.floor() as u32;
    let top = y.floor() as u32;
    let fx = x.fract();
    let fy = y.fract();
    let mut sum = [0.0; 4];
    for (px, py, weight) in [
        (left, top, (1.0 - fx) * (1.0 - fy)),
        ((left + 1).min(image.width() - 1), top, fx * (1.0 - fy)),
        (left, (top + 1).min(image.height() - 1), (1.0 - fx) * fy),
        (
            (left + 1).min(image.width() - 1),
            (top + 1).min(image.height() - 1),
            fx * fy,
        ),
    ] {
        let pixel = image.get_pixel(px, py);
        let alpha = f32::from(pixel[3]) * weight;
        for i in 0..3 {
            sum[i] += f32::from(pixel[i]) * alpha;
        }
        sum[3] += alpha;
    }
    if sum[3] == 0.0 {
        return Rgba([0; 4]);
    }
    Rgba([
        (sum[0] / sum[3]).round() as u8,
        (sum[1] / sum[3]).round() as u8,
        (sum[2] / sum[3]).round() as u8,
        sum[3].round() as u8,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crop_fill_and_position_control_the_visible_image_without_stretching() {
        let source = RgbaImage::from_fn(80, 40, |x, _| {
            Rgba(if x < 40 {
                [255, 0, 0, 255]
            } else {
                [0, 255, 0, 255]
            })
        });
        let edit = Composition {
            crop: [5000, 0, 5000, 10000],
            background: Background::Solid {
                color: [10, 20, 30],
            },
            ..Default::default()
        };
        let fitted = edit.render(&source, 100, 50);
        assert_eq!(fitted.get_pixel(0, 25).0, [10, 20, 30, 255]);
        assert_eq!(fitted.get_pixel(50, 25).0, [0, 255, 0, 255]);
        let filled = Composition {
            fit: Fit::Cover,
            ..edit.clone()
        }
        .render(&source, 100, 50);
        assert!(filled.pixels().all(|p| p.0 == [0, 255, 0, 255]));
        let moved = Composition {
            offset: [25, 0],
            ..edit
        }
        .render(&source, 100, 50);
        assert_eq!(moved.get_pixel(30, 25).0, [10, 20, 30, 255]);
        assert_eq!(moved.get_pixel(90, 25).0, [0, 255, 0, 255]);
    }

    #[test]
    fn transparent_edges_do_not_bleed_hidden_color_and_gradients_reach_both_endpoints() {
        let source = RgbaImage::from_fn(2, 1, |x, _| {
            Rgba(if x == 0 {
                [255, 0, 0, 0]
            } else {
                [0, 0, 255, 255]
            })
        });
        let result = Composition::default().render(&source, 20, 10);
        assert!(
            result
                .pixels()
                .filter(|p| p[3] > 0)
                .all(|p| p[0] == 0 && p[2] == 255)
        );
        let empty = RgbaImage::new(1, 1);
        for angle in [0, 90] {
            let result = Composition {
                background: Background::Gradient {
                    start: [3, 14, 25],
                    end: [103, 114, 125],
                    angle,
                },
                ..Default::default()
            }
            .render(&empty, 101, 51);
            assert_eq!(result.get_pixel(0, 0).0, [3, 14, 25, 255]);
            assert_eq!(result.get_pixel(100, 50).0, [103, 114, 125, 255]);
        }
    }
}

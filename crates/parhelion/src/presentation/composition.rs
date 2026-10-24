//! Saved image placement and background shared by previews and package output.
use image::{Rgba, RgbaImage};
use serde::{Deserialize, Serialize};
mod adjustments;
pub(crate) use adjustments::Adjustments;

pub(crate) const UNITS: u16 = 10_000;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Fit {
    #[default]
    Contain,
    Cover,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, Hash)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Background {
    #[default]
    Transparent,
    Sunrise,
    Dawn,
    Solid {
        color: [u8; 3],
    },
    Gradient {
        start: [u8; 3],
        end: [u8; 3],
        angle: u16,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Hash)]
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
    #[serde(skip_serializing_if = "Adjustments::is_identity")]
    pub adjustments: Adjustments,
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
            adjustments: Adjustments::default(),
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
        self.adjustments.validate()
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
        self.adjustments.apply(&mut oriented);
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
        // A bilinear lookup alone skips narrow rays and other details when shrinking.
        // Filter to the displayed footprint first. This only allocates a smaller image,
        // so Cover and zoom still cannot create an oversized intermediate canvas.
        let filtered = (scale < 1.0).then(|| {
            crate::image_import::resize(
                &source,
                (source.width() as f32 * scale).ceil().max(1.0) as u32,
                (source.height() as f32 * scale).ceil().max(1.0) as u32,
            )
        });
        let sampling = filtered.as_ref().unwrap_or(&source);
        let sample_x = sampling.width() as f32 / source.width() as f32;
        let sample_y = sampling.height() as f32 / source.height() as f32;
        RgbaImage::from_fn(width, height, |x, y| {
            let mut background = self.background.pixel(x, y, width, height);
            let px = (x as f32 + 0.5 - left) / scale;
            let py = (y as f32 + 0.5 - top) / scale;
            if px >= 0.0 && py >= 0.0 && px < source.width() as f32 && py < source.height() as f32 {
                let foreground =
                    crate::image_import::sample(sampling, px * sample_x - 0.5, py * sample_y - 0.5);
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
            Self::Dawn => crate::branding::dawn_background(y, height),
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

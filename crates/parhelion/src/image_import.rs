//! Bounded image decoding and alpha-correct resizing shared by artwork importers, and pictures
//! embedded in recipes.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use image::{ImageFormat, ImageReader, Rgba, RgbaImage, imageops::FilterType};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{
    fmt,
    fs::File,
    io::{Cursor, Read},
    path::Path,
    sync::Arc,
};

pub(crate) const MAX_FILE_BYTES: usize = 16 * 1024 * 1024;
pub(crate) const MAX_SOURCE_EDGE: u32 = 4096;

pub(crate) fn read_path(path: &Path) -> Result<Vec<u8>, String> {
    let file = File::open(path).map_err(|error| format!("Could not open image: {error}"))?;
    let mut bytes = Vec::new();
    file.take((MAX_FILE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Could not read image: {error}"))?;
    check_size(&bytes)?;
    Ok(bytes)
}

fn check_size(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() > MAX_FILE_BYTES {
        return Err("Choose an image no larger than 16 MiB.".to_owned());
    }
    Ok(())
}

pub(crate) fn decode_source(bytes: &[u8]) -> Result<RgbaImage, String> {
    check_size(bytes)?;
    let format =
        image::guess_format(bytes).map_err(|_| "Choose a PNG or JPEG image.".to_owned())?;
    if !matches!(format, ImageFormat::Png | ImageFormat::Jpeg) {
        return Err("Choose a PNG or JPEG image.".to_owned());
    }
    decode(bytes, format, MAX_SOURCE_EDGE)
}

pub(crate) fn decode_png(bytes: &[u8]) -> Result<RgbaImage, String> {
    check_size(bytes)?;
    decode(bytes, ImageFormat::Png, MAX_SOURCE_EDGE)
}

pub(crate) fn decode(
    bytes: &[u8],
    format: ImageFormat,
    max_edge: u32,
) -> Result<RgbaImage, String> {
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(max_edge);
    limits.max_image_height = Some(max_edge);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let decoded = reader
        .decode()
        .map_err(|error| {
            format!("Could not decode image (maximum {max_edge}×{max_edge}): {error}")
        })?
        .into_rgba8();
    if decoded.width() == 0 || decoded.height() == 0 {
        return Err("Image dimensions must be nonzero.".to_owned());
    }
    Ok(decoded)
}

pub(crate) fn fit(source: &RgbaImage, width: u32, height: u32) -> RgbaImage {
    if source.dimensions() == (width, height) {
        return source.clone();
    }
    let scale = (f64::from(width) / f64::from(source.width()))
        .min(f64::from(height) / f64::from(source.height()));
    let fitted_width = ((f64::from(source.width()) * scale).round() as u32).clamp(1, width);
    let fitted_height = ((f64::from(source.height()) * scale).round() as u32).clamp(1, height);
    let resized = resize(source, fitted_width, fitted_height);
    let mut canvas = RgbaImage::new(width, height);
    image::imageops::replace(
        &mut canvas,
        &resized,
        i64::from((width - fitted_width) / 2),
        i64::from((height - fitted_height) / 2),
    );
    canvas
}

/// `source` scaled to cover `width` x `height`, with what overflows cropped evenly from both edges.
pub(crate) fn cover(source: &RgbaImage, width: u32, height: u32) -> RgbaImage {
    if source.dimensions() == (width, height) {
        return source.clone();
    }
    if width == 0 || height == 0 || source.width() == 0 || source.height() == 0 {
        return RgbaImage::new(width, height);
    }
    let scale = (width as f32 / source.width() as f32).max(height as f32 / source.height() as f32);
    // Only shrink the source. Enlarging the entire image before cropping can
    // allocate gigabytes for a one-pixel-wide import and a modest output canvas.
    let filtered = (scale < 1.0).then(|| {
        resize(
            source,
            (source.width() as f32 * scale).ceil().max(1.0) as u32,
            (source.height() as f32 * scale).ceil().max(1.0) as u32,
        )
    });
    let sampling = filtered.as_ref().unwrap_or(source);
    let sample_x = sampling.width() as f32 / source.width() as f32;
    let sample_y = sampling.height() as f32 / source.height() as f32;
    RgbaImage::from_fn(width, height, |x, y| {
        let px = (x as f32 + 0.5 - width as f32 * 0.5) / scale + source.width() as f32 * 0.5;
        let py = (y as f32 + 0.5 - height as f32 * 0.5) / scale + source.height() as f32 * 0.5;
        sample(sampling, px * sample_x - 0.5, py * sample_y - 0.5)
    })
}

/// Bilinear sampling of unpremultiplied RGBA, interpolated in premultiplied space
/// so hidden color in transparent source pixels cannot bleed into visible edges.
pub(crate) fn sample(image: &RgbaImage, x: f32, y: f32) -> Rgba<u8> {
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

/// The most an embedded picture keeps on its long side. A larger import is scaled down to it.
pub(crate) const MAX_EMBEDDED_EDGE: u32 = 2400;
// A 2400×2400 RGBA PNG needs roughly 30 MiB in base64 even when the imported
// JPEG fits the source limit. Keep the reader compatible with everything we save.
const MAX_EMBEDDED_BASE64_BYTES: usize = 32 * 1024 * 1024;

/// A picture saved in a recipe as PNG at its imported size, up to [`MAX_EMBEDDED_EDGE`] on its long
/// side, so the recipe needs no source file. Each use sizes it, with [`cover`] or [`fit`]. Clones
/// share the pixels.
#[derive(Clone, Eq, PartialEq)]
pub struct EmbeddedImage(Arc<EmbeddedImageData>);

#[derive(Eq, PartialEq)]
struct EmbeddedImageData {
    rgba: RgbaImage,
    png_base64: String,
}

impl fmt::Debug for EmbeddedImage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EmbeddedImage")
            .field("size", &self.0.rgba.dimensions())
            .finish_non_exhaustive()
    }
}

impl EmbeddedImage {
    pub(crate) fn from_path(path: &Path) -> Result<Self, String> {
        Self::from_rgba(decode_source(&read_path(path)?)?)
    }

    pub(crate) fn from_rgba(rgba: RgbaImage) -> Result<Self, String> {
        let edge = rgba.width().max(rgba.height());
        let rgba = if edge > MAX_EMBEDDED_EDGE {
            resize(
                &rgba,
                (rgba.width() * MAX_EMBEDDED_EDGE / edge).max(1),
                (rgba.height() * MAX_EMBEDDED_EDGE / edge).max(1),
            )
        } else {
            rgba
        };
        let mut png = Cursor::new(Vec::new());
        rgba.write_to(&mut png, ImageFormat::Png)
            .map_err(|error| format!("Could not encode image: {error}"))?;
        Ok(Self(Arc::new(EmbeddedImageData {
            rgba,
            png_base64: STANDARD.encode(png.into_inner()),
        })))
    }

    pub(crate) fn pixels(&self) -> &RgbaImage {
        &self.0.rgba
    }

    /// Names the picture by what it saves as, for caches that should not hold its pixels.
    pub(crate) fn fingerprint(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.0.png_base64.hash(&mut hasher);
        hasher.finish()
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EmbeddedPng {
    png_base64: String,
}

impl Serialize for EmbeddedImage {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        EmbeddedPng {
            png_base64: self.0.png_base64.clone(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for EmbeddedImage {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let embedded = EmbeddedPng::deserialize(deserializer)?;
        if embedded.png_base64.len() > MAX_EMBEDDED_BASE64_BYTES {
            return Err(serde::de::Error::custom(
                "Embedded image exceeds the size limit",
            ));
        }
        let bytes = STANDARD
            .decode(&embedded.png_base64)
            .map_err(serde::de::Error::custom)?;
        let rgba = decode(&bytes, ImageFormat::Png, MAX_EMBEDDED_EDGE)
            .map_err(serde::de::Error::custom)?;
        Ok(Self(Arc::new(EmbeddedImageData {
            rgba,
            png_base64: embedded.png_base64,
        })))
    }
}

// Filter premultiplied pixels to prevent hidden RGB in transparent PNGs from bleeding at edges.
pub(crate) fn resize(source: &RgbaImage, width: u32, height: u32) -> RgbaImage {
    if source.dimensions() == (width, height) {
        return source.clone();
    }
    let mut premultiplied = source.clone();
    for pixel in premultiplied.pixels_mut() {
        let alpha = u32::from(pixel[3]);
        for channel in &mut pixel.0[..3] {
            *channel = ((u32::from(*channel) * alpha + 127) / 255) as u8;
        }
    }
    let mut resized = image::imageops::resize(&premultiplied, width, height, FilterType::Lanczos3);
    for pixel in resized.pixels_mut() {
        let alpha = u32::from(pixel[3]);
        for channel in &mut pixel.0[..3] {
            *channel = if alpha == 0 {
                0
            } else {
                ((u32::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8
            };
        }
    }
    resized
}

#[cfg(test)]
mod tests;

//! Bounded image decoding and alpha-correct resizing shared by artwork importers.

use image::{ImageFormat, ImageReader, RgbaImage, imageops::FilterType};
use std::{
    fs::File,
    io::{Cursor, Read},
    path::Path,
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

// Filter premultiplied pixels to prevent hidden RGB in transparent PNGs from bleeding at edges.
pub(crate) fn fit(source: &RgbaImage, width: u32, height: u32) -> RgbaImage {
    if source.dimensions() == (width, height) {
        return source.clone();
    }
    let scale = (f64::from(width) / f64::from(source.width()))
        .min(f64::from(height) / f64::from(source.height()));
    let fitted_width = ((f64::from(source.width()) * scale).round() as u32).clamp(1, width);
    let fitted_height = ((f64::from(source.height()) * scale).round() as u32).clamp(1, height);
    let mut premultiplied = source.clone();
    for pixel in premultiplied.pixels_mut() {
        let alpha = u32::from(pixel[3]);
        for channel in &mut pixel.0[..3] {
            *channel = ((u32::from(*channel) * alpha + 127) / 255) as u8;
        }
    }
    let mut resized = image::imageops::resize(
        &premultiplied,
        fitted_width,
        fitted_height,
        FilterType::Lanczos3,
    );
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
    let mut canvas = RgbaImage::new(width, height);
    image::imageops::replace(
        &mut canvas,
        &resized,
        i64::from((width - fitted_width) / 2),
        i64::from((height - fitted_height) / 2),
    );
    canvas
}

#[cfg(test)]
mod tests;

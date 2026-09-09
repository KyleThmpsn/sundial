//! Bounded image imports embedded in recipes, with no dependency on the original file.

use std::{
    fmt,
    fs::File,
    io::{Cursor, Read},
    path::Path,
    sync::Arc,
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use image::{ImageFormat, ImageReader, RgbaImage, imageops::FilterType};
use serde::{Deserialize, Deserializer, Serialize, Serializer, ser::SerializeStruct};

use super::ICON_PREVIEW_SIZE;

const MAX_FILE_BYTES: usize = 16 * 1024 * 1024;
const MAX_SOURCE_EDGE: u32 = 4096;
const MAX_EMBEDDED_BYTES: usize = 64 * 1024;
const ICON_EDGE: u32 = ICON_PREVIEW_SIZE as u32;

/// Validated, normalized primary artwork. Clones share immutable pixels.
#[derive(Clone, Eq, PartialEq)]
pub struct ImportedIcon(Arc<ImportedIconData>);

#[derive(Eq, PartialEq)]
struct ImportedIconData {
    rgba: RgbaImage,
    // Recipe fingerprints are computed during UI updates; never re-encode PNG on that hot path.
    png_base64: String,
}

impl fmt::Debug for ImportedIcon {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ImportedIcon")
            .field("size", &self.0.rgba.dimensions())
            .finish_non_exhaustive()
    }
}

impl ImportedIcon {
    pub(super) fn from_path(path: &Path) -> Result<Self, String> {
        let file = File::open(path).map_err(|error| format!("Could not open image: {error}"))?;
        let mut bytes = Vec::new();
        file.take((MAX_FILE_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|error| format!("Could not read image: {error}"))?;
        Self::from_bytes(&bytes)
    }

    pub(crate) fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_FILE_BYTES {
            return Err("Choose an image no larger than 16 MiB.".to_owned());
        }
        let format =
            image::guess_format(bytes).map_err(|_| "Choose a PNG or JPEG image.".to_owned())?;
        if !matches!(format, ImageFormat::Png | ImageFormat::Jpeg) {
            return Err("Choose a PNG or JPEG image.".to_owned());
        }
        let source = decode(bytes, format, MAX_SOURCE_EDGE)?;
        Self::from_normalized(fit(&source, ICON_EDGE, ICON_EDGE))
    }

    fn from_normalized(rgba: RgbaImage) -> Result<Self, String> {
        let mut png = Cursor::new(Vec::new());
        rgba.write_to(&mut png, ImageFormat::Png)
            .map_err(|error| format!("Could not encode imported icon: {error}"))?;
        Ok(Self(Arc::new(ImportedIconData {
            rgba,
            png_base64: STANDARD.encode(png.into_inner()),
        })))
    }

    pub(super) fn fit_to(&self, width: u32, height: u32) -> RgbaImage {
        fit(&self.0.rgba, width, height)
    }
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

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EmbeddedImage {
    png_base64: String,
}

impl Serialize for ImportedIcon {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("EmbeddedImage", 1)?;
        state.serialize_field("png_base64", &self.0.png_base64)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for ImportedIcon {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let encoded = EmbeddedImage::deserialize(deserializer)?;
        if encoded.png_base64.len() > MAX_EMBEDDED_BYTES * 4 / 3 + 4 {
            return Err(serde::de::Error::custom(
                "Embedded icon exceeds the size limit",
            ));
        }
        let bytes = STANDARD
            .decode(encoded.png_base64)
            .map_err(serde::de::Error::custom)?;
        let image =
            decode(&bytes, ImageFormat::Png, ICON_EDGE).map_err(serde::de::Error::custom)?;
        if image.dimensions() != (ICON_EDGE, ICON_EDGE) {
            return Err(serde::de::Error::custom(
                "Embedded icons must be normalized to 96×96",
            ));
        }
        Self::from_normalized(image).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests;

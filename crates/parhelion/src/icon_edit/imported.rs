//! Bounded image imports embedded in recipes, with no dependency on the original file.

use std::{fmt, io::Cursor, path::Path, sync::Arc};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use image::{ImageFormat, RgbaImage};
use serde::{Deserialize, Deserializer, Serialize, Serializer, ser::SerializeStruct};

use super::ICON_PREVIEW_SIZE;
use crate::image_import::{decode, decode_source, fit, read_path};

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
        let bytes = read_path(path)?;
        Self::from_bytes(&bytes)
    }

    pub(crate) fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        let source = decode_source(bytes)?;
        Self::from_normalized(fit(&source, ICON_EDGE, ICON_EDGE))
    }

    pub(super) fn from_normalized(rgba: RgbaImage) -> Result<Self, String> {
        let mut png = Cursor::new(Vec::new());
        rgba.write_to(&mut png, ImageFormat::Png)
            .map_err(|error| format!("Could not encode imported icon: {error}"))?;
        Ok(Self(Arc::new(ImportedIconData {
            rgba,
            png_base64: STANDARD.encode(png.into_inner()),
        })))
    }

    pub(crate) fn fit_to(&self, width: u32, height: u32) -> RgbaImage {
        fit(&self.0.rgba, width, height)
    }
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

use base64::{Engine as _, engine::general_purpose::STANDARD};
use image::{ImageFormat, RgbaImage};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{
    fmt,
    io::{Cursor, Read},
    path::Path,
    sync::Arc,
};

const MAX_BYTES: usize = 16 * 1024 * 1024;
pub(crate) const WIDTH: u32 = 512;
pub(crate) const HEIGHT: u32 = 512;
#[derive(Clone, Eq, PartialEq)]
pub struct Artwork(Arc<Data>);
#[derive(Eq, PartialEq)]
struct Data {
    pixels: RgbaImage,
    encoded: String,
}
impl fmt::Debug for Artwork {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Artwork")
            .field("size", &(WIDTH, HEIGHT))
            .finish_non_exhaustive()
    }
}
impl Artwork {
    pub fn from_png(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_BYTES {
            return Err("Choose a PNG no larger than 16 MiB.".into());
        }
        let source = crate::icon_edit::decode_image(bytes, ImageFormat::Png, 4096)?;
        Self::normalized(crate::icon_edit::fit_rgba_image(&source, WIDTH, HEIGHT))
    }
    pub(crate) fn from_path(path: &Path) -> Result<Self, String> {
        let mut bytes = vec![];
        std::fs::File::open(path)
            .map_err(|e| e.to_string())?
            .take((MAX_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        Self::from_png(&bytes)
    }
    fn normalized(pixels: RgbaImage) -> Result<Self, String> {
        let mut output = Cursor::new(vec![]);
        pixels
            .write_to(&mut output, ImageFormat::Png)
            .map_err(|e| e.to_string())?;
        Ok(Self(Arc::new(Data {
            pixels,
            encoded: STANDARD.encode(output.into_inner()),
        })))
    }
    pub(crate) fn pixels(&self) -> &RgbaImage {
        &self.0.pixels
    }
    pub(crate) fn rgba(&self) -> &[u8] {
        self.0.pixels.as_raw()
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Embedded {
    png_base64: String,
}
impl Serialize for Artwork {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        Embedded {
            png_base64: self.0.encoded.clone(),
        }
        .serialize(s)
    }
}
impl<'de> Deserialize<'de> for Artwork {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let embedded = Embedded::deserialize(d)?;
        if embedded.png_base64.len() > 2 * 1024 * 1024 {
            return Err(serde::de::Error::custom(
                "Embedded badge and corner PNG exceeds size limit",
            ));
        }
        let bytes = STANDARD
            .decode(embedded.png_base64)
            .map_err(serde::de::Error::custom)?;
        let pixels = crate::icon_edit::decode_image(&bytes, ImageFormat::Png, WIDTH)
            .map_err(serde::de::Error::custom)?;
        if pixels.dimensions() != (WIDTH, HEIGHT) {
            return Err(serde::de::Error::custom(format!(
                "Embedded badge artwork and release watermarks must be {WIDTH}×{HEIGHT}"
            )));
        }
        Self::normalized(pixels).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_artwork_reports_the_required_normalized_dimensions() {
        let mut png = Cursor::new(Vec::new());
        RgbaImage::new(96, 96)
            .write_to(&mut png, ImageFormat::Png)
            .unwrap();
        let embedded = serde_json::json!({ "png_base64": STANDARD.encode(png.into_inner()) });
        let error = serde_json::from_value::<Artwork>(embedded)
            .unwrap_err()
            .to_string();
        assert!(error.contains("512\u{00d7}512"), "{error}");
        assert!(!error.contains("96\u{00d7}96"), "{error}");
    }
}

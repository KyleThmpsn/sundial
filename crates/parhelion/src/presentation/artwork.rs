use super::composition::Composition;
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
const SOURCE_EDGE: u32 = 1024;
#[derive(Clone, Eq, PartialEq)]
pub struct Artwork {
    source: Arc<Data>,
    composition: Option<Composition>,
}
#[derive(Eq, PartialEq)]
struct Data {
    pixels: RgbaImage,
    encoded: String,
}
impl fmt::Debug for Artwork {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Artwork")
            .field("size", &self.source.pixels.dimensions())
            .field("composition", &self.composition)
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
        if bytes.len() > MAX_BYTES {
            return Err("Choose an image no larger than 16 MiB.".into());
        }
        let format = image::guess_format(&bytes).map_err(|_| "Choose a PNG or JPEG image.")?;
        if !matches!(format, ImageFormat::Png | ImageFormat::Jpeg) {
            return Err("Choose a PNG or JPEG image.".into());
        }
        let source = crate::icon_edit::decode_image(&bytes, format, 4096)?;
        Self::from_source(source)
    }
    pub(crate) fn from_source(source: RgbaImage) -> Result<Self, String> {
        if source.width() == 0 || source.height() == 0 {
            return Err("Image dimensions must be nonzero.".into());
        }
        let edge = source.width().max(source.height());
        let pixels = if edge > SOURCE_EDGE {
            crate::icon_edit::fit_rgba_image(
                &source,
                (source.width() * SOURCE_EDGE / edge).max(1),
                (source.height() * SOURCE_EDGE / edge).max(1),
            )
        } else {
            source
        };
        let mut artwork = Self::normalized(pixels)?;
        artwork.composition = Some(Composition::default());
        Ok(artwork)
    }
    fn normalized(pixels: RgbaImage) -> Result<Self, String> {
        let mut output = Cursor::new(vec![]);
        pixels
            .write_to(&mut output, ImageFormat::Png)
            .map_err(|e| e.to_string())?;
        Ok(Self {
            source: Arc::new(Data {
                pixels,
                encoded: STANDARD.encode(output.into_inner()),
            }),
            composition: None,
        })
    }
    pub(crate) fn pixels(&self) -> &RgbaImage {
        &self.source.pixels
    }
    pub(crate) fn composition(&self) -> Option<&Composition> {
        self.composition.as_ref()
    }
    pub(crate) fn with_composition(&self, composition: Composition) -> Result<Self, String> {
        composition.validate()?;
        Ok(Self {
            source: self.source.clone(),
            composition: Some(composition),
        })
    }
    pub(crate) fn render(&self, width: u32, height: u32) -> RgbaImage {
        self.composition.as_ref().map_or_else(
            || crate::icon_edit::fit_rgba_image(self.pixels(), width, height),
            |composition| composition.render(self.pixels(), width, height),
        )
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Embedded {
    png_base64: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    composition: Option<Composition>,
}
impl Serialize for Artwork {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        Embedded {
            png_base64: self.source.encoded.clone(),
            composition: self.composition.clone(),
        }
        .serialize(s)
    }
}
impl<'de> Deserialize<'de> for Artwork {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let embedded = Embedded::deserialize(d)?;
        let edited = embedded.composition.is_some();
        if embedded.png_base64.len()
            > if edited {
                6 * 1024 * 1024
            } else {
                2 * 1024 * 1024
            }
        {
            return Err(serde::de::Error::custom(
                "Embedded badge and corner PNG exceeds size limit",
            ));
        }
        let bytes = STANDARD
            .decode(embedded.png_base64)
            .map_err(serde::de::Error::custom)?;
        let pixels = crate::icon_edit::decode_image(
            &bytes,
            ImageFormat::Png,
            if edited { SOURCE_EDGE } else { WIDTH },
        )
        .map_err(serde::de::Error::custom)?;
        if !edited && pixels.dimensions() != (WIDTH, HEIGHT) {
            return Err(serde::de::Error::custom(format!(
                "Embedded badge artwork and release watermarks must be {WIDTH}×{HEIGHT}"
            )));
        }
        if let Some(composition) = &embedded.composition {
            composition.validate().map_err(serde::de::Error::custom)?;
        }
        let mut artwork = Self::normalized(pixels).map_err(serde::de::Error::custom)?;
        artwork.composition = embedded.composition;
        Ok(artwork)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_compositions_retain_source_and_reject_invalid_crops() {
        let source = RgbaImage::from_fn(120, 60, |x, y| image::Rgba([x as u8, y as u8, 40, 255]));
        let artwork = Artwork::from_source(source.clone())
            .unwrap()
            .with_composition(Composition {
                crop: [2500, 1000, 5000, 8000],
                scale: 175,
                offset: [-10, 20],
                rotation: 1,
                background: super::super::composition::Background::Gradient {
                    start: [0, 10, 20],
                    end: [40, 80, 120],
                    angle: 45,
                },
                ..Default::default()
            })
            .unwrap();
        let saved = serde_json::to_string(&artwork).unwrap();
        let reopened: Artwork = serde_json::from_str(&saved).unwrap();
        assert_eq!(artwork, reopened);
        assert_eq!(
            *reopened.pixels(),
            source,
            "edits must not bake over the original source"
        );
        assert_eq!(artwork.render(440, 268), reopened.render(440, 268));
        for crop in [
            [0, 0, 0, 10000],
            [9000, 0, 2000, 10000],
            [0, 9999, 10000, 2],
        ] {
            let mut invalid: serde_json::Value = serde_json::from_str(&saved).unwrap();
            invalid["composition"]["crop"] = serde_json::json!(crop);
            assert!(serde_json::from_value::<Artwork>(invalid).is_err());
        }
    }

    #[test]
    fn legacy_artwork_keeps_its_serialized_shape_and_pixels() {
        let artwork = Artwork::from_png(include_bytes!(
            "../../../../assets/parhelion/watermark/sunrise-watermark-2-45x45.png"
        ))
        .unwrap();
        let saved = serde_json::to_string(&artwork).unwrap();
        assert!(!saved.contains("composition"));
        assert_eq!(serde_json::from_str::<Artwork>(&saved).unwrap(), artwork);
        assert_eq!(artwork.pixels().dimensions(), (512, 512));
    }

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

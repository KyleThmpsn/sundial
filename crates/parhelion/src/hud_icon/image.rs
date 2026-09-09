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
pub(crate) const WIDTH: u32 = 137;
pub(crate) const HEIGHT: u32 = 76;
#[derive(Clone, Eq, PartialEq)]
pub struct HudImage(Arc<Data>);
#[derive(Eq, PartialEq)]
struct Data {
    pixels: RgbaImage,
    encoded: String,
}
impl fmt::Debug for HudImage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HudImage")
            .field("size", &(WIDTH, HEIGHT))
            .finish_non_exhaustive()
    }
}
impl HudImage {
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
    pub(crate) fn rgba(&self) -> &[u8] {
        self.0.pixels.as_raw()
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Embedded {
    png_base64: String,
}
impl Serialize for HudImage {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        Embedded {
            png_base64: self.0.encoded.clone(),
        }
        .serialize(s)
    }
}
impl<'de> Deserialize<'de> for HudImage {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let embedded = Embedded::deserialize(d)?;
        if embedded.png_base64.len() > 128 * 1024 {
            return Err(serde::de::Error::custom(
                "Embedded HUD PNG exceeds size limit",
            ));
        }
        let bytes = STANDARD
            .decode(embedded.png_base64)
            .map_err(serde::de::Error::custom)?;
        let pixels = crate::icon_edit::decode_image(&bytes, ImageFormat::Png, WIDTH)
            .map_err(serde::de::Error::custom)?;
        if pixels.dimensions() != (WIDTH, HEIGHT) {
            return Err(serde::de::Error::custom(
                "Embedded HUD icons must be 137×76",
            ));
        }
        Self::normalized(pixels).map_err(serde::de::Error::custom)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_native_pixels_and_recipe_roundtrip() {
        let pixels = RgbaImage::from_fn(WIDTH, HEIGHT, |x, y| {
            image::Rgba([x as u8, y as u8, 210, (x + y) as u8])
        });
        let image = HudImage::normalized(pixels).unwrap();
        let bytes = STANDARD.decode(&image.0.encoded).unwrap();
        assert_eq!(HudImage::from_png(&bytes).unwrap(), image);
        let value = serde_json::to_string(&image).unwrap();
        assert_eq!(serde_json::from_str::<HudImage>(&value).unwrap(), image);
        let mut recipe = crate::WeaponRecipe::every_end();
        recipe.overrides.hud_icon = Some(image.clone());
        let decoded: crate::WeaponRecipe =
            serde_json::from_str(&serde_json::to_string(&recipe).unwrap()).unwrap();
        assert_eq!(decoded.to_spec().unwrap().overrides.hud_icon, Some(image));
        assert!(HudImage::from_png(b"invalid png").is_err());
    }
}

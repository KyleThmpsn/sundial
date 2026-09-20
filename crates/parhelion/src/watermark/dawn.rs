//! Dawn's purpose-made native-size lanes, with the same output scale as Sunrise.
#[cfg(test)]
mod tests;
use super::*;

const TEXTURES: [&[u8]; 6] = [
    include_bytes!("../../../../assets/parhelion/watermark/dawn-watermark-0-96x96.png"),
    include_bytes!("../../../../assets/parhelion/watermark/dawn-watermark-1-54x54.png"),
    include_bytes!("../../../../assets/parhelion/watermark/dawn-watermark-2-45x45.png"),
    include_bytes!("../../../../assets/parhelion/watermark/dawn-watermark-3-45x45.png"),
    include_bytes!("../../../../assets/parhelion/watermark/dawn-watermark-4-96x96.png"),
    include_bytes!("../../../../assets/parhelion/watermark/dawn-watermark-5-54x54.png"),
];

pub(crate) fn render(index: usize) -> AuthoringResult<image::RgbaImage> {
    let png = TEXTURES
        .get(index)
        .ok_or_else(|| invalid("Unknown Dawn watermark texture"))?;
    let image = image::load_from_memory_with_format(png, ImageFormat::Png)
        .map_err(|error| invalid(format!("Could not decode Dawn watermark {index}: {error}")))?
        .into_rgba8();
    let (width, height) = TEXTURE_DIMENSIONS[index];
    if image.dimensions() != (width, height) {
        return Err(invalid(format!(
            "Dawn watermark {index} must be {width}x{height}"
        )));
    }
    upscale_texture(width, height, image.into_raw())
}

//! Source HUD silhouettes as native ammunition HUD artwork.
use anyhow::Result;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use image::RgbaImage;
use serde_json::{Value, json};
use std::io::Cursor;

/// The native ammunition HUD artwork size a recipe's `hud_icon` must have.
pub const SIZE: [u32; 2] = [137, 76];

/// `glyph` uniformly fitted and centered on a transparent canvas of [`SIZE`], as a PNG.
pub fn fit(glyph: &RgbaImage) -> Result<Vec<u8>> {
    let [canvas_width, canvas_height] = SIZE;
    let scale = (f64::from(canvas_width) / f64::from(glyph.width()))
        .min(f64::from(canvas_height) / f64::from(glyph.height()));
    let width = (f64::from(glyph.width()) * scale).round().max(1.0) as u32;
    let height = (f64::from(glyph.height()) * scale).round().max(1.0) as u32;
    let resized =
        image::imageops::resize(glyph, width, height, image::imageops::FilterType::Lanczos3);
    let mut canvas = RgbaImage::new(canvas_width, canvas_height);
    image::imageops::replace(
        &mut canvas,
        &resized,
        i64::from((canvas_width - width) / 2),
        i64::from((canvas_height - height) / 2),
    );
    let mut png = Cursor::new(Vec::new());
    canvas.write_to(&mut png, image::ImageFormat::Png)?;
    Ok(png.into_inner())
}

/// Sets `recipe`'s HUD artwork to fitted PNG `png` unless it already names its own.
pub fn apply(recipe: &mut Value, png: &[u8]) {
    if recipe["overrides"]["hud_icon"].is_object() {
        return;
    }
    if recipe["overrides"].is_null() {
        recipe["overrides"] = json!({});
    }
    recipe["overrides"]["hud_icon"] = json!({"png_base64":STANDARD.encode(png)});
}

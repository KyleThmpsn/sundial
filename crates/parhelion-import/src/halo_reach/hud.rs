//! Source-labelled HUD silhouettes from the bounded MCC icon font format.
use super::{Scene, cache::Cache, resource::Pages};
use anyhow::{Context, Result, ensure};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::{fs, io::Cursor, path::Path};
mod font;
pub(super) mod scope;

pub(super) fn prepare(
    cache: &Cache,
    pages: &mut Pages,
    scene: &Scene,
    recipe: &mut Value,
    source: &Path,
) -> Result<Value> {
    if recipe["overrides"]["hud_icon"].is_object() {
        return Ok(json!({"status":"Explicit HUD artwork retained"}));
    }
    let prefix = match scene.object.tag.group.as_str() {
        "weap" => "weapon",
        "vehi" => "vehicle",
        _ => return Ok(Value::Null),
    };
    let stem = scene
        .object
        .tag
        .path
        .rsplit('\\')
        .next()
        .context("Source object name")?;
    let label = format!("{prefix}_{stem}");
    let Some(owner) = cache
        .tags
        .iter()
        .flatten()
        .find(|tag| tag.group == "unic" && tag.path == r"ui\cui\strings\scoreboard_strings")
    else {
        return Ok(
            json!({"status":"source_only", "reason":"Source cache has no scoreboard icon labels"}),
        );
    };
    let labels = cache.english(owner)?;
    let Some(text) = labels.get(&label) else {
        return Ok(
            json!({"status":"source_only", "label":label, "reason":"No matching source silhouette label"}),
        );
    };
    let mut characters = text.chars();
    let code = characters.next().context("Empty source silhouette label")? as u32;
    ensure!(
        characters.next().is_none() && (0xe000..=0xf8ff).contains(&code),
        "Source silhouette is not one private-use glyph"
    );
    let path = cache
        .path
        .parent()
        .context("Source maps directory")?
        .join("fonts/font_package_icon_x3.bin");
    pages.record(&path)?;
    let (glyph, offset) = font::read(&fs::read(&path)?, code as u16)?;
    let scale = (137.0 / f64::from(glyph.width())).min(76.0 / f64::from(glyph.height()));
    let width = (f64::from(glyph.width()) * scale).round().max(1.0) as u32;
    let height = (f64::from(glyph.height()) * scale).round().max(1.0) as u32;
    let resized =
        image::imageops::resize(&glyph, width, height, image::imageops::FilterType::Lanczos3);
    let mut canvas = image::RgbaImage::new(137, 76);
    image::imageops::replace(
        &mut canvas,
        &resized,
        i64::from((137 - width) / 2),
        i64::from((76 - height) / 2),
    );
    let mut png = Cursor::new(Vec::new());
    canvas.write_to(&mut png, image::ImageFormat::Png)?;
    fs::write(source.join("hud.png"), png.get_ref())?;
    glyph.save(source.join("hud-source.png"))?;
    if recipe["overrides"].is_null() {
        recipe["overrides"] = json!({});
    }
    recipe["overrides"]["hud_icon"] = json!({"png_base64":STANDARD.encode(png.into_inner())});
    Ok(
        json!({"status":"translated", "label":label, "owner":owner, "glyph":format!("{code:04X}"),
        "font":"icon_x3\\fixedsys_hud-15", "font_file":path, "record_offset":offset,
        "source_dimensions":[glyph.width(),glyph.height()], "dimensions":[137,76],
        "method":"Source glyph uniformly fitted and centered on a transparent canvas"}),
    )
}

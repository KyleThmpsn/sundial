//! Source-labelled HUD silhouettes from the bounded MCC icon font format.
use super::{Scene, cache::Cache, resource::Pages};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{fs, path::Path};
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
    let png = crate::hud_icon::fit(&glyph)?;
    fs::write(source.join("hud.png"), &png)?;
    glyph.save(source.join("hud-source.png"))?;
    crate::hud_icon::apply(recipe, &png);
    Ok(
        json!({"status":"translated", "label":label, "owner":owner, "glyph":format!("{code:04X}"),
        "font":"icon_x3\\fixedsys_hud-15", "font_file":path, "record_offset":offset,
        "source_dimensions":[glyph.width(),glyph.height()], "dimensions":crate::hud_icon::SIZE,
        "method":"Source glyph uniformly fitted and centered on a transparent canvas"}),
    )
}

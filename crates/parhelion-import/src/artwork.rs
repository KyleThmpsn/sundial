//! Inventory artwork for imports whose source has no Destiny icon.
use crate::GraphReference;
use anyhow::{Context, Result};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{fs, io::Cursor, path::Path};
use sundial::ui::model_preview::icon::{self, Frame, Pose};

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub pose: Pose,
    pub frame: Frame,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            pose: Pose {
                yaw: 2.5,
                pitch: 0.25,
                roll: 0.35,
            },
            frame: Frame {
                zoom: 1.55,
                offset: [0.0, 0.0],
            },
        }
    }
}

impl Settings {
    pub fn sword() -> Self {
        Self {
            pose: Pose {
                yaw: 0.35,
                pitch: 0.2,
                roll: -0.6,
            },
            frame: Frame {
                zoom: 1.5,
                offset: [0.11, -0.075],
            },
        }
    }

    pub fn for_recipe(recipe: &Value) -> Self {
        if recipe["type_name"]
            .as_str()
            .is_some_and(|name| name.to_ascii_lowercase().contains("sword"))
        {
            Self::sword()
        } else {
            Self::default()
        }
    }
}

/// Render a checked local weapon graph without changing any of its payloads.
pub fn render(reference: &GraphReference, packages: &Path, settings: Settings) -> Result<Vec<u8>> {
    let appearance = crate::preview::appearance(reference, "weapon");
    let art = icon::local(packages, &appearance, settings.pose, settings.frame, 768)
        .map_err(anyhow::Error::msg)?;
    let source = image::RgbaImage::from_raw(art.size as u32, art.size as u32, art.rgba)
        .context("Invalid rendered inventory artwork")?;
    let image = image::imageops::resize(&source, 96, 96, image::imageops::FilterType::Lanczos3);
    let mut png = Cursor::new(Vec::new());
    image.write_to(&mut png, image::ImageFormat::Png)?;
    Ok(png.into_inner())
}

/// Add artwork to a freshly converted graph before it is published or pinned by a recipe.
/// A source-provided Destiny icon always wins. Errors abort the caller's conversion.
pub(crate) fn prepare(directory: &Path, packages: &Path, settings: Settings) -> Result<()> {
    let path = directory.join("asset-graph.json");
    let mut graph: Value = serde_json::from_slice(&fs::read(&path)?)?;
    if graph["source_icon_png"].is_string() {
        return Ok(());
    }
    let item = crate::graph::hash(&graph, "item_hash")?;
    let reference = GraphReference::new(directory, item)?;
    let png = render(&reference, packages, settings).context("Generating imported weapon icon")?;
    reference.validate(item)?;
    let file = "generated-icon.png";
    fs::write(directory.join(file), png)?;
    graph["source_icon_png"] = json!(file);
    graph["generated_icon"] = json!({"revision":1,"settings":settings,"size":96});
    crate::io::write_json(&path, &graph)
}

/// Copy available source artwork into the recipe while preserving custom art and explicit icon donors.
pub fn apply(recipe: &mut Value, directory: &Path) -> Result<()> {
    anyhow::ensure!(recipe.is_object(), "Imported recipe must be an object");
    if !recipe["icon_donor"].is_null()
        || !recipe["overrides"]["icon_edit"]["imported_image"].is_null()
    {
        return Ok(());
    }
    let graph: Value = serde_json::from_slice(&fs::read(directory.join("asset-graph.json"))?)?;
    if graph["source_icon_png"].is_null() {
        return Ok(());
    }
    let root = directory.canonicalize()?;
    let file = graph["source_icon_png"]
        .as_str()
        .context("Generated icon path missing")?;
    let path = root.join(file).canonicalize()?;
    anyhow::ensure!(
        path.starts_with(&root),
        "Generated icon escapes its graph folder"
    );
    let reference = GraphReference::new(&root, crate::graph::hash(&graph, "item_hash")?)?;
    let bytes = fs::read(path)?;
    reference.validate(crate::graph::hash(&graph, "item_hash")?)?;
    if recipe["overrides"].is_null() {
        recipe["overrides"] = json!({});
    }
    anyhow::ensure!(
        recipe["overrides"].is_object(),
        "Recipe overrides must be an object"
    );
    if recipe["overrides"]["icon_edit"].is_null() {
        recipe["overrides"]["icon_edit"] = json!({});
    }
    anyhow::ensure!(
        recipe["overrides"]["icon_edit"].is_object(),
        "Icon edit must be an object"
    );
    recipe["overrides"]["icon_edit"]["imported_image"] =
        json!({"png_base64":STANDARD.encode(bytes)});
    Ok(())
}

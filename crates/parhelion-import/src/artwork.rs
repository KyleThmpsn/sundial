//! Inventory artwork for imports whose source has no Destiny icon.
use crate::GraphReference;
use anyhow::{Context, Result};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{fs, io::Cursor, path::Path};
use sundial::ui::model_preview::icon::{self, Frame, Lighting, Pose};

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub pose: Pose,
    pub frame: Frame,
    pub lighting: Lighting,
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
            lighting: Lighting::default(),
        }
    }
}

impl Settings {
    /// Marathon assemblies point along positive X. Kinetic imports face upper
    /// left, while Energy and Power face upper right. Render each camera pose
    /// directly so lettering and asymmetric details are not mirrored.
    pub fn marathon(kinetic: bool) -> Self {
        Self {
            pose: Pose {
                yaw: if kinetic {
                    std::f32::consts::PI - 0.65
                } else {
                    0.65
                },
                pitch: 0.25,
                roll: if kinetic { 0.35 } else { -0.35 },
            },
            frame: Frame {
                zoom: 1.5,
                offset: [0.0, 0.0],
            },
            lighting: Lighting {
                key: 1.4,
                fill: 0.08,
                exposure: 0.4,
                filmic: true,
                normalize_exposure: false,
            },
        }
    }

    pub fn for_graph(graph: &Value, recipe: &Value) -> Self {
        if graph["source"] == "marathon" {
            Self::marathon(graph["inventory_slot"] == "kinetic")
        } else {
            Self::for_recipe(recipe)
        }
    }

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
            lighting: Lighting::default(),
        }
    }

    pub fn for_recipe(recipe: &Value) -> Self {
        if recipe["kind"] == "sparrow" {
            Self {
                pose: Pose {
                    yaw: 3.8,
                    pitch: 0.65,
                    roll: -0.1,
                },
                // Keep wings, wheels and other extremities within the complete-model fit.
                frame: Frame::default(),
                lighting: Lighting::default(),
            }
        } else if recipe["type_name"]
            .as_str()
            .is_some_and(|name| name.to_ascii_lowercase().contains("sword"))
        {
            Self::sword()
        } else {
            Self::default()
        }
    }
}

/// Render a checked local appearance graph without changing any of its payloads.
pub fn render(reference: &GraphReference, packages: &Path, settings: Settings) -> Result<Vec<u8>> {
    Ok(render_with_notices(reference, packages, settings)?.0)
}

/// The rendered artwork and the preview's notices: what it could not load or evaluate as the
/// game would, which a generated icon then misrepresents.
pub fn render_with_notices(
    reference: &GraphReference,
    packages: &Path,
    settings: Settings,
) -> Result<(Vec<u8>, Vec<String>)> {
    let graph: Value =
        serde_json::from_slice(&fs::read(reference.directory.join("asset-graph.json"))?)?;
    let appearance =
        crate::preview::appearance(reference, graph["kind"].as_str().unwrap_or("weapon"));
    let render = if matches!(graph["source"].as_str(), Some("marathon" | "halo_reach")) {
        icon::local_programs
    } else {
        icon::local_with_lighting
    };
    let art = render(
        packages,
        &appearance,
        settings.pose,
        settings.frame,
        settings.lighting,
        768,
    )
    .map_err(anyhow::Error::msg)?;
    let source = image::RgbaImage::from_raw(art.size as u32, art.size as u32, art.rgba)
        .context("Invalid rendered inventory artwork")?;
    let image = image::imageops::resize(&source, 96, 96, image::imageops::FilterType::Lanczos3);
    let mut png = Cursor::new(Vec::new());
    image.write_to(&mut png, image::ImageFormat::Png)?;
    Ok((png.into_inner(), art.notices))
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
    let png =
        render(&reference, packages, settings).context("Generating imported inventory icon")?;
    reference.validate(item)?;
    let file = "generated-icon.png";
    fs::write(directory.join(file), png)?;
    graph["source_icon_png"] = json!(file);
    // Record the renderer version so cached artwork can be refreshed after rendering changes.
    graph["generated_icon"] = json!({"revision":8,"settings":settings,"size":96});
    crate::io::write_json(&path, &graph)
}

/// Recognize only artwork that still matches the pinned graph's generated pixels.
/// Custom images and source artwork without renderer provenance remain authored selections.
pub fn generated_image(reference: &GraphReference, image: &Value) -> Result<bool> {
    let Some(encoded) = image["png_base64"].as_str() else {
        return Ok(false);
    };
    let graph: Value =
        serde_json::from_slice(&fs::read(reference.directory.join("asset-graph.json"))?)?;
    if !graph["generated_icon"].is_object() {
        return Ok(false);
    }
    let Some(file) = graph["source_icon_png"].as_str() else {
        return Ok(false);
    };
    let root = reference.directory.canonicalize()?;
    let path = root.join(file).canonicalize()?;
    anyhow::ensure!(
        path.starts_with(&root),
        "Generated icon escapes its graph folder"
    );
    let saved = image::load_from_memory(&STANDARD.decode(encoded)?)?.to_rgba8();
    let generated = image::load_from_memory(&fs::read(path)?)?.to_rgba8();
    Ok(saved == generated)
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

//! Adopt a converter's generated artwork at the normal recipe persistence boundary.
use crate::{WeaponRecipe, icon_edit::ImportedIcon};
use serde_json::Value;
use std::fs;

pub(crate) fn available(recipe: &WeaponRecipe) -> Result<Option<ImportedIcon>, String> {
    if !recipe.kind.is_weapon()
        || recipe.icon_donor.is_some()
        || recipe.overrides.icon_edit.imported_image.is_some()
    {
        return Ok(None);
    }
    let Some(reference) = &recipe.overrides.imported_graph else {
        return Ok(None);
    };
    let bytes = match fs::read(reference.directory.join("asset-graph.json")) {
        Ok(bytes) => bytes,
        // Older recipes can still be opened to repair a missing asset path.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let graph: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if graph["source_icon_png"].is_null() {
        return Ok(None);
    }
    let item = recipe
        .identity
        .item_hash
        .parse_u32()
        .map_err(|e| e.to_string())?;
    reference.validate(item).map_err(|e| format!("{e:#}"))?;
    let root = reference
        .directory
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let path = root
        .join(
            graph["source_icon_png"]
                .as_str()
                .ok_or("Generated icon path missing")?,
        )
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if !path.starts_with(&root) {
        return Err("Generated icon escapes its graph folder".into());
    }
    let icon = ImportedIcon::from_bytes(&fs::read(path).map_err(|e| e.to_string())?)?;
    reference.validate(item).map_err(|e| format!("{e:#}"))?;
    Ok(Some(icon))
}

pub(crate) fn apply(recipe: &mut WeaponRecipe) -> Result<(), String> {
    if let Some(icon) = available(recipe)? {
        recipe.overrides.icon_edit.imported_image = Some(icon);
    }
    Ok(())
}

/// Prepare the appearance's artwork without mutating the editor's recipe during background work.
pub(crate) fn for_model(
    baseline: &WeaponRecipe,
    source: &WeaponRecipe,
    graph: &parhelion_import::GraphReference,
    packages: &std::path::Path,
) -> Result<Option<ImportedIcon>, String> {
    if baseline.icon_donor.is_some() || baseline.overrides.icon_edit.imported_image.is_some() {
        return Ok(baseline.overrides.icon_edit.imported_image.clone());
    }
    if let Some(icon) = &source.overrides.icon_edit.imported_image {
        return Ok(Some(icon.clone()));
    }
    let mut next = baseline.clone();
    next.overrides.imported_graph = Some(graph.clone());
    if let Some(icon) = available(&next)? {
        return Ok(Some(icon));
    }
    let settings = parhelion_import::artwork::Settings::for_recipe(
        &serde_json::to_value(&next).map_err(|e| e.to_string())?,
    );
    let png = parhelion_import::artwork::render(graph, packages, settings)
        .map_err(|e| format!("{e:#}"))?;
    ImportedIcon::from_bytes(&png).map(Some)
}

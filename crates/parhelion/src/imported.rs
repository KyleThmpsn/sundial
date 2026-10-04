//! Reusable source assets shared by gear editing, portable recipes and native emission.
use crate::ItemKind;
pub(crate) mod archive;

pub(crate) fn source_icon(
    recipe: &crate::WeaponRecipe,
) -> Result<crate::icon_edit::ImportedIcon, String> {
    let graph = recipe
        .overrides
        .imported_graph
        .as_ref()
        .ok_or("No imported source selected")?;
    let family = kind(recipe.kind).ok_or("Unsupported source item kind")?;
    graph
        .validate_reusable(family)
        .map_err(|e| format!("{e:#}"))?;
    let document: serde_json::Value = serde_json::from_slice(
        &std::fs::read(graph.directory.join("asset-graph.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let path = match document["source_icon_png"].as_str() {
        Some(file) => {
            let root = graph.directory.canonicalize().map_err(|e| e.to_string())?;
            let path = root.join(file).canonicalize().map_err(|e| e.to_string())?;
            if !path.starts_with(&root) {
                return Err("Source icon escapes its asset folder".into());
            }
            path
        }
        None if recipe.kind == ItemKind::Shader => graph
            .directory
            .parent()
            .ok_or("Source directory missing")?
            .join("source/item-icon.png"),
        None => return Err("Source icon missing".into()),
    };
    crate::icon_edit::ImportedIcon::from_bytes(&std::fs::read(path).map_err(|e| e.to_string())?)
}

pub(crate) const fn kind(kind: ItemKind) -> Option<&'static str> {
    match kind {
        ItemKind::Shader => Some("shader"),
        ItemKind::Armor => Some("armor"),
        ItemKind::GhostShell => Some("ghost_shell"),
        ItemKind::Ship => Some("ship"),
        ItemKind::Sparrow => Some("sparrow"),
        _ => None,
    }
}

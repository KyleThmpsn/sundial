//! Family metadata follows the appearance, independently of its gameplay template.
use super::Group;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Origin {
    Native(u32),
    #[cfg(feature = "d2-model-importer")]
    Imported(Option<Group>),
}

impl Origin {
    #[cfg(feature = "d2-model-importer")]
    pub(crate) fn document(document: &serde_json::Value) -> Option<Self> {
        if document.get("kind")?.as_str()? != "armor" {
            return None;
        }
        let reference = document.pointer("/overrides/imported_graph")?;
        if reference.is_null() {
            return None;
        }
        let group = if let Some(encoded) = reference.pointer("/embedded_assets/graph") {
            encoded.as_str().and_then(|encoded| {
                let graph: serde_json::Value = serde_json::from_str(encoded).ok()?;
                graph_group(&graph).ok().flatten()
            })
        } else {
            serde_json::from_value(reference.clone())
                .ok()
                .and_then(|reference| imported(&reference).ok().flatten())
        };
        Some(Self::Imported(group))
    }

    pub(crate) fn recipe(recipe: &crate::WeaponRecipe) -> Option<Self> {
        if recipe.kind != crate::ItemKind::Armor {
            return None;
        }
        #[cfg(feature = "d2-model-importer")]
        if let Some(reference) = &recipe.overrides.imported_graph {
            return Some(Self::Imported(imported(reference).ok().flatten()));
        }
        let donor = recipe.presentation_donor.as_ref().unwrap_or(&recipe.donor);
        donor.item_hash.parse_u32().ok().map(Self::Native)
    }

    pub(crate) fn group(
        &self,
        catalog: Option<&sundial::investment::InvestmentCatalog>,
    ) -> Option<Group> {
        match self {
            Self::Native(item) => catalog?.item_armor_set(*item).map(Group::Native),
            #[cfg(feature = "d2-model-importer")]
            Self::Imported(group) => group.clone(),
        }
    }
}

#[cfg(feature = "d2-model-importer")]
pub(crate) fn imported(
    reference: &parhelion_import::GraphReference,
) -> Result<Option<Group>, String> {
    use std::sync::{Mutex, OnceLock};
    static GROUPS: OnceLock<Mutex<std::collections::BTreeMap<String, Option<Group>>>> =
        OnceLock::new();
    let cache = GROUPS.get_or_init(Mutex::default);
    if let Some(group) = cache
        .lock()
        .map_err(|e| e.to_string())?
        .get(&reference.sha256)
    {
        return Ok(group.clone());
    }
    // This reads only the manifest. Payload validation remains in the build.
    let path = reference.directory.join("asset-graph.json");
    let graph: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?,
    )
    .map_err(|e| format!("{}: {e}", path.display()))?;
    let group = graph_group(&graph)?;
    cache
        .lock()
        .map_err(|e| e.to_string())?
        .insert(reference.sha256.clone(), group.clone());
    Ok(group)
}

#[cfg(feature = "d2-model-importer")]
fn graph_group(graph: &serde_json::Value) -> Result<Option<Group>, String> {
    match graph.get("collection_set") {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(key)) if !key.is_empty() => {
            Ok(Some(Group::Imported(key.clone())))
        }
        _ => Err("Imported armor set identity is invalid".into()),
    }
}

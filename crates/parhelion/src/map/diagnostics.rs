//! Source labels exported alongside a staged map generation. Native identities stay unchanged.
use std::path::Path;
#[cfg(feature = "d2-model-importer")]
use sundial::package_authoring::mod_content::map_diagnostics::Zone;
use sundial::package_authoring::mod_content::map_diagnostics::{self, Catalog, Map};

use super::MapRecipe;
use crate::artifact::{ArtifactMetadata, digest_file};

pub(crate) fn stage(
    directory: &Path,
    recipes: &[MapRecipe],
) -> Result<Option<ArtifactMetadata>, String> {
    let Some(catalog) = source(recipes)? else {
        return Ok(None);
    };
    let bytes = serde_json::to_vec_pretty(&catalog).map_err(|error| error.to_string())?;
    if bytes.len() as u64 > map_diagnostics::BYTE_LIMIT {
        return Err("Map diagnostics exceed their byte limit".into());
    }
    let path = directory.join(map_diagnostics::FILE_NAME);
    std::fs::write(&path, bytes).map_err(|error| error.to_string())?;
    let digest = digest_file(&path).map_err(|error| error.to_string())?;
    Ok(Some(ArtifactMetadata {
        file_name: map_diagnostics::FILE_NAME.into(),
        byte_length: digest.byte_length,
        sha256: digest.sha256,
    }))
}

/// Reads selected source labels without rebuilding geometry or changing native identities.
/// The staging workflow uses the same catalog. No source-backed maps produce no sidecar.
pub fn source(recipes: &[MapRecipe]) -> Result<Option<Catalog>, String> {
    let maps = collect(recipes)?;
    if maps.is_empty() {
        return Ok(None);
    }
    let catalog = Catalog { schema: 1, maps };
    catalog.validate()?;
    Ok(Some(catalog))
}

#[cfg(feature = "d2-model-importer")]
fn collect(recipes: &[MapRecipe]) -> Result<Vec<Map>, String> {
    let mut maps = Vec::new();
    for recipe in recipes {
        let Some(source) = &recipe.source else {
            continue;
        };
        let available = parhelion_import::halo_reach::level::zone_sets(&source.maps, &source.map)
            .map_err(|error| format!("Map {} source labels: {error:#}", recipe.name))?;
        let mut zones = Vec::new();
        let mut selected = std::collections::BTreeSet::new();
        for &index in &source.zone_sets {
            if !selected.insert(index) {
                continue;
            }
            let zone = available
                .get(index)
                .ok_or_else(|| format!("Map {} names missing zone set {index}", recipe.name))?;
            zones.push(Zone {
                index: u32::try_from(index).map_err(|error| error.to_string())?,
                name: zone.name.clone(),
                bsps: zone.bsps.clone(),
            });
        }
        maps.push(Map {
            activity: recipe.name.clone(),
            bubble_hash: sundial::package_authoring::fnv1_name_hash(&recipe.name),
            source_cache: Path::new(&source.map)
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| format!("Map {} source cache has no filename", recipe.name))?
                .into(),
            source_build: parhelion_import::halo_reach::BUILD.into(),
            zones,
        });
    }
    Ok(maps)
}

#[cfg(not(feature = "d2-model-importer"))]
fn collect(recipes: &[MapRecipe]) -> Result<Vec<Map>, String> {
    if recipes.iter().any(|recipe| recipe.source.is_some()) {
        return Err("Map source labels require the D2 model importer".into());
    }
    Ok(Vec::new())
}

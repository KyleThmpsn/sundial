//! Mission controllers are linked to the activity entries and staged with their package generation.
#[cfg(feature = "d2-model-importer")]
mod controller;

use super::MapRecipe;
use crate::artifact::{ArtifactMetadata, digest_bytes};
use std::path::Path;
use sundial::package_authoring::mod_content::mission_script;

#[cfg(feature = "d2-model-importer")]
pub(super) struct Squad {
    pub source: String,
    pub name: String,
    pub count: usize,
}

#[cfg(feature = "d2-model-importer")]
pub(super) struct Encounter {
    pub key: u32,
    pub template: tiger_pkg::TagHash,
    pub squads: Vec<Squad>,
}

#[cfg(feature = "d2-model-importer")]
pub(super) struct Triggers {
    pub key: u32,
    pub template: tiger_pkg::TagHash,
    pub names: Vec<String>,
}

#[cfg(feature = "d2-model-importer")]
pub(super) fn volumes(
    source: Option<&super::MapSource>,
) -> crate::AuthoringResult<Vec<parhelion_import::halo_reach::level::volumes::Volume>> {
    use crate::error::invalid;
    use parhelion_import::halo_reach::level::{mission, volumes};
    let Some(source) = source
        .filter(|source| !source.encounter_starts.is_empty() || !source.mission_scripts.is_empty())
    else {
        return Ok(Vec::new());
    };
    let program = mission::read(&source.maps.join(&source.map))
        .map_err(|error| invalid(format!("Reading mission trigger bindings: {error:#}")))?;
    let available = program
        .bindings
        .named
        .iter()
        .filter(|binding| binding.domain == "trigger_volume")
        .filter_map(|binding| binding.name.as_deref())
        .collect::<std::collections::BTreeSet<_>>();
    let names = source
        .encounter_gates
        .iter()
        .flat_map(|gate| &gate.volumes)
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    if names.iter().any(|name| !available.contains(name)) {
        return Err(invalid(
            "A mission gate has no source script volume binding",
        ));
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut result = Vec::new();
    for mut volume in volumes::triggers(&source.maps, &source.map)
        .map_err(|error| invalid(format!("Reading mission trigger geometry: {error:#}")))?
    {
        if !names.contains(volume.name.as_str()) {
            continue;
        }
        if !seen.insert(volume.name.clone()) {
            return Err(invalid("Duplicate source mission trigger name"));
        }
        for corner in &mut volume.corners {
            for (axis, coordinate) in corner.iter_mut().enumerate() {
                *coordinate += source.offset[axis] as f32;
            }
        }
        for height in &mut volume.z {
            *height += source.offset[2] as f32;
        }
        result.push(volume);
    }
    if seen.len() != names.len() {
        return Err(invalid(
            "A mission gate has no usable source volume geometry",
        ));
    }
    Ok(result)
}

#[cfg(feature = "d2-model-importer")]
pub(super) fn prepare(
    recipe: &MapRecipe,
    encounters: Vec<(usize, Encounter)>,
    triggers: Vec<(usize, Triggers)>,
    package: u16,
    tags: &[crate::NewTagSpec],
) -> crate::AuthoringResult<()> {
    let Some(source) = recipe
        .source
        .as_ref()
        .filter(|source| !source.encounter_starts.is_empty())
    else {
        return Ok(());
    };
    if !crate::package_profile::host().is_standalone() || !recipe.name.starts_with("mission_") {
        return Err(crate::error::invalid(
            "Encounter controllers require a managed mission activity",
        ));
    }
    if !source.mission_scripts.is_empty() {
        return Err(crate::error::invalid(
            "Select either a full script controller or encounter openings",
        ));
    }
    let lua = controller::build(recipe, source, encounters, triggers, package, tags)?;
    if lua.len() as u64 > mission_script::BYTE_LIMIT {
        return Err(crate::error::invalid(
            "Encounter controller exceeds the host source limit",
        ));
    }
    crate::mod_family::build::mission_script(&recipe.name, lua).map_err(crate::error::invalid)
}

pub(crate) fn stage(
    directory: &Path,
    recipes: &[MapRecipe],
) -> Result<Vec<ArtifactMetadata>, String> {
    let sources = crate::mod_family::build::mission_scripts();
    let mut artifacts = Vec::new();
    for (activity, source) in sources {
        if !recipes.iter().any(|recipe| recipe.name == activity) {
            return Err("Mission controller belongs to an unselected map".into());
        }
        let file_name = format!("{activity}.lua");
        if mission_script::path(&file_name).is_none()
            || source.len() as u64 > mission_script::BYTE_LIMIT
        {
            return Err("Invalid staged mission controller".into());
        }
        let digest = digest_bytes(source.as_bytes());
        std::fs::write(directory.join(&file_name), source).map_err(|error| error.to_string())?;
        artifacts.push(ArtifactMetadata {
            file_name,
            byte_length: digest.byte_length,
            sha256: digest.sha256,
        });
    }
    Ok(artifacts)
}

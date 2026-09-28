//! Explicit native component experiments carried by a prepared import graph.
use super::*;
use parhelion_import::GraphReference;
use serde::Deserialize;

mod callbacks;
mod input;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::weapon) struct Extension {
    source_entity: u32,
    owners: BTreeSet<u32>,
    select_bindings: BTreeSet<u32>,
    #[serde(default)]
    secondary_input_only: bool,
}

/// Apply input policy through the existing private owner allocation path.
pub(in crate::weapon) fn input_patches(
    manager: &PackageManager,
    extensions: &[Extension],
    entity: &[u8],
) -> AuthoringResult<Vec<WeaponRuntimeResourcePatch>> {
    let selected = extensions
        .iter()
        .filter(|extension| extension.secondary_input_only)
        .collect::<Vec<_>>();
    match selected.as_slice() {
        [] => Ok(Vec::new()),
        [extension] => Ok(vec![input::secondary_only(
            manager,
            entity,
            &extension.owners,
        )?]),
        _ => Err(invalid(
            "Multiple extensions request the same secondary input policy",
        )),
    }
}

/// Keep the composed ability from bypassing its separately filtered input.
pub(in crate::weapon) fn input_callbacks(
    manager: &PackageManager,
    extensions: &[Extension],
    entity: &mut [u8],
    allocator: AppendedTagAllocator,
    tags: &mut Vec<NewTagSpec>,
) -> AuthoringResult<()> {
    for extension in extensions
        .iter()
        .filter(|extension| extension.secondary_input_only)
    {
        callbacks::author(manager, extension, entity, allocator, tags)?;
    }
    Ok(())
}

/// No extension is inferred from an item hash or its display type. The prepared graph must
/// request it explicitly, and incompatible native wiring rejects the entire compilation.
pub(in crate::weapon) fn load(graph: &GraphReference) -> AuthoringResult<Vec<Extension>> {
    let value: serde_json::Value = serde_json::from_slice(
        &std::fs::read(graph.directory.join("asset-graph.json"))
            .map_err(|error| invalid(format!("Imported component graph: {error}")))?,
    )
    .map_err(|error| invalid(format!("Imported component graph: {error}")))?;
    let Some(value) = value.get("experimental_component_extensions") else {
        return Ok(Vec::new());
    };
    serde_json::from_value(value.clone())
        .map_err(|error| invalid(format!("Imported component extensions: {error}")))
}

pub(in crate::weapon) fn author(
    manager: &PackageManager,
    extensions: &[Extension],
    entity: &mut Vec<u8>,
) -> AuthoringResult<()> {
    if extensions.is_empty() {
        return Ok(());
    }
    let mut candidate = entity.clone();
    for extension in extensions {
        let source = read_tag(
            manager,
            TagHash(extension.source_entity),
            "component extension source",
        )?;
        let entry = manager
            .get_entry(TagHash(extension.source_entity))
            .ok_or_else(|| invalid("Component extension source is missing"))?;
        if entry.reference != WEAPON_ENTITY_CLASS {
            return Err(invalid("Component extension source is not a native entity"));
        }
        sundial::package_authoring::weapon_entity::extend_weapon_components(
            manager,
            &mut candidate,
            &source,
            &extension.owners,
            &extension.select_bindings,
        )
        .map_err(|error| invalid(format!("Experimental component composition: {error}")))?;
    }
    *entity = candidate;
    Ok(())
}

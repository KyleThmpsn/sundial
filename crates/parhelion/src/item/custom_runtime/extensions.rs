//! Explicit native component experiments carried by a prepared import graph.
use super::imports::Inputs;
use super::*;
use serde::Deserialize;

mod attachments;
mod callbacks;
mod input;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::item) struct Extension {
    source_entity: u32,
    owners: BTreeSet<u32>,
    select_bindings: BTreeSet<u32>,
    #[serde(default)]
    secondary_input_only: bool,
    /// Compose only part of the source entity: connections into owners left out, or into
    /// anything this weapon lacks, are emptied or dropped instead of refused.
    #[serde(default)]
    detach_unmatched: bool,
    /// Bindings that only the composed owners back, left out so nothing finds those owners
    /// through them. A composed melee ability without its ability binding stays inert.
    #[serde(default)]
    omit_bindings: BTreeSet<u32>,
    /// Entities a composed owner attaches, swapped for private copies without some components.
    #[serde(default)]
    private_attachments: Vec<attachments::AttachmentEdit>,
    /// With `secondary_input_only`, give the composed melee ability an input method that never
    /// starts it, so neither fire nor melee reaches it and the class melee keeps the button.
    #[serde(default)]
    inert_ability_input: bool,
}

/// Private copies of attached entities, as runtime resource patches with graph removals.
pub(in crate::item) fn attachment_patches(
    manager: &PackageManager,
    extensions: &[Extension],
    entity: &[u8],
) -> AuthoringResult<Vec<WeaponRuntimeResourcePatch>> {
    let mut patches = Vec::new();
    for extension in extensions {
        patches.extend(attachments::patches(
            manager,
            &extension.private_attachments,
            entity,
        )?);
    }
    Ok(patches)
}

/// Apply input policy through the existing private owner allocation path.
pub(in crate::item) fn input_patches(
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
pub(in crate::item) fn input_callbacks(
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
pub(in crate::item) fn load(graph: &Inputs) -> AuthoringResult<Vec<Extension>> {
    let value = graph.value();
    let Some(value) = value.get("experimental_component_extensions") else {
        return Ok(Vec::new());
    };
    serde_json::from_value(value.clone())
        .map_err(|error| invalid(format!("Imported component extensions: {error}")))
}

pub(in crate::item) fn author(
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
        let composed = if extension.detach_unmatched {
            sundial::package_authoring::entity::extend_weapon_components_detaching(
                manager,
                &mut candidate,
                &source,
                &extension.owners,
                &extension.select_bindings,
                &extension.omit_bindings,
            )
            .map(|_| ())
        } else {
            sundial::package_authoring::entity::extend_weapon_components(
                manager,
                &mut candidate,
                &source,
                &extension.owners,
                &extension.select_bindings,
                &extension.omit_bindings,
            )
        };
        composed
            .map_err(|error| invalid(format!("Experimental component composition: {error}")))?;
    }
    *entity = candidate;
    Ok(())
}

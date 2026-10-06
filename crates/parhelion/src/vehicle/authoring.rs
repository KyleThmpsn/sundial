//! Checked donor selection and private motion owner allocation.
use super::{Sparrow, motion};
use crate::{
    AuthoringResult, NewTagSpec, NewTagStorageMode, appended_tags::AppendedTagAllocator,
    error::invalid, tag_payload::read_u16,
};
use std::collections::{BTreeMap, BTreeSet};
use sundial::package_authoring::{
    PackageManager,
    entity::{
        retarget_weapon_component_owner, retarget_weapon_component_owner_payload,
        validate_weapon_entity,
    },
    runtime::{WeaponRuntimeGraph, load_weapon_runtime_graph_for_entity},
};
use tiger_pkg::TagHash;

const ENTITY: u32 = 0x8080_9C0F;
const HOVER_INSTANCE: u32 = 0x8080_3CDE;
const HOVER_DEFINITION: u32 = 0x8080_3171;

fn graph(
    manager: &PackageManager,
    tag: TagHash,
    payload: &[u8],
) -> AuthoringResult<WeaponRuntimeGraph> {
    let entry = manager
        .get_entry(tag)
        .ok_or_else(|| invalid(format!("Vehicle entity {tag} is missing")))?;
    if entry.reference != ENTITY || read_u16(payload, 0x96)? != 15 {
        return Err(invalid(format!(
            "Selected vehicle {tag} must be a rideable vehicle entity"
        )));
    }
    validate_weapon_entity(payload).map_err(invalid)?;
    let graph = load_weapon_runtime_graph_for_entity(manager, 0, 0, tag.0, payload)
        .map_err(|error| invalid(format!("Selected vehicle {tag}: {error}")))?;
    let schemas = graph
        .owners
        .iter()
        .flat_map(|o| o.roots.iter().map(|r| r.schema))
        .chain(graph.resources.iter().flat_map(|r| {
            std::iter::once(r.instance.schema).chain(r.definition.iter().map(|d| d.schema))
        }))
        .collect::<BTreeSet<_>>();
    if !schemas.contains(&0x8080_42AD) {
        return Err(invalid(format!(
            "Selected vehicle {tag} has no supported entry markers"
        )));
    }
    if !graph.resources.iter().any(|resource| {
        matches!(
            (
                resource.instance.schema,
                resource.definition.as_ref().map(|r| r.schema)
            ),
            (HOVER_INSTANCE, Some(HOVER_DEFINITION)) | (0x8080_3CAA, Some(0x8080_3CB2))
        )
    }) {
        return Err(invalid(format!(
            "Selected vehicle {tag} has no supported vehicle motion"
        )));
    }
    Ok(graph)
}

pub(crate) fn select(
    manager: &PackageManager,
    settings: Option<&Sparrow>,
    source_tag: TagHash,
    source_payload: Vec<u8>,
) -> AuthoringResult<(TagHash, Vec<u8>)> {
    let Some(tag) = settings
        .map(|s| s.summon.entity())
        .transpose()
        .map_err(invalid)?
        .flatten()
    else {
        return Ok((source_tag, source_payload));
    };
    let tag = TagHash(tag);
    // Check the class before decoding a payload as an entity.
    if manager
        .get_entry(tag)
        .is_none_or(|entry| entry.reference != ENTITY)
    {
        return Err(invalid(format!(
            "Selected vehicle {tag} is not a live vehicle entity"
        )));
    }
    let payload = manager
        .read_tag(tag)
        .map_err(|error| invalid(format!("Selected vehicle {tag}: {error}")))?;
    graph(manager, tag, &payload)?;
    Ok((tag, payload))
}

pub(crate) fn speed(
    manager: &PackageManager,
    settings: Option<&Sparrow>,
    source_tag: TagHash,
    entity: &mut Vec<u8>,
    allocator: &AppendedTagAllocator,
    tags: &mut Vec<NewTagSpec>,
) -> AuthoringResult<()> {
    let Some(settings) = settings.filter(|s| s.speed_percent != 100) else {
        return Ok(());
    };
    // Imported animation may already have private owners. Inspect the source graph for motion
    // roots and retarget only its unchanged motion owner in the composed graph.
    let original = manager
        .read_tag(source_tag)
        .map_err(|error| invalid(format!("Vehicle source {source_tag}: {error}")))?;
    let graph = graph(manager, source_tag, &original)?;
    let mut owners = BTreeMap::<u32, BTreeSet<usize>>::new();
    for resource in &graph.resources {
        if resource.instance.schema != HOVER_INSTANCE {
            continue;
        }
        let root = resource
            .definition
            .as_ref()
            .filter(|r| r.schema == HOVER_DEFINITION)
            .ok_or_else(|| {
                invalid("Driving Speed requires the supported hover motion definition")
            })?;
        owners
            .entry(resource.owner_tag)
            .or_default()
            .insert(root.owner_offset as usize);
    }
    if owners.is_empty() {
        return Err(invalid(
            "Driving Speed requires hover vehicle motion. Tank motion is not supported.",
        ));
    }
    // Plan every owner first. Unsupported programs must not leave partial allocations behind.
    let mut planned = Vec::new();
    for (owner, roots) in owners {
        let mut payload = manager
            .read_tag(TagHash(owner))
            .map_err(|error| invalid(format!("Vehicle motion owner 0x{owner:08X}: {error}")))?;
        motion::scale(&mut payload, owner, roots, settings.speed_percent)?;
        planned.push((owner, payload));
    }
    let mut edited = entity.clone();
    let mut appended = Vec::new();
    for (owner, mut payload) in planned {
        let tag = allocator.assigned_tag(
            tags.len() + appended.len(),
            "Private vehicle motion",
            "vehicle motion owner",
        )?;
        retarget_weapon_component_owner_payload(&mut payload, &edited, owner, tag.0)
            .map_err(invalid)?;
        retarget_weapon_component_owner(&mut edited, owner, tag.0).map_err(invalid)?;
        appended.push(NewTagSpec {
            template_tag: TagHash(owner),
            payload,
            storage: NewTagStorageMode::InheritTemplate,
        });
    }
    validate_weapon_entity(&edited).map_err(invalid)?;
    *entity = edited;
    tags.extend(appended);
    Ok(())
}

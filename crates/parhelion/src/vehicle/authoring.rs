//! Checked donor selection and transactional private vehicle owner allocation.
use super::{Durability, Sparrow, Weapons, health, motion, weapons};
use crate::{
    AuthoringResult, NewTagSpec, NewTagStorageMode,
    appended_tags::AppendedTagAllocator,
    error::invalid,
    tag_payload::{read_u16, read_u32, write_u64},
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

#[derive(Default)]
struct Edits {
    motion: BTreeSet<usize>,
    health: BTreeSet<usize>,
    barrels: BTreeSet<usize>,
}

/// Firing graphs swapped into owner payloads need their own residency closure.
pub(crate) fn projectile_dependency(
    manager: &PackageManager,
    settings: Option<&Sparrow>,
) -> AuthoringResult<Option<u32>> {
    match settings {
        Some(settings) => weapons::projectile(manager, &settings.weapons.projectile),
        None => Ok(None),
    }
}

pub(crate) fn apply(
    manager: &PackageManager,
    settings: Option<&Sparrow>,
    source_tag: TagHash,
    entity: &mut Vec<u8>,
    allocator: &AppendedTagAllocator,
    tags: &mut Vec<NewTagSpec>,
) -> AuthoringResult<()> {
    let Some(settings) = settings.filter(|s| s.runtime_changes()) else {
        return Ok(());
    };
    settings.validate().map_err(invalid)?;
    // Imported animation may already have private owners. Inspect the source for tuning roots
    // and retarget the source owners that remain in the composed graph.
    let original = manager
        .read_tag(source_tag)
        .map_err(|error| invalid(format!("Vehicle source {source_tag}: {error}")))?;
    let graph = graph(manager, source_tag, &original)?;
    let mut owners = BTreeMap::<u32, Edits>::new();
    for resource in &graph.resources {
        let Some(root) = resource.definition.as_ref() else {
            continue;
        };
        let offset = root.owner_offset as usize;
        let (size, roots) = match (resource.instance.schema, root.schema) {
            (HOVER_INSTANCE, HOVER_DEFINITION) if settings.motion_changes() => (
                0xD00,
                &mut owners.entry(resource.owner_tag).or_default().motion,
            ),
            (0x8080_4BEE, 0x8080_4B8A) if settings.durability != Durability::default() => (
                0x5C8,
                &mut owners.entry(resource.owner_tag).or_default().health,
            ),
            (0x8080_3889, 0x8080_3865) if settings.weapons != Weapons::default() => (
                0x1DC0,
                &mut owners.entry(resource.owner_tag).or_default().barrels,
            ),
            _ => continue,
        };
        if root.byte_size != size {
            return Err(invalid(format!(
                "Vehicle owner 0x{:08X} has an unsupported definition size for class 0x{:08X}",
                resource.owner_tag, root.schema
            )));
        }
        roots.insert(offset);
    }
    if settings.motion_changes() && owners.values().all(|e| e.motion.is_empty()) {
        return Err(invalid(
            "Driving controls require hover vehicle motion. Tank motion is not supported.",
        ));
    }
    if settings.durability != Durability::default() && owners.values().all(|e| e.health.is_empty())
    {
        return Err(invalid(
            "Durability requires supported vehicle health and shield settings",
        ));
    }
    if settings.weapons != Weapons::default() && owners.values().all(|e| e.barrels.is_empty()) {
        return Err(invalid(
            "Vehicle weapon controls require a supported armed vehicle barrel",
        ));
    }
    let projectile = weapons::projectile(manager, &settings.weapons.projectile)?;
    // Plan every owner first. Unsupported programs must not leave partial allocations behind.
    let mut planned = Vec::new();
    for (owner, edits) in owners {
        let mut payload = manager
            .read_tag(TagHash(owner))
            .map_err(|error| invalid(format!("Vehicle owner 0x{owner:08X}: {error}")))?;
        for (roots, instance) in [
            (&edits.motion, HOVER_INSTANCE),
            (&edits.health, 0x8080_4BEE),
            (&edits.barrels, 0x8080_3889),
        ] {
            for root in roots {
                // A base-class definition may be inline in a derived definition and carry no
                // preceding class marker. Its typed graph root and paired instance agree.
                if read_u32(&payload, *root)? != owner || read_u32(&payload, *root + 4)? != instance
                {
                    return Err(invalid(format!(
                        "Vehicle owner 0x{owner:08X} has an unsupported paired definition"
                    )));
                }
            }
        }
        motion::tune(&mut payload, owner, edits.motion, settings)?;
        health::tune(&mut payload, edits.health, &settings.durability)?;
        weapons::tune(&mut payload, edits.barrels, &settings.weapons, projectile)?;
        let size =
            u64::try_from(payload.len()).map_err(|_| invalid("Vehicle owner is too large"))?;
        write_u64(&mut payload, 0, size)?;
        planned.push((owner, payload));
    }
    let mut edited = entity.clone();
    let mut appended = Vec::new();
    for (owner, mut payload) in planned {
        let tag = allocator.assigned_tag(
            tags.len() + appended.len(),
            "Private vehicle tuning",
            "vehicle component owner",
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

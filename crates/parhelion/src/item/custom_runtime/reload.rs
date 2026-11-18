//! Imported magazine reloads retain their ordinary firing controller in private storage.
use super::payload::{private_index, read};
use super::*;
use sundial::package_authoring::entity::reload;

pub(in crate::item) fn imported(graph: &serde_json::Value) -> AuthoringResult<bool> {
    let Some(mode) = graph.pointer("/animation/reload") else {
        return Ok(false);
    };
    if mode != "magazine"
        || graph["source"] != "marathon"
        || graph["animation"]["first_person_status"] != "linked"
        || graph["animation"]["first_person"]["controller"] != "marathon_source"
    {
        return Err(invalid(
            "Imported reload requires a linked source magazine controller",
        ));
    }
    let inputs = graph["source_gameplay"]["inputs"]
        .as_array()
        .ok_or_else(|| invalid("Magazine reload requires source capacity and refill inputs"))?;
    let value = |input| {
        inputs
            .iter()
            .find(|v| v["component"] == 1 && v["input"] == input)
            .and_then(|v| v["value"].as_f64())
    };
    let capacity = value(0).ok_or_else(|| invalid("Magazine reload has no source capacity"))?;
    if value(3) != Some(capacity) || !(1. ..=500.).contains(&capacity) || capacity.fract() != 0. {
        return Err(invalid(
            "Magazine reload requires a matching full-magazine refill input",
        ));
    }
    Ok(true)
}

pub(in crate::item) fn apply(
    manager: &PackageManager,
    entity: &mut [u8],
    allocator: AppendedTagAllocator,
    tags: &mut Vec<NewTagSpec>,
) -> AuthoringResult<()> {
    let binding = reload::magazine_candidate(entity)
        .map_err(invalid)?
        .ok_or_else(|| {
            invalid("Imported magazine reload requires the ordinary shell-reload composition")
        })?;
    let mut owner = read(manager, binding.owner_tag, allocator, tags)?;
    let mut authored = entity.to_vec();
    let existing = private_index(binding.owner_tag, allocator, tags)?;
    let template = existing.map_or(TagHash(binding.owner_tag), |index| tags[index].template_tag);
    if existing.is_none() {
        let private =
            allocator.assigned_tag(tags.len(), "Magazine reload", "runtime component owner")?;
        retarget_weapon_component_owner_payload(
            &mut owner,
            &authored,
            binding.owner_tag,
            private.0,
        )
        .map_err(invalid)?;
        retarget_weapon_component_owner(&mut authored, binding.owner_tag, private.0)
            .map_err(invalid)?;
    }
    reload::magazine(manager, template.0, &mut authored, &mut owner).map_err(invalid)?;
    entity.copy_from_slice(&authored);
    if let Some(index) = existing {
        tags[index].payload = owner;
    } else {
        tags.push(NewTagSpec {
            template_tag: template,
            payload: owner,
            storage: crate::NewTagStorageMode::InheritTemplate,
        });
    }
    Ok(())
}

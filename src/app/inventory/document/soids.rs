//! Instance SOID discovery, uniqueness validation, and allocation.

use std::collections::BTreeMap;

#[cfg(test)]
use std::collections::BTreeSet;

use serde_json::{Map, Value};

use crate::hash::parse_unsigned_value;

use super::{
    fields::format_instance_soid,
    model::{InventoryError, InventoryResult},
    parsing::{optional_object_member, optional_root_object_member, parse_nonzero_soid},
    schema::schema_mode,
};

#[cfg(test)]
use super::schema::GENERATED_INSTANCE_SOID_START;

#[cfg(test)]
pub(crate) fn collect_used_soids(document: &Value) -> InventoryResult<BTreeSet<u64>> {
    let mut used = BTreeSet::new();
    visit_soids(document, |soid, _path| {
        used.insert(soid);
        Ok(())
    })?;
    Ok(used)
}

#[cfg(test)]
pub(crate) fn allocate_instance_soid(document: &Value) -> InventoryResult<u64> {
    next_available_instance_soid(document, GENERATED_INSTANCE_SOID_START)
}

#[cfg(test)]
pub(crate) fn next_available_instance_soid(
    document: &Value,
    first_candidate: u64,
) -> InventoryResult<u64> {
    if first_candidate == 0 {
        return Err(InventoryError::new(
            "instance_soid",
            "the first generated instance SOID must be nonzero",
        ));
    }
    let used = collect_used_soids(document)?;
    let mut candidate = first_candidate;
    loop {
        if !used.contains(&candidate) {
            return Ok(candidate);
        }
        candidate = candidate.checked_add(1).ok_or_else(|| {
            InventoryError::new(
                "instance_soid",
                "no unused instance SOID remains at or above the requested start",
            )
        })?;
    }
}

pub(in crate::app::inventory) fn validate_unique_soids(document: &Value) -> InventoryResult<()> {
    let mut first_seen = BTreeMap::<u64, String>::new();
    visit_soids(document, |soid, path| {
        if let Some(first_path) = first_seen.get(&soid) {
            return Err(InventoryError::new(
                path,
                format!(
                    "duplicate nonzero SOID {}; first used at {first_path}",
                    format_instance_soid(soid)
                ),
            ));
        }
        first_seen.insert(soid, path.to_owned());
        Ok(())
    })
}

pub(in crate::app::inventory) fn visit_soids(
    document: &Value,
    mut visit: impl FnMut(u64, &str) -> InventoryResult<()>,
) -> InventoryResult<()> {
    let future_schema = schema_mode(document).is_future();
    let Some(state) = optional_root_object_member(document, "state", "/state")? else {
        return Ok(());
    };

    if let Some(account) = optional_object_member(state, "account", "/state/account")? {
        for key in ["primary_soid", "soid"] {
            if let Some(value) = account.get(key) {
                let path = format!("/state/account/{key}");
                visit(parse_nonzero_soid(value, &path)?, &path)?;
            }
        }
    }

    let Some(characters_value) = state.get("characters") else {
        return Ok(());
    };
    let characters = characters_value
        .as_array()
        .ok_or_else(|| InventoryError::new("/state/characters", "characters must be an array"))?;
    for (character_index, character_value) in characters.iter().enumerate() {
        let character_path = format!("/state/characters/{character_index}");
        let character = character_value
            .as_object()
            .ok_or_else(|| InventoryError::new(&character_path, "character must be an object"))?;
        if let Some(value) = character.get("soid") {
            let path = format!("{character_path}/soid");
            visit(parse_nonzero_soid(value, &path)?, &path)?;
        }
        visit_equipment_soids(character, character_index, future_schema, &mut visit)?;
        visit_inventory_soids(character, character_index, &mut visit)?;
    }
    Ok(())
}

pub(in crate::app::inventory) fn visit_equipment_soids(
    character: &Map<String, Value>,
    character_index: usize,
    future_schema: bool,
    visit: &mut impl FnMut(u64, &str) -> InventoryResult<()>,
) -> InventoryResult<()> {
    let Some(value) = character.get("equipment") else {
        return Ok(());
    };
    let path = format!("/state/characters/{character_index}/equipment");
    let equipment = value
        .as_object()
        .ok_or_else(|| InventoryError::new(&path, "equipment must be an object"))?;
    for (slot, value) in equipment {
        if value.is_null() {
            continue;
        }
        let item_path = format!("{path}/{slot}");
        let known_slot = crate::account_contract::ALL_EQUIPMENT_SLOTS
            .iter()
            .any(|(known_slot, _, _)| *known_slot == slot);
        if future_schema && !known_slot {
            if let Some(soid) = value
                .as_object()
                .and_then(|item| item.get("instance_soid"))
                .and_then(parse_unsigned_value)
                .filter(|soid| *soid != 0)
            {
                visit(soid, &format!("{item_path}/instance_soid"))?;
            }
            continue;
        }
        let item = value.as_object().ok_or_else(|| {
            InventoryError::new(&item_path, "equipped item must be an object or null")
        })?;
        let soid_path = format!("{item_path}/instance_soid");
        let soid = item
            .get("instance_soid")
            .ok_or_else(|| {
                InventoryError::new(&item_path, "equipped item is missing instance_soid")
            })
            .and_then(|value| parse_nonzero_soid(value, &soid_path))?;
        visit(soid, &soid_path)?;
    }
    Ok(())
}

pub(in crate::app::inventory) fn visit_inventory_soids(
    character: &Map<String, Value>,
    character_index: usize,
    visit: &mut impl FnMut(u64, &str) -> InventoryResult<()>,
) -> InventoryResult<()> {
    let Some(value) = character.get("inventory") else {
        return Ok(());
    };
    let path = format!("/state/characters/{character_index}/inventory");
    let items = value
        .as_array()
        .ok_or_else(|| InventoryError::new(&path, "inventory must be an array"))?;
    for (item_index, value) in items.iter().enumerate() {
        let item_path = format!("{path}/{item_index}");
        let item = value
            .as_object()
            .ok_or_else(|| InventoryError::new(&item_path, "inventory item must be an object"))?;
        let soid_path = format!("{item_path}/instance_soid");
        let soid = item
            .get("instance_soid")
            .ok_or_else(|| {
                InventoryError::new(&item_path, "inventory item is missing instance_soid")
            })
            .and_then(|value| parse_nonzero_soid(value, &soid_path))?;
        visit(soid, &soid_path)?;
    }
    Ok(())
}

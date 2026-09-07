//! Frozen legacy JSON character mutations used only as differential-test oracles.

use serde_json::{Map, Value};

use super::*;

pub(super) fn add_inventory_item(
    document: &mut Value,
    character_index: usize,
    item: NewInventoryItem,
) -> InventoryResult<InventoryItemLocation> {
    let mode = require_inventory_mutation(document)?;
    validate_inventory_definition_hash(
        item.definition_hash,
        &format!("/state/characters/{character_index}/inventory/<new>/definition_hash"),
    )?;
    validate_nonnegative_i32(
        item.level,
        &format!("/state/characters/{character_index}/inventory/<new>/level"),
    )?;
    validate_positive_i32(
        item.quantity,
        &format!("/state/characters/{character_index}/inventory/<new>/quantity"),
    )?;

    validate_existing_character_inventories(document, mode)?;
    let existing = character_inventory(document, character_index)?;
    let length = existing.as_ref().map_or(0, Vec::len);
    if length >= CHARACTER_INVENTORY_CAPACITY {
        return Err(InventoryError::new(
            format!("/state/characters/{character_index}/inventory"),
            format!("character inventory is full (maximum {CHARACTER_INVENTORY_CAPACITY} items)"),
        ));
    }

    let instance_soid = allocate_instance_soid(document)?;
    let mut object = Map::new();
    object.insert(
        "instance_soid".into(),
        Value::String(format_instance_soid(instance_soid)),
    );
    object.insert(
        "definition_hash".into(),
        Value::String(format_definition_hash_hex(item.definition_hash)),
    );
    object.insert("level".into(), Value::from(item.level));
    object.insert("quantity".into(), Value::from(item.quantity));
    object.insert("plugs".into(), Value::Null);

    let character = character_object_mut(document, character_index)?;
    match character.get_mut("inventory") {
        Some(Value::Array(items)) => items.push(Value::Object(object)),
        Some(_) => unreachable!("inventory shape was validated before mutation"),
        None => {
            character.insert(
                "inventory".into(),
                Value::Array(vec![Value::Object(object)]),
            );
        }
    }
    Ok(InventoryItemLocation {
        character_index,
        item_index: length,
    })
}

pub(super) fn apply_inventory_item_action(
    document: &mut Value,
    location: InventoryItemLocation,
    action: InventoryItemAction,
) -> InventoryResult<()> {
    require_inventory_mutation(document)?;
    let snapshots = character_inventory(document, location.character_index)?.ok_or_else(|| {
        InventoryError::new(
            format!("/state/characters/{}/inventory", location.character_index),
            "character inventory is missing; add an item before editing a row",
        )
    })?;
    if location.item_index >= snapshots.len() {
        return Err(InventoryError::new(
            inventory_item_path(location),
            "inventory item index is out of range",
        ));
    }
    validate_inventory_action(location, &action, INVENTORY_FLAG_MASK)?;

    let items = inventory_array_mut(document, location.character_index)?;
    match action {
        InventoryItemAction::Remove => {
            items.remove(location.item_index);
        }
        InventoryItemAction::SetDefinitionHash(hash) => {
            inventory_object_mut(items, location.item_index).insert(
                "definition_hash".into(),
                Value::String(format_definition_hash_hex(hash)),
            );
        }
        InventoryItemAction::SetLevel(level) => {
            inventory_object_mut(items, location.item_index)
                .insert("level".into(), Value::from(level));
        }
        InventoryItemAction::SetQuantity(quantity) => {
            inventory_object_mut(items, location.item_index)
                .insert("quantity".into(), Value::from(quantity));
        }
        InventoryItemAction::SetPlugs(plugs) => {
            inventory_object_mut(items, location.item_index)
                .insert("plugs".into(), encode_plugs(plugs));
        }
        InventoryItemAction::SetFlags(Some(flags)) => {
            inventory_object_mut(items, location.item_index)
                .insert("flags".into(), Value::from(flags));
        }
        InventoryItemAction::SetFlags(None) => {
            inventory_object_mut(items, location.item_index).remove("flags");
        }
    }
    Ok(())
}

pub(super) fn swap_inventory_item_with_equipment(
    document: &mut Value,
    location: InventoryItemLocation,
    slot: &str,
) -> InventoryResult<bool> {
    require_inventory_mutation(document)?;
    if !super::super::SLOTS
        .iter()
        .any(|(known_slot, _, _)| *known_slot == slot)
    {
        return Err(InventoryError::new(
            format!(
                "/state/characters/{}/equipment/{slot}",
                location.character_index
            ),
            "unknown equipment slot",
        ));
    }

    let snapshots = character_inventory(document, location.character_index)?.ok_or_else(|| {
        InventoryError::new(
            format!("/state/characters/{}/inventory", location.character_index),
            "character inventory is missing; add an item before equipping a row",
        )
    })?;
    if location.item_index >= snapshots.len() {
        return Err(InventoryError::new(
            inventory_item_path(location),
            "inventory item index is out of range",
        ));
    }

    let character = character_object(document, location.character_index)?;
    let stored_item = character
        .get("inventory")
        .and_then(Value::as_array)
        .and_then(|items| items.get(location.item_index))
        .cloned()
        .expect("the selected inventory row was validated before the swap");
    let equipment_path = format!("/state/characters/{}/equipment", location.character_index);
    let equipment = character
        .get("equipment")
        .ok_or_else(|| InventoryError::new(&equipment_path, "equipment is missing"))?
        .as_object()
        .ok_or_else(|| InventoryError::new(&equipment_path, "equipment must be an object"))?;
    let previous_item = match equipment.get(slot) {
        Some(Value::Object(_)) => equipment.get(slot).cloned(),
        Some(Value::Null) | None => None,
        Some(_) => {
            return Err(InventoryError::new(
                format!("{equipment_path}/{slot}"),
                "equipped item must be an object or null",
            ));
        }
    };
    let replaced_item = previous_item.is_some();

    let mut candidate = document.clone();
    let candidate_character = character_object_mut(&mut candidate, location.character_index)?;
    candidate_character
        .get_mut("equipment")
        .and_then(Value::as_object_mut)
        .expect("the equipment object was validated before the swap")
        .insert(slot.to_owned(), stored_item);
    let inventory = candidate_character
        .get_mut("inventory")
        .and_then(Value::as_array_mut)
        .expect("the inventory array was validated before the swap");
    if let Some(previous_item) = previous_item {
        inventory[location.item_index] = previous_item;
    } else {
        inventory.remove(location.item_index);
    }

    let _ = character_inventory(&candidate, location.character_index)?;
    *document = candidate;
    Ok(replaced_item)
}

pub(super) fn move_inventory_item_to_character(
    document: &mut Value,
    location: InventoryItemLocation,
    destination_character_index: usize,
) -> InventoryResult<InventoryItemLocation> {
    require_inventory_mutation(document)?;
    if destination_character_index == location.character_index {
        return Err(InventoryError::new(
            format!("/state/characters/{destination_character_index}"),
            "source and destination characters must be different",
        ));
    }

    let source_inventory =
        character_inventory(document, location.character_index)?.ok_or_else(|| {
            InventoryError::new(
                format!("/state/characters/{}/inventory", location.character_index),
                "character inventory is missing; add an item before moving a row",
            )
        })?;
    if location.item_index >= source_inventory.len() {
        return Err(InventoryError::new(
            inventory_item_path(location),
            "inventory item index is out of range",
        ));
    }

    let destination_inventory = character_inventory(document, destination_character_index)?;
    let destination_length = destination_inventory.as_ref().map_or(0, Vec::len);
    if destination_length >= CHARACTER_INVENTORY_CAPACITY {
        return Err(InventoryError::new(
            format!("/state/characters/{destination_character_index}/inventory"),
            format!("character inventory is full (maximum {CHARACTER_INVENTORY_CAPACITY} items)"),
        ));
    }

    let source_character = character_object(document, location.character_index)?;
    let moved_item = source_character
        .get("inventory")
        .and_then(Value::as_array)
        .and_then(|items| items.get(location.item_index))
        .cloned()
        .expect("the selected inventory row was validated before the move");

    let mut candidate = document.clone();
    character_object_mut(&mut candidate, location.character_index)?
        .get_mut("inventory")
        .and_then(Value::as_array_mut)
        .expect("the source inventory array was validated before the move")
        .remove(location.item_index);
    let destination_character = character_object_mut(&mut candidate, destination_character_index)?;
    match destination_character.get_mut("inventory") {
        Some(Value::Array(items)) => items.push(moved_item),
        Some(_) => unreachable!("the destination inventory shape was validated before the move"),
        None => {
            destination_character.insert("inventory".into(), Value::Array(vec![moved_item]));
        }
    }

    let _ = character_inventory(&candidate, location.character_index)?;
    let _ = character_inventory(&candidate, destination_character_index)?;
    *document = candidate;
    Ok(InventoryItemLocation {
        character_index: destination_character_index,
        item_index: destination_length,
    })
}

pub(super) fn move_equipment_item_to_inventory(
    document: &mut Value,
    character_index: usize,
    slot: &str,
) -> InventoryResult<()> {
    require_inventory_mutation(document)?;
    if !super::super::SLOTS
        .iter()
        .any(|(known_slot, _, _)| *known_slot == slot)
    {
        return Err(InventoryError::new(
            format!("/state/characters/{character_index}/equipment/{slot}"),
            "unknown equipment slot",
        ));
    }

    let existing_inventory = character_inventory(document, character_index)?;
    let inventory_length = existing_inventory.as_ref().map_or(0, Vec::len);
    if inventory_length >= CHARACTER_INVENTORY_CAPACITY {
        return Err(InventoryError::new(
            format!("/state/characters/{character_index}/inventory"),
            format!("character inventory is full (maximum {CHARACTER_INVENTORY_CAPACITY} items)"),
        ));
    }

    let character = character_object(document, character_index)?;
    let equipment_path = format!("/state/characters/{character_index}/equipment");
    let equipment = character
        .get("equipment")
        .ok_or_else(|| InventoryError::new(&equipment_path, "equipment is missing"))?
        .as_object()
        .ok_or_else(|| InventoryError::new(&equipment_path, "equipment must be an object"))?;
    let equipped_item = match equipment.get(slot) {
        Some(Value::Object(_)) => equipment
            .get(slot)
            .cloned()
            .expect("the equipped item was just found"),
        Some(Value::Null) | None => {
            return Err(InventoryError::new(
                format!("{equipment_path}/{slot}"),
                "equipment slot is already empty",
            ));
        }
        Some(_) => {
            return Err(InventoryError::new(
                format!("{equipment_path}/{slot}"),
                "equipped item must be an object or null",
            ));
        }
    };

    let mut candidate = document.clone();
    let candidate_character = character_object_mut(&mut candidate, character_index)?;
    candidate_character
        .get_mut("equipment")
        .and_then(Value::as_object_mut)
        .expect("the equipment object was validated before the move")
        .insert(slot.to_owned(), Value::Null);
    match candidate_character.get_mut("inventory") {
        Some(Value::Array(items)) => items.push(equipped_item),
        Some(_) => unreachable!("the inventory shape was validated before the move"),
        None => {
            candidate_character.insert("inventory".into(), Value::Array(vec![equipped_item]));
        }
    }

    let _ = character_inventory(&candidate, character_index)?;
    *document = candidate;
    Ok(())
}

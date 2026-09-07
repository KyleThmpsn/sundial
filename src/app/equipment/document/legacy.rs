//! Frozen pre-domain equipment mutations used only as differential test oracles.

use super::*;

pub(in crate::app) fn equip_definition(
    document: &mut Value,
    character_index: usize,
    slot: &str,
    definition_hash: u64,
    default_plugs: &[Option<String>],
) -> Result<(), String> {
    if u32::try_from(definition_hash).is_err() {
        return Err(format!(
            "Cannot equip an invalid definition hash in the {} slot",
            equipment_slot_label(slot)
        ));
    }
    let current = document
        .pointer("/state/characters")
        .and_then(Value::as_array)
        .and_then(|characters| characters.get(character_index))
        .and_then(|character| character.get("equipment"))
        .and_then(Value::as_object)
        .and_then(|equipment| equipment.get(slot));
    let replacement = match current {
        Some(Value::Object(_)) => None,
        Some(Value::Null) | None => {
            let instance_soid = next_instance_soid(document)
                .ok_or("Could not allocate a unique instance SOID for the selected item")?;
            Some(serde_json::json!({
                "instance_soid": format!("0x{instance_soid:016X}"),
                "definition_hash": format_hash_hex(definition_hash),
                "level": inferred_item_level(document, character_index),
                "quantity": 1,
                "plugs": default_plug_values(default_plugs),
            }))
        }
        Some(_) => {
            return Err(format!(
                "The {} slot must be an object or null before it can be changed",
                equipment_slot_label(slot)
            ));
        }
    };

    let equipment = document
        .pointer_mut("/state/characters")
        .and_then(Value::as_array_mut)
        .and_then(|characters| characters.get_mut(character_index))
        .and_then(|character| character.get_mut("equipment"))
        .and_then(Value::as_object_mut)
        .ok_or("The selected character has no equipment object")?;
    if let Some(replacement) = replacement {
        equipment.insert(slot.into(), replacement);
        return Ok(());
    }
    let equipped = equipment
        .get_mut(slot)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| format!("Missing equipment slot: {slot}"))?;
    equipped.insert(
        "definition_hash".into(),
        Value::String(format_hash_hex(definition_hash)),
    );
    equipped.insert(
        "plugs".into(),
        Value::Array(default_plug_values(default_plugs)),
    );
    Ok(())
}

pub(in crate::app) fn set_equipment_item_level(
    document: &mut Value,
    character_index: usize,
    slot: &str,
    level: i64,
) -> Result<(), String> {
    if !(0..=i64::from(i32::MAX)).contains(&level) {
        return Err("Equipment level must be a non-negative signed 32-bit integer".to_owned());
    }
    equipment_item_object_mut(document, character_index, slot)?
        .insert("level".to_owned(), Value::from(level));
    Ok(())
}

pub(in crate::app) fn set_equipment_item_plug(
    document: &mut Value,
    character_index: usize,
    slot: &str,
    socket_index: usize,
    default_plugs: &[Option<String>],
    hash: Option<u64>,
) -> Result<(), String> {
    if socket_index >= super::super::inventory::MAX_ITEM_PLUGS {
        return Err(format!(
            "Equipment socket index must be below {}",
            super::super::inventory::MAX_ITEM_PLUGS
        ));
    }
    if hash.is_some_and(|hash| u32::try_from(hash).is_err()) {
        return Err("Equipment plug hash must fit in an unsigned 32-bit integer".to_owned());
    }
    let item = equipment_item_object_mut(document, character_index, slot)?;
    let plugs_value = item
        .get_mut("plugs")
        .ok_or_else(|| format!("Missing plugs value for {slot}"))?;
    let plugs = materialize_authored_plugs(plugs_value, default_plugs)
        .ok_or_else(|| format!("Invalid plugs value for {slot}"))?;
    while plugs.len() <= socket_index {
        plugs.push(Value::Null);
    }
    plugs[socket_index] = hash.map(format_hash_hex).map_or(Value::Null, Value::String);
    Ok(())
}

pub(in crate::app) fn set_equipment_item_flags(
    document: &mut Value,
    character_index: usize,
    slot: &str,
    flags: Option<u8>,
) -> Result<(), String> {
    if !super::super::inventory::schema_mode(document).can_mutate_equipment_flags() {
        return Err(format!(
            "Equipment flags require a writable settings schema {} or newer",
            super::super::inventory::EQUIPMENT_FLAGS_SCHEMA_VERSION
        ));
    }
    if flags.is_some_and(|flags| flags > super::super::inventory::INVENTORY_FLAG_MASK) {
        return Err(format!(
            "Equipment flags must be between 0 and {}",
            super::super::inventory::INVENTORY_FLAG_MASK
        ));
    }
    let item = equipment_item_object_mut(document, character_index, slot)?;
    if let Some(flags) = flags {
        item.insert("flags".to_owned(), Value::from(flags));
    } else {
        item.remove("flags");
    }
    Ok(())
}

pub(in crate::app) fn set_weapon_slot_empty(
    document: &mut Value,
    character_index: usize,
    slot: &str,
) -> Result<(), String> {
    if !WEAPON_SLOTS.contains(&slot) {
        return Err(format!(
            "Only weapon slots can be set to empty; {} was not changed",
            equipment_slot_label(slot)
        ));
    }
    let equipment = document
        .pointer_mut("/state/characters")
        .and_then(Value::as_array_mut)
        .and_then(|characters| characters.get_mut(character_index))
        .and_then(|character| character.get_mut("equipment"))
        .and_then(Value::as_object_mut)
        .ok_or("The selected character has no equipment object")?;
    match equipment.get(slot) {
        Some(Value::Object(_) | Value::Null) | None => {
            equipment.insert(slot.into(), Value::Null);
            Ok(())
        }
        Some(_) => Err(format!(
            "The {} slot contains unexpected data and was not changed",
            equipment_slot_label(slot)
        )),
    }
}

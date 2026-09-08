//! Resize only the authored plug suffix when a retained native definition changes size.
use super::*;
use crate::hash::{format_hash_hex, parse_unsigned_value};

pub(super) fn resize(
    document: &mut Value,
    removed_hashes: &BTreeSet<u32>,
    changes: &[AuthoredSocketChange],
) -> Result<BTreeMap<u32, usize>, String> {
    if changes.is_empty() {
        return Ok(BTreeMap::new());
    }
    let mut by_hash = BTreeMap::new();
    for change in changes {
        if removed_hashes.contains(&change.definition_hash)
            || change.previous_socket_count > inventory::MAX_ITEM_PLUGS
            || change.default_plugs.len() > inventory::MAX_ITEM_PLUGS
            || change
                .default_plugs
                .iter()
                .flatten()
                .any(|hash| *hash == u32::MAX)
            || by_hash.insert(change.definition_hash, change).is_some()
        {
            return Err("The replacement has conflicting or unsupported socket layouts".into());
        }
    }
    let mode = inventory::schema_mode(document);
    if mode.is_read_only() || mode.is_future() {
        return Err("Automatic socket updates require a supported settings schema".into());
    }
    let mut updated = document.clone();
    let mut resized = BTreeMap::new();
    let Some(characters) = updated.pointer_mut("/state/characters") else {
        return Ok(resized);
    };
    let characters = characters
        .as_array_mut()
        .ok_or("Characters must be an array")?;
    for (index, character) in characters.iter_mut().enumerate() {
        if let Some(equipment) = character.get_mut("equipment") {
            let equipment = equipment
                .as_object_mut()
                .ok_or("Equipment must be an object")?;
            for &(slot, _, _) in mode.equipment_slots() {
                if let Some(item) = equipment.get_mut(slot) {
                    resize_item(
                        item,
                        &by_hash,
                        &mut resized,
                        &format!("character {index} {slot}"),
                    )?;
                }
            }
        }
        if let Some(items) = character.get_mut("inventory") {
            let items = items.as_array_mut().ok_or("Inventory must be an array")?;
            for (row, item) in items.iter_mut().enumerate() {
                resize_item(
                    item,
                    &by_hash,
                    &mut resized,
                    &format!("character {index} inventory {row}"),
                )?;
            }
        }
    }
    inventory::validate_document_items(&updated).map_err(|e| e.to_string())?;
    *document = updated;
    Ok(resized)
}

fn resize_item(
    item: &mut Value,
    changes: &BTreeMap<u32, &AuthoredSocketChange>,
    resized: &mut BTreeMap<u32, usize>,
    location: &str,
) -> Result<(), String> {
    let Some(hash) = item
        .get("definition_hash")
        .and_then(parse_unsigned_value)
        .and_then(|hash| u32::try_from(hash).ok())
    else {
        return Ok(());
    };
    let Some(change) = changes.get(&hash) else {
        return Ok(());
    };
    let plugs = item
        .get_mut("plugs")
        .ok_or_else(|| format!("Missing plugs at {location}"))?;
    if plugs.is_null() {
        return Ok(());
    }
    let plugs = plugs
        .as_array_mut()
        .ok_or_else(|| format!("Malformed plugs at {location}"))?;
    for plug in plugs.iter() {
        if !plug.is_null()
            && parse_unsigned_value(plug).is_none_or(|hash| hash >= u64::from(u32::MAX))
        {
            return Err(format!(
                "Malformed plug for item 0x{hash:08X} at {location}"
            ));
        }
    }
    let incoming_count = change.default_plugs.len();
    if plugs.len() == incoming_count {
        return Ok(());
    }
    if plugs.len() != change.previous_socket_count {
        return Err(format!(
            "Item 0x{hash:08X} at {location} has {} saved plugs, expected {} installed sockets or {incoming_count} incoming sockets. Repair its socket selections in Sundial before installing.",
            plugs.len(),
            change.previous_socket_count,
        ));
    }
    // Keep existing JSON values exactly, including explicit empty selections and hash spelling.
    // A reviewed shrink removes only the suffix no longer present in the incoming definition.
    plugs.truncate(incoming_count);
    plugs.extend(change.default_plugs[plugs.len()..].iter().map(|hash| {
        hash.map_or(Value::Null, |hash| {
            Value::String(format_hash_hex(u64::from(hash)))
        })
    }));
    *resized.entry(hash).or_default() += 1;
    Ok(())
}

#[cfg(test)]
mod tests;

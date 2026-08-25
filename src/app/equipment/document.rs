use super::*;

/// Returns the present, non-null equipment rows for one character in [`SLOTS`] order.
///
/// A malformed row is represented by an [`EquippedItemSnapshot`] with issues instead
/// of being discarded. Errors are reserved for an unusable character/equipment path.
pub(in crate::app) fn equipped_item_snapshots(
    document: &Value,
    character_index: usize,
) -> Result<Vec<EquippedItemSnapshot>, String> {
    let characters = document
        .pointer("/state/characters")
        .ok_or("Missing /state/characters")?
        .as_array()
        .ok_or("/state/characters must be an array")?;
    let character = characters
        .get(character_index)
        .ok_or_else(|| format!("Missing character at index {character_index}"))?
        .as_object()
        .ok_or_else(|| format!("Character {character_index} must be an object"))?;
    let Some(equipment_value) = character.get("equipment") else {
        return Ok(Vec::new());
    };
    let equipment = equipment_value
        .as_object()
        .ok_or_else(|| format!("Character {character_index} equipment must be an object"))?;
    let allow_unknown_members = super::inventory::schema_mode(document).is_future();

    Ok(SLOTS
        .iter()
        .filter_map(|&(slot, slot_label, bucket_hash)| {
            let value = equipment.get(slot)?;
            (!value.is_null()).then(|| {
                equipped_item_snapshot(slot, slot_label, bucket_hash, value, allow_unknown_members)
            })
        })
        .collect())
}

fn equipped_item_snapshot(
    slot: &'static str,
    slot_label: &'static str,
    bucket_hash: u64,
    value: &Value,
    allow_unknown_members: bool,
) -> EquippedItemSnapshot {
    const NO_DEFINITION_HASH: u64 = 0x811C_9DC5;

    let raw_item_text = compact_json_text(value);
    let Some(item) = value.as_object() else {
        return EquippedItemSnapshot {
            slot,
            slot_label,
            bucket_hash,
            raw_item_text: raw_item_text.clone(),
            definition_hash: None,
            definition_text: "<missing>".to_owned(),
            instance_soid: None,
            instance_soid_text: "<missing>".to_owned(),
            level: None,
            quantity: None,
            plugs: EquippedItemPlugs::Malformed(raw_item_text),
            issues: vec!["equipment row must be an object".to_owned()],
        };
    };

    let mut issues = Vec::new();
    if !allow_unknown_members {
        for member in item.keys() {
            if !super::inventory::KNOWN_ITEM_MEMBERS.contains(&member.as_str()) {
                issues.push(format!("unknown item member {member}"));
            }
        }
    }

    let definition_value = item.get("definition_hash");
    let definition_hash = definition_value.and_then(parse_unsigned_value);
    let definition_text =
        definition_hash.map_or_else(|| field_display_text(definition_value), format_hash_hex);
    match (definition_value, definition_hash) {
        (None, _) => issues.push("missing definition_hash".to_owned()),
        (Some(_), None) => {
            issues.push("definition_hash must be an unsigned integer or a 0x hex string".to_owned())
        }
        (_, Some(hash)) if u32::try_from(hash).is_err() => {
            issues.push("definition_hash must fit in an unsigned 32-bit value".to_owned());
        }
        (_, Some(NO_DEFINITION_HASH)) => {
            issues.push("definition_hash is the engine no-definition sentinel".to_owned());
        }
        _ => {}
    }

    let soid_value = item.get("instance_soid");
    let instance_soid = soid_value.and_then(parse_unsigned_value);
    let instance_soid_text = instance_soid.map_or_else(
        || field_display_text(soid_value),
        |soid| format!("0x{soid:016X}"),
    );
    match (soid_value, instance_soid) {
        (None, _) => issues.push("missing instance_soid".to_owned()),
        (Some(_), None) => {
            issues.push("instance_soid must be an unsigned integer or a 0x hex string".to_owned());
        }
        (_, Some(0)) => issues.push("instance_soid must not be zero".to_owned()),
        _ => {}
    }

    let level = item.get("level").and_then(Value::as_i64);
    match item.get("level") {
        None => issues.push("missing level".to_owned()),
        Some(_) if level.is_none() => {
            issues.push("level must be a signed 32-bit integer".to_owned());
        }
        Some(_) if !level.is_some_and(|value| (0..=i64::from(i32::MAX)).contains(&value)) => {
            issues.push("level must be a non-negative signed 32-bit integer".to_owned());
        }
        _ => {}
    }

    let quantity = item.get("quantity").and_then(Value::as_i64);
    match item.get("quantity") {
        None => issues.push("missing quantity".to_owned()),
        Some(_) if quantity.is_none() => {
            issues.push("quantity must be a signed 32-bit integer".to_owned());
        }
        Some(_) if !quantity.is_some_and(|value| (1..=i64::from(i32::MAX)).contains(&value)) => {
            issues.push("quantity must be a positive signed 32-bit integer".to_owned());
        }
        _ => {}
    }

    let plugs = equipped_item_plugs(item.get("plugs"), &mut issues, NO_DEFINITION_HASH);

    if let Some(flags) = item.get("flags")
        && parse_unsigned_value(flags)
            .is_none_or(|flags| flags > u64::from(super::inventory::INVENTORY_FLAG_MASK))
    {
        issues.push(format!(
            "flags must be between 0 and {}",
            super::inventory::INVENTORY_FLAG_MASK
        ));
    }

    EquippedItemSnapshot {
        slot,
        slot_label,
        bucket_hash,
        raw_item_text,
        definition_hash,
        definition_text,
        instance_soid,
        instance_soid_text,
        level,
        quantity,
        plugs,
        issues,
    }
}

fn equipped_item_plugs(
    value: Option<&Value>,
    issues: &mut Vec<String>,
    no_definition_hash: u64,
) -> EquippedItemPlugs {
    let Some(value) = value else {
        issues.push("missing plugs".to_owned());
        return EquippedItemPlugs::Missing;
    };
    if value.is_null() {
        return EquippedItemPlugs::NativeDefaults;
    }
    let Some(plugs) = value.as_array() else {
        let raw = compact_json_text(value);
        issues.push("plugs must be null or an array".to_owned());
        return EquippedItemPlugs::Malformed(raw);
    };
    if plugs.len() > super::inventory::MAX_ITEM_PLUGS {
        issues.push(format!(
            "plugs cannot contain more than {} entries",
            super::inventory::MAX_ITEM_PLUGS
        ));
    }
    let values = plugs
        .iter()
        .enumerate()
        .map(|(index, value)| {
            if value.is_null() {
                return EquippedPlugValue::Empty;
            }
            let Some(hash) = parse_unsigned_value(value) else {
                let raw = compact_json_text(value);
                issues.push(format!(
                    "plug {index} must be null, an unsigned integer, or a 0x hex string"
                ));
                return EquippedPlugValue::Malformed(raw);
            };
            if u32::try_from(hash).is_err() {
                issues.push(format!(
                    "plug {index} hash must fit in an unsigned 32-bit value"
                ));
            } else if hash == no_definition_hash {
                issues.push(format!(
                    "plug {index} hash is the engine no-definition sentinel"
                ));
            }
            EquippedPlugValue::Hash(hash)
        })
        .collect();
    EquippedItemPlugs::Authored(values)
}

pub(super) fn field_display_text(value: Option<&Value>) -> String {
    match value {
        None => "<missing>".to_owned(),
        Some(Value::String(text)) => text.clone(),
        Some(value) => compact_json_text(value),
    }
}

fn compact_json_text(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| format!("{value:?}"))
}

fn default_plug_values(defaults: &[Option<String>]) -> Vec<Value> {
    defaults
        .iter()
        .map(|plug| plug.clone().map_or(Value::Null, Value::String))
        .collect()
}

pub(in crate::app) fn equipment_slot_label(slot: &str) -> &str {
    SLOTS
        .iter()
        .find_map(|(name, label, _)| (*name == slot).then_some(*label))
        .unwrap_or(slot)
}

fn next_instance_soid(document: &Value) -> Option<u64> {
    super::inventory::allocate_instance_soid(document).ok()
}

pub(in crate::app) fn inferred_item_level(document: &Value, character_index: usize) -> i64 {
    document
        .pointer("/state/characters")
        .and_then(Value::as_array)
        .and_then(|characters| characters.get(character_index))
        .and_then(|character| character.get("equipment"))
        .and_then(Value::as_object)
        .and_then(|equipment| {
            equipment
                .values()
                .filter_map(|item| {
                    item.get("level")
                        .and_then(Value::as_i64)
                        .filter(|level| (1..=i64::from(i32::MAX)).contains(level))
                })
                .max()
        })
        .unwrap_or(106)
}

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

/// Equips a subclass and resets the character's coordinated ability fields as one edit.
///
/// Work is performed on a clone so a malformed equipment or character path cannot leave
/// the subclass and ability selections out of sync.
pub(in crate::app) fn equip_subclass_with_default_abilities(
    document: &mut Value,
    character_index: usize,
    item: &ItemDef,
) -> Result<(), String> {
    let subclass_bucket = SLOTS
        .iter()
        .find_map(|(slot, _, bucket)| (*slot == "subclass").then_some(*bucket))
        .expect("SLOTS must contain the subclass slot");
    if item.bucket_hash != subclass_bucket {
        return Err("The selected definition is not a subclass".to_owned());
    }
    let class_type = document
        .pointer("/state/characters")
        .and_then(Value::as_array)
        .and_then(|characters| characters.get(character_index))
        .and_then(|character| character.get("class"))
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("Character {} has no valid class", character_index + 1))?;
    if item.class_type != 3 && item.class_type != class_type {
        return Err(format!(
            "{} is not compatible with {}",
            item.name,
            class_name(class_type)
        ));
    }

    let mut candidate = document.clone();
    equip_definition(
        &mut candidate,
        character_index,
        "subclass",
        item.hash,
        &item.default_plugs,
    )?;
    set_default_subclass_abilities(&mut candidate, character_index, class_type, item)?;
    *document = candidate;
    Ok(())
}

/// Equips one exact stored instance, moving the previously equipped instance back to inventory.
/// Subclass swaps also reset the coordinated character ability entries just like the definition
/// picker does.
pub(in crate::app) fn equip_inventory_item(
    document: &mut Value,
    location: super::inventory::InventoryItemLocation,
    slot: &str,
    item: &ItemDef,
) -> Result<bool, String> {
    let expected_bucket = SLOTS
        .iter()
        .find_map(|(known_slot, _, bucket)| (*known_slot == slot).then_some(*bucket))
        .ok_or_else(|| format!("Unknown equipment slot: {slot}"))?;
    if item.bucket_hash != expected_bucket {
        return Err(format!(
            "{} is not valid for the {} slot",
            item.name,
            equipment_slot_label(slot)
        ));
    }

    let inventory = super::inventory::character_inventory(document, location.character_index)
        .map_err(|error| error.to_string())?
        .ok_or("The selected character has no inventory array")?;
    let snapshot = inventory
        .iter()
        .find(|snapshot| snapshot.location == location)
        .ok_or("The selected inventory item no longer exists")?;
    if u64::from(snapshot.definition_hash) != item.hash {
        return Err("The selected inventory item changed before it could be equipped".to_owned());
    }
    if snapshot.quantity != 1 {
        return Err("Only a single inventory item can be equipped at a time".to_owned());
    }

    let class_type = document
        .pointer("/state/characters")
        .and_then(Value::as_array)
        .and_then(|characters| characters.get(location.character_index))
        .and_then(|character| character.get("class"))
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            format!(
                "Character {} has no valid class",
                location.character_index + 1
            )
        })?;
    if item.class_type != 3 && item.class_type != class_type {
        return Err(format!(
            "{} is not compatible with {}",
            item.name,
            class_name(class_type)
        ));
    }

    let mut candidate = document.clone();
    let replaced_item =
        super::inventory::swap_inventory_item_with_equipment(&mut candidate, location, slot)
            .map_err(|error| error.to_string())?;
    if slot == "subclass" {
        set_default_subclass_abilities(&mut candidate, location.character_index, class_type, item)?;
    }
    *document = candidate;
    Ok(replaced_item)
}

fn set_default_subclass_abilities(
    document: &mut Value,
    character_index: usize,
    class_type: u64,
    item: &ItemDef,
) -> Result<(), String> {
    let defaults = default_ability_values(
        class_type,
        &item.abilities,
        game_settings::schema_version(document),
    );
    let character = document
        .pointer_mut("/state/characters")
        .and_then(Value::as_array_mut)
        .and_then(|characters| characters.get_mut(character_index))
        .and_then(Value::as_object_mut)
        .ok_or_else(|| format!("Character {} must be an object", character_index + 1))?;
    for (field, value) in [
        ("movement_ability", defaults.0),
        ("grenade_ability", defaults.1),
        ("super_ability", defaults.2),
        ("melee_ability", defaults.3),
        ("class_ability", defaults.4),
    ] {
        character.insert(field.to_owned(), Value::from(value));
    }
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
    if socket_index >= super::inventory::MAX_ITEM_PLUGS {
        return Err(format!(
            "Equipment socket index must be below {}",
            super::inventory::MAX_ITEM_PLUGS
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
    if !super::inventory::schema_mode(document).can_mutate_equipment_flags() {
        return Err(format!(
            "Equipment flags require a writable settings schema {} or newer",
            super::inventory::EQUIPMENT_FLAGS_SCHEMA_VERSION
        ));
    }
    if flags.is_some_and(|flags| flags > super::inventory::INVENTORY_FLAG_MASK) {
        return Err(format!(
            "Equipment flags must be between 0 and {}",
            super::inventory::INVENTORY_FLAG_MASK
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

fn equipment_item_object_mut<'a>(
    document: &'a mut Value,
    character_index: usize,
    slot: &str,
) -> Result<&'a mut serde_json::Map<String, Value>, String> {
    if !SLOTS.iter().any(|(known_slot, _, _)| *known_slot == slot) {
        return Err(format!("Unknown equipment slot: {slot}"));
    }
    document
        .pointer_mut("/state/characters")
        .and_then(Value::as_array_mut)
        .and_then(|characters| characters.get_mut(character_index))
        .and_then(|character| character.get_mut("equipment"))
        .and_then(Value::as_object_mut)
        .and_then(|equipment| equipment.get_mut(slot))
        .and_then(Value::as_object_mut)
        .ok_or_else(|| {
            format!(
                "The {} slot must contain an item object before it can be edited",
                equipment_slot_label(slot)
            )
        })
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

pub(in crate::app) fn displayed_plugs(
    plugs: Option<&Value>,
    defaults: &[Option<String>],
) -> (Vec<Value>, bool) {
    let default_plugs = || default_plug_values(defaults);
    match plugs {
        Some(Value::Array(plugs)) => {
            let native_defaults = *plugs == default_plugs();
            (plugs.clone(), native_defaults)
        }
        Some(Value::Null) => (default_plugs(), true),
        _ => (Vec::new(), false),
    }
}

pub(in crate::app) fn materialize_authored_plugs<'a>(
    plugs: &'a mut Value,
    defaults: &[Option<String>],
) -> Option<&'a mut Vec<Value>> {
    if plugs.is_null() {
        *plugs = Value::Array(default_plug_values(defaults));
    }
    plugs.as_array_mut()
}

pub(in crate::app) fn native_plug_default(
    defaults: &[Option<String>],
    socket_index: usize,
) -> Option<NativePlugDefault> {
    match defaults.get(socket_index)? {
        Some(hash_hex) => parse_hash_hex(hash_hex).map(NativePlugDefault::Plug),
        None => Some(NativePlugDefault::Empty),
    }
}

pub(super) fn equipped_header_label(id_scope: &str, slot_label: &str) -> String {
    if id_scope == "character-inventory-equipped" {
        "Equipped".to_owned()
    } else {
        format!("{slot_label} Slot")
    }
}

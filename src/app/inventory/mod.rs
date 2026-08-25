//! Schema-aware access, validation, and mutation helpers for authored items.
//!
//! This module deliberately has no UI or catalog dependencies. Read operations never
//! materialize missing JSON fields, while explicit add operations may create the leaf array
//! they target after validating the containing document shape.

use std::collections::BTreeSet;

use serde_json::{Map, Value};

mod document;

#[allow(unused_imports)]
pub(crate) use document::{
    CHARACTER_INVENTORY_CAPACITY, DISMANTLE_REWARDS_SCHEMA_VERSION, DismantleGearClass,
    DismantleRarity, DismantleRewardAction, DismantleRewardLocation, DismantleRewardSnapshot,
    EQUIPMENT_FLAGS_SCHEMA_VERSION, FILTERED_DISMANTLE_REWARD_CAPACITY,
    GENERATED_INSTANCE_SOID_START, INVENTORY_FLAG_LOCKED, INVENTORY_FLAG_MASK,
    INVENTORY_FLAG_TRACKED, INVENTORY_SCHEMA_VERSION, InventoryError, InventoryItemAction,
    InventoryItemLocation, InventoryItemSnapshot, ItemPlugs, LEGACY_PROFILE_ITEM_CAPACITY,
    MAX_ITEM_PLUGS, NewInventoryItem, PROFILE_ITEM_CAPACITY, ProfileItemAction,
    ProfileItemLocation, ProfileItemSnapshot, SchemaMode, allocate_instance_soid,
    character_inventory, collect_used_soids, dismantle_rewards, next_available_instance_soid,
    profile_item_capacity, profile_item_target_exists, profile_items, schema_mode,
    set_inventory_locked_flag, validate_document_items,
};

pub(super) use document::KNOWN_ITEM_MEMBERS;

use document::{
    InventoryResult, account_object_mut, character_object, character_object_mut, encode_plugs,
    ensure_account_object, format_definition_hash_hex, format_instance_soid, inventory_array_mut,
    inventory_item_path, inventory_object_mut, profile_array_mut, read_only_schema_error,
    require_inventory_mutation, require_profile_mutation, require_readable_schema,
    validate_existing_character_inventories, validate_inventory_action,
    validate_inventory_definition_hash, validate_nonnegative_i32, validate_positive_i32,
};

#[cfg(test)]
use crate::game_settings::MAX_SUPPORTED_SCHEMA;

pub(crate) fn add_profile_item(
    document: &mut Value,
    definition_hash: u32,
    quantity: i32,
) -> InventoryResult<ProfileItemLocation> {
    let mode = require_profile_mutation(document)?;
    validate_inventory_definition_hash(
        definition_hash,
        "/state/account/profile_items/<new>/definition_hash",
    )?;
    validate_positive_i32(quantity, "/state/account/profile_items/<new>/quantity")?;

    let existing = profile_items(document)?;
    let length = existing.as_ref().map_or(0, Vec::len);
    let capacity = mode
        .profile_item_capacity()
        .expect("writable schemas always have a known profile capacity");
    if length >= capacity {
        return Err(InventoryError::new(
            "/state/account/profile_items",
            format!("profile_items is full for this schema (maximum {capacity})"),
        ));
    }
    ensure_account_object(document)?;

    let mut item = Map::new();
    item.insert(
        "definition_hash".into(),
        Value::String(format_definition_hash_hex(definition_hash)),
    );
    item.insert("quantity".into(), Value::from(quantity));

    let account = account_object_mut(document)?;
    match account.get_mut("profile_items") {
        Some(Value::Array(items)) => items.push(Value::Object(item)),
        Some(_) => unreachable!("profile_items shape was validated before mutation"),
        None => {
            account.insert(
                "profile_items".into(),
                Value::Array(vec![Value::Object(item)]),
            );
        }
    }
    Ok(ProfileItemLocation { index: length })
}

pub(crate) fn apply_profile_item_action(
    document: &mut Value,
    location: ProfileItemLocation,
    action: ProfileItemAction,
) -> InventoryResult<()> {
    require_profile_mutation(document)?;
    let snapshots = profile_items(document)?.ok_or_else(|| {
        InventoryError::new(
            "/state/account/profile_items",
            "profile_items is missing; add an item before editing a row",
        )
    })?;
    if location.index >= snapshots.len() {
        return Err(InventoryError::new(
            format!("/state/account/profile_items/{}", location.index),
            "profile item index is out of range",
        ));
    }
    match &action {
        ProfileItemAction::SetDefinitionHash(hash) => validate_inventory_definition_hash(
            *hash,
            &format!(
                "/state/account/profile_items/{}/definition_hash",
                location.index
            ),
        )?,
        ProfileItemAction::SetQuantity(quantity) => validate_positive_i32(
            *quantity,
            &format!("/state/account/profile_items/{}/quantity", location.index),
        )?,
        ProfileItemAction::Remove => {}
    }

    let items = profile_array_mut(document)?;
    match action {
        ProfileItemAction::Remove => {
            items.remove(location.index);
        }
        ProfileItemAction::SetDefinitionHash(hash) => {
            let item = items[location.index]
                .as_object_mut()
                .expect("profile row shape was validated before mutation");
            item.insert(
                "definition_hash".into(),
                Value::String(format_definition_hash_hex(hash)),
            );
        }
        ProfileItemAction::SetQuantity(quantity) => {
            let item = items[location.index]
                .as_object_mut()
                .expect("profile row shape was validated before mutation");
            item.insert("quantity".into(), Value::from(quantity));
        }
    }
    Ok(())
}

pub(crate) fn add_dismantle_reward(
    document: &mut Value,
    definition_hash: u32,
) -> InventoryResult<DismantleRewardLocation> {
    let mode = require_dismantle_reward_mutation(document)?;
    validate_inventory_definition_hash(
        definition_hash,
        "/state/account/dismantle_rewards/<new>/definition_hash",
    )?;
    let existing = dismantle_rewards(document)?.unwrap_or_default();
    let capacity = mode
        .dismantle_reward_capacity()
        .expect("writable dismantle schemas have a known capacity");
    if existing.len() >= capacity {
        return Err(InventoryError::new(
            "/state/account/dismantle_rewards",
            format!("dismantle_rewards is full for this schema (maximum {capacity})"),
        ));
    }

    let occupied = existing
        .iter()
        .map(dismantle_policy_key)
        .collect::<BTreeSet<_>>();
    let mut selected = None;
    let rarity_masks = if mode.supports_filtered_dismantle_rewards() {
        0..32
    } else {
        0..1
    };
    let gear_classes: &[Option<DismantleGearClass>] = if mode.supports_filtered_dismantle_rewards()
    {
        &[
            None,
            Some(DismantleGearClass::Weapon),
            Some(DismantleGearClass::Armor),
        ]
    } else {
        &[None]
    };
    let masterwork_filters: &[Option<bool>] = if mode.supports_filtered_dismantle_rewards() {
        &[None, Some(false), Some(true)]
    } else {
        &[None]
    };
    'policies: for rarity_mask in rarity_masks {
        let rarities = DismantleRarity::ALL
            .into_iter()
            .enumerate()
            .filter_map(|(index, rarity)| (rarity_mask & (1 << index) != 0).then_some(rarity))
            .collect::<Vec<_>>();
        for &gear_class in gear_classes {
            for &masterworked in masterwork_filters {
                let candidate = (
                    definition_hash,
                    rarity_mask_of(&rarities),
                    gear_class.map_or(0, DismantleGearClass::mask),
                    masterworked.map_or(0, |value| if value { 1 } else { 2 }),
                );
                if !occupied.contains(&candidate) {
                    selected = Some((rarities, gear_class, masterworked));
                    break 'policies;
                }
            }
        }
    }
    let Some((rarities, gear_class, masterworked)) = selected else {
        return Err(InventoryError::new(
            "/state/account/dismantle_rewards",
            "every supported filter combination for this material is already present",
        ));
    };

    let mut candidate = document.clone();
    ensure_account_object(&candidate)?;
    let mut reward = Map::new();
    write_dismantle_policy(
        &mut reward,
        definition_hash,
        1,
        &rarities,
        gear_class,
        masterworked,
        mode.supports_filtered_dismantle_rewards(),
    );
    let account = account_object_mut(&mut candidate)?;
    match account.get_mut("dismantle_rewards") {
        Some(Value::Array(rewards)) => rewards.push(Value::Object(reward)),
        Some(_) => unreachable!("dismantle rewards were validated before mutation"),
        None => {
            account.insert(
                "dismantle_rewards".into(),
                Value::Array(vec![Value::Object(reward)]),
            );
        }
    }
    validate_document_items(&candidate)?;
    *document = candidate;
    Ok(DismantleRewardLocation {
        index: existing.len(),
    })
}

pub(crate) fn apply_dismantle_reward_action(
    document: &mut Value,
    location: DismantleRewardLocation,
    action: DismantleRewardAction,
) -> InventoryResult<()> {
    let mode = require_dismantle_reward_mutation(document)?;
    let snapshots = dismantle_rewards(document)?.ok_or_else(|| {
        InventoryError::new(
            "/state/account/dismantle_rewards",
            "dismantle_rewards is missing; add a policy before editing a row",
        )
    })?;
    if location.index >= snapshots.len() {
        return Err(InventoryError::new(
            format!("/state/account/dismantle_rewards/{}", location.index),
            "dismantle reward index is out of range",
        ));
    }
    if let DismantleRewardAction::SetPolicy {
        definition_hash,
        quantity,
        rarities,
        gear_class,
        masterworked,
    } = &action
    {
        validate_inventory_definition_hash(
            *definition_hash,
            &format!(
                "/state/account/dismantle_rewards/{}/definition_hash",
                location.index
            ),
        )?;
        validate_positive_i32(
            *quantity,
            &format!(
                "/state/account/dismantle_rewards/{}/quantity",
                location.index
            ),
        )?;
        if !mode.supports_filtered_dismantle_rewards()
            && (!rarities.is_empty() || gear_class.is_some() || masterworked.is_some())
        {
            return Err(InventoryError::new(
                format!("/state/account/dismantle_rewards/{}", location.index),
                "dismantle filters require settings schema 8",
            ));
        }
    }

    let mut candidate = document.clone();
    let rewards = candidate
        .pointer_mut("/state/account/dismantle_rewards")
        .and_then(Value::as_array_mut)
        .expect("dismantle rewards were validated before mutation");
    match action {
        DismantleRewardAction::Remove => {
            rewards.remove(location.index);
        }
        DismantleRewardAction::SetPolicy {
            definition_hash,
            quantity,
            rarities,
            gear_class,
            masterworked,
        } => {
            let reward = rewards[location.index]
                .as_object_mut()
                .expect("dismantle reward row was validated before mutation");
            write_dismantle_policy(
                reward,
                definition_hash,
                quantity,
                &rarities,
                gear_class,
                masterworked,
                mode.supports_filtered_dismantle_rewards(),
            );
        }
    }
    validate_document_items(&candidate)?;
    *document = candidate;
    Ok(())
}

fn require_dismantle_reward_mutation(document: &Value) -> InventoryResult<SchemaMode> {
    let mode = require_readable_schema(document)?;
    if mode.can_mutate_dismantle_rewards() {
        Ok(mode)
    } else {
        Err(read_only_schema_error(mode, "dismantle rewards"))
    }
}

fn dismantle_policy_key(snapshot: &DismantleRewardSnapshot) -> (u32, u8, u8, u8) {
    (
        snapshot.definition_hash,
        rarity_mask_of(&snapshot.rarities),
        snapshot.gear_class.map_or(0, DismantleGearClass::mask),
        snapshot
            .masterworked
            .map_or(0, |value| if value { 1 } else { 2 }),
    )
}

fn rarity_mask_of(rarities: &[DismantleRarity]) -> u8 {
    rarities.iter().fold(0, |mask, rarity| mask | rarity.bit())
}

fn write_dismantle_policy(
    reward: &mut Map<String, Value>,
    definition_hash: u32,
    quantity: i32,
    rarities: &[DismantleRarity],
    gear_class: Option<DismantleGearClass>,
    masterworked: Option<bool>,
    filtered: bool,
) {
    reward.insert(
        "definition_hash".into(),
        Value::String(format_definition_hash_hex(definition_hash)),
    );
    reward.insert("quantity".into(), Value::from(quantity));
    if filtered && !rarities.is_empty() {
        let value = if rarities.len() == 1 {
            Value::String(rarities[0].token().into())
        } else {
            Value::Array(
                rarities
                    .iter()
                    .map(|rarity| Value::String(rarity.token().into()))
                    .collect(),
            )
        };
        reward.insert("rarity".into(), value);
    } else {
        reward.remove("rarity");
    }
    if filtered {
        if let Some(gear_class) = gear_class {
            reward.insert("class".into(), Value::String(gear_class.token().into()));
        } else {
            reward.remove("class");
        }
        if let Some(masterworked) = masterworked {
            reward.insert("masterworked".into(), Value::Bool(masterworked));
        } else {
            reward.remove("masterworked");
        }
    } else {
        reward.remove("class");
        reward.remove("masterworked");
    }
}

pub(crate) fn add_inventory_item(
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

pub(crate) fn apply_inventory_item_action(
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
    validate_inventory_action(location, &action)?;

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

/// Moves a stored item into an equipment slot and puts the previous equipped item in its place.
///
/// The complete authored rows are moved rather than rebuilt, preserving instance SOIDs, plugs,
/// flags, and any unrecognized members. If the equipment slot is empty, the stored row is removed
/// from the inventory array. Work is performed on a clone so a malformed destination cannot leave
/// the document partially changed.
pub(crate) fn swap_inventory_item_with_equipment(
    document: &mut Value,
    location: InventoryItemLocation,
    slot: &str,
) -> InventoryResult<bool> {
    require_inventory_mutation(document)?;
    if !super::SLOTS
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

    // An equipped row can be more malformed than a stored row. Refuse the swap if moving it into
    // inventory would make that inventory unreadable by the guided editor.
    let _ = character_inventory(&candidate, location.character_index)?;
    *document = candidate;
    Ok(replaced_item)
}

/// Moves a complete authored inventory row from one character to another.
///
/// The source row is removed and appended to the destination inventory without rebuilding it,
/// preserving its instance SOID, plugs, flags, and any unrecognized members. Work is performed
/// on a clone so an invalid or full destination cannot leave either character partially changed.
pub(crate) fn move_inventory_item_to_character(
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

/// Moves the complete authored item in an equipment slot to the end of character inventory.
///
/// The source slot is set to null only after the resulting inventory has been validated, so a
/// full inventory or malformed equipped row cannot leave the document partially changed.
pub(crate) fn move_equipment_item_to_inventory(
    document: &mut Value,
    character_index: usize,
    slot: &str,
) -> InventoryResult<()> {
    require_inventory_mutation(document)?;
    if !super::SLOTS
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

#[cfg(test)]
mod tests;

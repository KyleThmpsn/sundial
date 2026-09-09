//! Schema-aware access, validation, and mutation helpers for authored items.
//!
//! This module deliberately has no UI or catalog dependencies. Read operations never
//! materialize missing JSON fields, while explicit add operations may create the leaf array
//! they target after validating the containing document shape.

use std::collections::BTreeSet;

use serde_json::Value;
use sundial_account as account_domain;

mod document;

pub(crate) use document::{
    CHARACTER_INVENTORY_CAPACITY, DismantleGearClass, DismantleRarity, DismantleRewardAction,
    DismantleRewardLocation, DismantleRewardSnapshot, EQUIPMENT_FLAGS_SCHEMA_VERSION,
    GENERATED_INSTANCE_SOID_START, INVENTORY_FLAG_LOCKED, InventoryError, InventoryItemAction,
    InventoryItemLocation, InventoryItemSnapshot, ItemPlugs, MAX_ITEM_PLUGS, NewInventoryItem,
    ProfileItemAction, ProfileItemLocation, ProfileItemSnapshot, SchemaMode, character_inventory,
    dismantle_rewards, profile_item_target_exists, profile_items, schema_mode,
    set_inventory_locked_flag, validate_document_items,
};

#[cfg(test)]
pub(crate) use document::{
    FILTERED_DISMANTLE_REWARD_CAPACITY, INVENTORY_FLAG_TRACKED, LEGACY_PROFILE_ITEM_CAPACITY,
    PROFILE_ITEM_CAPACITY, profile_item_capacity,
};

pub(super) use document::KNOWN_ITEM_MEMBERS;

use crate::persistence::json_account::{
    JsonAccountError, JsonCharacterAdapter, JsonProfileAdapter, JsonProfileError,
};
use document::{
    InventoryResult, inventory_item_path, require_inventory_mutation, validate_inventory_action,
    validate_inventory_definition_hash, validate_nonnegative_i32, validate_positive_i32,
};

#[cfg(test)]
use document::{
    account_object_mut, character_object, character_object_mut, encode_plugs,
    ensure_account_object, format_definition_hash_hex, format_instance_soid, inventory_array_mut,
    inventory_object_mut, profile_array_mut, read_only_schema_error, require_profile_mutation,
    require_readable_schema, validate_existing_character_inventories,
};

#[cfg(test)]
pub(in crate::app) use document::{
    allocate_instance_soid, collect_used_soids, next_available_instance_soid,
};

#[cfg(test)]
use crate::game_settings::MAX_SUPPORTED_SCHEMA;

pub(crate) fn add_profile_item(
    document: &mut Value,
    definition_hash: u32,
    quantity: i32,
) -> InventoryResult<ProfileItemLocation> {
    let adapter = load_json_profile_items(document, "/state/account/profile_items")?;
    let index = adapter.state().profile_items().len();
    let item = account_domain::ProfileItem {
        id: adapter.next_entity_id(),
        definition_hash: account_domain::DefinitionHash::new(definition_hash),
        quantity,
    };
    let (_, candidate) = adapter
        .apply_profile_item(document, account_domain::ProfileItemCommand::Add(item))
        .map_err(|error| {
            json_profile_error(
                error,
                "/state/account/profile_items/<new>",
                "/state/account/profile_items",
            )
        })?;
    *document = candidate;
    Ok(ProfileItemLocation { index })
}

pub(crate) fn apply_profile_item_action(
    document: &mut Value,
    location: ProfileItemLocation,
    action: ProfileItemAction,
) -> InventoryResult<()> {
    let row_path = format!("/state/account/profile_items/{}", location.index);
    let adapter = load_json_profile_items(document, &row_path)?;
    if document.pointer("/state/account/profile_items").is_none() {
        return Err(InventoryError::new(
            "/state/account/profile_items",
            "profile_items is missing; add an item before editing a row",
        ));
    }
    let id = adapter
        .state()
        .profile_items()
        .get(location.index)
        .map(|item| item.id)
        .ok_or_else(|| InventoryError::new(&row_path, "profile item index is out of range"))?;
    let command = match action {
        ProfileItemAction::SetDefinitionHash(hash) => {
            account_domain::ProfileItemCommand::SetDefinitionHash {
                id,
                definition_hash: account_domain::DefinitionHash::new(hash),
            }
        }
        ProfileItemAction::SetQuantity(quantity) => {
            account_domain::ProfileItemCommand::SetQuantity { id, quantity }
        }
        ProfileItemAction::Remove => account_domain::ProfileItemCommand::Remove { id },
    };
    let (_, candidate) = adapter
        .apply_profile_item(document, command)
        .map_err(|error| json_profile_error(error, &row_path, "/state/account/profile_items"))?;
    *document = candidate;
    Ok(())
}

pub(crate) fn add_dismantle_reward(
    document: &mut Value,
    definition_hash: u32,
) -> InventoryResult<DismantleRewardLocation> {
    let adapter = load_json_profile(document, "/state/account/dismantle_rewards")?;
    let index = adapter.state().dismantle_rewards().len();
    let command = account_domain::DismantleRewardCommand::AddForDefinition {
        id: adapter.next_entity_id(),
        definition_hash: account_domain::DefinitionHash::new(definition_hash),
    };
    let (_, candidate) = adapter
        .apply_dismantle_reward(document, command)
        .map_err(|error| {
            json_profile_error(
                error,
                "/state/account/dismantle_rewards/<new>",
                "/state/account/dismantle_rewards",
            )
        })?;
    validate_document_items(&candidate)?;
    *document = candidate;
    Ok(DismantleRewardLocation { index })
}

pub(crate) fn apply_dismantle_reward_action(
    document: &mut Value,
    location: DismantleRewardLocation,
    action: DismantleRewardAction,
) -> InventoryResult<()> {
    let row_path = format!("/state/account/dismantle_rewards/{}", location.index);
    let adapter = load_json_profile(document, &row_path)?;
    if document
        .pointer("/state/account/dismantle_rewards")
        .is_none()
    {
        return Err(InventoryError::new(
            "/state/account/dismantle_rewards",
            "dismantle_rewards is missing; add a policy before editing a row",
        ));
    }
    let id = adapter
        .state()
        .dismantle_rewards()
        .get(location.index)
        .map(|reward| reward.id)
        .ok_or_else(|| InventoryError::new(&row_path, "dismantle reward index is out of range"))?;
    let command = match action {
        DismantleRewardAction::Remove => account_domain::DismantleRewardCommand::Remove { id },
        DismantleRewardAction::SetPolicy {
            definition_hash,
            quantity,
            rarities,
            gear_class,
            masterworked,
        } => account_domain::DismantleRewardCommand::SetPolicy(account_domain::DismantleReward {
            id,
            definition_hash: account_domain::DefinitionHash::new(definition_hash),
            quantity,
            rarities: rarities.into_iter().map(domain_dismantle_rarity).collect(),
            gear_class: gear_class.map(domain_dismantle_gear_class),
            masterworked,
        }),
    };
    let (_, candidate) = adapter
        .apply_dismantle_reward(document, command)
        .map_err(|error| {
            json_profile_error(error, &row_path, "/state/account/dismantle_rewards")
        })?;
    validate_document_items(&candidate)?;
    *document = candidate;
    Ok(())
}

fn load_json_profile(document: &Value, fallback_path: &str) -> InventoryResult<JsonProfileAdapter> {
    JsonProfileAdapter::load(document)
        .map_err(|error| json_profile_error(error, fallback_path, fallback_path))
}

fn load_json_profile_items(
    document: &Value,
    fallback_path: &str,
) -> InventoryResult<JsonProfileAdapter> {
    JsonProfileAdapter::load_profile_items(document)
        .map_err(|error| json_profile_error(error, fallback_path, fallback_path))
}

fn json_profile_error(
    error: JsonProfileError,
    entity_path: &str,
    collection_path: &str,
) -> InventoryError {
    let (path, message): (String, String) = match error.domain_error() {
        Some(account_domain::AccountError::ReadOnly(_)) => {
            ("/version".into(), "dismantle rewards is read-only".into())
        }
        Some(account_domain::AccountError::CapacityExceeded {
            entity, capacity, ..
        }) => {
            let collection = match entity {
                account_domain::EntityKind::ProfileItem => "profile_items",
                account_domain::EntityKind::DismantleReward => "dismantle_rewards",
                account_domain::EntityKind::Character => "characters",
                account_domain::EntityKind::ItemInstance => "items",
            };
            (
                collection_path.into(),
                format!("{collection} is full for this schema (maximum {capacity})"),
            )
        }
        Some(account_domain::AccountError::InvalidDefinitionHash) => (
            format!("{entity_path}/definition_hash"),
            "the engine no-definition sentinel is not a valid authored hash".into(),
        ),
        Some(account_domain::AccountError::InvalidQuantity) => (
            format!("{entity_path}/quantity"),
            "quantity must be a positive signed 32-bit integer".into(),
        ),
        Some(account_domain::AccountError::UnsupportedDismantleFilters) => (
            entity_path.into(),
            "dismantle filters require settings schema 8".into(),
        ),
        Some(account_domain::AccountError::DuplicateDismantlePolicy) => (
            format!("{entity_path}/definition_hash"),
            "dismantle reward material and filter combinations must be unique".into(),
        ),
        Some(account_domain::AccountError::NoAvailableDismantlePolicy) => (
            collection_path.into(),
            "every supported filter combination for this material is already present".into(),
        ),
        _ => (error.path().unwrap_or(entity_path).into(), error.detail()),
    };
    InventoryError::new(path, message)
}

fn domain_dismantle_rarity(value: DismantleRarity) -> account_domain::DismantleRarity {
    match value {
        DismantleRarity::Common => account_domain::DismantleRarity::Common,
        DismantleRarity::Uncommon => account_domain::DismantleRarity::Uncommon,
        DismantleRarity::Rare => account_domain::DismantleRarity::Rare,
        DismantleRarity::Legendary => account_domain::DismantleRarity::Legendary,
        DismantleRarity::Exotic => account_domain::DismantleRarity::Exotic,
    }
}

fn domain_dismantle_gear_class(value: DismantleGearClass) -> account_domain::DismantleGearClass {
    match value {
        DismantleGearClass::Weapon => account_domain::DismantleGearClass::Weapon,
        DismantleGearClass::Armor => account_domain::DismantleGearClass::Armor,
        DismantleGearClass::Both => account_domain::DismantleGearClass::Both,
    }
}

fn json_character_error(
    error: JsonAccountError,
    entity_path: &str,
    inventory_path: &str,
) -> InventoryError {
    let (path, message): (String, String) = match error.domain_error() {
        Some(account_domain::AccountError::CapacityExceeded {
            entity: account_domain::EntityKind::ItemInstance,
            capacity,
        }) => (
            inventory_path.into(),
            format!("character inventory is full (maximum {capacity} items)"),
        ),
        Some(account_domain::AccountError::InvalidDefinitionHash) => (
            entity_path.into(),
            "the engine no-definition sentinel is not a valid authored hash".into(),
        ),
        Some(account_domain::AccountError::InvalidLevel) => (
            entity_path.into(),
            "level must be a non-negative signed 32-bit integer".into(),
        ),
        Some(account_domain::AccountError::InvalidQuantity) => (
            entity_path.into(),
            "quantity must be a positive signed 32-bit integer".into(),
        ),
        Some(account_domain::AccountError::TooManyItemPlugs { maximum }) => (
            entity_path.into(),
            format!("plugs cannot contain more than {maximum} entries"),
        ),
        Some(account_domain::AccountError::InvalidItemFlags { maximum }) => (
            entity_path.into(),
            format!("flags must be between 0 and {maximum}"),
        ),
        Some(account_domain::AccountError::InventoryReadOnly) => {
            ("/version".into(), "character inventory is read-only".into())
        }
        Some(account_domain::AccountError::EquipmentReadOnly) => {
            ("/version".into(), "equipment is read-only".into())
        }
        Some(account_domain::AccountError::EquipmentFlagsReadOnly) => {
            ("/version".into(), "equipment flags are read-only".into())
        }
        Some(account_domain::AccountError::SameCharacterMove) => (
            entity_path.into(),
            "source and destination characters must be different".into(),
        ),
        Some(account_domain::AccountError::EquipmentSlotEmpty) => {
            (entity_path.into(), "equipment slot is already empty".into())
        }
        Some(account_domain::AccountError::NoAvailableInstanceSoid) => (
            "instance_soid".into(),
            "no unused instance SOID remains at or above the requested start".into(),
        ),
        _ => (error.path().unwrap_or(entity_path).into(), error.detail()),
    };
    InventoryError::new(path, message)
}

fn json_character_error_with_required_inventory(
    error: JsonAccountError,
    entity_path: &str,
    inventory_path: &str,
    missing_message: &str,
) -> InventoryError {
    if error.is_missing_character_inventory() {
        return InventoryError::new(error.path().unwrap_or(inventory_path), missing_message);
    }
    json_character_error(error, entity_path, inventory_path)
}

fn json_character_id(
    adapter: &JsonCharacterAdapter,
    character_index: usize,
) -> InventoryResult<account_domain::EntityId> {
    adapter
        .character_id_at_index(character_index)
        .ok_or_else(|| {
            InventoryError::new(
                format!("/state/characters/{character_index}"),
                "character index is out of range",
            )
        })
}

fn json_character_inventory_length(
    adapter: &JsonCharacterAdapter,
    character_index: usize,
) -> InventoryResult<usize> {
    let character_id = json_character_id(adapter, character_index)?;
    adapter
        .state()
        .characters()
        .iter()
        .find(|character| character.id == character_id)
        .map(|character| character.inventory.len())
        .ok_or_else(|| {
            InventoryError::new(
                format!("/state/characters/{character_index}"),
                "character index is out of range",
            )
        })
}

fn json_character_inventory_item_id(
    adapter: &JsonCharacterAdapter,
    location: InventoryItemLocation,
) -> Option<account_domain::EntityId> {
    let character_id = adapter.character_id_at_index(location.character_index)?;
    adapter
        .state()
        .characters()
        .iter()
        .find(|character| character.id == character_id)?
        .inventory
        .get(location.item_index)
        .map(|item| item.id)
}

fn domain_item_plugs(plugs: ItemPlugs) -> account_domain::ItemPlugs {
    match plugs {
        ItemPlugs::NativeDefaults => account_domain::ItemPlugs::NativeDefaults,
        ItemPlugs::Authored(plugs) => account_domain::ItemPlugs::Authored(
            plugs
                .into_iter()
                .map(|plug| plug.map(account_domain::DefinitionHash::new))
                .collect(),
        ),
    }
}

pub(crate) fn add_inventory_item(
    document: &mut Value,
    character_index: usize,
    item: NewInventoryItem,
) -> InventoryResult<InventoryItemLocation> {
    require_inventory_mutation(document)?;
    require_available_definition(document, item.definition_hash)?;
    let row_path = format!("/state/characters/{character_index}/inventory/<new>");
    let inventory_path = format!("/state/characters/{character_index}/inventory");
    validate_inventory_definition_hash(
        item.definition_hash,
        &format!("{row_path}/definition_hash"),
    )?;
    validate_nonnegative_i32(item.level, &format!("{row_path}/level"))?;
    validate_positive_i32(item.quantity, &format!("{row_path}/quantity"))?;

    let adapter = JsonCharacterAdapter::load_for_inventory_add(document)
        .map_err(|error| json_character_error(error, &row_path, &inventory_path))?;
    let character_id = json_character_id(&adapter, character_index)?;
    let length = json_character_inventory_length(&adapter, character_index)?;
    let first_instance_soid =
        account_domain::InstanceSoid::try_from_u64(GENERATED_INSTANCE_SOID_START)
            .expect("the generated instance SOID range starts at a nonzero value");
    let instance_soid = adapter
        .state()
        .next_available_instance_soid(first_instance_soid)
        .map_err(|error| json_character_error(error.into(), &row_path, &inventory_path))?;
    let command = account_domain::CharacterCommand::AddInventoryItem {
        character_id,
        item: account_domain::ItemInstance {
            id: adapter.next_entity_id(),
            instance_soid,
            definition_hash: account_domain::DefinitionHash::new(item.definition_hash),
            level: item.level,
            quantity: item.quantity,
            plugs: account_domain::ItemPlugs::NativeDefaults,
            flags: None,
        },
    };
    let (_, candidate, _) = adapter
        .apply(document, command)
        .map_err(|error| json_character_error(error, &row_path, &inventory_path))?;
    *document = candidate;
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
    if let InventoryItemAction::SetDefinitionHash(hash) = &action {
        require_available_definition(document, *hash)?;
    }
    let row_path = inventory_item_path(location);
    let inventory_path = format!("/state/characters/{}/inventory", location.character_index);
    let adapter = JsonCharacterAdapter::load_inventory_item(document, location.character_index)
        .map_err(|error| {
            json_character_error_with_required_inventory(
                error,
                &row_path,
                &inventory_path,
                "character inventory is missing; add an item before editing a row",
            )
        })?;
    let _ = json_character_id(&adapter, location.character_index)?;
    let item_id = json_character_inventory_item_id(&adapter, location)
        .ok_or_else(|| InventoryError::new(&row_path, "inventory item index is out of range"))?;
    validate_inventory_action(location, &action, schema_mode(document).item_flag_mask())?;
    let command = match action {
        InventoryItemAction::SetDefinitionHash(hash) => {
            account_domain::CharacterCommand::UpdateInventoryItem {
                item_id,
                update: account_domain::ItemUpdate::SetDefinitionHash(
                    account_domain::DefinitionHash::new(hash),
                ),
            }
        }
        InventoryItemAction::SetLevel(level) => {
            account_domain::CharacterCommand::UpdateInventoryItem {
                item_id,
                update: account_domain::ItemUpdate::SetLevel(level),
            }
        }
        InventoryItemAction::SetQuantity(quantity) => {
            account_domain::CharacterCommand::UpdateInventoryItem {
                item_id,
                update: account_domain::ItemUpdate::SetQuantity(quantity),
            }
        }
        InventoryItemAction::SetPlugs(plugs) => {
            account_domain::CharacterCommand::UpdateInventoryItem {
                item_id,
                update: account_domain::ItemUpdate::SetPlugs(domain_item_plugs(plugs)),
            }
        }
        InventoryItemAction::SetFlags(flags) => {
            account_domain::CharacterCommand::UpdateInventoryItem {
                item_id,
                update: account_domain::ItemUpdate::SetFlags(flags.map(u32::from)),
            }
        }
        InventoryItemAction::Remove => {
            account_domain::CharacterCommand::RemoveInventoryItem { item_id }
        }
    };
    let (_, candidate, _) = adapter
        .apply(document, command)
        .map_err(|error| json_character_error(error, &row_path, &inventory_path))?;
    *document = candidate;
    Ok(())
}

/// Removes a selected set of stored rows in one character-aggregate transaction.
pub(crate) fn remove_character_inventory_items(
    document: &mut Value,
    character_index: usize,
    item_indices: impl IntoIterator<Item = usize>,
) -> InventoryResult<usize> {
    require_inventory_mutation(document)?;
    let indices = item_indices.into_iter().collect::<BTreeSet<_>>();
    let inventory_path = format!("/state/characters/{character_index}/inventory");
    let adapter =
        JsonCharacterAdapter::load_inventory_item(document, character_index).map_err(|error| {
            json_character_error_with_required_inventory(
                error,
                &inventory_path,
                &inventory_path,
                "character inventory is missing",
            )
        })?;
    let character_id = json_character_id(&adapter, character_index)?;
    let character = adapter
        .state()
        .characters()
        .iter()
        .find(|character| character.id == character_id)
        .expect("the adapter character ID belongs to its loaded state");
    let commands = indices
        .iter()
        .map(|item_index| {
            character
                .inventory
                .get(*item_index)
                .map(
                    |item| account_domain::CharacterCommand::RemoveInventoryItem {
                        item_id: item.id,
                    },
                )
                .ok_or_else(|| {
                    InventoryError::new(
                        format!("{inventory_path}/{item_index}"),
                        "inventory item index is out of range",
                    )
                })
        })
        .collect::<InventoryResult<Vec<_>>>()?;
    if commands.is_empty() {
        return Ok(0);
    }
    let removed = commands.len();
    let (_, candidate, _) = adapter
        .apply(document, account_domain::CharacterCommand::Batch(commands))
        .map_err(|error| json_character_error(error, &inventory_path, &inventory_path))?;
    *document = candidate;
    Ok(removed)
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
    if !schema_mode(document)
        .equipment_slots()
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

    let row_path = inventory_item_path(location);
    let inventory_path = format!("/state/characters/{}/inventory", location.character_index);
    let equipment_path = format!("/state/characters/{}/equipment", location.character_index);
    let slot_path = format!("{equipment_path}/{slot}");
    let adapter =
        JsonCharacterAdapter::load_inventory_swap(document, location.character_index, slot)
            .map_err(|error| {
                json_character_error_with_required_inventory(
                    error,
                    &slot_path,
                    &inventory_path,
                    "character inventory is missing; add an item before equipping a row",
                )
            })?;
    let _ = json_character_id(&adapter, location.character_index)?;
    let item_id = json_character_inventory_item_id(&adapter, location)
        .ok_or_else(|| InventoryError::new(&row_path, "inventory item index is out of range"))?;
    let command = account_domain::CharacterCommand::SwapInventoryItemWithEquipment {
        item_id,
        slot: account_domain::EquipmentSlot::new(slot),
    };
    let (_, candidate, result) = adapter
        .apply(document, command)
        .map_err(|error| json_character_error(error, &slot_path, &inventory_path))?;
    let account_domain::CharacterCommandResult::EquipmentSwapped { replaced } = result else {
        unreachable!("a swap command must return its replacement status")
    };
    *document = candidate;
    Ok(replaced)
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

    let row_path = inventory_item_path(location);
    let destination_inventory_path =
        format!("/state/characters/{destination_character_index}/inventory");
    let adapter = JsonCharacterAdapter::load_inventory_move(
        document,
        location.character_index,
        destination_character_index,
    )
    .map_err(|error| {
        json_character_error_with_required_inventory(
            error,
            &row_path,
            &destination_inventory_path,
            "character inventory is missing; add an item before moving a row",
        )
    })?;
    let _ = json_character_id(&adapter, location.character_index)?;
    let item_id = json_character_inventory_item_id(&adapter, location)
        .ok_or_else(|| InventoryError::new(&row_path, "inventory item index is out of range"))?;
    let destination_character_id = json_character_id(&adapter, destination_character_index)?;
    let destination_length =
        json_character_inventory_length(&adapter, destination_character_index)?;
    let command = account_domain::CharacterCommand::MoveInventoryItem {
        item_id,
        destination_character_id,
    };
    let (_, candidate, _) = adapter
        .apply(document, command)
        .map_err(|error| json_character_error(error, &row_path, &destination_inventory_path))?;
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
    if !schema_mode(document)
        .equipment_slots()
        .iter()
        .any(|(known_slot, _, _)| *known_slot == slot)
    {
        return Err(InventoryError::new(
            format!("/state/characters/{character_index}/equipment/{slot}"),
            "unknown equipment slot",
        ));
    }

    let equipment_path = format!("/state/characters/{character_index}/equipment");
    let slot_path = format!("{equipment_path}/{slot}");
    let inventory_path = format!("/state/characters/{character_index}/inventory");
    let adapter =
        JsonCharacterAdapter::load_inventory_equipment_slot(document, character_index, slot)
            .map_err(|error| json_character_error(error, &slot_path, &inventory_path))?;
    let character_id = json_character_id(&adapter, character_index)?;
    let command = account_domain::CharacterCommand::MoveEquipmentItemToInventory {
        character_id,
        slot: account_domain::EquipmentSlot::new(slot),
    };
    let (_, candidate, _) = adapter
        .apply(document, command)
        .map_err(|error| json_character_error(error, &slot_path, &inventory_path))?;
    *document = candidate;
    Ok(())
}

#[cfg(test)]
mod character_domain_parity_tests;
#[cfg(test)]
mod domain_parity_tests;
#[cfg(test)]
mod legacy_character_tests;
#[cfg(test)]
mod legacy_profile_tests;
#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) use crate::account_contract::INVENTORY_FLAG_MASK;

fn require_available_definition(document: &Value, hash: u32) -> InventoryResult<()> {
    if crate::account_contract::definition_available(
        u64::from(hash),
        schema_mode(document).supports_v13(),
    ) {
        Ok(())
    } else {
        Err(InventoryError::new(
            "definition_hash",
            "The emote wheel requires JSON schema 13 or newer",
        ))
    }
}

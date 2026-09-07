//! Whole-document validation and mutation precondition helpers.

use serde_json::{Map, Value};

use crate::game_settings::MIN_SUPPORTED_SCHEMA;

use super::{
    fields::{
        inventory_item_path, validate_inventory_definition_hash, validate_nonnegative_i32,
        validate_plug_snapshot, validate_positive_i32,
    },
    model::{InventoryError, InventoryItemAction, InventoryItemLocation, InventoryResult},
    parsing::{
        optional_object_member, optional_root_object_member, parse_nonzero_soid, profile_items,
        validate_dismantle_rewards, validate_existing_character_inventories,
    },
    schema::{KNOWN_ITEM_MEMBERS, SchemaMode, schema_mode},
    soids::validate_unique_soids,
};

pub(crate) fn validate_document_items(document: &Value) -> InventoryResult<()> {
    let mode = match schema_mode(document) {
        SchemaMode::MissingOrInvalid => {
            return Err(InventoryError::new(
                "/version",
                "settings schema version is missing or invalid",
            ));
        }
        mode @ SchemaMode::Future(_) => mode,
        SchemaMode::Unsupported(version) => {
            return Err(InventoryError::new(
                "/version",
                format!(
                    "settings schema {version} predates supported schema {MIN_SUPPORTED_SCHEMA}; inventory is read-only"
                ),
            ));
        }
        mode => mode,
    };

    validate_required_account_primary_soid(document)?;
    let _ = profile_items(document)?;
    validate_existing_character_inventories(document, mode)?;
    if mode.supports_dismantle_rewards() && !mode.is_future() {
        validate_dismantle_rewards(document, mode)?;
    }
    if let SchemaMode::Inventory(version) = mode {
        validate_known_schema_item_members(document, version)?;
    }
    validate_unique_soids(document)
}

pub(in crate::app::inventory) fn validate_known_schema_item_members(
    document: &Value,
    schema_version: u64,
) -> InventoryResult<()> {
    let Some(characters) = document
        .pointer("/state/characters")
        .and_then(Value::as_array)
    else {
        return Ok(());
    };
    for (character_index, character) in characters.iter().enumerate() {
        let Some(character) = character.as_object() else {
            continue;
        };
        if let Some(equipment) = character.get("equipment").and_then(Value::as_object) {
            for (slot, item) in equipment {
                if let Some(item) = item.as_object() {
                    validate_known_item_members(
                        item,
                        &format!("/state/characters/{character_index}/equipment/{slot}"),
                        KNOWN_ITEM_MEMBERS,
                        schema_version,
                    )?;
                }
            }
        }
        if let Some(inventory) = character.get("inventory").and_then(Value::as_array) {
            for (item_index, item) in inventory.iter().enumerate() {
                if let Some(item) = item.as_object() {
                    validate_known_item_members(
                        item,
                        &format!("/state/characters/{character_index}/inventory/{item_index}"),
                        KNOWN_ITEM_MEMBERS,
                        schema_version,
                    )?;
                }
            }
        }
    }
    Ok(())
}

pub(in crate::app::inventory) fn validate_known_item_members(
    item: &Map<String, Value>,
    path: &str,
    known_members: &[&str],
    schema_version: u64,
) -> InventoryResult<()> {
    if let Some(key) = item
        .keys()
        .find(|key| !known_members.contains(&key.as_str()))
    {
        Err(InventoryError::new(
            format!("{path}/{key}"),
            format!(
                "schema {schema_version} item member {key:?} is preserved by Sundial but is not accepted by Sunrise"
            ),
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
pub(in crate::app::inventory) fn character_object_mut(
    document: &mut Value,
    character_index: usize,
) -> InventoryResult<&mut Map<String, Value>> {
    document
        .get_mut("state")
        .and_then(Value::as_object_mut)
        .and_then(|state| state.get_mut("characters"))
        .and_then(Value::as_array_mut)
        .and_then(|characters| characters.get_mut(character_index))
        .and_then(Value::as_object_mut)
        .ok_or_else(|| {
            InventoryError::new(
                format!("/state/characters/{character_index}"),
                "character object disappeared before mutation",
            )
        })
}

#[cfg(test)]
pub(in crate::app::inventory) fn ensure_account_object(document: &Value) -> InventoryResult<()> {
    let state = document
        .get("state")
        .and_then(Value::as_object)
        .ok_or_else(|| InventoryError::new("/state", "state must be an object"))?;
    state
        .get("account")
        .and_then(Value::as_object)
        .map(|_| ())
        .ok_or_else(|| InventoryError::new("/state/account", "account must be an object"))
}

#[cfg(test)]
pub(in crate::app::inventory) fn account_object_mut(
    document: &mut Value,
) -> InventoryResult<&mut Map<String, Value>> {
    document
        .get_mut("state")
        .and_then(Value::as_object_mut)
        .and_then(|state| state.get_mut("account"))
        .and_then(Value::as_object_mut)
        .ok_or_else(|| {
            InventoryError::new(
                "/state/account",
                "account object disappeared before mutation",
            )
        })
}

#[cfg(test)]
pub(in crate::app::inventory) fn profile_array_mut(
    document: &mut Value,
) -> InventoryResult<&mut Vec<Value>> {
    account_object_mut(document)?
        .get_mut("profile_items")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| {
            InventoryError::new(
                "/state/account/profile_items",
                "profile_items array disappeared before mutation",
            )
        })
}

#[cfg(test)]
pub(in crate::app::inventory) fn inventory_array_mut(
    document: &mut Value,
    character_index: usize,
) -> InventoryResult<&mut Vec<Value>> {
    character_object_mut(document, character_index)?
        .get_mut("inventory")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| {
            InventoryError::new(
                format!("/state/characters/{character_index}/inventory"),
                "inventory array disappeared before mutation",
            )
        })
}

#[cfg(test)]
pub(in crate::app::inventory) fn inventory_object_mut(
    items: &mut [Value],
    item_index: usize,
) -> &mut Map<String, Value> {
    items[item_index]
        .as_object_mut()
        .expect("inventory row shape was validated before mutation")
}

pub(in crate::app::inventory) fn validate_inventory_action(
    location: InventoryItemLocation,
    action: &InventoryItemAction,
    flag_mask: u8,
) -> InventoryResult<()> {
    let path = inventory_item_path(location);
    match action {
        InventoryItemAction::SetDefinitionHash(hash) => {
            validate_inventory_definition_hash(*hash, &format!("{path}/definition_hash"))
        }
        InventoryItemAction::SetLevel(level) => {
            validate_nonnegative_i32(*level, &format!("{path}/level"))
        }
        InventoryItemAction::SetQuantity(quantity) => {
            validate_positive_i32(*quantity, &format!("{path}/quantity"))
        }
        InventoryItemAction::SetPlugs(plugs) => {
            validate_plug_snapshot(plugs, &format!("{path}/plugs"))
        }
        InventoryItemAction::SetFlags(Some(flags)) if *flags > flag_mask => {
            Err(InventoryError::new(
                format!("{path}/flags"),
                format!("flags must be between 0 and {flag_mask}"),
            ))
        }
        InventoryItemAction::SetFlags(_) | InventoryItemAction::Remove => Ok(()),
    }
}

pub(in crate::app::inventory) fn validate_required_account_primary_soid(
    document: &Value,
) -> InventoryResult<()> {
    let state = optional_root_object_member(document, "state", "/state")?
        .ok_or_else(|| InventoryError::new("/state", "settings state is missing"))?;
    let account = optional_object_member(state, "account", "/state/account")?
        .ok_or_else(|| InventoryError::new("/state/account", "account is missing"))?;
    let path = "/state/account/primary_soid";
    account
        .get("primary_soid")
        .ok_or_else(|| InventoryError::new(path, "account is missing primary_soid"))
        .and_then(|value| parse_nonzero_soid(value, path))?;
    Ok(())
}

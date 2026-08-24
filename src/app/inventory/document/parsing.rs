//! Read-only inventory document parsing and structural validation.

use std::collections::BTreeSet;

use serde_json::{Map, Value};

use crate::hash::parse_unsigned_value;

use super::{
    fields::{
        inventory_item_path, validate_inventory_definition_hash, validate_nonnegative_i32,
        validate_positive_i32,
    },
    model::{
        DismantleGearClass, DismantleRarity, DismantleRewardLocation, DismantleRewardSnapshot,
        InventoryError, InventoryItemLocation, InventoryItemSnapshot, InventoryResult, ItemPlugs,
        ProfileItemLocation, ProfileItemSnapshot,
    },
    schema::{
        CHARACTER_INVENTORY_CAPACITY, FILTERED_DISMANTLE_REWARD_CAPACITY,
        FILTERED_DISMANTLE_REWARDS_SCHEMA_VERSION, INVENTORY_FLAG_MASK,
        LEGACY_DISMANTLE_REWARD_CAPACITY, MAX_ITEM_PLUGS, SchemaMode, require_readable_schema,
    },
};

pub(crate) fn profile_items(document: &Value) -> InventoryResult<Option<Vec<ProfileItemSnapshot>>> {
    let mode = require_readable_schema(document)?;
    let Some(state) = optional_root_object_member(document, "state", "/state")? else {
        return Ok(None);
    };
    let Some(account) = optional_object_member(state, "account", "/state/account")? else {
        return Ok(None);
    };
    let Some(value) = account.get("profile_items") else {
        return Ok(None);
    };
    let array = value.as_array().ok_or_else(|| {
        InventoryError::new(
            "/state/account/profile_items",
            "profile_items must be an array",
        )
    })?;
    if mode.enforces_profile_item_capacity()
        && let Some(capacity) = mode.profile_item_capacity()
        && array.len() > capacity
    {
        return Err(InventoryError::new(
            "/state/account/profile_items",
            format!(
                "profile_items contains {} rows, but schema {} permits at most {capacity}",
                array.len(),
                mode.version().unwrap_or_default()
            ),
        ));
    }
    array
        .iter()
        .enumerate()
        .map(|(index, value)| parse_profile_item(value, index))
        .collect::<InventoryResult<Vec<_>>>()
        .map(Some)
}

pub(crate) fn profile_item_target_exists(document: &Value) -> InventoryResult<bool> {
    require_readable_schema(document)?;
    let Some(state) = optional_root_object_member(document, "state", "/state")? else {
        return Ok(false);
    };
    Ok(optional_object_member(state, "account", "/state/account")?.is_some())
}

pub(crate) fn dismantle_rewards(
    document: &Value,
) -> InventoryResult<Option<Vec<DismantleRewardSnapshot>>> {
    let mode = require_readable_schema(document)?;
    if !mode.supports_dismantle_rewards() || mode.is_future() {
        return Ok(None);
    }
    let Some(value) = document.pointer("/state/account/dismantle_rewards") else {
        return Ok(None);
    };
    validate_dismantle_rewards(document, mode)?;
    let rewards = value
        .as_array()
        .expect("dismantle rewards were validated as an array");
    rewards
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let reward = value
                .as_object()
                .expect("dismantle reward was validated as an object");
            let definition_hash = reward
                .get("definition_hash")
                .and_then(parse_unsigned_value)
                .and_then(|hash| u32::try_from(hash).ok())
                .expect("dismantle reward hash was validated");
            let quantity = reward
                .get("quantity")
                .and_then(Value::as_i64)
                .and_then(|quantity| i32::try_from(quantity).ok())
                .expect("dismantle reward quantity was validated");
            let rarities = if mode.supports_filtered_dismantle_rewards() {
                reward
                    .get("rarity")
                    .map(dismantle_rarity_values)
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
            let gear_class = if mode.supports_filtered_dismantle_rewards() {
                match reward.get("class").and_then(Value::as_str) {
                    Some("weapon") => Some(DismantleGearClass::Weapon),
                    Some("armor") => Some(DismantleGearClass::Armor),
                    _ => None,
                }
            } else {
                None
            };
            let masterworked = mode
                .supports_filtered_dismantle_rewards()
                .then(|| reward.get("masterworked").and_then(Value::as_bool))
                .flatten();
            Ok(DismantleRewardSnapshot {
                location: DismantleRewardLocation { index },
                definition_hash,
                quantity,
                rarities,
                gear_class,
                masterworked,
            })
        })
        .collect::<InventoryResult<Vec<_>>>()
        .map(Some)
}

pub(in crate::app::inventory) fn dismantle_rarity_values(value: &Value) -> Vec<DismantleRarity> {
    if let Some(token) = value.as_str() {
        return DismantleRarity::from_token(token).into_iter().collect();
    }
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter_map(DismantleRarity::from_token)
        .collect()
}

pub(crate) fn character_inventory(
    document: &Value,
    character_index: usize,
) -> InventoryResult<Option<Vec<InventoryItemSnapshot>>> {
    let mode = require_readable_schema(document)?;
    let character = character_object(document, character_index)?;
    let Some(value) = character.get("inventory") else {
        return Ok(None);
    };
    parse_character_inventory(value, character_index, mode).map(Some)
}

pub(in crate::app::inventory) fn optional_object_member<'a>(
    object: &'a Map<String, Value>,
    key: &str,
    path: &str,
) -> InventoryResult<Option<&'a Map<String, Value>>> {
    object
        .get(key)
        .map(|value| {
            value
                .as_object()
                .ok_or_else(|| InventoryError::new(path, format!("{key} must be an object")))
        })
        .transpose()
}

pub(in crate::app::inventory) fn optional_root_object_member<'a>(
    document: &'a Value,
    key: &str,
    path: &str,
) -> InventoryResult<Option<&'a Map<String, Value>>> {
    let root = document
        .as_object()
        .ok_or_else(|| InventoryError::new("", "settings document must be an object"))?;
    optional_object_member(root, key, path)
}

pub(in crate::app::inventory) fn parse_profile_item(
    value: &Value,
    index: usize,
) -> InventoryResult<ProfileItemSnapshot> {
    let path = format!("/state/account/profile_items/{index}");
    let object = value
        .as_object()
        .ok_or_else(|| InventoryError::new(&path, "profile item must be an object"))?;
    let definition_hash = parse_hash_field(object, "definition_hash", &path)?;
    let quantity = parse_positive_i32_field(object, "quantity", &path)?;
    Ok(ProfileItemSnapshot {
        location: ProfileItemLocation { index },
        definition_hash,
        quantity,
    })
}

pub(in crate::app::inventory) fn parse_character_inventory(
    value: &Value,
    character_index: usize,
    mode: SchemaMode,
) -> InventoryResult<Vec<InventoryItemSnapshot>> {
    let path = format!("/state/characters/{character_index}/inventory");
    let array = value
        .as_array()
        .ok_or_else(|| InventoryError::new(&path, "inventory must be an array"))?;
    if mode.enforces_character_inventory_capacity() && array.len() > CHARACTER_INVENTORY_CAPACITY {
        return Err(InventoryError::new(
            &path,
            format!(
                "inventory contains {} items, but at most {CHARACTER_INVENTORY_CAPACITY} are permitted",
                array.len()
            ),
        ));
    }
    array
        .iter()
        .enumerate()
        .map(|(item_index, value)| parse_inventory_item(value, character_index, item_index))
        .collect()
}

pub(in crate::app::inventory) fn parse_inventory_item(
    value: &Value,
    character_index: usize,
    item_index: usize,
) -> InventoryResult<InventoryItemSnapshot> {
    let location = InventoryItemLocation {
        character_index,
        item_index,
    };
    let path = inventory_item_path(location);
    let object = value
        .as_object()
        .ok_or_else(|| InventoryError::new(&path, "inventory item must be an object"))?;
    let instance_soid = object
        .get("instance_soid")
        .ok_or_else(|| InventoryError::new(&path, "inventory item is missing instance_soid"))
        .and_then(|value| parse_nonzero_soid(value, &format!("{path}/instance_soid")))?;
    let definition_hash = parse_hash_field(object, "definition_hash", &path)?;
    let level = parse_nonnegative_i32_field(object, "level", &path)?;
    let quantity = parse_positive_i32_field(object, "quantity", &path)?;
    let plugs = object
        .get("plugs")
        .ok_or_else(|| InventoryError::new(&path, "inventory item is missing plugs"))
        .and_then(|value| parse_plugs(value, &format!("{path}/plugs")))?;
    let flags = object
        .get("flags")
        .map(|value| parse_flags(value, &format!("{path}/flags")))
        .transpose()?;
    Ok(InventoryItemSnapshot {
        location,
        instance_soid,
        definition_hash,
        level,
        quantity,
        plugs,
        flags,
    })
}

pub(in crate::app::inventory) fn validate_existing_character_inventories(
    document: &Value,
    mode: SchemaMode,
) -> InventoryResult<()> {
    let Some(state) = optional_root_object_member(document, "state", "/state")? else {
        return Ok(());
    };
    let Some(value) = state.get("characters") else {
        return Ok(());
    };
    let characters = value
        .as_array()
        .ok_or_else(|| InventoryError::new("/state/characters", "characters must be an array"))?;
    for (character_index, value) in characters.iter().enumerate() {
        let path = format!("/state/characters/{character_index}");
        let character = value
            .as_object()
            .ok_or_else(|| InventoryError::new(&path, "character must be an object"))?;
        if let Some(inventory) = character.get("inventory") {
            parse_character_inventory(inventory, character_index, mode)?;
        }
    }
    Ok(())
}

pub(in crate::app::inventory) fn validate_dismantle_rewards(
    document: &Value,
    mode: SchemaMode,
) -> InventoryResult<()> {
    let path = "/state/account/dismantle_rewards";
    let Some(value) = document.pointer(path) else {
        return Ok(());
    };
    let rewards = value
        .as_array()
        .ok_or_else(|| InventoryError::new(path, "dismantle_rewards must be an array"))?;
    let filtered = mode
        .version()
        .is_some_and(|version| version >= FILTERED_DISMANTLE_REWARDS_SCHEMA_VERSION);
    let capacity = if filtered {
        FILTERED_DISMANTLE_REWARD_CAPACITY
    } else {
        LEGACY_DISMANTLE_REWARD_CAPACITY
    };
    if rewards.len() > capacity {
        return Err(InventoryError::new(
            path,
            format!("dismantle_rewards cannot contain more than {capacity} entries"),
        ));
    }

    let mut policies = BTreeSet::new();
    for (index, value) in rewards.iter().enumerate() {
        let reward_path = format!("{path}/{index}");
        let reward = value.as_object().ok_or_else(|| {
            InventoryError::new(&reward_path, "dismantle reward must be an object")
        })?;

        let hash_path = format!("{reward_path}/definition_hash");
        let hash = reward
            .get("definition_hash")
            .ok_or_else(|| {
                InventoryError::new(&hash_path, "dismantle reward is missing definition_hash")
            })
            .and_then(|value| {
                parse_unsigned_value(value).ok_or_else(|| {
                    InventoryError::new(
                        &hash_path,
                        "definition_hash must be an unsigned integer or a 0x hex string",
                    )
                })
            })
            .and_then(|value| {
                u32::try_from(value).map_err(|_| {
                    InventoryError::new(
                        &hash_path,
                        "definition_hash must fit in an unsigned 32-bit value",
                    )
                })
            })?;
        if hash == 0 {
            return Err(InventoryError::new(
                &hash_path,
                "definition_hash must be nonzero",
            ));
        }
        validate_inventory_definition_hash(hash, &hash_path)?;

        let rarity_mask = if filtered {
            reward
                .get("rarity")
                .map(|value| validate_dismantle_rarity(value, &format!("{reward_path}/rarity")))
                .transpose()?
                .unwrap_or_default()
        } else {
            0
        };
        let class_mask = if filtered {
            match reward.get("class") {
                None => 0,
                Some(Value::String(class)) if class == "weapon" => 1,
                Some(Value::String(class)) if class == "armor" => 2,
                Some(_) => {
                    return Err(InventoryError::new(
                        format!("{reward_path}/class"),
                        "class must be \"weapon\" or \"armor\"",
                    ));
                }
            }
        } else {
            0
        };
        let masterwork_filter = if filtered {
            match reward.get("masterworked") {
                None => 0,
                Some(Value::Bool(true)) => 1,
                Some(Value::Bool(false)) => 2,
                Some(_) => {
                    return Err(InventoryError::new(
                        format!("{reward_path}/masterworked"),
                        "masterworked must be true or false",
                    ));
                }
            }
        } else {
            0
        };
        if !policies.insert((hash, rarity_mask, class_mask, masterwork_filter)) {
            return Err(InventoryError::new(
                &hash_path,
                if filtered {
                    "dismantle reward material and filter combinations must be unique"
                } else {
                    "dismantle reward definition_hash values must be unique"
                },
            ));
        }

        let quantity_path = format!("{reward_path}/quantity");
        reward
            .get("quantity")
            .ok_or_else(|| {
                InventoryError::new(&quantity_path, "dismantle reward is missing quantity")
            })
            .and_then(|value| {
                value.as_u64().ok_or_else(|| {
                    InventoryError::new(
                        &quantity_path,
                        "quantity must be a positive 32-bit integer",
                    )
                })
            })
            .and_then(|quantity| {
                if (1..=i32::MAX as u64).contains(&quantity) {
                    Ok(quantity)
                } else {
                    Err(InventoryError::new(
                        &quantity_path,
                        "quantity must be a positive 32-bit integer",
                    ))
                }
            })?;
    }
    Ok(())
}

pub(in crate::app::inventory) fn validate_dismantle_rarity(
    value: &Value,
    path: &str,
) -> InventoryResult<u8> {
    fn rarity_bit(value: &Value, path: &str) -> InventoryResult<u8> {
        let name = value
            .as_str()
            .ok_or_else(|| InventoryError::new(path, "rarity must contain rarity names"))?;
        DismantleRarity::from_token(name)
            .map(DismantleRarity::bit)
            .ok_or_else(|| {
                InventoryError::new(
                    path,
                    "rarity must be common, uncommon, rare, legendary, or exotic",
                )
            })
    }

    if value.is_string() {
        return rarity_bit(value, path);
    }
    let values = value
        .as_array()
        .ok_or_else(|| InventoryError::new(path, "rarity must be a name or an array of names"))?;
    if values.is_empty() {
        return Err(InventoryError::new(
            path,
            "rarity arrays must contain at least one name",
        ));
    }
    let mut mask = 0;
    for (index, value) in values.iter().enumerate() {
        let value_path = format!("{path}/{index}");
        let bit = rarity_bit(value, &value_path)?;
        if mask & bit != 0 {
            return Err(InventoryError::new(
                value_path,
                "rarity arrays cannot contain duplicate names",
            ));
        }
        mask |= bit;
    }
    Ok(mask)
}

pub(in crate::app::inventory) fn character_object(
    document: &Value,
    character_index: usize,
) -> InventoryResult<&Map<String, Value>> {
    let state = document
        .get("state")
        .and_then(Value::as_object)
        .ok_or_else(|| InventoryError::new("/state", "state must be an object"))?;
    let characters = state
        .get("characters")
        .and_then(Value::as_array)
        .ok_or_else(|| InventoryError::new("/state/characters", "characters must be an array"))?;
    characters
        .get(character_index)
        .ok_or_else(|| {
            InventoryError::new(
                format!("/state/characters/{character_index}"),
                "character index is out of range",
            )
        })?
        .as_object()
        .ok_or_else(|| {
            InventoryError::new(
                format!("/state/characters/{character_index}"),
                "character must be an object",
            )
        })
}

pub(in crate::app::inventory) fn parse_hash_field(
    object: &Map<String, Value>,
    key: &str,
    base_path: &str,
) -> InventoryResult<u32> {
    let path = format!("{base_path}/{key}");
    let value = object
        .get(key)
        .ok_or_else(|| InventoryError::new(&path, format!("item is missing {key}")))?;
    let raw = parse_unsigned_value(value).ok_or_else(|| {
        InventoryError::new(&path, "hash must be an unsigned integer or 0x hex string")
    })?;
    let hash = u32::try_from(raw)
        .map_err(|_| InventoryError::new(&path, "hash must fit in an unsigned 32-bit value"))?;
    validate_inventory_definition_hash(hash, &path)?;
    Ok(hash)
}

pub(in crate::app::inventory) fn parse_nonzero_soid(
    value: &Value,
    path: &str,
) -> InventoryResult<u64> {
    parse_unsigned_value(value)
        .filter(|soid| *soid != 0)
        .ok_or_else(|| {
            InventoryError::new(
                path,
                "SOID must be a nonzero unsigned integer or 0x hex string",
            )
        })
}

pub(in crate::app::inventory) fn parse_nonnegative_i32_field(
    object: &Map<String, Value>,
    key: &str,
    base_path: &str,
) -> InventoryResult<i32> {
    let path = format!("{base_path}/{key}");
    let value = object
        .get(key)
        .ok_or_else(|| InventoryError::new(&path, format!("item is missing {key}")))?;
    let value = value.as_i64().ok_or_else(|| {
        InventoryError::new(
            &path,
            format!("{key} must be a non-negative 32-bit integer"),
        )
    })?;
    let value = i32::try_from(value).map_err(|_| {
        InventoryError::new(&path, format!("{key} must fit in a signed 32-bit integer"))
    })?;
    validate_nonnegative_i32(value, &path)?;
    Ok(value)
}

pub(in crate::app::inventory) fn parse_positive_i32_field(
    object: &Map<String, Value>,
    key: &str,
    base_path: &str,
) -> InventoryResult<i32> {
    let path = format!("{base_path}/{key}");
    let value = object
        .get(key)
        .ok_or_else(|| InventoryError::new(&path, format!("item is missing {key}")))?;
    let value = value.as_i64().ok_or_else(|| {
        InventoryError::new(&path, format!("{key} must be a positive 32-bit integer"))
    })?;
    let value = i32::try_from(value).map_err(|_| {
        InventoryError::new(&path, format!("{key} must fit in a signed 32-bit integer"))
    })?;
    validate_positive_i32(value, &path)?;
    Ok(value)
}

pub(in crate::app::inventory) fn parse_plugs(
    value: &Value,
    path: &str,
) -> InventoryResult<ItemPlugs> {
    if value.is_null() {
        return Ok(ItemPlugs::NativeDefaults);
    }
    let array = value
        .as_array()
        .ok_or_else(|| InventoryError::new(path, "plugs must be null or an array"))?;
    if array.len() > MAX_ITEM_PLUGS {
        return Err(InventoryError::new(
            path,
            format!("plugs cannot contain more than {MAX_ITEM_PLUGS} entries"),
        ));
    }
    let plugs = array
        .iter()
        .enumerate()
        .map(|(index, value)| {
            if value.is_null() {
                return Ok(None);
            }
            let entry_path = format!("{path}/{index}");
            let raw = parse_unsigned_value(value).ok_or_else(|| {
                InventoryError::new(
                    &entry_path,
                    "plug hash must be null, an unsigned integer, or a 0x hex string",
                )
            })?;
            let hash = u32::try_from(raw).map_err(|_| {
                InventoryError::new(
                    &entry_path,
                    "plug hash must fit in an unsigned 32-bit value",
                )
            })?;
            validate_inventory_definition_hash(hash, &entry_path)?;
            Ok(Some(hash))
        })
        .collect::<InventoryResult<Vec<_>>>()?;
    Ok(ItemPlugs::Authored(plugs))
}

pub(in crate::app::inventory) fn parse_flags(value: &Value, path: &str) -> InventoryResult<u8> {
    parse_unsigned_value(value)
        .filter(|flags| *flags <= u64::from(INVENTORY_FLAG_MASK))
        .map(|flags| flags as u8)
        .ok_or_else(|| {
            InventoryError::new(
                path,
                format!("flags must be a whole number between 0 and {INVENTORY_FLAG_MASK}"),
            )
        })
}

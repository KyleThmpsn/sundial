//! Authored field validation, normalization, and JSON encoding helpers.

use serde_json::Value;

use super::{
    model::{InventoryError, InventoryItemLocation, InventoryResult, ItemPlugs},
    schema::{MAX_ITEM_PLUGS, NO_DEFINITION_HASH},
};

pub(in crate::app::inventory) fn validate_positive_i32(
    value: i32,
    path: &str,
) -> InventoryResult<()> {
    if value > 0 {
        Ok(())
    } else {
        Err(InventoryError::new(
            path,
            "quantity must be a positive signed 32-bit integer",
        ))
    }
}

pub(in crate::app::inventory) fn validate_nonnegative_i32(
    value: i32,
    path: &str,
) -> InventoryResult<()> {
    if value >= 0 {
        Ok(())
    } else {
        Err(InventoryError::new(
            path,
            "level must be a non-negative signed 32-bit integer",
        ))
    }
}

pub(in crate::app::inventory) fn validate_inventory_definition_hash(
    hash: u32,
    path: &str,
) -> InventoryResult<()> {
    if hash == NO_DEFINITION_HASH {
        Err(InventoryError::new(
            path,
            "the engine no-definition sentinel is not a valid authored hash",
        ))
    } else {
        Ok(())
    }
}

pub(in crate::app::inventory) fn validate_plug_snapshot(
    plugs: &ItemPlugs,
    path: &str,
) -> InventoryResult<()> {
    let ItemPlugs::Authored(plugs) = plugs else {
        return Ok(());
    };
    if plugs.len() > MAX_ITEM_PLUGS {
        return Err(InventoryError::new(
            path,
            format!("plugs cannot contain more than {MAX_ITEM_PLUGS} entries"),
        ));
    }
    for (index, hash) in plugs.iter().enumerate() {
        if let Some(hash) = hash {
            validate_inventory_definition_hash(*hash, &format!("{path}/{index}"))?;
        }
    }
    Ok(())
}

pub(in crate::app::inventory) fn encode_plugs(plugs: ItemPlugs) -> Value {
    match plugs {
        ItemPlugs::NativeDefaults => Value::Null,
        ItemPlugs::Authored(plugs) => Value::Array(
            plugs
                .into_iter()
                .map(|hash| {
                    hash.map(format_definition_hash_hex)
                        .map_or(Value::Null, Value::String)
                })
                .collect(),
        ),
    }
}

pub(in crate::app::inventory) fn format_definition_hash_hex(hash: u32) -> String {
    crate::hash::format_hash_hex(u64::from(hash))
}

pub(in crate::app::inventory) fn format_instance_soid(soid: u64) -> String {
    format!("0x{soid:016X}")
}

pub(in crate::app::inventory) fn inventory_item_path(location: InventoryItemLocation) -> String {
    format!(
        "/state/characters/{}/inventory/{}",
        location.character_index, location.item_index
    )
}

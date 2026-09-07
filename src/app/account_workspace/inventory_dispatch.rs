//! Inventory and profile operations routed to the selected account backend.

use super::*;

pub(in crate::app) fn profile_items(
    document: &WorkspaceDocument,
) -> Result<Option<Vec<ProfileItemSnapshot>>, InventoryError> {
    match &document.account {
        AccountDocument::Json(_) => crate::app::inventory::profile_items(&document.json),
        #[cfg(feature = "sqlite-account")]
        AccountDocument::Sqlite(document) => Ok(sqlite::profile_items(document)),
        AccountDocument::Blocked(_) => Err(blocked_inventory(document)),
    }
}

pub(in crate::app) fn dismantle_rewards(
    document: &WorkspaceDocument,
) -> Result<Option<Vec<DismantleRewardSnapshot>>, InventoryError> {
    match &document.account {
        AccountDocument::Json(_) => crate::app::inventory::dismantle_rewards(&document.json),
        #[cfg(feature = "sqlite-account")]
        AccountDocument::Sqlite(document) => Ok(sqlite::dismantle_rewards(document)),
        AccountDocument::Blocked(_) => Err(blocked_inventory(document)),
    }
}

pub(in crate::app) fn character_inventory(
    document: &WorkspaceDocument,
    character_index: usize,
) -> Result<Option<Vec<InventoryItemSnapshot>>, InventoryError> {
    match &document.account {
        AccountDocument::Json(_) => {
            crate::app::inventory::character_inventory(&document.json, character_index)
        }
        #[cfg(feature = "sqlite-account")]
        AccountDocument::Sqlite(document) => sqlite::character_inventory(document, character_index),
        AccountDocument::Blocked(_) => Err(blocked_inventory(document)),
    }
}

pub(in crate::app) fn add_profile_item(
    document: &mut WorkspaceDocument,
    definition_hash: u32,
    quantity: i32,
) -> Result<ProfileItemLocation, InventoryError> {
    match &mut document.account {
        AccountDocument::Json(_) => {
            crate::app::inventory::add_profile_item(&mut document.json, definition_hash, quantity)
        }
        #[cfg(feature = "sqlite-account")]
        AccountDocument::Sqlite(document) => {
            sqlite::add_profile_item(document, definition_hash, quantity)
        }
        AccountDocument::Blocked(reason) => {
            Err(InventoryError::new("state.sqlite3", reason.clone()))
        }
    }
}

pub(in crate::app) fn apply_profile_item_action(
    document: &mut WorkspaceDocument,
    location: ProfileItemLocation,
    action: ProfileItemAction,
) -> Result<(), InventoryError> {
    match &mut document.account {
        AccountDocument::Json(_) => {
            crate::app::inventory::apply_profile_item_action(&mut document.json, location, action)
        }
        #[cfg(feature = "sqlite-account")]
        AccountDocument::Sqlite(document) => {
            sqlite::apply_profile_item_action(document, location, action)
        }
        AccountDocument::Blocked(reason) => {
            Err(InventoryError::new("state.sqlite3", reason.clone()))
        }
    }
}

pub(in crate::app) fn add_dismantle_reward(
    document: &mut WorkspaceDocument,
    definition_hash: u32,
) -> Result<DismantleRewardLocation, InventoryError> {
    match &mut document.account {
        AccountDocument::Json(_) => {
            crate::app::inventory::add_dismantle_reward(&mut document.json, definition_hash)
        }
        #[cfg(feature = "sqlite-account")]
        AccountDocument::Sqlite(document) => {
            sqlite::add_dismantle_reward(document, definition_hash)
        }
        AccountDocument::Blocked(reason) => {
            Err(InventoryError::new("state.sqlite3", reason.clone()))
        }
    }
}

pub(in crate::app) fn apply_dismantle_reward_action(
    document: &mut WorkspaceDocument,
    location: DismantleRewardLocation,
    action: DismantleRewardAction,
) -> Result<(), InventoryError> {
    match &mut document.account {
        AccountDocument::Json(_) => crate::app::inventory::apply_dismantle_reward_action(
            &mut document.json,
            location,
            action,
        ),
        #[cfg(feature = "sqlite-account")]
        AccountDocument::Sqlite(document) => {
            sqlite::apply_dismantle_reward_action(document, location, action)
        }
        AccountDocument::Blocked(reason) => {
            Err(InventoryError::new("state.sqlite3", reason.clone()))
        }
    }
}

pub(in crate::app) fn add_inventory_item(
    document: &mut WorkspaceDocument,
    character_index: usize,
    item: NewInventoryItem,
) -> Result<InventoryItemLocation, InventoryError> {
    require_definition(document, item.definition_hash)?;
    match &mut document.account {
        AccountDocument::Json(_) => {
            crate::app::inventory::add_inventory_item(&mut document.json, character_index, item)
        }
        #[cfg(feature = "sqlite-account")]
        AccountDocument::Sqlite(document) => {
            sqlite::add_inventory_item(document, character_index, item)
        }
        AccountDocument::Blocked(reason) => {
            Err(InventoryError::new("state.sqlite3", reason.clone()))
        }
    }
}

pub(in crate::app) fn apply_inventory_item_action(
    document: &mut WorkspaceDocument,
    location: InventoryItemLocation,
    action: InventoryItemAction,
) -> Result<(), InventoryError> {
    if let InventoryItemAction::SetDefinitionHash(hash) = &action {
        require_definition(document, *hash)?;
    }
    match &mut document.account {
        AccountDocument::Json(_) => {
            crate::app::inventory::apply_inventory_item_action(&mut document.json, location, action)
        }
        #[cfg(feature = "sqlite-account")]
        AccountDocument::Sqlite(document) => {
            sqlite::apply_inventory_item_action(document, location, action)
        }
        AccountDocument::Blocked(reason) => {
            Err(InventoryError::new("state.sqlite3", reason.clone()))
        }
    }
}

pub(in crate::app) fn remove_character_inventory_items(
    document: &mut WorkspaceDocument,
    character_index: usize,
    item_indices: impl IntoIterator<Item = usize>,
) -> Result<usize, InventoryError> {
    match &mut document.account {
        AccountDocument::Json(_) => crate::app::inventory::remove_character_inventory_items(
            &mut document.json,
            character_index,
            item_indices,
        ),
        #[cfg(feature = "sqlite-account")]
        AccountDocument::Sqlite(document) => {
            sqlite::remove_character_inventory_items(document, character_index, item_indices)
        }
        AccountDocument::Blocked(reason) => {
            Err(InventoryError::new("state.sqlite3", reason.clone()))
        }
    }
}

pub(in crate::app) fn swap_inventory_item_with_equipment(
    document: &mut WorkspaceDocument,
    location: InventoryItemLocation,
    slot: &str,
) -> Result<bool, InventoryError> {
    if !document
        .equipment_slots()
        .iter()
        .any(|(known, _, _)| *known == slot)
    {
        return Err(InventoryError::new(
            "equipment",
            format!("Unknown equipment slot for the active account source: {slot}"),
        ));
    }
    match &mut document.account {
        AccountDocument::Json(_) => crate::app::inventory::swap_inventory_item_with_equipment(
            &mut document.json,
            location,
            slot,
        ),
        #[cfg(feature = "sqlite-account")]
        AccountDocument::Sqlite(document) => {
            sqlite::swap_inventory_item_with_equipment(document, location, slot)
        }
        AccountDocument::Blocked(reason) => {
            Err(InventoryError::new("state.sqlite3", reason.clone()))
        }
    }
}

/// Returns the ability selection PR-88 stores with one unequipped item instance.
///
/// Legacy JSON schemas keep abilities on the character row, so they deliberately return no
/// item-specific selection and retain their existing default-on-equip behavior.
pub(in crate::app) fn persisted_inventory_item_abilities(
    document: &WorkspaceDocument,
    _location: InventoryItemLocation,
) -> Option<sundial_account::CharacterAbilities> {
    match &document.account {
        AccountDocument::Json(_) | AccountDocument::Blocked(_) => None,
        #[cfg(feature = "sqlite-account")]
        AccountDocument::Sqlite(document) => sqlite::inventory_item_abilities(document, _location),
    }
}

pub(in crate::app) fn move_inventory_item_to_character(
    document: &mut WorkspaceDocument,
    location: InventoryItemLocation,
    destination_character_index: usize,
) -> Result<InventoryItemLocation, InventoryError> {
    match &mut document.account {
        AccountDocument::Json(_) => crate::app::inventory::move_inventory_item_to_character(
            &mut document.json,
            location,
            destination_character_index,
        ),
        #[cfg(feature = "sqlite-account")]
        AccountDocument::Sqlite(document) => sqlite::move_inventory_item_to_character(
            document,
            location,
            destination_character_index,
        ),
        AccountDocument::Blocked(reason) => {
            Err(InventoryError::new("state.sqlite3", reason.clone()))
        }
    }
}

pub(in crate::app) fn move_equipment_item_to_inventory(
    document: &mut WorkspaceDocument,
    character_index: usize,
    slot: &str,
) -> Result<(), InventoryError> {
    match &mut document.account {
        AccountDocument::Json(_) => crate::app::inventory::move_equipment_item_to_inventory(
            &mut document.json,
            character_index,
            slot,
        ),
        #[cfg(feature = "sqlite-account")]
        AccountDocument::Sqlite(document) => {
            sqlite::move_equipment_item_to_inventory(document, character_index, slot)
        }
        AccountDocument::Blocked(reason) => {
            Err(InventoryError::new("state.sqlite3", reason.clone()))
        }
    }
}

fn require_definition(document: &WorkspaceDocument, hash: u32) -> Result<(), InventoryError> {
    if crate::account_contract::definition_available(
        u64::from(hash),
        document.supports_v13_account(),
    ) {
        Ok(())
    } else {
        Err(InventoryError::new(
            "definition_hash",
            "The emote wheel requires a v13+ JSON account",
        ))
    }
}

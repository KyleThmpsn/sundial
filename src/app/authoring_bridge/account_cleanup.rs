//! Exact-identity cleanup proposals for package replacement and uninstall.
//! Does not save settings or scan backup accounts.
use crate::app::{equipment, inventory, progression};
use crate::investment::{
    AuthoredAccountCleanup, AuthoredCollectionUnlock, AuthoredSlotReplacement, AuthoredSocketChange,
};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
mod placement;
mod sockets;
#[cfg(test)]
mod tests;

pub(crate) fn preview_account_cleanup(
    install: &Path,
    hashes: &BTreeSet<u32>,
    unlocks: &[AuthoredCollectionUnlock],
) -> Result<AuthoredAccountCleanup, String> {
    preview_account_replacement(install, hashes, unlocks, &[], None)
}

pub(crate) fn preview_account_replacement(
    install: &Path,
    hashes: &BTreeSet<u32>,
    unlocks: &[AuthoredCollectionUnlock],
    socket_changes: &[AuthoredSocketChange],
    slots: Option<&AuthoredSlotReplacement>,
) -> Result<AuthoredAccountCleanup, String> {
    let preferences = crate::app::settings::load_preferences().preferences;
    let settings_path = super::authored_unlock_settings_path(install, &preferences)?;
    let original_bytes = std::fs::read(&settings_path).map_err(|e| e.to_string())?;
    let original: Value = crate::package_authoring::read_json(original_bytes.as_slice())
        .map_err(|e| e.to_string())?;
    let database_path = crate::persistence::investment_path(&settings_path);
    if crate::game_settings::requires_sqlite_account(&original) {
        return crate::persistence::sqlite_account::package::preview_replacement(
            &database_path,
            hashes,
            unlocks,
            socket_changes,
            slots,
        );
    }
    crate::investment::validate_authored_cleanup_backend(&settings_path)?;
    let (mut cleaned, removed_items, cleared_plugs, cleared_unlocks, removed_reward_rules) =
        clean(&original, hashes, unlocks)?;
    let slot_moves = placement::relocate(&mut cleaned, hashes, slots)?;
    let resized_items = sockets::resize(&mut cleaned, hashes, socket_changes)?;
    let cleaned_bytes = if cleaned == original {
        original_bytes.clone()
    } else {
        serde_json::to_vec_pretty(&cleaned).map_err(|e| e.to_string())?
    };
    Ok(AuthoredAccountCleanup {
        settings_path,
        original_bytes,
        cleaned_bytes,
        removed_items,
        cleared_plugs,
        removed_reward_rules,
        cleared_unlocks,
        resized_items,
        slot_moves,
    })
}

type Cleaned = (Value, BTreeMap<u32, usize>, usize, usize, usize);

fn clean(
    original: &Value,
    hashes: &BTreeSet<u32>,
    unlocks: &[AuthoredCollectionUnlock],
) -> Result<Cleaned, String> {
    if crate::game_settings::schema_version(original).is_some_and(|v| v >= 18) {
        return Err("Settings v18 requires a Sunrise investment database".into());
    }
    let mode = inventory::schema_mode(original);
    if mode.is_read_only() || mode.is_future() {
        return Err("Automatic cleanup requires a supported settings schema".into());
    }
    inventory::validate_document_items(original).map_err(|e| e.to_string())?;
    progression::validate(original)?;
    let mut document = original.clone();
    let mut removed = BTreeMap::new();
    let mut plugs = 0;
    let count = match original.pointer("/state/characters") {
        None => 0,
        Some(value) => value.as_array().ok_or("Characters must be an array")?.len(),
    };
    for character in 0..count {
        plugs += clean_inventory(&mut document, character, hashes, &mut removed)?;
        plugs += clean_equipment(&mut document, character, hashes, &mut removed)?;
    }
    for row in inventory::profile_items(&document)
        .map_err(|e| e.to_string())?
        .unwrap_or_default()
        .into_iter()
        .rev()
    {
        if hashes.contains(&row.definition_hash) {
            inventory::apply_profile_item_action(
                &mut document,
                row.location,
                inventory::ProfileItemAction::Remove,
            )
            .map_err(|e| e.to_string())?;
            *removed.entry(row.definition_hash).or_default() += 1;
        }
    }
    let cleared = progression::remove_authored_collection_state(&mut document, unlocks)?;
    let mut rewards = 0;
    for row in inventory::dismantle_rewards(&document)
        .map_err(|e| e.to_string())?
        .unwrap_or_default()
        .into_iter()
        .rev()
    {
        if hashes.contains(&row.definition_hash) {
            inventory::apply_dismantle_reward_action(
                &mut document,
                row.location,
                inventory::DismantleRewardAction::Remove,
            )
            .map_err(|e| e.to_string())?;
            rewards += 1;
        }
    }
    inventory::validate_document_items(&document).map_err(|e| e.to_string())?;
    Ok((document, removed, plugs, cleared, rewards))
}

fn clean_inventory(
    document: &mut Value,
    character: usize,
    hashes: &BTreeSet<u32>,
    removed: &mut BTreeMap<u32, usize>,
) -> Result<usize, String> {
    let mut plugs = 0;
    let rows = inventory::character_inventory(document, character)
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    for row in rows.into_iter().rev() {
        if hashes.contains(&row.definition_hash) {
            inventory::apply_inventory_item_action(
                document,
                row.location,
                inventory::InventoryItemAction::Remove,
            )
            .map_err(|e| e.to_string())?;
            *removed.entry(row.definition_hash).or_default() += 1;
        } else if let inventory::ItemPlugs::Authored(mut values) = row.plugs {
            let mut changed = 0;
            for value in &mut values {
                if value.is_some_and(|hash| hashes.contains(&hash)) {
                    *value = None;
                    changed += 1;
                }
            }
            if changed > 0 {
                inventory::apply_inventory_item_action(
                    document,
                    row.location,
                    inventory::InventoryItemAction::SetPlugs(inventory::ItemPlugs::Authored(
                        values,
                    )),
                )
                .map_err(|e| e.to_string())?;
                plugs += changed;
            }
        }
    }
    Ok(plugs)
}

fn clean_equipment(
    document: &mut Value,
    character: usize,
    hashes: &BTreeSet<u32>,
    removed: &mut BTreeMap<u32, usize>,
) -> Result<usize, String> {
    let mut plugs = 0;
    for row in equipment::equipped_item_snapshots(document, character)? {
        if let Some(hash) = row
            .definition_hash
            .and_then(|hash| u32::try_from(hash).ok())
            .filter(|hash| hashes.contains(hash))
        {
            equipment::set_weapon_slot_empty(document, character, row.slot)?;
            *removed.entry(hash).or_default() += 1;
        } else if let equipment::EquippedItemPlugs::Authored(values) = row.plugs {
            for (index, value) in values.iter().enumerate() {
                if let equipment::EquippedPlugValue::Hash(hash) = value
                    && u32::try_from(*hash).is_ok_and(|hash| hashes.contains(&hash))
                {
                    equipment::set_equipment_item_plug(
                        document,
                        character,
                        row.slot,
                        index,
                        &[],
                        None,
                    )?;
                    plugs += 1;
                }
            }
        }
    }
    Ok(plugs)
}

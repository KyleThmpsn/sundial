//! Lossless JSON equipment moves after the shared replacement capacity plan succeeds.
use super::*;
use crate::investment::{AuthoredItemMove, AuthoredMoveOutcome, account_sync::placement};

pub(super) fn relocate(
    document: &mut Value,
    removed: &BTreeSet<u32>,
    replacement: Option<&AuthoredSlotReplacement>,
) -> Result<Vec<AuthoredItemMove>, String> {
    let Some(replacement) = replacement else {
        return Ok(vec![]);
    };
    let count = document
        .pointer("/state/characters")
        .map(|value| value.as_array().ok_or("Characters must be an array"))
        .transpose()?
        .map_or(0, Vec::len);
    let mut characters = vec![];
    for character_index in 0..count {
        let inventory = inventory::character_inventory(document, character_index)
            .map_err(|e| e.to_string())?
            .unwrap_or_default()
            .into_iter()
            .map(|row| row.definition_hash)
            .collect();
        let equipment = equipment::equipped_item_snapshots(document, character_index)?
            .into_iter()
            .filter_map(|row| row.definition_hash.map(|hash| (row.slot, hash)))
            .map(|(slot, hash)| {
                Ok((
                    slot.to_owned(),
                    u32::try_from(hash)
                        .map_err(|_| "An equipped item has an invalid definition hash")?,
                ))
            })
            .collect::<Result<Vec<_>, String>>()?;
        characters.push(placement::CharacterItems {
            character_index,
            inventory,
            equipment,
        });
    }
    let moves = placement::plan(&characters, removed, replacement)?;
    let mut updated = document.clone();
    for movement in &moves {
        match movement.outcome {
            AuthoredMoveOutcome::MovedToInventory => {
                inventory::move_equipment_item_to_inventory(
                    &mut updated,
                    movement.character_index,
                    &movement.equipment_slot,
                )
                .map_err(|e| e.to_string())?;
            }
            AuthoredMoveOutcome::DeletedInventoryFull => {
                equipment::set_weapon_slot_empty(
                    &mut updated,
                    movement.character_index,
                    &movement.equipment_slot,
                )?;
            }
        }
    }
    inventory::validate_document_items(&updated).map_err(|e| e.to_string())?;
    *document = updated;
    Ok(moves)
}

#[cfg(test)]
mod tests;

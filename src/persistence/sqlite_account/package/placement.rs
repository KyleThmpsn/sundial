//! SQLite equipment moves preserve complete item rows and their socket foreign keys.
use super::*;
use crate::investment::{
    AuthoredItemMove, AuthoredMoveOutcome, AuthoredSlotReplacement, account_sync::placement,
};

pub(super) fn relocate(
    db: &Connection,
    removed: &BTreeSet<u32>,
    replacement: Option<&AuthoredSlotReplacement>,
) -> Result<Vec<AuthoredItemMove>, String> {
    let Some(replacement) = replacement else {
        return Ok(vec![]);
    };
    let mut statement = db.prepare("SELECT character_slot,location,position,definition_hash FROM items ORDER BY character_slot,location,position").map_err(err)?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, usize>(0)?,
                row.get::<_, u8>(1)?,
                row.get::<_, usize>(2)?,
                row.get::<_, u32>(3)?,
            ))
        })
        .map_err(err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(err)?;
    let mut characters = BTreeMap::new();
    for (character_index, location, position, hash) in rows {
        let character =
            characters
                .entry(character_index)
                .or_insert_with(|| placement::CharacterItems {
                    character_index,
                    inventory: vec![],
                    equipment: vec![],
                });
        match location {
            0 => character.equipment.push((
                crate::account_contract::ALL_EQUIPMENT_SLOTS
                    .get(position)
                    .ok_or("An equipped item has an unsupported slot")?
                    .0
                    .to_owned(),
                hash,
            )),
            1 => character.inventory.push(hash),
            _ => return Err("An item has an unsupported account location".into()),
        }
    }
    let moves = placement::plan(
        &characters.into_values().collect::<Vec<_>>(),
        removed,
        replacement,
    )?;
    for movement in &moves {
        let position = crate::account_contract::ALL_EQUIPMENT_SLOTS
            .iter()
            .position(|(slot, _, _)| *slot == movement.equipment_slot)
            .ok_or("An equipment move has an unsupported slot")?;
        let affected = match movement.outcome {
            AuthoredMoveOutcome::MovedToInventory => db.execute(
                "UPDATE items SET location=1,position=(SELECT count(*) FROM items WHERE character_slot=?1 AND location=1) WHERE character_slot=?1 AND location=0 AND position=?2 AND definition_hash=?3",
                params![movement.character_index, position, movement.definition_hash]),
            AuthoredMoveOutcome::DeletedInventoryFull => db.execute(
                "DELETE FROM items WHERE character_slot=?1 AND location=0 AND position=?2 AND definition_hash=?3",
                params![movement.character_index, position, movement.definition_hash]),
        }.map_err(err)?;
        if affected != 1 {
            return Err("The reviewed equipment move no longer matches its account item".into());
        }
    }
    Ok(moves)
}

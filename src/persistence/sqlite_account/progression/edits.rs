//! Replay legacy artifact overrides while retaining each character's effective ownership.
use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::{Progression, SqliteAccountError, integer, invalid};

impl Progression {
    pub(super) fn apply_artifact_seed(
        &mut self,
        before: &Self,
        selected_character: i32,
        document: &Value,
    ) -> Result<(), SqliteAccountError> {
        let Some(request) = document.get("_native_artifact_seed") else {
            return Ok(());
        };
        let flags = artifact_flags(&request["flags"])?;
        let rows = request["overrides"]
            .as_array()
            .filter(|rows| !rows.is_empty() && rows.len() <= flags.len())
            .ok_or_else(invalid)?;
        let mut seen = BTreeSet::new();
        for row in rows {
            let row = row
                .as_array()
                .filter(|row| row.len() == 2)
                .ok_or_else(invalid)?;
            let index = integer(&row[0])?;
            let expected = integer(&row[1])?;
            let slot = *flags.get(&index).ok_or_else(invalid)?;
            if !(0..=255).contains(&expected)
                || !seen.insert(index)
                || before.family.get(&(0, index)) != Some(&expected)
            {
                return Err(SqliteAccountError::invalid_data(
                    "family5",
                    "Artifact state changed before the edit could be applied",
                ));
            }
            self.family.remove(&(0, index));
            if expected == super::FLAG_SET {
                for &character in &self.character_slots {
                    if character != selected_character {
                        self.unlocks
                            .insert((character, 4, slot, 0), super::FLAG_SET);
                    }
                }
            }
        }
        if flags
            .keys()
            .any(|index| before.family.contains_key(&(0, *index)) && !seen.contains(index))
        {
            return Err(invalid());
        }
        for &character in &self.character_slots {
            if character != selected_character {
                let used = flags
                    .values()
                    .filter(|&&slot| {
                        self.unlocks.get(&(character, 4, slot, 0)) == Some(&super::FLAG_SET)
                    })
                    .count() as i32;
                self.unlocks.insert(
                    (
                        character,
                        5,
                        crate::investment::seasonal::USED_CHARACTER_SLOT as i32,
                        0,
                    ),
                    used,
                );
            }
        }
        Ok(())
    }
}

fn artifact_flags(value: &Value) -> Result<BTreeMap<i32, i32>, SqliteAccountError> {
    let rows = value
        .as_array()
        .filter(|rows| rows.len() == 25)
        .ok_or_else(invalid)?;
    let mut flags = BTreeMap::new();
    let mut slots = BTreeSet::new();
    for row in rows {
        let row = row
            .as_array()
            .filter(|row| row.len() == 2)
            .ok_or_else(invalid)?;
        let index = integer(&row[0])?;
        let slot = integer(&row[1])?;
        if !(0..=65535).contains(&index)
            || !(0..4096).contains(&slot)
            || flags.insert(index, slot).is_some()
            || !slots.insert(slot)
        {
            return Err(invalid());
        }
    }
    Ok(flags)
}

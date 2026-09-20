//! Sparse native unlock and family-5 state, projected through the existing progression editor.
use super::SqliteAccountError;
use rusqlite::{Connection, params};
use serde_json::Value;

use crate::persistence::native_account::progression::Progression as State;
use std::ops::{Deref, DerefMut};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Progression {
    state: State,
    native_unlocks: Vec<super::writer::NativeRow>,
    native_family: Vec<super::writer::NativeRow>,
}
impl Progression {
    pub(super) fn load(db: &Connection) -> Result<Self, SqliteAccountError> {
        let mut result = Self::default();
        let mut stmt = db
            .prepare("SELECT character_slot,bank,slot,lane,value FROM unlocks")
            .map_err(sql)?;
        let rows = stmt
            .query_map([], |r| {
                Ok(((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?), r.get(4)?))
            })
            .map_err(sql)?;
        result.unlocks = rows.collect::<Result<_, _>>().map_err(sql)?;
        let mut stmt = db
            .prepare("SELECT kind,slot,value FROM family5 ORDER BY kind,position")
            .map_err(sql)?;
        result.family = stmt
            .query_map([], |r| Ok(((r.get(0)?, r.get(1)?), r.get(2)?)))
            .map_err(sql)?
            .collect::<Result<_, _>>()
            .map_err(sql)?;
        let mut stmt = db
            .prepare("SELECT slot FROM characters ORDER BY slot")
            .map_err(sql)?;
        result.character_slots = stmt
            .query_map([], |r| r.get(0))
            .map_err(sql)?
            .collect::<Result<_, _>>()
            .map_err(sql)?;
        result.native_unlocks = super::writer::rows(db, "unlocks")?;
        result.native_family = super::writer::rows(db, "family5")?;
        result.validate_native_rows(db)?;
        Ok(result)
    }
    fn validate_native_rows(&self, db: &Connection) -> Result<(), SqliteAccountError> {
        let capacities = [12_300, 512, 256, 6_200, 4_096, 768, 256, 256];
        for (&(_, bank, slot, lane), &value) in &self.unlocks {
            let capacity = usize::try_from(bank)
                .ok()
                .and_then(|bank| capacities.get(bank))
                .ok_or_else(invalid)?;
            if !(0..*capacity).contains(&slot)
                || !(0..if bank < 6 { 1 } else { 3 }).contains(&lane)
                || (matches!(bank, 0 | 1 | 2 | 4) && !(0..=255).contains(&value))
            {
                return Err(invalid());
            }
        }
        let invalid_rows: i64 = db.query_row("SELECT count(*) FROM (SELECT *,row_number() OVER(PARTITION BY kind ORDER BY position)-1 AS expected FROM family5) WHERE position!=expected OR (kind=0 AND value NOT BETWEEN 0 AND 255)", [], |r| r.get(0)).map_err(sql)?;
        if invalid_rows != 0 {
            return Err(invalid());
        }
        Ok(())
    }
    pub(super) fn apply(&mut self, index: usize, value: &Value) -> Result<(), SqliteAccountError> {
        self.state
            .apply(index, value)
            .map_err(|error| SqliteAccountError::invalid_data("unlocks", error))
    }
    pub(super) fn set_account_flag(
        &mut self,
        index: u16,
        slot: u16,
    ) -> Result<bool, SqliteAccountError> {
        self.state
            .set_account_flag(index, slot)
            .map_err(|error| SqliteAccountError::invalid_data("unlocks", error))
    }
    pub(super) fn save(&self, db: &Connection) -> Result<(), SqliteAccountError> {
        let before = Self::load(db)?;
        for (&(character, bank, slot, lane), &value) in &self.unlocks {
            if before.unlocks.get(&(character, bank, slot, lane)) != Some(&value) {
                if before.unlocks.contains_key(&(character, bank, slot, lane)) {
                    db.execute("UPDATE unlocks SET value=? WHERE character_slot=? AND bank=? AND slot=? AND lane=?", params![value,character,bank,slot,lane]).map_err(sql)?;
                } else {
                    insert_preserved(
                        db,
                        "unlocks",
                        &self.native_unlocks,
                        &[
                            ("character_slot", character),
                            ("bank", bank),
                            ("slot", slot),
                            ("lane", lane),
                        ],
                        &[("value", value)],
                    )?;
                }
            }
        }
        for &(character, bank, slot, lane) in before.unlocks.keys() {
            if !self.unlocks.contains_key(&(character, bank, slot, lane)) {
                db.execute(
                    "DELETE FROM unlocks WHERE character_slot=? AND bank=? AND slot=? AND lane=?",
                    params![character, bank, slot, lane],
                )
                .map_err(sql)?;
            }
        }
        // Existing rows keep native positions and unknown columns. New rows use a free position.
        for &(kind, slot) in before.family.keys() {
            if !self.family.contains_key(&(kind, slot)) {
                db.execute(
                    "DELETE FROM family5 WHERE kind=? AND slot=?",
                    params![kind, slot],
                )
                .map_err(sql)?;
            }
        }
        for (&(kind, slot), &value) in &self.family {
            if before.family.contains_key(&(kind, slot)) {
                db.execute(
                    "UPDATE family5 SET value=? WHERE kind=? AND slot=?",
                    params![value, kind, slot],
                )
                .map_err(sql)?;
            } else {
                let position:i32=db.query_row("WITH RECURSIVE n(x) AS (VALUES(0) UNION ALL SELECT x+1 FROM n WHERE x<99) SELECT x FROM n WHERE NOT EXISTS(SELECT 1 FROM family5 WHERE kind=? AND position=x) ORDER BY x LIMIT 1",[kind],|r|r.get(0)).map_err(sql)?;
                insert_preserved(
                    db,
                    "family5",
                    &self.native_family,
                    &[("kind", kind), ("slot", slot)],
                    &[("position", position), ("value", value)],
                )?;
            }
        }
        for kind in 0..2 {
            super::positions::compact(db, "family5", &format!("kind={kind}"))
                .map_err(|error| SqliteAccountError::invalid_data("family5", error))?;
        }
        Ok(())
    }
}
fn insert_preserved(
    db: &Connection,
    table: &str,
    rows: &[super::writer::NativeRow],
    keys: &[(&str, i32)],
    values: &[(&str, i32)],
) -> Result<(), SqliteAccountError> {
    let mut row = rows
        .iter()
        .find(|row| {
            keys.iter().all(|(name, value)| {
                row.get(*name) == Some(&super::snapshot::Cell::Integer(i64::from(*value)))
            })
        })
        .cloned()
        .unwrap_or_default();
    for &(name, value) in keys.iter().chain(values) {
        row.insert(
            name.into(),
            super::snapshot::Cell::Integer(i64::from(value)),
        );
    }
    super::writer::insert(db, table, row)
}
fn invalid() -> SqliteAccountError {
    SqliteAccountError::invalid_data("unlocks", "Invalid progression data")
}
fn sql(error: rusqlite::Error) -> SqliteAccountError {
    SqliteAccountError::sqlite("edit progression in", error)
}

impl Deref for Progression {
    type Target = State;
    fn deref(&self) -> &State {
        &self.state
    }
}
impl DerefMut for Progression {
    fn deref_mut(&mut self) -> &mut State {
        &mut self.state
    }
}

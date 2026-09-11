//! Sparse native unlock and family-5 state, projected through the existing progression editor.
use super::SqliteAccountError;
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::collections::BTreeMap;

mod edits;

type UnlockKey = (i32, i32, i32, i32);
// Sunrise's biased acquired flag is exactly 2. Other byte values are not acquired.
const FLAG_SET: i32 = 2;
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Progression {
    unlocks: BTreeMap<UnlockKey, i32>,
    family: BTreeMap<(i32, i32), i32>,
    character_slots: Vec<i32>,
    native_unlocks: Vec<super::writer::NativeRow>,
    native_family: Vec<super::writer::NativeRow>,
}
const FIELDS: [&str; 8] = [
    "account_flag_runs",
    "profile_flag_runs",
    "character_flags",
    "objective_values",
    "character_flag_runs",
    "character_objective_values",
    "account_progressions",
    "character_progressions",
];
fn scope(bank: i32, character: i32) -> i32 {
    if matches!(bank, 0 | 1 | 3 | 6) {
        -1
    } else {
        character
    }
}
impl Progression {
    pub(super) fn account_flag_is_set(&self, definition_index: u16, slot: u16) -> bool {
        self.unlocks.get(&(-1, 0, i32::from(slot), 0)) == Some(&FLAG_SET)
            && self
                .family
                .get(&(0, i32::from(definition_index)))
                .is_none_or(|value| *value == FLAG_SET)
    }

    pub(super) fn set_account_flag(
        &mut self,
        definition_index: u16,
        slot: u16,
    ) -> Result<bool, SqliteAccountError> {
        if usize::from(slot) >= crate::package_authoring::SHADOWKEEP_ACCOUNT_FLAG_REGION_CAPACITY {
            return Err(SqliteAccountError::invalid_data(
                "unlocks",
                "The account claim flag is outside Sunrise's supported bank",
            ));
        }
        let changed = !self.account_flag_is_set(definition_index, slot);
        self.unlocks.insert((-1, 0, i32::from(slot), 0), FLAG_SET);
        // An existing family-5 override must agree with the native claim checked by Sunrise.
        // Updating it does not consume another override row or change unrelated flags.
        if let Some(value) = self.family.get_mut(&(0, i32::from(definition_index))) {
            *value = FLAG_SET;
        }
        Ok(changed)
    }

    pub(super) fn account_flags_changed_from(&self, before: &Self) -> bool {
        !self
            .unlocks
            .iter()
            .filter(|((owner, bank, _, _), _)| *owner == -1 && *bank == 0)
            .eq(before
                .unlocks
                .iter()
                .filter(|((owner, bank, _, _), _)| *owner == -1 && *bank == 0))
            || !self
                .family
                .iter()
                .filter(|((kind, _), _)| *kind == 0)
                .eq(before.family.iter().filter(|((kind, _), _)| *kind == 0))
    }

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
    pub(super) fn view(&self, character_index: usize) -> Value {
        let character = self
            .character_slots
            .get(character_index)
            .copied()
            .unwrap_or(0);
        let mut view = json!({"version":16,"state":{"unlocks":{},"investment":{}}});
        for (bank, field) in FIELDS.iter().enumerate() {
            let bank = bank as i32;
            let mut slots: BTreeMap<i32, [i32; 3]> = BTreeMap::new();
            for (&(owner, b, slot, lane), &value) in &self.unlocks {
                if owner == scope(bank, character) && bank == b && (0..3).contains(&lane) {
                    slots.entry(slot).or_default()[lane as usize] = value;
                }
            }
            let values = match bank {
                0 | 1 | 4 => compress(
                    slots
                        .iter()
                        .filter_map(|(&slot, lanes)| (lanes[0] == FLAG_SET).then_some(slot))
                        .collect(),
                ),
                2 => slots
                    .iter()
                    .filter_map(|(&slot, lanes)| (lanes[0] == FLAG_SET).then_some(json!(slot)))
                    .collect(),
                3 | 5 => slots
                    .iter()
                    .filter_map(|(&slot, lanes)| (lanes[0] != 0).then_some(json!([slot, lanes[0]])))
                    .collect(),
                _ => slots
                    .iter()
                    .map(|(&slot, lanes)| json!([slot, lanes[0], lanes[1], lanes[2]]))
                    .collect(),
            };
            view["state"]["unlocks"][field] = Value::Array(values);
        }
        for (kind, field) in [
            (0, "family5_flag_overrides"),
            (1, "family5_value_overrides"),
        ] {
            view["state"]["investment"][field] = json!(
                self.family
                    .iter()
                    .filter_map(|(&(k, slot), &value)| (k == kind
                        && visible_family(k, slot, value))
                    .then_some(json!([slot, value])))
                    .collect::<Vec<_>>()
            );
        }
        // Read-only provenance for the editor. Never serialized into the native database.
        view["_native_progression"] = json!({
            "character_slot": character,
            "character_slots": self.character_slots,
            "unlocks": self.unlocks.iter().filter_map(|(&(owner, bank, slot, lane), &value)|
                (owner == scope(bank, character)).then_some([bank, slot, lane, value])
            ).collect::<Vec<_>>(),
            "family": self.family.iter().map(|(&(kind, slot), &value)| [kind, slot, value]).collect::<Vec<_>>(),
            "character_flags": self.unlocks.iter().filter_map(|(&(owner, bank, slot, lane), &value)|
                (bank == 4 && lane == 0).then_some([owner, slot, value])
            ).collect::<Vec<_>>(),
            "hidden_family_counts": ([0, 1].map(|kind| self.family.iter().filter(|entry| {
                let (&(k, slot), &value) = *entry;
                k == kind && !visible_family(k, slot, value)
            }).count())),
        });
        view
    }
    pub(super) fn apply(
        &mut self,
        character_index: usize,
        after: &Value,
    ) -> Result<(), SqliteAccountError> {
        let before = self.view(character_index);
        let character = self
            .character_slots
            .get(character_index)
            .copied()
            .unwrap_or(0);
        let mut candidate = self.clone();
        for (bank, field) in FIELDS.iter().enumerate() {
            let old = decode(bank, &before["state"]["unlocks"][field])?;
            let new = decode(bank, &after["state"]["unlocks"][field])?;
            for (&(slot, lane), &value) in &new {
                if old.get(&(slot, lane)) != Some(&value) {
                    candidate.unlocks.insert(
                        (scope(bank as i32, character), bank as i32, slot, lane),
                        value,
                    );
                }
            }
            for &(slot, lane) in old.keys() {
                if !new.contains_key(&(slot, lane)) {
                    candidate.unlocks.remove(&(
                        scope(bank as i32, character),
                        bank as i32,
                        slot,
                        lane,
                    ));
                }
            }
        }
        for (kind, field) in [
            (0, "family5_flag_overrides"),
            (1, "family5_value_overrides"),
        ] {
            let rows = after["state"]["investment"][field]
                .as_array()
                .ok_or_else(invalid)?;
            candidate
                .family
                .retain(|&(k, slot), value| k != kind || !visible_family(k, slot, *value));
            for row in rows {
                candidate
                    .family
                    .insert((kind, integer(&row[0])?), integer(&row[1])?);
            }
        }
        candidate.apply_artifact_seed(self, character, after)?;
        if (0..2).any(|kind| candidate.family.keys().filter(|&&(k, _)| k == kind).count() > 100) {
            return Err(SqliteAccountError::invalid_data(
                "family5",
                "Each native override list supports at most 100 rows, including preserved rows.",
            ));
        }
        *self = candidate;
        Ok(())
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
            super::package::compact(db, "family5", &format!("kind={kind}"))
                .map_err(|error| SqliteAccountError::invalid_data("family5", error))?;
        }
        Ok(())
    }
}
fn visible_family(kind: i32, slot: i32, value: i32) -> bool {
    match kind {
        0 => (0..=23_499).contains(&slot) && (0..=2).contains(&value),
        1 => (0..=15_499).contains(&slot),
        _ => false,
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
                row.get(*name) == Some(&super::package::Cell::Integer(i64::from(*value)))
            })
        })
        .cloned()
        .unwrap_or_default();
    for &(name, value) in keys.iter().chain(values) {
        row.insert(name.into(), super::package::Cell::Integer(i64::from(value)));
    }
    super::writer::insert(db, table, row)
}
fn compress(slots: Vec<i32>) -> Vec<Value> {
    let mut ranges: Vec<(i32, i32)> = Vec::new();
    for slot in slots {
        if let Some((start, length)) = ranges.last_mut()
            && *start + *length == slot
        {
            *length += 1;
        } else {
            ranges.push((slot, 1));
        }
    }
    ranges
        .into_iter()
        .map(|(start, length)| json!([start, length]))
        .collect()
}
fn decode(bank: usize, value: &Value) -> Result<BTreeMap<(i32, i32), i32>, SqliteAccountError> {
    let rows = value.as_array().ok_or_else(invalid)?;
    let mut result = BTreeMap::new();
    for row in rows {
        if bank == 2 {
            result.insert((integer(row)?, 0), FLAG_SET);
            continue;
        }
        let slot = integer(&row[0])?;
        if matches!(bank, 0 | 1 | 4) {
            let length = integer(&row[1])?;
            if slot < 0 || length < 0 || slot.checked_add(length).is_none_or(|end| end > 65536) {
                return Err(invalid());
            }
            for i in slot..slot + length {
                result.insert((i, 0), FLAG_SET);
            }
        } else if matches!(bank, 3 | 5) {
            result.insert((slot, 0), integer(&row[1])?);
        } else {
            for lane in 0..3 {
                result.insert((slot, lane), integer(&row[(lane + 1) as usize])?);
            }
        }
    }
    Ok(result)
}
fn integer(value: &Value) -> Result<i32, SqliteAccountError> {
    value
        .as_i64()
        .and_then(|v| i32::try_from(v).ok())
        .ok_or_else(invalid)
}
fn invalid() -> SqliteAccountError {
    SqliteAccountError::invalid_data("unlocks", "invalid progression edit")
}
fn sql(error: rusqlite::Error) -> SqliteAccountError {
    SqliteAccountError::sqlite("edit progression in", error)
}

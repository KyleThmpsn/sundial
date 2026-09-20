//! Dawn schema 5 storage for the shared progression editor.
//! Scope and owner mapping follow Dawn's read_unlocks/write_unlocks contract.
use std::collections::BTreeMap;

use rusqlite::{Connection, params};
use serde_json::Value;

use super::{DawnAccountDocument, contract, error::DawnAccountError};
use crate::persistence::native_account::progression::Progression as State;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Progression {
    state: State,
    account: String,
    characters: BTreeMap<i32, String>,
}

// Shared editor bank, Dawn table, Dawn scope, capacity, index column.
const BANKS: [(i32, &str, i32, i32, &str); 8] = [
    (0, "durable_flags", 0, 12_300, "slot"),
    (1, "durable_flags", 1, 512, "slot"),
    (2, "durable_flags", 2, 256, "slot"),
    (3, "durable_objectives", 0, 6_200, "slot"),
    (4, "durable_flags", 3, 4_096, "slot"),
    (5, "durable_objectives", 3, 768, "slot"),
    (6, "durable_progressions", 0, 256, "definition_index"),
    (7, "durable_progressions", 2, 256, "definition_index"),
];

fn invalid(detail: impl Into<String>) -> DawnAccountError {
    DawnAccountError::Unwritable(format!("Dawn progression: {}", detail.into()))
}

impl Progression {
    pub(super) fn load(db: &Connection) -> Result<Self, DawnAccountError> {
        let account: String =
            db.query_row("SELECT primary_soid FROM account WHERE id=1", [], |r| {
                r.get(0)
            })?;
        let characters: BTreeMap<i32, String> = db
            .prepare("SELECT position,soid FROM characters ORDER BY position")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<_, _>>()?;
        let mut result = Self {
            state: State {
                character_slots: characters.keys().copied().collect(),
                ..State::default()
            },
            account,
            characters,
        };
        for table in [
            "durable_flags",
            "durable_objectives",
            "durable_progressions",
        ] {
            let (column, lane) = if table == "durable_progressions" {
                ("definition_index", "lane")
            } else {
                ("slot", "0")
            };
            let mut query = db.prepare(&format!(
                "SELECT scope,owner_soid,{column},{lane},value FROM {table}"
            ))?;
            let rows = query.query_map([], |r| {
                Ok((
                    r.get::<_, i32>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i32>(2)?,
                    r.get::<_, i32>(3)?,
                    r.get::<_, i32>(4)?,
                ))
            })?;
            for row in rows {
                let (scope, owner, slot, lane, value) = row?;
                let &(bank, _, _, capacity, _) = BANKS
                    .iter()
                    .find(|(_, t, s, _, _)| *t == table && *s == scope)
                    .ok_or_else(|| invalid(format!("unsupported scope {scope} in {table}")))?;
                let character = if matches!(scope, 0 | 1) {
                    if contract::parse_soid(&owner) != contract::parse_soid(&result.account) {
                        return Err(invalid("account owner does not match"));
                    }
                    -1
                } else {
                    *result
                        .characters
                        .iter()
                        .find(|(_, soid)| {
                            contract::parse_soid(soid) == contract::parse_soid(&owner)
                        })
                        .map(|(slot, _)| slot)
                        .ok_or_else(|| invalid("character owner does not exist"))?
                };
                if !(0..capacity).contains(&slot)
                    || !(0..if bank >= 6 { 3 } else { 1 }).contains(&lane)
                    || (table == "durable_flags" && !(0..=255).contains(&value))
                {
                    return Err(invalid(format!("invalid slot, lane or value in {table}")));
                }
                if result
                    .state
                    .unlocks
                    .insert((character, bank, slot, lane), value)
                    .is_some()
                {
                    return Err(invalid("duplicate progression owner"));
                }
            }
        }
        for (kind, table) in [(0, "family5_flags"), (1, "family5_values")] {
            let mut query = db.prepare(&format!("SELECT slot,value FROM {table}"))?;
            let rows = query.query_map([], |r| Ok((r.get::<_, i32>(0)?, r.get::<_, i32>(1)?)))?;
            let mut count = 0;
            for row in rows {
                let (slot, value) = row?;
                count += 1;
                if count > 100
                    || !(0..=65535).contains(&slot)
                    || (kind == 0 && !(0..=255).contains(&value))
                {
                    return Err(invalid(format!("invalid override in {table}")));
                }
                result.state.family.insert((kind, slot), value);
            }
        }
        Ok(result)
    }

    pub(super) fn save(&self, db: &Connection, before: &Self) -> Result<(), DawnAccountError> {
        // Some Dawn sparse writes can happen independently of the account graph. Compare the
        // loaded progression too, so a stale editor cannot overwrite a newly earned unlock.
        if Self::load(db)? != *before {
            return Err(invalid(
                "progression changed while it was open. Reload before saving.",
            ));
        }
        for (&key, &value) in &self.state.unlocks {
            if before.state.unlocks.get(&key) != Some(&value) {
                self.write_row(db, key, Some(value))?;
            }
        }
        for &key in before.state.unlocks.keys() {
            if !self.state.unlocks.contains_key(&key) {
                self.write_row(db, key, None)?;
            }
        }
        for (kind, table) in [(0, "family5_flags"), (1, "family5_values")] {
            for (&(k, slot), &value) in &self.state.family {
                if k == kind && before.state.family.get(&(k, slot)) != Some(&value) {
                    db.execute(&format!("INSERT INTO {table}(slot,value) VALUES(?1,?2) ON CONFLICT(slot) DO UPDATE SET value=excluded.value"), params![slot,value])?;
                }
            }
            for &(k, slot) in before.state.family.keys() {
                if k == kind && !self.state.family.contains_key(&(k, slot)) {
                    db.execute(&format!("DELETE FROM {table} WHERE slot=?1"), [slot])?;
                }
            }
        }
        Ok(())
    }

    fn write_row(
        &self,
        db: &Connection,
        (character, bank, slot, lane): (i32, i32, i32, i32),
        value: Option<i32>,
    ) -> Result<(), DawnAccountError> {
        let &(_, table, scope, capacity, column) = BANKS
            .get(bank as usize)
            .ok_or_else(|| invalid("invalid bank"))?;
        let owner = if matches!(scope, 0 | 1) {
            &self.account
        } else {
            self.characters
                .get(&character)
                .ok_or_else(|| invalid("select a valid character"))?
        };
        if !(0..capacity).contains(&slot)
            || !(0..if bank >= 6 { 3 } else { 1 }).contains(&lane)
            || (table == "durable_flags" && value.is_some_and(|v| !(0..=255).contains(&v)))
        {
            return Err(invalid("invalid slot, lane or value"));
        }
        if bank >= 6 {
            if let Some(value) = value {
                if db.execute(&format!("UPDATE {table} SET value=?5 WHERE scope=?1 AND upper(owner_soid)=upper(?2) AND {column}=?3 AND lane=?4"), params![scope,owner,slot,lane,value])? == 0 {
                    db.execute(&format!("INSERT INTO {table}(scope,owner_soid,{column},lane,value) VALUES(?1,?2,?3,?4,?5)"), params![scope,owner,slot,lane,value])?;
                }
            } else {
                db.execute(&format!("DELETE FROM {table} WHERE scope=?1 AND upper(owner_soid)=upper(?2) AND {column}=?3 AND lane=?4"), params![scope,owner,slot,lane])?;
            }
        } else if let Some(value) = value {
            if db.execute(&format!("UPDATE {table} SET value=?4 WHERE scope=?1 AND upper(owner_soid)=upper(?2) AND {column}=?3"), params![scope,owner,slot,value])? == 0 {
                db.execute(&format!("INSERT INTO {table}(scope,owner_soid,{column},value) VALUES(?1,?2,?3,?4)"), params![scope,owner,slot,value])?;
            }
        } else {
            db.execute(&format!("DELETE FROM {table} WHERE scope=?1 AND upper(owner_soid)=upper(?2) AND {column}=?3"), params![scope,owner,slot])?;
        }
        Ok(())
    }
}

impl DawnAccountDocument {
    /// The unlock banks alone, as the shared progression editor reads and writes them. Two views
    /// differ only when progression itself changed, which is what a change summary asks.
    pub(crate) fn progression_state_view(&self, index: usize) -> Value {
        let mut view = self.progression.state.view(index);
        view["_native_progression"]["runtime"] = Value::from("dawn");
        view
    }

    /// The state view plus read-only provenance for the inspector: the vendor and mission rows
    /// this account and the selected character hold. Never applied back; `apply` reads only the
    /// unlock banks.
    pub(crate) fn progression_view(&self, index: usize) -> Value {
        let mut view = self.progression_state_view(index);
        let account = self.progression.account.as_str();
        let character = i32::try_from(index)
            .ok()
            .and_then(|index| self.progression.characters.get(&index))
            .map(String::as_str);
        let scope_of = |owner: &str| {
            if owner.eq_ignore_ascii_case(account) {
                Some("Account")
            } else if character.is_some_and(|soid| owner.eq_ignore_ascii_case(soid)) {
                Some("Character")
            } else {
                None
            }
        };
        let activity = self.activity_state();
        let vendors = activity
            .vendors
            .iter()
            .filter_map(|row| {
                let scope = scope_of(&row.owner)?;
                Some(serde_json::json!({
                    "scope": scope,
                    "vendor": row.vendor,
                    "points": row.points,
                    "rewards": row.rewards,
                }))
            })
            .collect::<Vec<_>>();
        let missions = activity
            .missions
            .iter()
            .filter_map(|row| {
                let scope = scope_of(&row.owner)?;
                Some(serde_json::json!({
                    "scope": scope,
                    "hash": row.hash,
                    "completed": row.completed,
                    "progress": row.progress,
                    "activity": row.activity,
                    "checkpoint": row.checkpoint,
                }))
            })
            .collect::<Vec<_>>();
        view["_dawn_activity"] = serde_json::json!({ "vendors": vendors, "missions": missions });
        view
    }

    pub(crate) fn apply_progression_view(
        &mut self,
        index: usize,
        view: &Value,
    ) -> Result<(), String> {
        crate::persistence::progression::validate(view)?;
        if index >= self.progression.state.character_slots.len() {
            return Err("Select a valid Dawn character before editing progression".into());
        }
        if view.get("_progression_rewards").is_some()
            || view.get("_progression_consumables").is_some()
        {
            return Err(
                "Dawn progression reward claims are not supported by Sundial. Claim rewards in game.".into(),
            );
        }
        self.progression.state.apply(index, view)
    }
}

#[cfg(test)]
mod tests;

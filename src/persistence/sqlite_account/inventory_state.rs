//! Character materials and delivery state outside instanced inventory.
use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, params};
use serde::Serialize;
use sundial_account::EntityId;

use super::{
    SqliteAccountError,
    writer::{insert, matching, put},
};

mod rewards;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct CharacterStack {
    pub definition_hash: u32,
    pub quantity: i32,
    pub mutation_serial: i32,
    pub(super) original_position: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct PendingReward {
    pub id: i64,
    pub character_slot: usize,
    pub kind: u8,
    pub definition_hash: u32,
    pub quantity: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub(super) struct InventoryState {
    pub stacks: BTreeMap<usize, Vec<CharacterStack>>,
    pub rewards: Vec<PendingReward>,
    pub last_reward_id: i64,
    pub seen_items: BTreeMap<u64, bool>,
    pub seen_profile: BTreeMap<EntityId, bool>,
}

impl InventoryState {
    pub fn load(db: &Connection) -> Result<Self, SqliteAccountError> {
        let mut result = Self::default();
        let mut statement = db.prepare("SELECT character_slot, position, definition_hash, quantity, mutation_serial FROM character_stacks ORDER BY character_slot, position").map_err(sql)?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, usize>(0)?,
                    CharacterStack {
                        original_position: Some(row.get(1)?),
                        definition_hash: row.get(2)?,
                        quantity: row.get(3)?,
                        mutation_serial: row.get(4)?,
                    },
                ))
            })
            .map_err(sql)?;
        for row in rows {
            let (slot, stack) = row.map_err(sql)?;
            result.stacks.entry(slot).or_default().push(stack);
        }
        result.rewards = rewards::load(db)?;
        result.last_reward_id = db
            .query_row(
                "SELECT COALESCE((SELECT seq FROM sqlite_sequence WHERE name='pending_rewards'), 0)",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map_err(sql)?
            .max(result.rewards.iter().map(|reward| reward.id).max().unwrap_or(0));
        Ok(result)
    }

    pub fn save(
        &self,
        db: &rusqlite::Transaction<'_>,
        old: &[super::writer::NativeRow],
    ) -> Result<(), SqliteAccountError> {
        rewards::save(db, &self.rewards)?;
        // Keep unknown columns with the row that owned them, even after a removal compacts it.
        db.execute("DELETE FROM character_stacks", [])
            .map_err(sql)?;
        for (&slot, stacks) in &self.stacks {
            if stacks.len() > 32 {
                return Err(invalid("A character can hold at most 32 material stacks"));
            }
            let mut hashes = BTreeSet::new();
            for (position, stack) in stacks.iter().enumerate() {
                if stack.quantity <= 0
                    || stack.mutation_serial < 0
                    || stack.definition_hash == 0x811C9DC5
                    || !hashes.insert(stack.definition_hash)
                {
                    return Err(invalid("Invalid or duplicate character material"));
                }
                let mut row = stack
                    .original_position
                    .map(|original| {
                        matching(
                            old,
                            &[
                                ("character_slot", slot as i64),
                                ("position", original as i64),
                            ],
                        )
                    })
                    .unwrap_or_default();
                put(&mut row, "character_slot", slot as i64);
                put(&mut row, "position", position as i64);
                put(
                    &mut row,
                    "definition_hash",
                    i64::from(stack.definition_hash),
                );
                put(&mut row, "quantity", stack.quantity);
                put(&mut row, "mutation_serial", stack.mutation_serial);
                insert(db, "character_stacks", row)?;
            }
        }
        for (&soid, &seen) in &self.seen_items {
            db.execute(
                "UPDATE items SET seen=? WHERE instance_soid=?",
                params![seen, soid as i64],
            )
            .map_err(sql)?;
        }
        Ok(())
    }

    pub fn refresh_positions(&mut self) {
        self.seen_items.clear();
        self.seen_profile.clear();
        for stacks in self.stacks.values_mut() {
            for (position, stack) in stacks.iter_mut().enumerate() {
                stack.original_position = Some(position);
            }
        }
    }

    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({
            "character_materials": self.stacks,
            "pending_rewards": self.rewards,
            "item_seen": self.seen_items,
            "profile_seen": self.seen_profile.iter().map(|(id, seen)| (id.get().to_string(), seen)).collect::<BTreeMap<_, _>>(),
        })
    }
}

pub(super) fn invalid(message: &str) -> SqliteAccountError {
    SqliteAccountError::invalid_data("character_stacks", message)
}
fn sql(error: rusqlite::Error) -> SqliteAccountError {
    SqliteAccountError::sqlite("access inventory state in", error)
}

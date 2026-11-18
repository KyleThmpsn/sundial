//! Character materials and delivery state outside instanced inventory.
use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, params};
use serde::Serialize;
use sundial_account::EntityId;

use super::{
    Schema, SunriseAccountError, snapshot,
    writer::{Preserved, insert, put},
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
    /// How the reward is delivered: 0 for a character item, 1 for a shared stack. Absent on
    /// schema 7, which stores no kind and places every reward by its definition.
    pub kind: Option<u8>,
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
    pub fn load(db: &Connection, schema: Schema) -> Result<Self, SunriseAccountError> {
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
        result.rewards = rewards::load(db, schema)?;
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
        schema: Schema,
        old: &[super::writer::NativeRow],
        old_rewards: &[super::writer::NativeRow],
    ) -> Result<(), SunriseAccountError> {
        rewards::save(db, schema, &self.rewards, old_rewards)?;
        // Keep unknown columns with the row that owned them, even after a removal compacts it.
        let old = Preserved::new(old, &["character_slot", "position"]);
        db.execute("DELETE FROM character_stacks", [])
            .map_err(sql)?;
        let capacity = schema.stack_capacity();
        for (&slot, stacks) in &self.stacks {
            if stacks.len() > capacity {
                return Err(invalid(&format!(
                    "A character can hold at most {capacity} material stacks"
                )));
            }
            // Sunrise tells a character's stacks apart by material before schema 7 and by
            // mutation serial from it on, when one material may fill several stacks.
            let mut identities = BTreeSet::new();
            for (position, stack) in stacks.iter().enumerate() {
                let identity = if schema.has_placement() {
                    i64::from(stack.mutation_serial)
                } else {
                    i64::from(stack.definition_hash)
                };
                if stack.quantity <= 0
                    || stack.mutation_serial < 0
                    || stack.definition_hash == 0x811C9DC5
                    || !identities.insert(identity)
                {
                    return Err(invalid("Invalid or duplicate character material"));
                }
                let mut row = stack
                    .original_position
                    .map(|original| old.get(&[slot as i64, original as i64]))
                    .unwrap_or_default();
                if schema.has_placement() {
                    // A stack added here starts in the bucket its definition authors.
                    row.entry("placement".into())
                        .or_insert(snapshot::Cell::Integer(0));
                }
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

pub(super) fn invalid(message: &str) -> SunriseAccountError {
    SunriseAccountError::invalid_data("character_stacks", message)
}
fn sql(error: rusqlite::Error) -> SunriseAccountError {
    SunriseAccountError::sqlite("access inventory state in", error)
}

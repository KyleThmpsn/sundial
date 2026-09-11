use super::super::{CharacterStack, PendingReward, inventory_state::invalid, package::Cell};
use super::*;

impl SqliteAccountDocument {
    pub(crate) fn character_stacks(&self, index: usize) -> &[CharacterStack] {
        self.inventory_state
            .stacks
            .get(&index)
            .map_or(&[], Vec::as_slice)
    }

    pub(crate) fn add_character_stack(
        &mut self,
        index: usize,
        definition_hash: u32,
        quantity: i32,
    ) -> Result<(), SqliteAccountError> {
        if index >= self.characters.characters().len()
            || definition_hash == 0
            || definition_hash == 0x811C9DC5
            || quantity <= 0
        {
            return Err(invalid("Invalid character material"));
        }
        let rows = self.inventory_state.stacks.entry(index).or_default();
        if rows.len() >= 32 {
            return Err(invalid("This character already has 32 material stacks"));
        }
        if rows
            .iter()
            .any(|row| row.definition_hash == definition_hash)
        {
            return Err(invalid(
                "This material is already in the character inventory",
            ));
        }
        rows.push(CharacterStack {
            definition_hash,
            quantity,
            mutation_serial: 0,
            original_position: None,
        });
        Ok(())
    }

    pub(crate) fn set_character_stack_quantity(
        &mut self,
        index: usize,
        position: usize,
        quantity: i32,
    ) -> Result<(), SqliteAccountError> {
        if quantity <= 0 {
            return Err(invalid("Material quantity must be positive"));
        }
        let row = self
            .inventory_state
            .stacks
            .get_mut(&index)
            .and_then(|rows| rows.get_mut(position))
            .ok_or_else(|| invalid("The material no longer exists"))?;
        if row.quantity != quantity {
            row.mutation_serial = row
                .mutation_serial
                .checked_add(1)
                .ok_or_else(|| invalid("The material mutation serial is exhausted"))?;
            row.quantity = quantity;
        }
        Ok(())
    }

    pub(crate) fn remove_character_stack(
        &mut self,
        index: usize,
        position: usize,
    ) -> Result<(), SqliteAccountError> {
        let rows = self
            .inventory_state
            .stacks
            .get_mut(&index)
            .filter(|rows| position < rows.len())
            .ok_or_else(|| invalid("The material no longer exists"))?;
        rows.remove(position);
        Ok(())
    }

    pub(crate) fn pending_rewards(&self) -> &[PendingReward] {
        &self.inventory_state.rewards
    }

    pub(crate) fn add_pending_reward(
        &mut self,
        character_slot: usize,
        kind: u8,
        definition_hash: u32,
        quantity: i32,
    ) -> Result<(), SqliteAccountError> {
        if character_slot >= self.characters.characters().len()
            || !matches!(kind, 0 | 1)
            || matches!(definition_hash, 0 | 0x811C9DC5)
            || quantity <= 0
            || (kind == 0 && quantity != 1)
        {
            return Err(invalid_reward("Invalid reward, quantity or character"));
        }
        let id = self
            .inventory_state
            .last_reward_id
            .checked_add(1)
            .ok_or_else(|| invalid_reward("The reward ID sequence is exhausted"))?;
        self.inventory_state.rewards.push(PendingReward {
            id,
            character_slot,
            kind,
            definition_hash,
            quantity,
        });
        self.inventory_state.last_reward_id = id;
        Ok(())
    }

    pub(crate) fn set_pending_reward_quantity(
        &mut self,
        id: i64,
        quantity: i32,
    ) -> Result<(), SqliteAccountError> {
        let reward = self
            .inventory_state
            .rewards
            .iter_mut()
            .find(|reward| reward.id == id)
            .ok_or_else(|| invalid_reward("The reward no longer exists"))?;
        if reward.kind != 1 || quantity <= 0 {
            return Err(invalid_reward(
                "Only shared rewards can have a stack quantity",
            ));
        }
        reward.quantity = quantity;
        Ok(())
    }

    pub(crate) fn remove_pending_reward(&mut self, id: i64) -> Result<(), SqliteAccountError> {
        let index = self
            .inventory_state
            .rewards
            .iter()
            .position(|reward| reward.id == id)
            .ok_or_else(|| invalid_reward("The reward no longer exists"))?;
        self.inventory_state.rewards.remove(index);
        Ok(())
    }

    pub(crate) fn item_seen(&self, soid: u64) -> Option<bool> {
        self.characters
            .characters()
            .iter()
            .flat_map(|character| {
                character
                    .inventory
                    .iter()
                    .chain(character.equipment.values().filter_map(Option::as_ref))
            })
            .find(|item| item.instance_soid.get() == soid)?;
        Some(
            self.inventory_state
                .seen_items
                .get(&soid)
                .copied()
                .unwrap_or_else(|| self.original_item_seen(soid)),
        )
    }

    pub(crate) fn set_item_seen(
        &mut self,
        soid: u64,
        seen: bool,
    ) -> Result<(), SqliteAccountError> {
        let current = self
            .item_seen(soid)
            .ok_or_else(|| invalid("The item no longer exists"))?;
        if current != seen {
            if seen == self.original_item_seen(soid) {
                self.inventory_state.seen_items.remove(&soid);
            } else {
                self.inventory_state.seen_items.insert(soid, seen);
            }
        }
        Ok(())
    }

    pub(crate) fn profile_item_seen(&self, position: usize) -> Option<bool> {
        let item = self.profile.profile_items().get(position)?;
        Some(
            self.inventory_state
                .seen_profile
                .get(&item.id)
                .copied()
                .unwrap_or_else(|| self.original_profile_seen(item.id)),
        )
    }

    pub(crate) fn set_profile_item_seen(
        &mut self,
        position: usize,
        seen: bool,
    ) -> Result<(), SqliteAccountError> {
        let current = self
            .profile_item_seen(position)
            .ok_or_else(|| invalid("The shared item no longer exists"))?;
        if current != seen {
            let id = self.profile.profile_items()[position].id;
            if seen == self.original_profile_seen(id) {
                self.inventory_state.seen_profile.remove(&id);
            } else {
                self.inventory_state.seen_profile.insert(id, seen);
            }
        }
        Ok(())
    }

    pub(in super::super) fn profile_seen_override(&self, id: EntityId) -> Option<bool> {
        self.inventory_state.seen_profile.get(&id).copied()
    }

    fn original_item_seen(&self, soid: u64) -> bool {
        self.original_seen("items", "instance_soid", soid as i64, false)
    }

    fn original_profile_seen(&self, id: EntityId) -> bool {
        self.original_profile_position(id).is_none_or(|position| {
            self.original_seen("profile_items", "position", position as i64, true)
        })
    }

    fn original_seen(&self, table: &str, key: &str, value: i64, default: bool) -> bool {
        self.preserved_rows(table)
            .iter()
            .find(|row| row.get(key) == Some(&Cell::Integer(value)))
            .map_or(default, |row| row.get("seen") == Some(&Cell::Integer(1)))
    }

    pub(in super::super) fn save_inventory_state(
        &self,
        db: &rusqlite::Transaction<'_>,
    ) -> Result<(), SqliteAccountError> {
        self.inventory_state
            .save(db, self.preserved_rows("character_stacks"))
    }

    pub(crate) fn inventory_state_summary(&self) -> serde_json::Value {
        self.inventory_state.summary()
    }
}

fn invalid_reward(message: &str) -> SqliteAccountError {
    SqliteAccountError::invalid_data("pending_rewards", message)
}

//! Direct consumable rewards share the progression edit's commit boundary.
use super::*;

impl SqliteAccountDocument {
    pub(super) fn apply_consumable_rewards(
        &mut self,
        index: usize,
        value: &serde_json::Value,
    ) -> Result<(), SqliteAccountError> {
        let Some(value) = value.get("_progression_consumables") else {
            return Ok(());
        };
        let rows = value
            .as_array()
            .ok_or_else(|| invalid("Invalid consumable reward plan"))?;
        for row in rows {
            let hash = row["hash"]
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .filter(|hash| !matches!(hash, 0 | 0x811C9DC5))
                .ok_or_else(|| invalid("Invalid consumable reward"))?;
            let quantity = positive(&row["quantity"])?;
            let maximum = positive(&row["maximum"])?;
            self.grant_consumable(index, hash, quantity, maximum)?;
        }
        Ok(())
    }

    fn grant_consumable(
        &mut self,
        index: usize,
        hash: u32,
        quantity: i32,
        maximum: i32,
    ) -> Result<(), SqliteAccountError> {
        let character = self
            .characters
            .characters()
            .get(index)
            .ok_or_else(|| invalid("The reward character is unavailable"))?;
        let character_id = character.id;
        if character
            .equipment
            .values()
            .flatten()
            .any(|item| item.definition_hash.get() == hash)
        {
            return Err(invalid("The consumable is in an equipment slot"));
        }
        let stored = self
            .character_stacks(index)
            .iter()
            .enumerate()
            .find(|(_, item)| item.definition_hash == hash)
            .map(|(position, item)| (position, item.quantity));
        let inventory = character
            .inventory
            .iter()
            .filter(|item| item.definition_hash.get() == hash)
            .map(|item| (item.id, item.quantity))
            .collect::<Vec<_>>();
        if inventory.len() + usize::from(stored.is_some()) > 1 {
            return Err(invalid("This consumable already occupies multiple stacks"));
        }
        let before = stored
            .map(|(_, quantity)| quantity)
            .or_else(|| inventory.first().map(|(_, quantity)| *quantity))
            .unwrap_or(0);
        let after = before
            .checked_add(quantity)
            .filter(|value| before >= 0 && *value <= maximum)
            .ok_or_else(|| invalid("The consumable reward exceeds its stack cap"))?;
        if let Some(&(item_id, _)) = inventory.first() {
            self.characters
                .apply(
                    Self::character_capabilities(),
                    sundial_account::CharacterCommand::UpdateInventoryItem {
                        item_id,
                        update: sundial_account::ItemUpdate::SetQuantity(after),
                    },
                )
                .map_err(|error| invalid(&error.to_string()))?;
            let serial = self.reserve_consumable_serial(character_id)?;
            let abilities = self
                .pending_item_abilities
                .remove(&item_id)
                .unwrap_or(DEFAULT_ABILITIES);
            self.item_persistence
                .entry(item_id)
                .or_insert(ItemPersistence {
                    mutation_serial: serial,
                    abilities,
                })
                .mutation_serial = serial;
        } else {
            let position = if let Some((position, _)) = stored {
                self.set_character_stack_quantity(index, position, after)?;
                position
            } else {
                self.add_character_stack(index, hash, after)?;
                self.character_stacks(index).len() - 1
            };
            let serial = self.reserve_consumable_serial(character_id)?;
            self.inventory_state
                .stacks
                .get_mut(&index)
                .expect("reward stack")[position]
                .mutation_serial = serial;
        }
        Ok(())
    }

    fn reserve_consumable_serial(
        &mut self,
        character_id: EntityId,
    ) -> Result<i32, SqliteAccountError> {
        let metadata = self
            .character_persistence
            .get_mut(&character_id)
            .ok_or_else(|| invalid("The reward character metadata is unavailable"))?;
        let serial = i32::try_from(metadata.next_inventory_serial)
            .ok()
            .filter(|serial| *serial < i32::MAX)
            .ok_or_else(|| invalid("The character inventory serial is exhausted"))?;
        metadata.next_inventory_serial += 1;
        Ok(serial)
    }
}

fn positive(value: &serde_json::Value) -> Result<i32, SqliteAccountError> {
    value
        .as_i64()
        .and_then(|n| i32::try_from(n).ok())
        .filter(|n| *n > 0)
        .ok_or_else(|| invalid("Invalid consumable quantity or stack cap"))
}

fn invalid(message: &str) -> SqliteAccountError {
    SqliteAccountError::invalid_data("character_stacks", message)
}

//! New editor items must not reuse Dawn's durable identities or their carried roll state.
use super::{DawnAccountDocument, InstanceSoid, contract};
use crate::persistence::native_account::NativeAccountDocument;
use sundial_account::{CharacterCommand, EquipmentSlot};

impl DawnAccountDocument {
    pub(super) fn available_item_identity(&self) -> Result<InstanceSoid, String> {
        let exhausted = || "Dawn has no item identity left to allocate".to_owned();
        let carried_max = self
            .carried
            .item_mutation_serials
            .keys()
            .filter_map(|key| contract::parse_soid(key))
            .max();
        let mut next = self.allocators.item.max(contract::FIRST_ITEM_SOID);
        if let Some(carried) = carried_max {
            next = next.max(carried.checked_add(1).ok_or_else(exhausted)?);
        }
        loop {
            let candidate = self
                .characters()
                .next_available_instance_soid(
                    InstanceSoid::try_from_u64(next).ok_or_else(exhausted)?,
                )
                .map_err(|e| e.to_string())?;
            // Leave room for Dawn's next-identity cursor, even for an imported allocator.
            next = candidate.get().checked_add(1).ok_or_else(exhausted)?;
            if !self
                .profile()
                .profile_items()
                .iter()
                .any(|item| item.instance_soid == Some(candidate))
            {
                return Ok(candidate);
            }
        }
    }

    /// Randomization replaces the item, unlike editing its existing definition in place.
    /// The caller keeps a candidate document so an entire loadout is applied atomically.
    pub(crate) fn renew_generated_equipment(
        &mut self,
        index: usize,
        slot: &str,
    ) -> Result<(), String> {
        let character = self
            .characters()
            .characters()
            .get(index)
            .ok_or("Choose a character")?;
        let slot = EquipmentSlot::new(slot);
        let Some(mut item) = character
            .equipment
            .get(&slot)
            .and_then(Option::as_ref)
            .cloned()
        else {
            return Ok(());
        };
        let character_id = character.id;
        let identity = self.next_item_identity()?;
        item.instance_soid = identity;
        item.id = self.next_entity_id()?;
        self.characters_mut()
            .apply(
                Self::character_capabilities(),
                CharacterCommand::SetEquipmentItem {
                    character_id,
                    slot,
                    item: Some(item),
                },
            )
            .map_err(|e| e.to_string())?;
        self.observe_item_identity(identity);
        Ok(())
    }
}

//! Postmaster recovery uses the installed destination bucket and stack limits.
use super::DawnAccountDocument;
use crate::catalog::{InventoryMetadata, InventoryScope};
use sundial_account::{CharacterState, ProfileItem, ProfileState};

impl DawnAccountDocument {
    pub(crate) fn preview_postmaster_recovery(
        &self,
        index: usize,
        soid: u64,
        quantity: i32,
        metadata: impl Fn(u32) -> Option<InventoryMetadata>,
    ) -> Result<Self, String> {
        let mut candidate = self.clone();
        candidate.recover_inner(index, soid, quantity, &metadata)?;
        Ok(candidate)
    }

    pub(crate) fn is_postmaster(&self, soid: u64) -> bool {
        self.carried
            .postmaster
            .iter()
            .any(|key| u64::from_str_radix(key, 16).ok() == Some(soid))
    }

    pub(crate) fn recover_postmaster(
        &mut self,
        index: usize,
        soid: u64,
        quantity: i32,
        metadata: impl Fn(u32) -> Option<InventoryMetadata>,
    ) -> Result<(), String> {
        *self = self.preview_postmaster_recovery(index, soid, quantity, metadata)?;
        Ok(())
    }

    fn recover_inner(
        &mut self,
        index: usize,
        soid: u64,
        quantity: i32,
        metadata: &impl Fn(u32) -> Option<InventoryMetadata>,
    ) -> Result<(), String> {
        let mut characters = self.characters().characters().to_vec();
        let character = characters.get_mut(index).ok_or("Choose a character")?;
        let at = character
            .inventory
            .iter()
            .position(|r| r.instance_soid.get() == soid)
            .ok_or("The Postmaster item is no longer present")?;
        let item = character.inventory[at].clone();
        if !self.is_postmaster(soid) || quantity <= 0 || quantity > item.quantity {
            return Err("Choose a valid Postmaster quantity".into());
        }
        let hash = item.definition_hash.get();
        let info = metadata(hash).ok_or("The installed item definition is unavailable")?;
        let capacity = usize::from(
            info.bucket_capacity
                .ok_or("The destination capacity is unknown")?,
        );
        match info.scope {
            InventoryScope::Character => {
                if quantity != item.quantity {
                    return Err("Character items must be recovered as a whole stack".into());
                }
                if info.max_stack_size.is_none_or(|max| quantity as u32 > max)
                    || (info.is_instanced_character_candidate() && quantity != 1)
                {
                    return Err(
                        "The recovery stack exceeds the installed item's quantity limit".into(),
                    );
                }
                let mut occupied = 0;
                for row in character
                    .inventory
                    .iter()
                    .chain(character.equipment.values().flatten())
                {
                    if self.is_postmaster(row.instance_soid.get()) {
                        continue;
                    }
                    let other = metadata(row.definition_hash.get())
                        .ok_or("An inventory item has an unknown destination")?;
                    occupied += usize::from(other.native_bucket_id == info.native_bucket_id);
                }
                if !info.is_character_inventory_candidate() || occupied >= capacity {
                    return Err("The destination bucket is full or unavailable".into());
                }
            }
            InventoryScope::Profile => {
                if !info.is_profile_items_candidate() {
                    return Err("This item cannot be recovered into profile inventory".into());
                }
                let max = i64::from(info.max_stack_size.ok_or("The stack limit is unknown")?);
                let mut profile = self.profile().profile_items().to_vec();
                let serial = self
                    .carried
                    .profile_rows
                    .values()
                    .map(|row| row.serial)
                    .max()
                    .unwrap_or_default();
                if !(0..i64::from(i32::MAX)).contains(&serial) {
                    return Err("Profile inventory has no mutation serial left to allocate".into());
                }
                let mut occupied = 0;
                let mut held = 0_i64;
                let mut matching = 0;
                for row in &profile {
                    let other = metadata(row.definition_hash.get())
                        .ok_or("A profile item has an unknown destination")?;
                    occupied += usize::from(other.native_bucket_id == info.native_bucket_id);
                    if row.definition_hash == item.definition_hash {
                        held += i64::from(row.quantity);
                        matching += 1;
                    }
                }
                if matching > 1 {
                    return Err(
                        "Dawn cannot recover into duplicate profile stacks for this item".into(),
                    );
                }
                if held + i64::from(quantity) > max {
                    return Err("There is not enough room in the destination stack".into());
                }
                if let Some(row) = profile
                    .iter_mut()
                    .find(|r| r.definition_hash == item.definition_hash)
                {
                    row.quantity = row
                        .quantity
                        .checked_add(quantity)
                        .ok_or("The destination stack would overflow")?;
                    self.carried
                        .profile_rows
                        .entry(row.id)
                        .or_insert_with(|| super::carried::ProfileRow {
                            soid: super::contract::format_soid(
                                row.instance_soid.map_or(0, |s| s.get()),
                            ),
                            serial: 0,
                        })
                        .serial = serial + 1;
                } else {
                    if occupied >= capacity {
                        return Err("The destination bucket is full".into());
                    }
                    let id = self.next_entity_id()?;
                    self.carried.profile_rows.insert(
                        id,
                        super::carried::ProfileRow {
                            soid: super::contract::format_soid(0),
                            serial: serial + 1,
                        },
                    );
                    profile.push(ProfileItem {
                        id,
                        instance_soid: None,
                        definition_hash: item.definition_hash,
                        quantity,
                    });
                }
                self.snapshot.profile = ProfileState::try_new(
                    Self::profile_capabilities(),
                    profile,
                    self.profile().dismantle_rewards().to_vec(),
                )
                .map_err(|e| e.to_string())?;
                character.inventory[at].quantity -= quantity;
                if character.inventory[at].quantity == 0 {
                    character.inventory.remove(at);
                }
            }
            _ => return Err("This Postmaster destination is not supported by Dawn".into()),
        }
        self.bump_inventory_serial(index, soid)?;
        if info.scope == InventoryScope::Character || quantity == item.quantity {
            self.carried
                .postmaster
                .retain(|key| u64::from_str_radix(key, 16).ok() != Some(soid));
        }
        self.snapshot.characters = CharacterState::try_new(
            Self::character_capabilities(),
            self.characters().reserved_soids().to_vec(),
            characters,
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Editor removal, not a simulated dismantle. Never grants rewards.
    pub(crate) fn discard_postmaster(&mut self, index: usize, soid: u64) -> Result<(), String> {
        let mut characters = self.characters().characters().to_vec();
        let character = characters.get_mut(index).ok_or("Choose a character")?;
        let at = character
            .inventory
            .iter()
            .position(|r| r.instance_soid.get() == soid)
            .ok_or("The Postmaster item is no longer present")?;
        if !self.is_postmaster(soid) {
            return Err("Only Postmaster items can be discarded here".into());
        }
        if character.inventory[at].flags.unwrap_or_default() & 1 != 0 {
            return Err("Unlock the item before discarding it".into());
        }
        character.inventory.remove(at);
        let state = CharacterState::try_new(
            Self::character_capabilities(),
            self.characters().reserved_soids().to_vec(),
            characters,
        )
        .map_err(|e| e.to_string())?;
        self.snapshot.characters = state;
        self.carried
            .postmaster
            .retain(|key| u64::from_str_radix(key, 16).ok() != Some(soid));
        Ok(())
    }

    pub(super) fn bump_inventory_serial(&mut self, index: usize, soid: u64) -> Result<(), String> {
        let owner = self.character_owner(index).ok_or("Choose a character")?;
        let row = self
            .carried
            .characters
            .iter_mut()
            .find(|(key, _)| key.eq_ignore_ascii_case(&owner))
            .map(|(_, r)| r)
            .ok_or("Character bookkeeping is unavailable")?;
        if !(0..i64::from(i32::MAX)).contains(&row.next_inventory_serial) {
            return Err("The character has no inventory serial left to allocate".into());
        }
        let key = self
            .carried
            .item_mutation_serials
            .keys()
            .find(|key| u64::from_str_radix(key, 16).ok() == Some(soid))
            .cloned()
            .unwrap_or_else(|| super::contract::format_soid(soid));
        self.carried
            .item_mutation_serials
            .insert(key, row.next_inventory_serial);
        row.next_inventory_serial += 1;
        Ok(())
    }
}

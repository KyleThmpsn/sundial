//! Adapter for the Dawn player-state database (schema 1).
//!
//! Dawn keeps durable player state in `player-state.db` beside its settings file, which is a
//! different layout from the Sunrise investment database: SOIDs are fixed-width hexadecimal text,
//! settings are a key and value table rather than typed columns, and equipment is sparse.
//!
//! The contract is pinned to one reviewed Dawn commit. Sundial never creates, migrates, or
//! checkpoints this database, and a layout Dawn would refuse to boot from is surfaced explicitly
//! so it cannot be mistaken for the pinned one.

mod contract;
mod document;
mod error;
mod reader;
mod writer;

use std::path::PathBuf;

use sundial_account::{AccountSettingsState, CharacterState, InstanceSoid, ProfileState};

pub(crate) use document::load;
pub(crate) use error::DawnAccountIncompatibility;
use reader::{DawnAllocators, DawnMetadata};
pub(crate) use writer::{DawnSaveReceipt, restore_backup, save};

/// The storage-neutral account state one player-state database holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DawnAccountSnapshot {
    primary_soid: InstanceSoid,
    profile: ProfileState,
    characters: CharacterState,
    settings: AccountSettingsState,
}

/// One loaded player-state database, with the runtime bookkeeping a later write must respect.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DawnAccountDocument {
    path: PathBuf,
    metadata: DawnMetadata,
    allocators: DawnAllocators,
    snapshot: DawnAccountSnapshot,
}

/// Read surface for the loaded account. The writer reads the revision and allocators through the
/// document's own fields, so these accessors exist for callers and tests rather than for it.
#[allow(dead_code)]
impl DawnAccountDocument {
    pub(crate) fn primary_soid(&self) -> InstanceSoid {
        self.snapshot.primary_soid
    }
    pub(crate) fn profile(&self) -> &ProfileState {
        &self.snapshot.profile
    }
    pub(crate) fn characters(&self) -> &CharacterState {
        &self.snapshot.characters
    }
    pub(crate) fn settings(&self) -> &AccountSettingsState {
        &self.snapshot.settings
    }
    /// The revision a write must advance, mirroring Dawn's own compare and swap.
    pub(crate) fn account_revision(&self) -> i64 {
        self.metadata.account_revision
    }
    /// The next item instance SOID Dawn would hand out.
    pub(crate) fn next_item_soid(&self) -> u64 {
        self.allocators.item
    }
    /// The next profile item instance SOID Dawn would hand out.
    pub(crate) fn next_profile_item_soid(&self) -> u64 {
        self.allocators.profile_item
    }
}

#[derive(Debug)]
pub(crate) enum DawnAccountDocumentLoad {
    /// Dawn has not created the database yet. It does that on its first boot.
    Missing,
    /// The file exists but Dawn has not written its schema.
    Empty,
    Incompatible(DawnAccountIncompatibility),
    Loaded(Box<DawnAccountDocument>),
}

#[cfg(test)]
mod tests;

impl DawnAccountDocument {
    pub(crate) fn profile_mut(&mut self) -> &mut ProfileState {
        &mut self.snapshot.profile
    }
    pub(crate) fn characters_mut(&mut self) -> &mut CharacterState {
        &mut self.snapshot.characters
    }
    pub(crate) fn settings_mut(&mut self) -> &mut AccountSettingsState {
        &mut self.snapshot.settings
    }

    /// Hands out the next identity above every loaded entity.
    pub(crate) fn next_entity_id(&self) -> Result<sundial_account::EntityId, String> {
        let highest = self
            .snapshot
            .profile
            .profile_items()
            .iter()
            .map(|item| item.id.get())
            .chain(
                self.snapshot
                    .characters
                    .characters()
                    .iter()
                    .flat_map(|character| {
                        std::iter::once(character.id.get())
                            .chain(character.inventory.iter().map(|item| item.id.get()))
                            .chain(
                                character
                                    .equipment
                                    .values()
                                    .flatten()
                                    .map(|item| item.id.get()),
                            )
                    }),
            )
            .max()
            .unwrap_or_default();
        std::num::NonZeroU64::new(highest.saturating_add(1))
            .map(sundial_account::EntityId::new)
            .ok_or_else(|| "player-state.db has no identity left to allocate.".to_owned())
    }

    pub(crate) const fn profile_capabilities() -> sundial_account::ProfileCapabilities {
        sundial_account::ProfileCapabilities {
            profile_items_writable: true,
            profile_item_capacity: Some(contract::PROFILE_ITEM_CAPACITY),
            enforce_loaded_profile_item_capacity: true,
            // Dawn keeps dismantle rewards but exposes no policy columns for them.
            dismantle_rewards_writable: false,
            dismantle_reward_capacity: None,
            filtered_dismantle_rewards: false,
            combined_dismantle_gear_class: false,
        }
    }

    pub(crate) const fn character_capabilities() -> sundial_account::CharacterCapabilities {
        sundial_account::CharacterCapabilities {
            metadata_writable: true,
            inventory_writable: true,
            equipment_writable: true,
            equipment_flags_writable: true,
            inventory_capacity: Some(contract::CHARACTER_ITEM_CAPACITY),
            enforce_loaded_inventory_capacity: true,
            max_item_plugs: contract::PLUG_CAPACITY,
            item_flag_mask: crate::account_contract::INVENTORY_FLAG_MASK as u32,
            enforce_unique_instance_soids: true,
        }
    }

    pub(crate) const fn settings_capabilities() -> sundial_account::AccountSettingsCapabilities {
        sundial_account::AccountSettingsCapabilities {
            writable: true,
            named_key_bindings_writable: false,
            numeric_key_bindings_writable: true,
            extended_field_of_view: false,
        }
    }
}

impl crate::persistence::native_account::NativeAccountDocument for DawnAccountDocument {
    const LABEL: &'static str = "player-state.db";

    fn profile(&self) -> &ProfileState {
        Self::profile(self)
    }
    fn profile_mut(&mut self) -> &mut ProfileState {
        Self::profile_mut(self)
    }
    fn characters(&self) -> &CharacterState {
        Self::characters(self)
    }
    fn characters_mut(&mut self) -> &mut CharacterState {
        Self::characters_mut(self)
    }
    fn settings(&self) -> &AccountSettingsState {
        Self::settings(self)
    }
    fn settings_mut(&mut self) -> &mut AccountSettingsState {
        Self::settings_mut(self)
    }
    fn next_entity_id(&self) -> Result<sundial_account::EntityId, String> {
        Self::next_entity_id(self)
    }
    /// Dawn stores abilities on the character row alone, so no item carries its own selection.
    fn persisted_item_abilities(
        &self,
        _id: sundial_account::EntityId,
    ) -> Option<sundial_account::CharacterAbilities> {
        None
    }
    fn set_persisted_item_abilities(
        &mut self,
        _id: sundial_account::EntityId,
        _abilities: sundial_account::CharacterAbilities,
    ) {
    }
    fn profile_capabilities() -> sundial_account::ProfileCapabilities {
        Self::profile_capabilities()
    }
    fn character_capabilities() -> sundial_account::CharacterCapabilities {
        Self::character_capabilities()
    }
    fn settings_capabilities() -> sundial_account::AccountSettingsCapabilities {
        Self::settings_capabilities()
    }
}

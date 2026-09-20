//! Adapter for the Dawn player-state database (schema 5).
//!
//! Dawn keeps durable player state in `player-state.db` beside its settings file, which is a
//! different layout from the Sunrise investment database: SOIDs are fixed-width hexadecimal text,
//! settings are a key and value table rather than typed columns, and equipment is sparse.
//!
//! The contract is pinned to one reviewed Dawn commit. Sundial never creates, migrates, or
//! checkpoints this database, and a layout Dawn would refuse to boot from is surfaced explicitly
//! so it cannot be mistaken for the pinned one.

mod activity;
mod carried;
mod contract;
mod dismantle;
mod document;
mod error;
mod identities;
mod package;
mod progression;
mod reader;
mod recovery;
mod rewards;
mod rolls;
mod schema_guard;
mod settings;
mod unlocks;
mod writer;

use std::path::PathBuf;

use sundial_account::{AccountSettingsState, CharacterState, InstanceSoid, ProfileState};

pub(crate) use activity::{ActivityState, VendorProgress, VendorUnlock};
pub(crate) use contract::PROFILE_ACTION_SOURCE_CAPACITY;
/// Exposed so the app layer can assert its user-facing contract line still matches.
#[cfg(test)]
pub(crate) use contract::SCHEMA_VERSION;
pub(crate) use document::load;
pub(crate) use error::DawnAccountIncompatibility;
pub(crate) use package::{preview_replacement, read as read_snapshot, replace};
use reader::{DawnAllocators, DawnMetadata};
pub(crate) use rewards::RewardDebt;
pub(crate) use rewards::{EDITOR_MISSION, supports_currency};
pub(crate) use rolls::SavedRoll;
pub(crate) use unlocks::apply_authored_unlocks;
pub(crate) use writer::{DawnSaveReceipt, rollback_save, save};

/// The storage-neutral account state one player-state database holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DawnAccountSnapshot {
    primary_soid: InstanceSoid,
    profile: ProfileState,
    characters: CharacterState,
    settings: AccountSettingsState,
}

/// One loaded player-state database, with the runtime bookkeeping a later write must respect.
///
/// Not `Eq`: the carried `characters.appearance` is a float.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DawnAccountDocument {
    path: PathBuf,
    metadata: DawnMetadata,
    allocators: DawnAllocators,
    snapshot: DawnAccountSnapshot,
    /// Rows this build does not model. A save replaces the account graph, so they are read with
    /// it and put back after it.
    carried: carried::Carried,
    loaded_carried: carried::Carried,
    loaded_characters: CharacterState,
    loaded_profile: ProfileState,
    activity: ActivityState,
    loaded_activity: ActivityState,
    /// Where each modelled setting was read from, so an edit is written back to that exact row.
    settings_index: settings::SettingsIndex,
    /// The settings as loaded. A save writes only what differs from this.
    loaded_settings: AccountSettingsState,
    progression: progression::Progression,
    loaded_progression: progression::Progression,
    loaded_dismantle: Vec<(u32, i32)>,
    reward_debts: Vec<RewardDebt>,
    loaded_reward_debts: Vec<RewardDebt>,
    reward_sequence: i64,
    editor_cancelled_debts: std::collections::BTreeSet<i64>,
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
    /// Whether the editable account differs from another copy of it.
    ///
    /// The comparison is the account and the rows carried with it, never `account_revision`: a
    /// save advances that, and a document compared against its pre-save self would otherwise look
    /// permanently edited.
    pub(crate) fn differs_from(&self, other: &Self) -> bool {
        self.snapshot != other.snapshot
            || self.carried != other.carried
            || self.progression != other.progression
            || self.reward_debts != other.reward_debts
            || self.activity != other.activity
    }

    /// Adopts another copy's revision, after that copy's bytes were put back on disk.
    pub(crate) fn adopt_revision(&mut self, source: &Self) {
        if self.metadata.account_revision != source.metadata.account_revision {
            self.allocators.item = self.allocators.item.max(source.allocators.item);
            self.allocators.profile_item = self
                .allocators
                .profile_item
                .max(source.allocators.profile_item);
        }
        self.metadata.account_revision = source.metadata.account_revision;
        self.loaded_settings = source.loaded_settings.clone();
        self.loaded_progression = source.loaded_progression.clone();
        self.loaded_activity = source.loaded_activity.clone();
        self.loaded_carried = source.loaded_carried.clone();
        self.loaded_characters = source.loaded_characters.clone();
        self.loaded_profile = source.loaded_profile.clone();
        self.loaded_dismantle = source.loaded_dismantle.clone();
        self.loaded_reward_debts = source.loaded_reward_debts.clone();
        self.reward_sequence = self.reward_sequence.max(source.reward_sequence);
        self.editor_cancelled_debts
            .extend(&source.editor_cancelled_debts);
    }

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
mod state_tests;
#[cfg(test)]
pub(crate) mod tests;

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
                    .profile
                    .dismantle_rewards()
                    .iter()
                    .map(|reward| reward.id.get()),
            )
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
            dismantle_rewards_writable: true,
            dismantle_reward_capacity: Some(contract::DISMANTLE_REWARD_CAPACITY),
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

    fn next_item_identity(&self) -> Result<InstanceSoid, String> {
        self.available_item_identity()
    }

    fn observe_item_identity(&mut self, identity: InstanceSoid) {
        self.allocators.item = self.allocators.item.max(identity.get().saturating_add(1));
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

//! The document surface shared by every database-backed account adapter.
//!
//! Account operations are written once against this trait so a Sunrise investment database and a
//! Dawn player-state database run the same storage-neutral commands. Each adapter still owns its
//! own reading, writing and safety contract.

pub(crate) mod progression;
pub(crate) mod snapshot;

use sundial_account::{
    AccountSettingsCapabilities, AccountSettingsState, CharacterAbilities, CharacterCapabilities,
    CharacterState, EntityId, ProfileCapabilities, ProfileState,
};

pub(crate) trait NativeAccountDocument {
    /// The file name to name in a message the user reads.
    const LABEL: &'static str;

    fn profile(&self) -> &ProfileState;
    fn profile_mut(&mut self) -> &mut ProfileState;
    fn characters(&self) -> &CharacterState;
    fn characters_mut(&mut self) -> &mut CharacterState;
    fn settings(&self) -> &AccountSettingsState;
    fn settings_mut(&mut self) -> &mut AccountSettingsState;

    /// Allocates the next identity for a newly created entity.
    fn next_entity_id(&self) -> Result<EntityId, String>;

    /// Plan an item identity without consuming it on a failed edit.
    fn next_item_identity(&self) -> Result<sundial_account::InstanceSoid, String> {
        self.characters()
            .next_available_instance_soid(
                sundial_account::InstanceSoid::try_from_u64(0x4000_0000_0000_0001)
                    .expect("the generated identity start is nonzero"),
            )
            .map_err(|error| error.to_string())
    }

    /// Commit an identity only after its item was successfully added.
    fn observe_item_identity(&mut self, _identity: sundial_account::InstanceSoid) {}

    /// Abilities a format stores per item rather than per character.
    ///
    /// Dawn keeps abilities only on the character row, so its adapter reports none and never
    /// pretends to round-trip a value it cannot store.
    fn persisted_item_abilities(&self, id: EntityId) -> Option<CharacterAbilities>;
    fn set_persisted_item_abilities(&mut self, id: EntityId, abilities: CharacterAbilities);

    fn profile_capabilities() -> ProfileCapabilities
    where
        Self: Sized;
    fn character_capabilities() -> CharacterCapabilities
    where
        Self: Sized;
    fn settings_capabilities() -> AccountSettingsCapabilities
    where
        Self: Sized;
}

/// Dispatch holds each document in a box, so the boxed form is a document too.
impl<T: NativeAccountDocument> NativeAccountDocument for Box<T> {
    const LABEL: &'static str = T::LABEL;

    fn profile(&self) -> &ProfileState {
        (**self).profile()
    }
    fn profile_mut(&mut self) -> &mut ProfileState {
        (**self).profile_mut()
    }
    fn characters(&self) -> &CharacterState {
        (**self).characters()
    }
    fn characters_mut(&mut self) -> &mut CharacterState {
        (**self).characters_mut()
    }
    fn settings(&self) -> &AccountSettingsState {
        (**self).settings()
    }
    fn settings_mut(&mut self) -> &mut AccountSettingsState {
        (**self).settings_mut()
    }
    fn next_entity_id(&self) -> Result<EntityId, String> {
        (**self).next_entity_id()
    }
    fn next_item_identity(&self) -> Result<sundial_account::InstanceSoid, String> {
        (**self).next_item_identity()
    }
    fn observe_item_identity(&mut self, identity: sundial_account::InstanceSoid) {
        (**self).observe_item_identity(identity);
    }
    fn persisted_item_abilities(&self, id: EntityId) -> Option<CharacterAbilities> {
        (**self).persisted_item_abilities(id)
    }
    fn set_persisted_item_abilities(&mut self, id: EntityId, abilities: CharacterAbilities) {
        (**self).set_persisted_item_abilities(id, abilities);
    }
    fn profile_capabilities() -> ProfileCapabilities {
        T::profile_capabilities()
    }
    fn character_capabilities() -> CharacterCapabilities {
        T::character_capabilities()
    }
    fn settings_capabilities() -> AccountSettingsCapabilities {
        T::settings_capabilities()
    }
}

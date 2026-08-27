//! Storage-neutral Project Sunrise account state and mutation rules.
//!
//! This crate deliberately has no serialization, database, filesystem, catalog, or UI
//! dependencies. Persistence adapters translate their formats at the crate boundary, while
//! account commands and validation remain shared by every adapter.

#![forbid(unsafe_code)]

mod account_settings;
mod character;
mod error;
mod identity;
mod profile;

pub use account_settings::{
    AccountSettingGroup, AccountSettingKey, AccountSettingValue, AccountSettingsCapabilities,
    AccountSettingsCommand, AccountSettingsState, FiniteF64, KeyBindingSlot,
    is_supported_key_binding_action, is_valid_named_binding_input,
};
pub use character::{
    Character, CharacterAbilities, CharacterCapabilities, CharacterCommand, CharacterCommandResult,
    CharacterMetadata, CharacterMetadataUpdate, CharacterState, EquipmentSlot, ItemInstance,
    ItemPlugs, ItemUpdate,
};
pub use error::{AccountError, AccountResult, EntityKind};
pub use identity::{DefinitionHash, EntityId, InstanceSoid};
pub use profile::{
    DismantleGearClass, DismantleRarity, DismantleReward, DismantleRewardCommand,
    ProfileCapabilities, ProfileItem, ProfileItemCommand, ProfileState,
};

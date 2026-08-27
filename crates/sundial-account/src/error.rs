//! Storage-neutral account mutation errors.

use std::{error::Error, fmt};

/// The account entity involved in a failed command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntityKind {
    ProfileItem,
    DismantleReward,
    Character,
    ItemInstance,
}

impl fmt::Display for EntityKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ProfileItem => "profile item",
            Self::DismantleReward => "dismantle reward",
            Self::Character => "character",
            Self::ItemInstance => "item instance",
        })
    }
}

/// A validation or mutation failure that does not expose a persistence format.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AccountError {
    ReadOnly(EntityKind),
    EntityNotFound(EntityKind),
    DuplicateEntityId(EntityKind),
    CapacityExceeded { entity: EntityKind, capacity: usize },
    InvalidDefinitionHash,
    InvalidQuantity,
    UnsupportedDismantleFilters,
    DuplicateDismantleRarity,
    DuplicateDismantlePolicy,
    NoAvailableDismantlePolicy,
    InvalidLevel,
    TooManyItemPlugs { maximum: usize },
    InvalidItemFlags { maximum: u32 },
    DuplicateInstanceSoid(u64),
    InventoryReadOnly,
    EquipmentReadOnly,
    EquipmentFlagsReadOnly,
    SameCharacterMove,
    EquipmentSlotEmpty,
    NoAvailableInstanceSoid,
    CharacterMetadataReadOnly,
    CharacterMetadataNotLoaded,
    InvalidCharacterMetadata,
    AccountSettingsReadOnly,
    KeyBindingsReadOnly,
    AccountSettingNotLoaded,
    UnknownAccountSetting,
    InvalidAccountSettingValue,
}

impl fmt::Display for AccountError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReadOnly(entity) => write!(formatter, "{entity} data is read-only"),
            Self::EntityNotFound(entity) => {
                write!(formatter, "the selected {entity} no longer exists")
            }
            Self::DuplicateEntityId(entity) => {
                write!(formatter, "the new {entity} reuses an existing entity ID")
            }
            Self::CapacityExceeded { entity, capacity } => {
                write!(
                    formatter,
                    "{entity} capacity of {capacity} has been reached"
                )
            }
            Self::InvalidDefinitionHash => {
                formatter.write_str("the definition hash is not valid for authored account data")
            }
            Self::InvalidQuantity => formatter.write_str("quantity must be positive"),
            Self::UnsupportedDismantleFilters => {
                formatter.write_str("dismantle filters are not supported by this account format")
            }
            Self::DuplicateDismantleRarity => {
                formatter.write_str("a dismantle rarity filter cannot be repeated")
            }
            Self::DuplicateDismantlePolicy => {
                formatter.write_str("an identical dismantle policy already exists")
            }
            Self::NoAvailableDismantlePolicy => formatter.write_str(
                "every supported dismantle filter combination for this material already exists",
            ),
            Self::InvalidLevel => formatter.write_str("level must be non-negative"),
            Self::TooManyItemPlugs { maximum } => {
                write!(
                    formatter,
                    "an item cannot contain more than {maximum} plugs"
                )
            }
            Self::InvalidItemFlags { maximum } => {
                write!(formatter, "item flags must be between 0 and {maximum}")
            }
            Self::DuplicateInstanceSoid(soid) => {
                write!(formatter, "instance SOID 0x{soid:016X} is already in use")
            }
            Self::InventoryReadOnly => formatter.write_str("character inventory is read-only"),
            Self::EquipmentReadOnly => formatter.write_str("equipment is read-only"),
            Self::EquipmentFlagsReadOnly => formatter.write_str("equipment flags are read-only"),
            Self::SameCharacterMove => {
                formatter.write_str("source and destination characters must be different")
            }
            Self::EquipmentSlotEmpty => formatter.write_str("equipment slot is already empty"),
            Self::NoAvailableInstanceSoid => formatter
                .write_str("no unused instance SOID remains at or above the requested start"),
            Self::CharacterMetadataReadOnly => {
                formatter.write_str("character metadata is read-only")
            }
            Self::CharacterMetadataNotLoaded => {
                formatter.write_str("the selected character metadata was not loaded")
            }
            Self::InvalidCharacterMetadata => {
                formatter.write_str("the selected character metadata is invalid")
            }
            Self::AccountSettingsReadOnly => formatter.write_str("account settings are read-only"),
            Self::KeyBindingsReadOnly => formatter.write_str("key bindings are read-only"),
            Self::AccountSettingNotLoaded => {
                formatter.write_str("the selected account setting was not loaded")
            }
            Self::UnknownAccountSetting => {
                formatter.write_str("the selected account setting is not supported")
            }
            Self::InvalidAccountSettingValue => {
                formatter.write_str("the selected account setting value is invalid")
            }
        }
    }
}

impl Error for AccountError {}

pub type AccountResult<T> = Result<T, AccountError>;

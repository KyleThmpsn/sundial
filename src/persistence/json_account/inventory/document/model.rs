//! Inventory document errors, locations, snapshots, and mutation commands.

use std::{error::Error, fmt};

pub(crate) use sundial_account::{DismantleGearClass, DismantleRarity};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct InventoryError {
    path: String,
    message: String,
}

impl InventoryError {
    pub(crate) fn new(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            message: message.into(),
        }
    }

    #[cfg(test)]
    pub(crate) fn path(&self) -> &str {
        &self.path
    }

    #[cfg(test)]
    pub(crate) fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for InventoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.path.is_empty() {
            formatter.write_str(&self.message)
        } else {
            write!(formatter, "{}: {}", self.path, self.message)
        }
    }
}

impl Error for InventoryError {}

pub(in crate::persistence::json_account::inventory) type InventoryResult<T> =
    Result<T, InventoryError>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ProfileItemLocation {
    pub index: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProfileItemSnapshot {
    pub location: ProfileItemLocation,
    pub definition_hash: u32,
    pub quantity: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DismantleRewardLocation {
    pub index: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DismantleRewardSnapshot {
    pub location: DismantleRewardLocation,
    pub definition_hash: u32,
    pub quantity: i32,
    pub rarities: Vec<DismantleRarity>,
    pub gear_class: Option<DismantleGearClass>,
    pub masterworked: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DismantleRewardAction {
    SetPolicy {
        definition_hash: u32,
        quantity: i32,
        rarities: Vec<DismantleRarity>,
        gear_class: Option<DismantleGearClass>,
        masterworked: Option<bool>,
    },
    Remove,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct InventoryItemLocation {
    pub character_index: usize,
    pub item_index: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ItemPlugs {
    NativeDefaults,
    Authored(Vec<Option<u32>>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct InventoryItemSnapshot {
    pub location: InventoryItemLocation,
    pub instance_soid: u64,
    pub definition_hash: u32,
    pub level: i32,
    pub quantity: i32,
    pub plugs: ItemPlugs,
    pub flags: Option<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ProfileItemAction {
    SetDefinitionHash(u32),
    SetQuantity(i32),
    Remove,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum InventoryItemAction {
    SetDefinitionHash(u32),
    SetLevel(i32),
    SetQuantity(i32),
    SetPlugs(ItemPlugs),
    SetFlags(Option<u8>),
    Remove,
}

impl InventoryItemAction {
    pub(crate) fn set_plug(
        current: &[Option<u32>],
        socket_index: usize,
        hash: Option<u64>,
    ) -> Self {
        let mut plugs = current.to_vec();
        if plugs.len() <= socket_index {
            plugs.resize(socket_index + 1, None);
        }
        plugs[socket_index] = hash.and_then(|hash| u32::try_from(hash).ok());
        Self::SetPlugs(ItemPlugs::Authored(plugs))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NewInventoryItem {
    pub definition_hash: u32,
    pub level: i32,
    pub quantity: i32,
}

impl NewInventoryItem {
    pub(crate) const fn single(definition_hash: u32, level: i32) -> Self {
        Self {
            definition_hash,
            level,
            quantity: 1,
        }
    }
}

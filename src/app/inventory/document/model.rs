//! Inventory document errors, locations, snapshots, and mutation commands.

use std::{error::Error, fmt};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct InventoryError {
    path: String,
    message: String,
}

impl InventoryError {
    pub(in crate::app) fn new(path: impl Into<String>, message: impl Into<String>) -> Self {
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

pub(in crate::app::inventory) type InventoryResult<T> = Result<T, InventoryError>;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum DismantleRarity {
    Common,
    Uncommon,
    Rare,
    Legendary,
    Exotic,
}

impl DismantleRarity {
    pub(crate) const ALL: [Self; 5] = [
        Self::Common,
        Self::Uncommon,
        Self::Rare,
        Self::Legendary,
        Self::Exotic,
    ];

    #[cfg(test)]
    pub(crate) const fn token(self) -> &'static str {
        match self {
            Self::Common => "common",
            Self::Uncommon => "uncommon",
            Self::Rare => "rare",
            Self::Legendary => "legendary",
            Self::Exotic => "exotic",
        }
    }

    pub(in crate::app::inventory) fn from_token(token: &str) -> Option<Self> {
        match token {
            "common" => Some(Self::Common),
            "uncommon" => Some(Self::Uncommon),
            "rare" => Some(Self::Rare),
            "legendary" => Some(Self::Legendary),
            "exotic" => Some(Self::Exotic),
            _ => None,
        }
    }

    pub(in crate::app::inventory) const fn bit(self) -> u8 {
        match self {
            Self::Common => 1 << 1,
            Self::Uncommon => 1 << 2,
            Self::Rare => 1 << 3,
            Self::Legendary => 1 << 4,
            Self::Exotic => 1 << 5,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DismantleGearClass {
    Weapon,
    Armor,
    Both,
}

impl DismantleGearClass {
    #[cfg(test)]
    pub(crate) const fn token(self) -> &'static str {
        match self {
            Self::Weapon => "weapon",
            Self::Armor => "armor",
            Self::Both => "both",
        }
    }

    #[cfg(test)]
    pub(in crate::app::inventory) const fn mask(self) -> u8 {
        match self {
            Self::Weapon => 1,
            Self::Armor => 2,
            Self::Both => 3,
        }
    }
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

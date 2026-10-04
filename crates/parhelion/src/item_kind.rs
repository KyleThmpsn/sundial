//! What a recipe builds. Weapons came first, so their recipes omit the kind.

use serde::{Deserialize, Serialize};

/// The kind of item a recipe authors.
///
/// The base item decides slot and class. A kind only fixes which stock items can serve as a base
/// and which native inventory buckets the build accepts for it.
#[derive(
    Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    #[default]
    Weapon,
    Armor,
    Sparrow,
    Ship,
    GhostShell,
    Shader,
    Subclass,
    Emblem,
}

impl ItemKind {
    /// Menu order: weapons, armor, then the rest of the loadout, emblems, shaders and subclasses.
    /// Declaration order stays the order kinds were added, which gear pages are allocated in.
    pub const ALL: [Self; 8] = [
        Self::Weapon,
        Self::Armor,
        Self::Sparrow,
        Self::Ship,
        Self::GhostShell,
        Self::Emblem,
        Self::Shader,
        Self::Subclass,
    ];

    /// Takes a reference so serde can skip the field for weapon recipes.
    #[must_use]
    pub const fn is_weapon(&self) -> bool {
        matches!(self, Self::Weapon)
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Weapon => "Weapon",
            Self::Armor => "Armor",
            Self::Sparrow => "Sparrow",
            Self::Ship => "Ship",
            Self::GhostShell => "Ghost Shell",
            Self::Shader => "Shader",
            Self::Subclass => "Subclass",
            Self::Emblem => "Emblem",
        }
    }

    #[must_use]
    pub const fn plural(self) -> &'static str {
        match self {
            Self::Weapon => "Weapons",
            Self::Armor => "Armor",
            Self::Sparrow => "Sparrows",
            Self::Ship => "Ships",
            Self::GhostShell => "Ghost Shells",
            Self::Shader => "Shaders",
            Self::Subclass => "Subclasses",
            Self::Emblem => "Emblems",
        }
    }

    /// The kind as a noun in running text. Bungie capitalizes Sparrow and Ghost Shell.
    #[must_use]
    pub const fn noun(self) -> &'static str {
        match self {
            Self::Weapon => "weapon",
            Self::Armor => "armor",
            Self::Sparrow => "Sparrow",
            Self::Ship => "ship",
            Self::GhostShell => "Ghost Shell",
            Self::Shader => "shader",
            Self::Subclass => "subclass",
            Self::Emblem => "emblem",
        }
    }

    /// Whether the item can show a lore tab. A shader is a plug with no lore block, and none of the
    /// 484 stock emblems carries one either.
    #[must_use]
    pub const fn has_lore_tab(self) -> bool {
        !matches!(self, Self::Shader | Self::Emblem)
    }

    /// Flavor text a new recipe starts with.
    #[must_use]
    pub const fn default_flavor(self) -> &'static str {
        match self {
            Self::Weapon => "A weapon authored with Parhelion.",
            Self::Armor => "Armor authored with Parhelion.",
            Self::Sparrow => "A Sparrow authored with Parhelion.",
            Self::Ship => "A ship authored with Parhelion.",
            Self::GhostShell => "A Ghost Shell authored with Parhelion.",
            Self::Shader => "A shader authored with Parhelion.",
            Self::Subclass => "A subclass authored with Parhelion.",
            Self::Emblem => "An emblem authored with Parhelion.",
        }
    }

    /// Inventory bucket hashes whose stock items can serve as a base for this kind.
    #[must_use]
    pub const fn bucket_hashes(self) -> &'static [u64] {
        match self {
            Self::Weapon => &[1_498_876_634, 2_465_295_065, 953_998_645],
            Self::Armor => &[
                3_448_274_439,
                3_551_918_588,
                14_239_492,
                20_886_954,
                1_585_787_867,
            ],
            Self::Sparrow => &[2_025_709_351],
            Self::Ship => &[284_967_655],
            Self::GhostShell => &[4_023_194_814],
            Self::Shader => &[2_973_005_342],
            Self::Subclass => &[3_284_755_031],
            Self::Emblem => &[4_274_335_291],
        }
    }

    /// The native inventory-bucket byte (item definition +0xB8) paired with the equipment-slot
    /// value every stock item of this kind carries. Read from the Shadowkeep definitions: helmets
    /// are bucket 3 with equipment slot 1, and chest armor skips equipment slot 3.
    pub(crate) const fn native_slots(self) -> &'static [(u8, u16)] {
        match self {
            Self::Weapon => &[(0, 7), (1, 8), (2, 9)],
            Self::Armor => &[(3, 1), (4, 2), (5, 4), (6, 5), (7, 6)],
            Self::GhostShell => &[(8, 12)],
            Self::Sparrow => &[(9, 11)],
            Self::Ship => &[(10, 10)],
            // A shader is a profile plug with no equipment block.
            Self::Shader => &[(14, 0)],
            // Equipment slot 0 with the second byte set, as all nine stock subclasses carry it.
            Self::Subclass => &[(16, 0x0100)],
            Self::Emblem => &[(27, 13)],
        }
    }

    /// Names `count` items of `kinds`. One kind names itself, so a weapons-only count still reads
    /// "Weapons". A mix reads "Items".
    #[must_use]
    pub fn count_noun(kinds: impl IntoIterator<Item = Self>, count: usize) -> &'static str {
        let mut kinds = kinds.into_iter();
        let first = kinds.next().unwrap_or_default();
        match (kinds.any(|kind| kind != first), count == 1) {
            (true, true) => "Item",
            (true, false) => "Items",
            (false, true) => first.label(),
            (false, false) => first.plural(),
        }
    }

    #[must_use]
    pub fn from_bucket_hash(bucket_hash: u64) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.bucket_hashes().contains(&bucket_hash))
    }
}

//! Every item definition sorted onto the shelves the inspector browses, by the rules Dawn's
//! Loadout Studio sorts its Armory by.

use std::collections::HashSet;

use super::{Catalog, ItemRarity, ItemStackability};

/// A tab of browsed items.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum Shelf {
    #[default]
    Weapons,
    Armor,
    Cosmetics,
    PerksAndMods,
    Other,
}

impl Shelf {
    pub(crate) const ALL: [Self; 5] = [
        Self::Weapons,
        Self::Armor,
        Self::Cosmetics,
        Self::PerksAndMods,
        Self::Other,
    ];

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Weapons => "Weapons",
            Self::Armor => "Armor",
            Self::Cosmetics => "Cosmetics",
            Self::PerksAndMods => "Perks & Mods",
            Self::Other => "Other",
        }
    }
}

/// What an item is, by the equipment slot it declares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Weapon,
    Armor,
    /// Any other equipment slot: Ghosts, vehicles, Ships, emblems, emotes and the like, whatever
    /// sits in a cosmetic inventory bucket, and every ornament, shader, transmat effect and
    /// projection.
    Cosmetic,
    Subclass,
    /// No equipment slot.
    Other,
}

/// One browsable item or plug.
#[derive(Clone, Debug)]
pub(crate) struct BrowseEntry {
    pub hash: u64,
    pub name: String,
    /// The type the game prints under the name, or empty.
    pub type_name: String,
    /// The equipment slot, for equipment.
    pub slot: Option<&'static str>,
    pub kind: Kind,
    /// Whether the item is a plug: it declares a plug category or a socket offers it.
    pub plug: bool,
    pub rarity: ItemRarity,
    /// 0 Titan, 1 Hunter, 2 Warlock. None for items any class uses.
    pub class: Option<u8>,
    /// Placeholder and test definitions: unnamed, gear without a type, or on the dummy list.
    pub internal: bool,
    /// No name in the catalog, so the entry is named for its hash.
    pub unnamed: bool,
}

impl BrowseEntry {
    /// Weapons and Armor hold equipment that is not a plug, Cosmetics every cosmetic, plugs or
    /// not, Perks & Mods every other plug, and Other what is left. A subclass sits on none.
    pub(crate) fn on_shelf(&self, shelf: Shelf) -> bool {
        match shelf {
            Shelf::Weapons => self.kind == Kind::Weapon && !self.plug,
            Shelf::Armor => self.kind == Kind::Armor && !self.plug,
            Shelf::Cosmetics => self.kind == Kind::Cosmetic,
            Shelf::PerksAndMods => self.plug && self.kind != Kind::Cosmetic,
            Shelf::Other => self.kind == Kind::Other && !self.plug,
        }
    }
}

/// Plug categories below this are small scalars some records keep at the category's place.
const SMALLEST_PLUG_CATEGORY: u64 = 0x1_0000;
/// Type words that make an item a cosmetic plug, whatever bucket it reuses. Restore Defaults
/// names the plugs that clear a cosmetic socket.
const COSMETIC_PLUG_WORDS: [&str; 8] = [
    "ornament",
    "shader",
    "transmat",
    "projection",
    "glow",
    "aura",
    "tracker",
    "restore defaults",
];
/// Type words that make an item a cosmetic, kept apart from its equipment slot: emotes, emblems
/// and finishers outside their own buckets, such as the Default Emblem among the mods.
const COSMETIC_ITEM_WORDS: [&str; 3] = ["emote", "emblem", "finisher"];
/// Native inventory buckets of cosmetics: Emote Collection, Shaders, Clan Banners, Emblems,
/// Emotes and Finishers.
const COSMETIC_BUCKETS: [u8; 6] = [12, 14, 17, 27, 41, 47];
/// Type words of items a player holds, which are never plugs, though some declare a plug
/// category. Weapon perks share their inventory bucket, so the bucket cannot tell them apart.
const HELD_TYPE_WORDS: [&str; 5] = [
    "material",
    "consumable",
    "transcript",
    "recipe",
    "redeemable",
];

/// Equipment slot names in slot order, as Dawn names them.
const SLOT_LABELS: [&str; 16] = [
    "Kinetic",
    "Energy",
    "Power",
    "Helmet",
    "Gauntlets",
    "Chest",
    "Legs",
    "Class Item",
    "Ghost",
    "Sparrow",
    "Ship",
    "Subclass",
    "Clan Banner",
    "Emblem",
    "Emote",
    "Finisher",
];

/// The equipment slot index, in `SLOT_LABELS` order, of a native equipment slot.
const fn semantic_slot(native: u8) -> Option<usize> {
    Some(match native {
        7 => 0,
        8 => 1,
        9 => 2,
        1 => 3,
        2 => 4,
        4 => 5,
        5 => 6,
        6 => 7,
        12 => 8,
        11 => 9,
        10 => 10,
        0 => 11,
        15 => 12,
        13 => 13,
        14 => 14,
        17 => 15,
        _ => return None,
    })
}

impl Catalog {
    /// Every item definition, sorted by name then hash. Built per call, so callers keep it.
    pub(crate) fn browse_entries(&self) -> Vec<BrowseEntry> {
        let mut hashes = self
            .item_package_metadata
            .keys()
            .chain(self.item_indices.keys())
            .chain(self.names.keys())
            .chain(self.package_item_names.keys())
            .copied()
            .collect::<Vec<_>>();
        hashes.sort_unstable();
        hashes.dedup();
        // Plugs a cosmetic socket offers: its shaders, ornaments, trackers and transmat effects.
        let cosmetic_pool_plugs = self
            .cosmetic_socket_pools
            .iter()
            .filter_map(|pool| self.plug_pools.get(*pool as usize))
            .flatten()
            .copied()
            .collect::<HashSet<_>>();
        let mut entries = hashes
            .into_iter()
            .map(|hash| self.browse_entry(hash, &cosmetic_pool_plugs))
            .collect::<Vec<_>>();
        entries.sort_by_cached_key(|entry| (entry.name.to_lowercase(), entry.hash));
        entries.shrink_to_fit();
        entries
    }

    fn browse_entry(&self, hash: u64, cosmetic_pool_plugs: &HashSet<u64>) -> BrowseEntry {
        let metadata = self.item_package_metadata(hash);
        let definition = self.item(hash);
        let type_name = definition
            .map(|item| item.type_name.trim())
            .filter(|name| !name.is_empty())
            .or_else(|| self.plug_type_name(hash))
            .or_else(|| self.package_item_type_name(hash))
            .unwrap_or_default()
            .trim()
            .to_owned();
        let slot = metadata
            .and_then(|metadata| metadata.equipment_slot)
            .and_then(semantic_slot);
        let (kind, plug, slot) = self.placement(hash, &type_name, slot, cosmetic_pool_plugs);
        let name = self
            .names
            .get(&hash)
            .map(|name| name.trim())
            .filter(|name| !name.is_empty())
            .or_else(|| self.package_item_name(hash).map(str::trim))
            .unwrap_or_default();
        let gear = matches!(kind, Kind::Weapon | Kind::Armor);
        let internal = name.is_empty()
            || (gear && type_name.is_empty())
            || crate::catalog::dummy_items::contains(hash);
        BrowseEntry {
            hash,
            name: if name.is_empty() {
                let what = if plug { "perk" } else { "item" };
                format!("Unnamed {what} {}", crate::hash::format_hash_hex(hash))
            } else {
                name.to_owned()
            },
            type_name,
            slot: slot.and_then(|slot| SLOT_LABELS.get(slot)).copied(),
            kind,
            plug,
            rarity: metadata.map_or(ItemRarity::Unknown, |metadata| metadata.rarity),
            class: definition
                .and_then(|item| u8::try_from(item.class_type).ok())
                .filter(|class| *class <= 2),
            internal,
            unnamed: name.is_empty(),
        }
    }

    /// What an item is, whether it is a plug, and its equipment slot, from its slot, its
    /// inventory bucket, its plug category and the type the game names it.
    fn placement(
        &self,
        hash: u64,
        type_name: &str,
        slot: Option<usize>,
        cosmetic_pool_plugs: &HashSet<u64>,
    ) -> (Kind, bool, Option<usize>) {
        let kind = match slot {
            Some(0..=2) => Kind::Weapon,
            Some(3..=7) => Kind::Armor,
            Some(11) => Kind::Subclass,
            Some(_) => Kind::Cosmetic,
            None => Kind::Other,
        };
        let lowered = type_name.to_lowercase();
        let named = |words: &[&str]| words.iter().any(|word| lowered.contains(word));
        // An ornament may reuse a weapon or armor bucket. It is a cosmetic and a plug, as is
        // anything a cosmetic socket offers.
        if cosmetic_pool_plugs.contains(&hash) || named(&COSMETIC_PLUG_WORDS) {
            return (Kind::Cosmetic, true, None);
        }
        let inventory = self.inventory_metadata(hash);
        let instanced = inventory
            .is_some_and(|inventory| inventory.stackability == ItemStackability::Instanced);
        // A plug category makes a plug unless it is instanced equipment. A socket offering the
        // item makes it one regardless. A held item is never one.
        let categorised = self
            .item_package_metadata(hash)
            .and_then(|metadata| metadata.plug_category_hash)
            .is_some_and(|category| category >= SMALLEST_PLUG_CATEGORY);
        let plug = !named(&HELD_TYPE_WORDS)
            && (self.plug_hashes.contains(&hash)
                || (categorised && !(slot.is_some() && instanced)));
        let cosmetic = inventory
            .is_some_and(|inventory| COSMETIC_BUCKETS.contains(&inventory.native_bucket_id))
            || named(&COSMETIC_ITEM_WORDS);
        (if cosmetic { Kind::Cosmetic } else { kind }, plug, slot)
    }
}

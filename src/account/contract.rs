//! Shared account-format limits and equipment-slot metadata.

pub(crate) const CHARACTER_CAPACITY: usize = 3;
pub(crate) const INVENTORY_SCHEMA_VERSION: u64 = 6;
pub(crate) const EQUIPMENT_FLAGS_SCHEMA_VERSION: u64 = 4;
pub(crate) const EXPANDED_PROFILE_ITEMS_SCHEMA_VERSION: u64 = 4;
pub(crate) const DISMANTLE_REWARDS_SCHEMA_VERSION: u64 = 5;
pub(crate) const FILTERED_DISMANTLE_REWARDS_SCHEMA_VERSION: u64 = 8;
/// Account layout additions introduced in schema 13 and retained by later schemas.
/// Current runtime configuration controls use their separate schema 16 gate.
pub(crate) const EXTENDED_EQUIPMENT_SCHEMA_VERSION: u64 = 13;
pub(crate) const EMOTE_COLLECTION_DEFINITION_HASH: u64 = 3_183_180_185;
pub(crate) const EMOTE_COLLECTION_NATIVE_BUCKET: u8 = 12;

pub(crate) const fn inventory_bucket_available(
    native_id: u8,
    supports_emote_collection: bool,
) -> bool {
    native_id != EMOTE_COLLECTION_NATIVE_BUCKET || supports_emote_collection
}

pub(crate) const EMOTE_BUCKET_HASH: u64 = 2_401_704_334;

pub(crate) const fn definition_available(hash: u64, supports_emote_collection: bool) -> bool {
    hash != EMOTE_COLLECTION_DEFINITION_HASH || supports_emote_collection
}

pub(crate) const INVENTORY_FLAG_MASTERWORK: u8 = 4;

pub(crate) const fn supports_emote_collection(schema_version: u64) -> bool {
    schema_version >= EXTENDED_EQUIPMENT_SCHEMA_VERSION
}

pub(crate) const fn uses_subclass_plug_abilities(schema_version: u64) -> bool {
    schema_version >= EXTENDED_EQUIPMENT_SCHEMA_VERSION
}

pub(crate) const fn item_flag_mask(schema_version: u64) -> u8 {
    if schema_version >= EXTENDED_EQUIPMENT_SCHEMA_VERSION {
        INVENTORY_FLAG_MASK | INVENTORY_FLAG_MASTERWORK
    } else {
        INVENTORY_FLAG_MASK
    }
}

pub(crate) const INVENTORY_FLAG_LOCKED: u8 = 1;
pub(crate) const INVENTORY_FLAG_TRACKED: u8 = 2;
pub(crate) const INVENTORY_FLAG_MASK: u8 = INVENTORY_FLAG_LOCKED | INVENTORY_FLAG_TRACKED;

pub(crate) const fn profile_item_capacity(schema_version: u64) -> usize {
    if schema_version < EXPANDED_PROFILE_ITEMS_SCHEMA_VERSION {
        LEGACY_PROFILE_ITEM_CAPACITY
    } else {
        PROFILE_ITEM_CAPACITY
    }
}

pub(crate) const LEGACY_PROFILE_ITEM_CAPACITY: usize = 32;
pub(crate) const PROFILE_ITEM_CAPACITY: usize = 701;
pub(crate) const CHARACTER_INVENTORY_CAPACITY: usize = 135;
pub(crate) const MAX_ITEM_PLUGS: usize = 12;
pub(crate) const LEGACY_DISMANTLE_REWARD_CAPACITY: usize = 8;
pub(crate) const FILTERED_DISMANTLE_REWARD_CAPACITY: usize = 32;

pub(crate) type EquipmentSlotContract = (&'static str, &'static str, u64);

pub(crate) const EQUIPMENT_SLOTS: &[EquipmentSlotContract] = &[
    ("kinetic", "Kinetic", 1_498_876_634),
    ("energy", "Energy", 2_465_295_065),
    ("heavy", "Power", 953_998_645),
    ("helmet", "Helmet", 3_448_274_439),
    ("gauntlets", "Gauntlets", 3_551_918_588),
    ("chest", "Chest", 14_239_492),
    ("legs", "Legs", 20_886_954),
    ("class_item", "Class Item", 1_585_787_867),
    ("ghost", "Ghost", 4_023_194_814),
    ("vehicle", "Vehicle", 2_025_709_351),
    ("ship", "Ship", 284_967_655),
    ("subclass", "Subclass", 3_284_755_031),
    ("clan_banner", "Clan Banner", 4_292_445_962),
    ("emblem", "Emblem", 4_274_335_291),
    ("emote", "Emote", 2_401_704_334),
    ("finisher", "Finisher", 3_683_254_069),
];

/// Complete metadata in SQLite native slot order. Use equipment_slots_for_schema for JSON.
pub(crate) const ALL_EQUIPMENT_SLOTS: &[EquipmentSlotContract] = &all_equipment_slots();

const fn all_equipment_slots() -> [EquipmentSlotContract; 17] {
    let mut slots = [("artifact", "Artifact", 1_506_418_338); 17];
    let mut index = 0;
    while index < EQUIPMENT_SLOTS.len() {
        slots[index] = EQUIPMENT_SLOTS[index];
        index += 1;
    }
    slots
}

pub(crate) const fn equipment_slots_for_schema(version: u64) -> &'static [EquipmentSlotContract] {
    if version >= EXTENDED_EQUIPMENT_SCHEMA_VERSION {
        ALL_EQUIPMENT_SLOTS
    } else {
        EQUIPMENT_SLOTS
    }
}

pub(crate) fn is_known_equipment_slot(slot: &str, version: u64) -> bool {
    equipment_slots_for_schema(version)
        .iter()
        .any(|(known_slot, _, _)| *known_slot == slot)
}

pub(crate) const WEAPON_SLOTS: &[&str] = &["kinetic", "energy", "heavy"];

/// One-based unlock-map bank backed by the Shadowkeep account object's primary flag region.
pub const SHADOWKEEP_ACCOUNT_FLAG_BANK: u8 = 1;
/// Start of the Shadowkeep account object's primary unlock-flag byte region.
pub const SHADOWKEEP_ACCOUNT_FLAG_REGION_OFFSET: usize = 29_740;
/// Start of the next account-object region after the primary unlock-flag bytes.
pub const SHADOWKEEP_ACCOUNT_VALUE_REGION_OFFSET: usize = 42_040;
/// Stock rows mapped into the primary account unlock-flag region.
pub const SHADOWKEEP_ACCOUNT_FLAG_STOCK_ROWS: usize = 11_923;
/// Complete byte capacity available before the following account-object region begins.
///
/// Authored flag-map rows may claim the stock padding after
/// [`SHADOWKEEP_ACCOUNT_FLAG_STOCK_ROWS`], but must never cross this boundary.
pub const SHADOWKEEP_ACCOUNT_FLAG_REGION_CAPACITY: usize =
    SHADOWKEEP_ACCOUNT_VALUE_REGION_OFFSET - SHADOWKEEP_ACCOUNT_FLAG_REGION_OFFSET;
/// Number of primary account unlock-flag rows available to authored extensions.
pub const SHADOWKEEP_ACCOUNT_FLAG_EXTENSION_CAPACITY: usize =
    SHADOWKEEP_ACCOUNT_FLAG_REGION_CAPACITY - SHADOWKEEP_ACCOUNT_FLAG_STOCK_ROWS;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_emote_collection_controls_do_not_leak_into_legacy_accounts() {
        for version in [6, 8, 12, 13, 14] {
            assert_eq!(
                inventory_bucket_available(12, supports_emote_collection(version)),
                version >= 13
            );
            assert_eq!(
                definition_available(
                    EMOTE_COLLECTION_DEFINITION_HASH,
                    supports_emote_collection(version)
                ),
                version >= 13
            );
            // Artifacts were already valid stored inventory before their new equipment slot.
            assert!(inventory_bucket_available(
                49,
                supports_emote_collection(version)
            ));
        }
    }
}

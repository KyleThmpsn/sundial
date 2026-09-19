//! Dawn player-state contract at prod-client-need-to-merge 06ce2d4.
//!
//! Every constant here mirrors a value Dawn compiles in. The runtime refuses to boot when a
//! durable row breaks one of them, so the reader checks the same limits rather than trusting
//! whatever the file happens to hold.

/// `PRAGMA user_version` Dawn writes when it creates the database.
pub(crate) const SCHEMA_VERSION: i64 = 5;

/// Dawn's own schema, used to build a database a test can read the way Dawn would.
#[allow(dead_code)]
pub(super) const SCHEMA: &str = include_str!("fixtures/player_state_schema.sql");

/// The three metadata rows Dawn reads in `ORDER BY key` sequence. A fourth row fails the load.
pub(super) const METADATA_KEYS: [&str; 3] =
    ["account_revision", "legacy_import_complete", "reward_epoch"];

/// The two allocator rows Dawn requires. Any other name fails the load.
pub(super) const ITEM_ALLOCATOR: &str = "item_instance";
pub(super) const PROFILE_ITEM_ALLOCATOR: &str = "profile_item_instance";
pub(super) const FIRST_ITEM_SOID: u64 = 0x4000_0000_0000_0001;
pub(super) const FIRST_PROFILE_ITEM_SOID: u64 = 0x5000_0000_0000_0001;

pub(super) const EQUIPMENT_LOCATION: i64 = 0;
pub(super) const INVENTORY_LOCATION: i64 = 1;

pub(super) const CHARACTER_CAPACITY: usize = crate::account_contract::CHARACTER_CAPACITY;
pub(super) const CHARACTER_ITEM_CAPACITY: usize =
    crate::account_contract::CHARACTER_INVENTORY_CAPACITY;
pub(super) const PROFILE_ITEM_CAPACITY: usize = 701;
pub(super) const PLUG_CAPACITY: usize = crate::account_contract::MAX_ITEM_PLUGS;

/// Dawn's `EquipmentSlot` enum, in declaration order. Position is the array index.
pub(super) const EQUIPMENT_SLOTS: [&str; 16] = {
    let mut slots = [""; 16];
    let metadata = crate::account_contract::EQUIPMENT_SLOTS;
    let mut index = 0;
    while index < slots.len() {
        slots[index] = metadata[index].0;
        index += 1;
    }
    slots
};

/// Inclusive maximum for each range-checked character column.
pub(super) const CHARACTER_RANGES: [(&str, i64); 9] = [
    ("race", 2),
    ("gender", 1),
    ("class", 2),
    ("level", 255),
    ("movement_ability", 255),
    ("grenade_ability", 255),
    ("super_ability", 255),
    ("melee_ability", 255),
    ("class_ability", 255),
];

/// Dawn stores every SOID as exactly sixteen uppercase hexadecimal digits.
pub(super) const SOID_TEXT_LENGTH: usize = 16;

#[must_use]
pub(super) fn format_soid(value: u64) -> String {
    format!("{value:016X}")
}

/// Parses Dawn's fixed-width SOID text. Dawn rejects any other length outright.
#[must_use]
pub(super) fn parse_soid(text: &str) -> Option<u64> {
    if text.len() != SOID_TEXT_LENGTH {
        return None;
    }
    u64::from_str_radix(text, 16).ok()
}

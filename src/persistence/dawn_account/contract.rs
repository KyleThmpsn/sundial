//! Dawn player-state contract for schemas 5 and 7.
//!
//! Every constant here mirrors a value Dawn compiles in. The runtime refuses to boot when a
//! durable row breaks one of them, so the reader checks the same limits rather than trusting
//! whatever the file happens to hold.

/// `PRAGMA user_version` of the schema Dawn 1.7 releases write.
pub(crate) const SCHEMA_VERSION: i64 = 5;

/// The schema that moves ability picks onto each subclass item and adds per-item values, a
/// selected title and the native customization block. Dawn passes through version 6 inside one
/// migration and never leaves a database there, so 6 is not a layout to open.
pub(crate) const PER_ITEM_SCHEMA_VERSION: i64 = 7;

/// Dawn's own schema 5. The writer checks a database's layout against it before rewriting, and
/// tests build databases from it that read the way Dawn's would.
pub(super) const SCHEMA: &str = include_str!("fixtures/player_state_schema.sql");

/// Dawn's own statements from schema 5 to 7, in the order its migration runs them. Column order
/// and default text follow from that order, and the layout check compares both.
pub(super) const SCHEMA_V7_DELTA: &str = include_str!("fixtures/player_state_schema_v7_delta.sql");

#[must_use]
pub(super) const fn supported(version: i64) -> bool {
    version == SCHEMA_VERSION || version == PER_ITEM_SCHEMA_VERSION
}

/// Whether the schema stores ability picks, values, a title and customization per row.
#[must_use]
pub(super) const fn per_item(version: i64) -> bool {
    version >= PER_ITEM_SCHEMA_VERSION
}

/// An empty database holding exactly the layout Dawn creates for one supported version.
pub(super) fn reference(version: i64) -> rusqlite::Result<rusqlite::Connection> {
    let reference = rusqlite::Connection::open_in_memory()?;
    reference.execute_batch(SCHEMA)?;
    if per_item(version) {
        reference.execute_batch(SCHEMA_V7_DELTA)?;
    }
    Ok(reference)
}

/// Inclusive maximum of an ability entry stored on an item. The character row allows 255.
pub(super) const ITEM_ABILITY_MAXIMUM: u8 = 63;

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

pub(super) const CHARACTER_CAPACITY: usize = crate::account::contract::CHARACTER_CAPACITY;
// Dawn inventory_state.h: kCharacterItemCapacity. Includes ordinary and recovery rows.
// Bucket limits are independent. Postmaster rows never consume weapon or armor slots.
pub(crate) const CHARACTER_ITEM_CAPACITY: usize = 334;
pub(super) const PROFILE_ITEM_CAPACITY: usize = 701;
pub(crate) const PROFILE_ACTION_SOURCE_CAPACITY: usize = 100;
pub(super) const DISMANTLE_REWARD_CAPACITY: usize = 8;
pub(super) const PLUG_CAPACITY: usize = crate::account::contract::MAX_ITEM_PLUGS;

/// Dawn's `EquipmentSlot` enum, in declaration order. Position is the array index.
pub(crate) const EQUIPMENT_SLOTS: [&str; 16] = {
    let mut slots = [""; 16];
    let metadata = crate::account::contract::EQUIPMENT_SLOTS;
    let mut index = 0;
    while index < slots.len() {
        slots[index] = metadata[index].0;
        index += 1;
    }
    slots
};

/// Inclusive maximum for each range-checked character column.
pub(crate) const CHARACTER_RANGES: [(&str, i64); 9] = [
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

//! Official Sunrise contract at 169fd296f27e30d8edb29ed2d866aa5b3db1dd81.

pub(super) const SCHEMA_VERSION: i64 = 2;
pub(super) const CHARACTER_CAPACITY: usize = crate::account_contract::CHARACTER_CAPACITY;
pub(super) const PROFILE_ITEM_CAPACITY: usize = crate::account_contract::PROFILE_ITEM_CAPACITY;
pub(super) const DISMANTLE_REWARD_CAPACITY: usize =
    crate::account_contract::FILTERED_DISMANTLE_REWARD_CAPACITY;
pub(super) const CHARACTER_ITEM_CAPACITY: usize =
    crate::account_contract::CHARACTER_INVENTORY_CAPACITY;
pub(super) const PLUG_CAPACITY: usize = crate::account_contract::MAX_ITEM_PLUGS;

pub(super) const EQUIPMENT_LOCATION: i64 = 0;
pub(super) const INVENTORY_LOCATION: i64 = 1;

pub(super) const EQUIPMENT_SLOTS: [&str; 17] = {
    let metadata = crate::account_contract::ALL_EQUIPMENT_SLOTS;
    let mut slots = [""; 17];
    let mut index = 0;
    while index < slots.len() {
        slots[index] = metadata[index].0;
        index += 1;
    }
    slots
};

pub(super) use sundial_account::KEY_BINDING_ACTIONS;

pub(super) const APPLICATION_ID: i64 = 1397902921;
pub(super) const SCHEMA: &str = include_str!("fixtures/investment_schema.sql");
pub(super) const SETTINGS_SCHEMA: &str = include_str!("fixtures/account_settings_schema.sql");

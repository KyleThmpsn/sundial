//! Inventory document schema, models, parsing, validation, and identity allocation.
//!
//! The parent inventory facade owns mutation workflows. This module keeps their document
//! contract split into focused implementation units while preserving the existing API.

mod fields;
mod model;
mod parsing;
mod schema;
mod soids;
mod validation;

pub(crate) use model::{
    DismantleGearClass, DismantleRarity, DismantleRewardAction, DismantleRewardLocation,
    DismantleRewardSnapshot, InventoryError, InventoryItemAction, InventoryItemLocation,
    InventoryItemSnapshot, ItemPlugs, NewInventoryItem, ProfileItemAction, ProfileItemLocation,
    ProfileItemSnapshot,
};
pub(crate) use parsing::{
    character_inventory, dismantle_rewards, profile_item_target_exists, profile_items,
};
pub(crate) use schema::{
    CHARACTER_INVENTORY_CAPACITY, DISMANTLE_REWARDS_SCHEMA_VERSION, EQUIPMENT_FLAGS_SCHEMA_VERSION,
    FILTERED_DISMANTLE_REWARD_CAPACITY, GENERATED_INSTANCE_SOID_START, INVENTORY_FLAG_LOCKED,
    INVENTORY_FLAG_MASK, INVENTORY_FLAG_TRACKED, INVENTORY_SCHEMA_VERSION,
    LEGACY_PROFILE_ITEM_CAPACITY, MAX_ITEM_PLUGS, PROFILE_ITEM_CAPACITY, SchemaMode,
    profile_item_capacity, schema_mode, set_inventory_locked_flag,
};
pub(crate) use soids::{allocate_instance_soid, collect_used_soids, next_available_instance_soid};
pub(crate) use validation::validate_document_items;

pub(in crate::app) use schema::KNOWN_ITEM_MEMBERS;

pub(super) use fields::{
    encode_plugs, format_definition_hash_hex, format_instance_soid, inventory_item_path,
    validate_inventory_definition_hash, validate_nonnegative_i32, validate_positive_i32,
};
pub(super) use model::InventoryResult;
pub(super) use parsing::{character_object, validate_existing_character_inventories};
pub(super) use schema::{
    read_only_schema_error, require_inventory_mutation, require_profile_mutation,
    require_readable_schema,
};
pub(super) use validation::{
    account_object_mut, character_object_mut, ensure_account_object, inventory_array_mut,
    inventory_object_mut, profile_array_mut, validate_inventory_action,
};

#[cfg(test)]
pub(super) use schema::{LEGACY_DISMANTLE_REWARD_CAPACITY, NO_DEFINITION_HASH};

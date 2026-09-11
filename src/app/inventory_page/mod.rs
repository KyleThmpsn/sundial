//! Guided editing for account profile items and per-character inventory.
//!
//! The page renders typed inventory snapshots and applies typed actions from `inventory`;
//! it never reaches into the document with ad-hoc JSON pointers.

mod buckets;
mod character;
mod definitions;
mod interactions;
mod materials;
mod model;
mod presentation;
mod profile;
mod rewards;

#[cfg(test)]
mod tests;

pub(in crate::app) use interactions::{
    displayed_inventory_plugs, inventory_item_state_key, inventory_item_ui_identities,
};
pub(in crate::app) use model::{CharacterInventoryEditorContext, InventoryItemUiId};

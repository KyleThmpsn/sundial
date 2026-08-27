//! Character equipment editing and inspection.
//!
//! This facade keeps the app-facing equipment API stable while the feature's
//! presentation, picker support, defaults, and document operations remain
//! independently maintainable.

mod actions;
mod armor_stat_allocation;
mod armor_stats_adjuster;
mod character;
mod defaults;
mod document;
mod model;
mod panoptes;
mod picker;
mod randomize;
mod view;

#[cfg(test)]
mod tests;

pub(in crate::app) use armor_stats_adjuster::{
    State as ArmorStatsAdjusterState, draw_entry_button as draw_armor_stats_button,
    draw_window as draw_armor_stats_window,
};
pub(super) use defaults::{
    class_name, collect_class_armor_default_characters, default_ability_values,
    default_subclass_name, selected_attunement_index,
};
#[cfg(test)]
pub(super) use defaults::{collect_class_armor_defaults, restore_class_armor};
#[cfg(test)]
pub(in crate::app) use document::legacy as legacy_document_tests;
pub(super) use document::{
    displayed_plugs, equip_definition, equip_inventory_item, equip_subclass_with_default_abilities,
    equipment_slot_label, equipped_item_snapshots, native_plug_default,
    restore_class_armor_from_character, set_equipment_item_flags, set_equipment_item_level,
    set_equipment_item_plug, set_weapon_slot_empty,
};
#[cfg(test)]
pub(super) use document::{inferred_item_level, materialize_authored_plugs};
pub(super) use model::{
    EquipmentSlotCard, EquippedItemPlugs, EquippedItemSnapshot, EquippedPlugValue,
};
pub(super) use picker::{ability_combo, combo_u64};
pub(in crate::app) use randomize::{
    draw_dialogs as draw_randomize_dialogs, draw_menu as draw_randomize_menu,
    request_inventory_item_builder,
};

use document::equipped_header_label;
use picker::{
    character_field_group_layout, equipment_definition_choices, equipment_inventory_choices,
    existing_inventory_choice_matches,
};

use std::{collections::HashMap, sync::Arc};

use eframe::egui;
use serde_json::Value;

use crate::{
    catalog::{self, AbilityChoice, Catalog, CatalogSearchQuery, ItemDef},
    game_settings,
    hash::{format_hash_hex, parse_hash_hex, parse_unsigned_value},
};

use super::{
    ARMOR_SLOTS, ConfirmationDialog, ITEM_PICKER_MAX_HEIGHT, ITEM_PICKER_MIN_HEIGHT,
    PLUG_PICKER_MAX_HEIGHT, PLUG_PICKER_MIN_HEIGHT, PlugSelectionMode, SLOTS, SundialApp, ViewMode,
    WEAPON_SLOTS, draw_plug_selection_warning, inventory, item_editor,
    settings::{character_ability_issue, character_ability_issue_for_values},
};

use super::item_editor::{
    ClearDefinitionChoice, DefinitionChoice, DefinitionPickerChoices, DefinitionSummary,
    ExistingInventoryChoice, ItemEditorAction, ItemHeader, NativePlugDefault, NumericItemFields,
    PickerHeight,
};

#[derive(Clone, Copy)]
struct DialogSizeConstraints {
    default: egui::Vec2,
    min: egui::Vec2,
    max: egui::Vec2,
    compact: bool,
}

fn dialog_size_constraints(
    context: &egui::Context,
    desired: egui::Vec2,
    requested_min: egui::Vec2,
) -> DialogSizeConstraints {
    const OUTER_MARGIN: egui::Vec2 = egui::vec2(48.0, 56.0);
    const ABSOLUTE_MIN: egui::Vec2 = egui::vec2(320.0, 240.0);

    let viewport = context.screen_rect().size();
    let max = egui::vec2(
        (viewport.x - OUTER_MARGIN.x).max(ABSOLUTE_MIN.x),
        (viewport.y - OUTER_MARGIN.y).max(ABSOLUTE_MIN.y),
    );
    let default = egui::vec2(desired.x.min(max.x), desired.y.min(max.y));
    let min = egui::vec2(requested_min.x.min(max.x), requested_min.y.min(max.y));
    DialogSizeConstraints {
        default,
        min,
        max,
        compact: default != desired,
    }
}

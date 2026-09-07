//! Item randomizer adapted from work by xSkullHD.
//! Loadout randomizer adapted from Kjam's Panoptes fork.
//!
//! The item workspace starts with a base weapon or armor piece, then lets the user
//! review and change its randomized socket plugs. A roll remains a preview until
//! it is equipped or added to inventory.
//!
//! The loadout action replaces the selected equipment sections and optional held
//! inventory in one validated transaction.

mod apply;
mod candidate;
mod item_builder;
mod loadout;
mod loadout_dialog;
mod rng;

use apply::*;
use candidate::*;
use item_builder::draw_item_workspace;
use loadout::*;
use loadout_dialog::draw_loadout_confirmation;
use rng::Rng;

use std::collections::{HashMap, HashSet};

use eframe::egui;
use serde_json::Value;
use sundial_account::NO_DEFINITION_HASH;

use crate::{
    app::{
        ARMOR_SLOTS, PlugSelectionMode, SLOTS, SundialApp, WEAPON_SLOTS, class_name, inventory,
        item_editor, settings,
    },
    catalog::{Catalog, InventoryMetadata, InventoryScope, ItemDef},
    hash::{format_hash_hex, parse_hash_hex, parse_unsigned_value},
};

use super::{
    EquippedItemPlugs, EquippedItemSnapshot, EquippedPlugValue, armor_stat_allocation,
    displayed_plugs, equip_subclass_with_default_abilities,
};

const MAX_VISIBLE_SEARCH_RESULTS: usize = 12;
const AVAILABLE_PLUG_ROW_HEIGHT: f32 = 42.0;
const PLUG_ICON_SIZE: f32 = 26.0;
const PLUG_ROW_HEIGHT: f32 = 36.0;
const MIN_PLUG_LIST_HEIGHT: f32 = 170.0;
const PLUG_SECTION_CHROME_HEIGHT: f32 = 58.0;
const FOOTER_RESERVE_HEIGHT: f32 = 34.0;
const BASE_FILTER_INLINE_WIDTH: f32 = 380.0;
const WINDOW_MIN_SIZE: egui::Vec2 = egui::vec2(640.0, 460.0);
const HELD_ITEMS_PER_SLOT: usize = 9;
const SUBCLASS_SLOT: &str = "subclass";
const CLAN_BANNER_SLOT: &str = "clan_banner";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app) enum Request {
    Item,
    Loadout,
}

#[derive(Clone, Debug, Default)]
struct ItemBuilderRequest {
    item_hash: u64,
    authored_plugs: Option<Value>,
}

fn item_builder_request_id(character_index: usize) -> egui::Id {
    egui::Id::new(("random-item-builder-request", character_index))
}

pub(in crate::app::equipment) fn request_item_builder(
    context: &egui::Context,
    character_index: usize,
    item_hash: u64,
    authored_plugs: Option<Value>,
) {
    context.data_mut(|data| {
        data.insert_temp(
            item_builder_request_id(character_index),
            ItemBuilderRequest {
                item_hash,
                authored_plugs,
            },
        );
    });
    context.request_repaint();
}

pub(in crate::app) fn request_inventory_item_builder(
    context: &egui::Context,
    character_index: usize,
    item_hash: u64,
    plugs: &inventory::ItemPlugs,
) {
    let authored_plugs = inventory_plugs_value(plugs);
    request_item_builder(context, character_index, item_hash, Some(authored_plugs));
}

fn inventory_plugs_value(plugs: &inventory::ItemPlugs) -> Value {
    match plugs {
        inventory::ItemPlugs::NativeDefaults => Value::Null,
        inventory::ItemPlugs::Authored(plugs) => Value::Array(
            plugs
                .iter()
                .map(|hash| hash.map_or(Value::Null, Value::from))
                .collect(),
        ),
    }
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
enum ItemFamily {
    Weapon,
    Armor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LoadoutScope {
    Weapons,
    Armor,
    EquipmentFlair,
    Subclass,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LoadoutOptions {
    weapons: bool,
    armor: bool,
    equipment_flair: bool,
    subclass: bool,
    replace_held_inventory: bool,
    keep_locked_items: bool,
}

impl Default for LoadoutOptions {
    fn default() -> Self {
        Self {
            weapons: false,
            armor: false,
            equipment_flair: false,
            subclass: false,
            replace_held_inventory: false,
            keep_locked_items: true,
        }
    }
}

impl LoadoutOptions {
    const fn includes(self, scope: LoadoutScope) -> bool {
        match scope {
            LoadoutScope::Weapons => self.weapons,
            LoadoutScope::Armor => self.armor,
            LoadoutScope::EquipmentFlair => self.equipment_flair,
            LoadoutScope::Subclass => self.subclass,
        }
    }

    const fn any(self) -> bool {
        self.weapons || self.armor || self.equipment_flair || self.subclass
    }
}

impl ItemFamily {
    const fn slots(self) -> &'static [&'static str] {
        match self {
            Self::Weapon => WEAPON_SLOTS,
            Self::Armor => ARMOR_SLOTS,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Weapon => "weapon",
            Self::Armor => "armor",
        }
    }
}

#[derive(Clone, Debug)]
struct Candidate {
    item_hash: u64,
    plugs: Vec<Option<u64>>,
}

#[derive(Clone, Debug)]
struct Feedback {
    text: String,
    is_error: bool,
}

#[derive(Clone, Debug)]
struct EquipReplacementWarning {
    current_item_name: String,
    candidate_item_name: String,
    reason: String,
}

impl EquipReplacementWarning {
    fn message(&self) -> String {
        format!(
            "{}. {} cannot be moved to inventory and will be deleted if you equip {}.",
            self.reason, self.current_item_name, self.candidate_item_name
        )
    }
}

#[derive(Clone, Debug)]
struct WorkspaceState {
    active_family: ItemFamily,
    weapon_slot: Option<usize>,
    armor_slot: Option<usize>,
    base_query: String,
    socket_query: String,
    selected_socket: usize,
    weapon_filter: item_editor::ItemFilter,
    armor_filter: item_editor::ItemFilter,
    armor_stat_allocation: armor_stat_allocation::State,
    last_plug_mode: Option<PlugSelectionMode>,
    candidate: Option<Candidate>,
    pending_destructive_equip: Option<Candidate>,
    feedback: Option<Feedback>,
}

impl Default for WorkspaceState {
    fn default() -> Self {
        Self {
            active_family: ItemFamily::Weapon,
            weapon_slot: None,
            armor_slot: None,
            base_query: String::new(),
            socket_query: String::new(),
            selected_socket: 0,
            weapon_filter: item_editor::ItemFilter::default(),
            armor_filter: item_editor::ItemFilter::default(),
            armor_stat_allocation: armor_stat_allocation::State::default(),
            last_plug_mode: None,
            candidate: None,
            pending_destructive_equip: None,
            feedback: None,
        }
    }
}

impl WorkspaceState {
    fn filter(&self, family: ItemFamily) -> &item_editor::ItemFilter {
        match family {
            ItemFamily::Weapon => &self.weapon_filter,
            ItemFamily::Armor => &self.armor_filter,
        }
    }

    fn filter_mut(&mut self, family: ItemFamily) -> &mut item_editor::ItemFilter {
        match family {
            ItemFamily::Weapon => &mut self.weapon_filter,
            ItemFamily::Armor => &mut self.armor_filter,
        }
    }
}

#[derive(Clone, Debug)]
enum BaseAction {
    Random(ItemFamily),
    SelectDefinition(u64),
    SelectInstance(ItemBuilderRequest),
}

#[derive(Clone, Debug)]
struct ItemInstanceChoice {
    request: ItemBuilderRequest,
    location: String,
}

#[derive(Clone, Debug)]
enum CandidateAction {
    Equip {
        candidate: Candidate,
        discard_replaced: bool,
    },
    AddToInventory(Candidate),
}

pub(in crate::app) fn draw_menu(
    ui: &mut egui::Ui,
    equipment_editable: bool,
    inventory_editable: bool,
) -> Option<Request> {
    let mut request = None;
    ui.add_enabled_ui(equipment_editable, |ui| {
        ui.menu_button("Randomize", |ui| {
            if ui
                .button("Random Item Builder…")
                .on_hover_text("Build and review one weapon or armor item")
                .clicked()
            {
                request = Some(Request::Item);
                ui.close_menu();
            }
            if ui
                .add_enabled(inventory_editable, egui::Button::new("Randomize Loadout…"))
                .on_hover_text(if inventory_editable {
                    "Replace equipped gear and held inventory"
                } else {
                    "Requires writable character inventory"
                })
                .clicked()
            {
                request = Some(Request::Loadout);
                ui.close_menu();
            }
        });
    });
    request
}

pub(in crate::app) fn draw_dialogs(
    app: &mut SundialApp,
    context: &egui::Context,
    character_index: usize,
    request: Option<Request>,
) {
    draw_item_workspace(
        app,
        context,
        character_index,
        request == Some(Request::Item),
    );
    draw_loadout_confirmation(
        app,
        context,
        character_index,
        request == Some(Request::Loadout),
    );
}

#[cfg(test)]
mod tests;

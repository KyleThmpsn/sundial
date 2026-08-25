//! Item randomizer adapted from work by xSkullHD.
//! Loadout randomizer adapted from Kjam's Panoptes fork.
//!
//! The item workspace starts with a base weapon or armor piece, then lets the user
//! review and change its randomized socket plugs. A roll remains a preview until
//! it is equipped or added to inventory.
//!
//! After explicit confirmation, the loadout action replaces the selected equipment
//! sections and their held inventory in one validated transaction.

use std::collections::HashSet;

use eframe::egui;
use serde_json::Value;

use crate::{
    app::{
        ARMOR_SLOTS, PlugSelectionMode, SLOTS, SundialApp, WEAPON_SLOTS, class_name, inventory,
        item_editor, settings,
    },
    catalog::{Catalog, InventoryMetadata, InventoryScope, ItemDef},
    hash::{format_hash_hex, parse_hash_hex, parse_unsigned_value},
};

use super::{
    EquippedItemPlugs, EquippedPlugValue, armor_stat_allocation, displayed_plugs, equip_definition,
    equip_subclass_with_default_abilities, equipped_item_snapshots, set_equipment_item_plug,
};

const MAX_VISIBLE_SEARCH_RESULTS: usize = 12;
const AVAILABLE_PLUG_ROW_HEIGHT: f32 = 42.0;
const PLUG_ICON_SIZE: f32 = 26.0;
const PLUG_ROW_HEIGHT: f32 = 36.0;
const MIN_PLUG_LIST_HEIGHT: f32 = 170.0;
const PLUG_SECTION_CHROME_HEIGHT: f32 = 58.0;
const FOOTER_RESERVE_HEIGHT: f32 = 34.0;
const WINDOW_MIN_SIZE: egui::Vec2 = egui::vec2(640.0, 460.0);
const BASE_FILTER_INLINE_WIDTH: f32 = 380.0;
const NO_DEFINITION_HASH: u64 = 0x811C_9DC5;
const HELD_ITEMS_PER_SLOT: usize = 9;
const SUBCLASS_SLOT: &str = "subclass";
const CLAN_BANNER_SLOT: &str = "clan_banner";
const EXOTIC_TRAIT_SOCKET_TYPES: [u16; 2] = [377, 677];

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
}

impl Default for LoadoutOptions {
    fn default() -> Self {
        Self {
            weapons: true,
            armor: true,
            equipment_flair: true,
            subclass: true,
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

fn draw_item_workspace(
    app: &mut SundialApp,
    context: &egui::Context,
    character_index: usize,
    open_requested: bool,
) {
    let dialog_id = egui::Id::new(("equipment-randomize", character_index));
    let state_id = dialog_id.with("state");
    let window_generation_id = dialog_id.with("window-generation");
    let builder_request = context.data_mut(|data| {
        data.remove_temp::<ItemBuilderRequest>(item_builder_request_id(character_index))
    });
    if open_requested || builder_request.is_some() {
        context.data_mut(|data| {
            data.insert_temp(dialog_id, true);
            let generation = data
                .get_temp::<u64>(window_generation_id)
                .unwrap_or_default()
                .wrapping_add(1);
            data.insert_temp(window_generation_id, generation);
        });
    }
    let mut open = context
        .data_mut(|data| data.get_temp::<bool>(dialog_id))
        .unwrap_or(false);
    if !open {
        return;
    }
    let window_generation = context
        .data_mut(|data| data.get_temp::<u64>(window_generation_id))
        .unwrap_or_default();
    let window_size =
        super::dialog_size_constraints(context, egui::vec2(980.0, 720.0), WINDOW_MIN_SIZE);

    let normal_open_requested = open_requested && builder_request.is_none();
    let mut state = if normal_open_requested {
        WorkspaceState::default()
    } else {
        context
            .data_mut(|data| data.get_temp::<WorkspaceState>(state_id))
            .unwrap_or_default()
    };
    if let Some(request) = builder_request {
        match open_builder_request(&app.manifest, &mut state, &request) {
            Ok(()) => {}
            Err(error) => {
                state.feedback = Some(Feedback {
                    text: error,
                    is_error: true,
                });
            }
        }
    } else if normal_open_requested
        && let Err(error) = roll_family(
            &app.manifest,
            character_class(&app.document, character_index),
            app.show_dummy_items,
            app.plug_selection_mode,
            &mut state,
            ItemFamily::Weapon,
        )
    {
        state.feedback = Some(Feedback {
            text: error,
            is_error: true,
        });
    }
    let mut apply_requested = None;

    egui::Window::new("Random Item Builder")
        .id(dialog_id.with(("window", window_generation, window_size.compact)))
        .collapsible(false)
        .resizable(true)
        .default_size(window_size.default)
        .min_size(window_size.min)
        .max_size(window_size.max)
        .open(&mut open)
        .show(context, |ui| {
            let scroll_height = (ui.available_height() - FOOTER_RESERVE_HEIGHT).max(1.0);
            egui::ScrollArea::vertical()
                .id_salt(dialog_id.with(("scroll", window_generation, window_size.compact)))
                .max_height(scroll_height)
                .min_scrolled_height(scroll_height)
                .auto_shrink([false, false])
                .show_viewport(ui, |ui, viewport| {
                    let content_top = ui.cursor().top();
                    let plug_mode = app.plug_selection_mode;
                    draw_base_section(
                        ui,
                        &app.document,
                        &app.manifest,
                        character_index,
                        app.show_dummy_items,
                        plug_mode,
                        &mut state,
                    );
                    ui.add_space(6.0);
                    ui.separator();
                    ui.add_space(6.0);
                    draw_plug_heading(ui, &app.manifest, plug_mode, &mut state);
                    app.draw_plug_safety_controls(ui);
                    let content_used = ui.cursor().top() - content_top;
                    let plug_list_height =
                        (viewport.height() - content_used - PLUG_SECTION_CHROME_HEIGHT)
                            .max(MIN_PLUG_LIST_HEIGHT);
                    draw_plug_section_contents(
                        ui,
                        &app.manifest,
                        app.plug_selection_mode,
                        &mut state,
                        plug_list_height,
                    );
                });
            ui.add_space(6.0);
            let inventory_blocker = inventory_add_blocker(
                &app.document,
                &app.manifest,
                character_index,
                state.candidate.as_ref(),
            );
            let equip_warning = equip_replacement_warning(
                &app.document,
                &app.manifest,
                character_index,
                state.candidate.as_ref(),
            );
            draw_footer(
                ui,
                &mut state,
                inventory_blocker.as_deref(),
                equip_warning.as_ref(),
                &mut apply_requested,
            );
        });

    if let Some(candidate) = state.pending_destructive_equip.clone() {
        if let Some(warning) = equip_replacement_warning(
            &app.document,
            &app.manifest,
            character_index,
            Some(&candidate),
        ) {
            let mut replace = false;
            let mut cancel = false;
            let response = egui::Modal::new(dialog_id.with("replace-equipped-confirmation")).show(
                context,
                |ui| {
                    ui.set_width(440.0);
                    ui.heading("Replace equipped item?");
                    ui.add_space(6.0);
                    ui.colored_label(ui.visuals().warn_fg_color, warning.message());
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if ui.button("Delete old item and equip").clicked() {
                            replace = true;
                        }
                        if ui.button("Cancel").clicked() {
                            cancel = true;
                        }
                    });
                },
            );
            cancel |= response.should_close();
            if replace {
                state.pending_destructive_equip = None;
                apply_requested = Some(CandidateAction::Equip {
                    candidate,
                    discard_replaced: true,
                });
            } else if cancel {
                state.pending_destructive_equip = None;
            }
        } else {
            state.pending_destructive_equip = None;
            apply_requested = Some(CandidateAction::Equip {
                candidate,
                discard_replaced: false,
            });
        }
    }

    if let Some(action) = apply_requested {
        let (result, failure_prefix, unchanged) = match action {
            CandidateAction::Equip {
                candidate,
                discard_replaced,
            } => (
                apply_candidate(
                    &mut app.document,
                    &app.manifest,
                    character_index,
                    &candidate,
                    discard_replaced,
                ),
                "Randomized item not equipped",
                None,
            ),
            CandidateAction::AddToInventory(candidate) => (
                add_candidate_to_inventory(
                    &mut app.document,
                    &app.manifest,
                    character_index,
                    &candidate,
                ),
                "Randomized item not added",
                Some("Equipped gear was not changed."),
            ),
        };
        match result {
            Ok(message) => {
                app.dirty = true;
                app.set_status(format!("{message}; click Save to write it"), false);
                state.feedback = Some(Feedback {
                    text: unchanged.map_or_else(
                        || message.clone(),
                        |unchanged| format!("{message}. {unchanged}"),
                    ),
                    is_error: false,
                });
            }
            Err(error) => {
                app.set_status(format!("{failure_prefix}: {error}"), true);
                state.feedback = Some(Feedback {
                    text: error,
                    is_error: true,
                });
            }
        }
    }
    context.data_mut(|data| {
        data.insert_temp(dialog_id, open);
        data.insert_temp(state_id, state);
    });
}

fn draw_loadout_confirmation(
    app: &mut SundialApp,
    context: &egui::Context,
    character_index: usize,
    open_requested: bool,
) {
    let dialog_id = egui::Id::new(("equipment-randomize-loadout", character_index));
    let options_id = dialog_id.with("options");
    if open_requested {
        context.data_mut(|data| data.insert_temp(dialog_id, true));
        context.data_mut(|data| data.insert_temp(options_id, LoadoutOptions::default()));
    }
    let mut open = context
        .data_mut(|data| data.get_temp::<bool>(dialog_id))
        .unwrap_or(false);
    if !open {
        return;
    }

    let mut cancel_requested = false;
    let mut confirm_requested = false;
    let mut options = context
        .data_mut(|data| data.get_temp::<LoadoutOptions>(options_id))
        .unwrap_or_default();
    egui::Window::new("Randomize Loadout")
        .id(dialog_id.with("window"))
        .collapsible(false)
        .resizable(true)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .default_width(430.0)
        .open(&mut open)
        .show(context, |ui| {
            ui.label("Choose which parts of this character to regenerate.");
            ui.add_space(8.0);
            egui::Grid::new(dialog_id.with("scopes"))
                .num_columns(2)
                .spacing(egui::vec2(18.0, 6.0))
                .show(ui, |ui| {
                    ui.checkbox(&mut options.weapons, "Weapons")
                        .on_hover_text("Equipped weapons and held weapon inventory");
                    ui.checkbox(&mut options.armor, "Armor")
                        .on_hover_text("Equipped armor and held armor inventory");
                    ui.end_row();
                    ui.checkbox(&mut options.equipment_flair, "Equipment / Flair")
                        .on_hover_text("Ghosts, vehicles, ships, banners, emblems, emotes, and finishers");
                    ui.checkbox(&mut options.subclass, "Subclass")
                        .on_hover_text("A class-compatible subclass with valid default abilities");
                    ui.end_row();
                });
            ui.add_space(8.0);
            ui.separator();
            ui.add_space(6.0);
            app.draw_plug_safety_controls(ui);
            ui.label(
                egui::RichText::new(
                    "Checked sections replace their equipped items and held character inventory. Unchecked sections stay unchanged. One equipped exotic is kept per weapon and armor set.",
                )
                .weak(),
            );
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(options.any(), egui::Button::new("Randomize Loadout"))
                        .on_disabled_hover_text("Select at least one section")
                        .clicked()
                    {
                        confirm_requested = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel_requested = true;
                    }
                });
            });
        });

    if confirm_requested {
        open = false;
        match randomize_full_loadout(
            &mut app.document,
            &app.manifest,
            character_index,
            app.plug_selection_mode,
            app.show_dummy_items,
            options,
        ) {
            Ok(message) => {
                app.dirty = true;
                app.set_status(format!("{message}; click Save to write it"), false);
            }
            Err(error) => {
                app.set_status(format!("Loadout not randomized: {error}"), true);
            }
        }
    } else if cancel_requested {
        open = false;
    }
    context.data_mut(|data| {
        data.insert_temp(dialog_id, open);
        data.insert_temp(options_id, options);
    });
}

fn draw_base_section(
    ui: &mut egui::Ui,
    document: &Value,
    catalog: &Catalog,
    character_index: usize,
    show_dummy_items: bool,
    plug_mode: PlugSelectionMode,
    state: &mut WorkspaceState,
) {
    let class_type = character_class(document, character_index);
    ui.vertical(|ui| {
        ui.set_width(ui.available_width());

        let mut action = None;
        let previous_family = state.active_family;
        if state
            .last_plug_mode
            .replace(plug_mode)
            .is_some_and(|previous| previous != plug_mode)
        {
            state.feedback = None;
            state.armor_stat_allocation.clear_feedback();
        }
        ui.horizontal(|ui| {
            ui.selectable_value(&mut state.active_family, ItemFamily::Weapon, "Weapon");
            ui.selectable_value(&mut state.active_family, ItemFamily::Armor, "Armor");
            if state.active_family != previous_family {
                state.base_query.clear();
                state.socket_query.clear();
                state.selected_socket = 0;
                state.candidate = None;
                state.feedback = None;
                state.armor_stat_allocation.clear_feedback();
                action = Some(BaseAction::Random(state.active_family));
            }
            let status = state
                .feedback
                .as_ref()
                .map(|feedback| (feedback.text.clone(), feedback.is_error))
                .or_else(|| {
                    state
                        .armor_stat_allocation
                        .feedback()
                        .map(|(text, is_error)| (text.to_owned(), is_error))
                });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some((text, is_error)) = &status {
                    let color = if *is_error {
                        ui.visuals().error_fg_color
                    } else {
                        ui.visuals().strong_text_color()
                    };
                    ui.add(egui::Label::new(egui::RichText::new(text).color(color)).truncate())
                        .on_hover_text(text);
                }
            });
        });
        ui.separator();

        ui.add_space(4.0);
        let family = state.active_family;
        let has_allocation =
            family == ItemFamily::Armor && armor_allocation_available(catalog, state);
        let inline_width = BASE_FILTER_INLINE_WIDTH
            + ui.spacing().item_spacing.x
            + armor_stat_allocation::INLINE_CONTENT_WIDTH;
        let can_place_allocation_inline = has_allocation && ui.available_width() >= inline_width;
        if can_place_allocation_inline {
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(BASE_FILTER_INLINE_WIDTH, 45.0),
                    egui::Layout::top_down(egui::Align::LEFT),
                    |ui| {
                        draw_base_filters(
                            ui,
                            catalog,
                            class_type,
                            show_dummy_items,
                            family,
                            state,
                            &mut action,
                        );
                    },
                );
                draw_armor_allocation(ui, catalog, plug_mode, state);
            });
        } else {
            draw_base_filters(
                ui,
                catalog,
                class_type,
                show_dummy_items,
                family,
                state,
                &mut action,
            );
            if has_allocation {
                ui.add_space(4.0);
                draw_armor_allocation(ui, catalog, plug_mode, state);
            }
        }

        ui.add_space(6.0);
        let search_hint = match family {
            ItemFamily::Weapon => "Search for an exact weapon…",
            ItemFamily::Armor => "Search for exact armor…",
        };
        let search = ui.add(
            egui::TextEdit::singleline(&mut state.base_query)
                .hint_text(search_hint)
                .desired_width(f32::INFINITY),
        );
        let search_popup_id = ui.make_persistent_id("randomize-base-results-popup");
        if state.base_query.trim().is_empty() {
            if ui.memory(|memory| memory.is_popup_open(search_popup_id)) {
                ui.memory_mut(|memory| memory.close_popup());
            }
        } else if search.changed() || search.gained_focus() {
            ui.memory_mut(|memory| memory.open_popup(search_popup_id));
        }
        if !state.base_query.trim().is_empty() {
            let definition_results = matching_items(
                catalog,
                class_type,
                show_dummy_items,
                state.base_query.trim(),
                family,
                state.filter(family),
            );
            let matching_hashes = definition_results
                .iter()
                .map(|item| item.hash)
                .collect::<HashSet<_>>();
            let instance_results =
                matching_item_instances(document, character_index, &matching_hashes);
            egui::popup::popup_below_widget(
                ui,
                search_popup_id,
                &search,
                egui::PopupCloseBehavior::CloseOnClickOutside,
                |ui| {
                    ui.set_width(search.rect.width());
                    if instance_results.is_empty() && definition_results.is_empty() {
                        ui.label(
                            egui::RichText::new("No items match the search and filters.").weak(),
                        );
                    } else {
                        egui::ScrollArea::vertical()
                            .id_salt("randomize-base-results")
                            .max_height(230.0)
                            .show(ui, |ui| {
                                if !instance_results.is_empty() {
                                    for choice in instance_results
                                        .into_iter()
                                        .take(MAX_VISIBLE_SEARCH_RESULTS)
                                    {
                                        let Some(item) = catalog.item(choice.request.item_hash)
                                        else {
                                            continue;
                                        };
                                        let secondary =
                                            format!("{} · {}", item.type_name, choice.location);
                                        let response = item_editor::draw_catalog_picker_row(
                                            ui,
                                            catalog,
                                            item_editor::CatalogPickerRow {
                                                hash: item.hash,
                                                primary: &item.name,
                                                primary_max_rows: 1,
                                                secondary: Some(&secondary),
                                                icon_size: 34.0,
                                                row_height: 46.0,
                                                selected: false,
                                            },
                                        );
                                        let response = item_editor::catalog_item_tooltip(
                                            response, catalog, item.hash,
                                        );
                                        if response.clicked() {
                                            action =
                                                Some(BaseAction::SelectInstance(choice.request));
                                            ui.memory_mut(|memory| memory.close_popup());
                                        }
                                    }
                                    if !definition_results.is_empty() {
                                        ui.separator();
                                    }
                                }
                                for item in definition_results
                                    .into_iter()
                                    .take(MAX_VISIBLE_SEARCH_RESULTS)
                                {
                                    let slot_label = slot_for_bucket(item.bucket_hash)
                                        .map_or("Unknown slot", |(_, label)| label);
                                    let secondary = format!(
                                        "{} · {slot_label} · {}",
                                        item.type_name,
                                        format_hash_hex(item.hash)
                                    );
                                    let response = item_editor::draw_catalog_picker_row(
                                        ui,
                                        catalog,
                                        item_editor::CatalogPickerRow {
                                            hash: item.hash,
                                            primary: &item.name,
                                            primary_max_rows: 1,
                                            secondary: Some(&secondary),
                                            icon_size: 34.0,
                                            row_height: 46.0,
                                            selected: state
                                                .candidate
                                                .as_ref()
                                                .map(|roll| roll.item_hash)
                                                == Some(item.hash),
                                        },
                                    );
                                    let response = item_editor::catalog_item_tooltip(
                                        response, catalog, item.hash,
                                    );
                                    if response.clicked() {
                                        action = Some(BaseAction::SelectDefinition(item.hash));
                                        ui.memory_mut(|memory| memory.close_popup());
                                    }
                                }
                            });
                    }
                },
            );
        }

        if let Some(action) = action {
            let result = match action {
                BaseAction::Random(family) => roll_family(
                    catalog,
                    class_type,
                    show_dummy_items,
                    plug_mode,
                    state,
                    family,
                ),
                BaseAction::SelectDefinition(hash) => select_base(catalog, plug_mode, state, hash),
                BaseAction::SelectInstance(request) => {
                    open_builder_request(catalog, state, &request)
                }
            };
            if let Err(error) = result {
                state.feedback = Some(Feedback {
                    text: error,
                    is_error: true,
                });
            }
        }

        if let Some(candidate) = state.candidate.as_ref()
            && let Some(item) = catalog.item(candidate.item_hash)
        {
            ui.add_space(6.0);
            ui.separator();
            ui.add_space(4.0);
            draw_candidate_header(ui, catalog, item);
        }
    });
}

fn draw_base_filters(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    class_type: u64,
    show_dummy_items: bool,
    family: ItemFamily,
    state: &mut WorkspaceState,
    action: &mut Option<BaseAction>,
) {
    ui.horizontal_wrapped(|ui| {
        draw_family_roll(ui, state, family, class_type, action);
        if family == ItemFamily::Armor {
            ui.separator();
            ui.label("Class");
            ui.label(egui::RichText::new(class_name(class_type)).strong());
        }
    });

    ui.add_space(6.0);
    let filter_candidates = random_item_candidates(
        catalog,
        class_type,
        show_dummy_items,
        family.slots().iter().copied(),
    );
    let filter_scope = match family {
        ItemFamily::Weapon => item_editor::ItemFilterScope::Weapon,
        ItemFamily::Armor => item_editor::ItemFilterScope::Armor,
    };
    item_editor::draw_item_filter_bar(
        ui,
        ("random-item", family),
        filter_scope,
        &filter_candidates,
        state.filter_mut(family),
    );
}

fn armor_allocation_available(catalog: &Catalog, state: &WorkspaceState) -> bool {
    state.candidate.as_ref().is_some_and(|candidate| {
        catalog.item(candidate.item_hash).is_some_and(|item| {
            armor_stat_allocation::is_available(catalog, item, candidate.plugs.len())
        })
    })
}

fn draw_armor_allocation(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    plug_mode: PlugSelectionMode,
    state: &mut WorkspaceState,
) {
    let Some((item_hash, mut plugs)) = state
        .candidate
        .as_ref()
        .map(|candidate| (candidate.item_hash, candidate.plugs.clone()))
    else {
        return;
    };
    let Some(item) = catalog.item(item_hash) else {
        return;
    };
    let changed = armor_stat_allocation::draw(
        ui,
        catalog,
        item,
        &mut plugs,
        plug_mode,
        &mut state.armor_stat_allocation,
    );
    if changed && let Some(candidate) = state.candidate.as_mut() {
        candidate.plugs = plugs;
        state.feedback = None;
    }
}

fn draw_family_roll(
    ui: &mut egui::Ui,
    state: &mut WorkspaceState,
    family: ItemFamily,
    class_type: u64,
    action: &mut Option<BaseAction>,
) {
    let selection = match family {
        ItemFamily::Weapon => &mut state.weapon_slot,
        ItemFamily::Armor => &mut state.armor_slot,
    };
    let (selection_label, any_label) = match family {
        ItemFamily::Weapon => ("Weapon slot", "Any weapon slot"),
        ItemFamily::Armor => ("Armor type", "Any armor type"),
    };
    let selected_text = selection
        .and_then(|index| family.slots().get(index).copied())
        .and_then(slot_definition)
        .map_or_else(|| any_label.to_owned(), |(_, label, _)| label.to_owned());
    ui.label(selection_label);
    egui::ComboBox::from_id_salt(("randomize-slot", family))
        .selected_text(selected_text)
        .width(130.0)
        .show_ui(ui, |ui| {
            ui.selectable_value(selection, None, any_label);
            for (index, &slot) in family.slots().iter().enumerate() {
                if let Some((_, label, _)) = slot_definition(slot) {
                    ui.selectable_value(selection, Some(index), label);
                }
            }
        });
    if ui
        .add_enabled(
            class_type <= 2,
            egui::Button::new(format!("Roll {}", family.label())),
        )
        .clicked()
    {
        *action = Some(BaseAction::Random(family));
    }
}

fn draw_candidate_header(ui: &mut egui::Ui, catalog: &Catalog, item: &ItemDef) {
    ui.horizontal(|ui| {
        if let Some(icon) = catalog.icon_texture(ui.ctx(), item.hash) {
            ui.add(egui::Image::new((icon.id(), egui::vec2(52.0, 52.0))).corner_radius(3));
        }
        ui.vertical(|ui| {
            ui.label(egui::RichText::new(&item.name).strong().size(16.0));
            let slot_label = slot_for_bucket(item.bucket_hash).map_or("Unknown slot", |(_, l)| l);
            ui.label(format!("{} · {slot_label}", item.type_name));
        });
    });
}

fn draw_plug_heading(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    plug_mode: PlugSelectionMode,
    state: &mut WorkspaceState,
) {
    let item = state
        .candidate
        .as_ref()
        .and_then(|candidate| catalog.item(candidate.item_hash));
    ui.horizontal(|ui| {
        ui.heading("Plugs");
        let reroll = ui.add_enabled(item.is_some(), egui::Button::new("Reroll plugs").small());
        if reroll.clicked()
            && let Some(item) = item
        {
            match rolled_candidate(catalog, item, plug_mode) {
                Ok(candidate) => {
                    state.candidate = Some(candidate);
                    state.feedback = None;
                    state.armor_stat_allocation.clear_feedback();
                }
                Err(error) => {
                    state.feedback = Some(Feedback {
                        text: error,
                        is_error: true,
                    });
                }
            }
        }
    });
}

fn draw_plug_section_contents(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    plug_mode: PlugSelectionMode,
    state: &mut WorkspaceState,
    plug_list_height: f32,
) {
    ui.vertical(|ui| {
        ui.set_width(ui.available_width());
        let Some(item_hash) = state
            .candidate
            .as_ref()
            .map(|candidate| candidate.item_hash)
        else {
            ui.label(
                egui::RichText::new("Choose a base item to generate and edit its plugs.").weak(),
            );
            return;
        };
        let Some(item) = catalog.item(item_hash) else {
            ui.colored_label(
                ui.visuals().error_fg_color,
                "The selected base item is unavailable.",
            );
            return;
        };
        let socket_count = item.sockets.len().min(inventory::MAX_ITEM_PLUGS);
        if socket_count == 0 {
            ui.label(egui::RichText::new("This item has no configurable sockets.").weak());
            return;
        }
        state.selected_socket = state.selected_socket.min(socket_count - 1);

        ui.columns(2, |columns| {
            let (left, right) = columns.split_at_mut(1);
            draw_selected_plugs(&mut left[0], catalog, item, state, plug_list_height);
            draw_available_plugs(
                &mut right[0],
                catalog,
                item,
                plug_mode,
                state,
                plug_list_height,
            );
        });
    });
}

fn draw_available_plugs(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    item: &ItemDef,
    plug_mode: PlugSelectionMode,
    state: &mut WorkspaceState,
    plug_list_height: f32,
) {
    ui.vertical(|ui| {
        ui.set_width(ui.available_width());
        let socket_index = state.selected_socket;
        let socket_label = item.sockets[socket_index].display_label(socket_index);
        let (choices, show_types) =
            item_editor::plug_choices_for_socket(catalog, item, socket_index, plug_mode);
        let current_hash = state
            .candidate
            .as_ref()
            .and_then(|candidate| candidate.plugs.get(socket_index))
            .copied()
            .flatten();
        let default_hash = default_plug(item, socket_index).ok().flatten();
        let mut selection = None::<Option<u64>>;

        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new("Available plugs").strong().size(15.0));
            ui.label(format!("· {socket_label}"));
            if ui
                .add_enabled(!choices.is_empty(), egui::Button::new("Reroll").small())
                .clicked()
            {
                let hashes = choices.iter().map(|choice| choice.hash).collect::<Vec<_>>();
                selection = Rng::from_clock().pick_valid_hash(&hashes).map(Some);
            }
            if ui
                .add_enabled(
                    current_hash != default_hash,
                    egui::Button::new("Reset").small(),
                )
                .clicked()
            {
                selection = Some(default_hash);
            }
        });

        let searchable = choices.len() > 12;
        if searchable {
            ui.add(
                egui::TextEdit::singleline(&mut state.socket_query)
                    .hint_text("Search plugs…")
                    .desired_width(f32::INFINITY),
            );
        } else {
            state.socket_query.clear();
        }
        let query = state.socket_query.trim().to_ascii_lowercase();
        egui::ScrollArea::vertical()
            .id_salt(("randomize-plugs", socket_index))
            .max_height(plug_list_height)
            .min_scrolled_height(plug_list_height)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                let mut visible = 0;
                for choice in choices.iter().filter(|choice| {
                    query.is_empty()
                        || choice.label.to_ascii_lowercase().contains(&query)
                        || choice.type_name.to_ascii_lowercase().contains(&query)
                        || catalog.description(choice.hash).is_some_and(|description| {
                            description.to_ascii_lowercase().contains(&query)
                        })
                }) {
                    visible += 1;
                    let description = catalog
                        .description(choice.hash)
                        .map(compact_text)
                        .unwrap_or_default();
                    let secondary = match (
                        show_types.then_some(choice.type_name.as_str()),
                        description.as_str(),
                    ) {
                        (Some(type_name), description)
                            if !type_name.is_empty() && !description.is_empty() =>
                        {
                            format!("{type_name} · {description}")
                        }
                        (Some(type_name), _) if !type_name.is_empty() => type_name.to_owned(),
                        (_, description) if !description.is_empty() => description.to_owned(),
                        _ => String::new(),
                    };
                    let response = item_editor::draw_catalog_picker_row(
                        ui,
                        catalog,
                        item_editor::CatalogPickerRow {
                            hash: choice.hash,
                            primary: &choice.label,
                            primary_max_rows: 1,
                            secondary: (!secondary.is_empty()).then_some(secondary.as_str()),
                            icon_size: PLUG_ICON_SIZE,
                            row_height: AVAILABLE_PLUG_ROW_HEIGHT,
                            selected: current_hash == Some(choice.hash),
                        },
                    );
                    let response =
                        item_editor::catalog_item_tooltip(response, catalog, choice.hash);
                    if response.clicked() {
                        selection = Some(Some(choice.hash));
                    }
                }
                if visible == 0 {
                    ui.label(egui::RichText::new("No matching plugs.").weak());
                }
            });

        if let Some(hash) = selection
            && let Some(candidate) = state.candidate.as_mut()
        {
            candidate.plugs[socket_index] = hash;
            state.feedback = None;
            state.armor_stat_allocation.clear_feedback();
        }
    });
}

fn draw_selected_plugs(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    item: &ItemDef,
    state: &mut WorkspaceState,
    plug_list_height: f32,
) {
    ui.vertical(|ui| {
        ui.set_width(ui.available_width());
        ui.label(egui::RichText::new("Selected plugs").strong().size(15.0));
        let mut selected = None;
        egui::ScrollArea::vertical()
            .id_salt("randomize-selected-plugs")
            .max_height(plug_list_height)
            .min_scrolled_height(plug_list_height)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for (socket_index, socket) in item
                    .sockets
                    .iter()
                    .take(inventory::MAX_ITEM_PLUGS)
                    .enumerate()
                {
                    let hash = state
                        .candidate
                        .as_ref()
                        .and_then(|candidate| candidate.plugs.get(socket_index))
                        .copied()
                        .flatten();
                    let plug_label = hash
                        .map_or_else(|| "None".to_owned(), |hash| catalog.plug_label(hash, false));
                    let socket_label = socket.display_label(socket_index);
                    let hover_text = format!("{socket_label}: {plug_label}");
                    let text = selected_plug_text(ui, &socket_label, &plug_label);
                    let icon = plug_icon_or_blank(ui, catalog, hash);
                    let button = egui::Button::image_and_text(
                        (icon.id(), egui::vec2(PLUG_ICON_SIZE, PLUG_ICON_SIZE)),
                        text,
                    )
                    .truncate()
                    .selected(state.selected_socket == socket_index);
                    let response = ui.add_sized([ui.available_width(), PLUG_ROW_HEIGHT], button);
                    let response = match hash {
                        Some(hash) => {
                            item_editor::catalog_item_tooltip_immediate(response, catalog, hash)
                        }
                        None => response.on_hover_text(hover_text),
                    };
                    if response.clicked() {
                        selected = Some(socket_index);
                    }
                }
            });
        if let Some(socket_index) = selected {
            state.selected_socket = socket_index;
            state.socket_query.clear();
        }
    });
}

fn selected_plug_text(
    ui: &egui::Ui,
    socket_label: &str,
    plug_label: &str,
) -> egui::text::LayoutJob {
    let font_id = egui::TextStyle::Button.resolve(ui.style());
    let mut text = egui::text::LayoutJob::default();
    text.append(
        socket_label,
        0.0,
        egui::TextFormat {
            font_id: font_id.clone(),
            color: ui.visuals().strong_text_color(),
            ..Default::default()
        },
    );
    text.append(
        &format!(": {plug_label}"),
        0.0,
        egui::TextFormat {
            font_id,
            color: egui::Color32::PLACEHOLDER,
            ..Default::default()
        },
    );
    text
}

fn compact_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn plug_icon_or_blank(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    hash: Option<u64>,
) -> egui::TextureHandle {
    if let Some(icon) = hash.and_then(|hash| catalog.icon_texture(ui.ctx(), hash)) {
        return icon;
    }

    let texture_id = egui::Id::new("randomize-blank-plug-icon");
    if let Some(texture) = ui
        .ctx()
        .data_mut(|data| data.get_temp::<egui::TextureHandle>(texture_id))
    {
        return texture;
    }

    let texture = ui.ctx().load_texture(
        "randomize-blank-plug-icon",
        egui::ColorImage::new([1, 1], egui::Color32::TRANSPARENT),
        egui::TextureOptions::NEAREST,
    );
    ui.ctx()
        .data_mut(|data| data.insert_temp(texture_id, texture.clone()));
    texture
}

fn draw_footer(
    ui: &mut egui::Ui,
    state: &mut WorkspaceState,
    inventory_blocker: Option<&str>,
    equip_warning: Option<&EquipReplacementWarning>,
    apply_requested: &mut Option<CandidateAction>,
) {
    ui.horizontal_wrapped(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let candidate = state.candidate.clone();
            let can_apply = candidate.is_some();
            let equip = ui.add_enabled(can_apply, egui::Button::new("Equip item"));
            let equip = if let Some(warning) = equip_warning {
                equip.on_hover_text(warning.message())
            } else {
                equip
            };
            if equip.clicked() {
                if equip_warning.is_some() {
                    state.pending_destructive_equip = candidate;
                } else {
                    *apply_requested = candidate.map(|candidate| CandidateAction::Equip {
                        candidate,
                        discard_replaced: false,
                    });
                }
            }
            let add = ui.add_enabled(
                can_apply && inventory_blocker.is_none(),
                egui::Button::new("Add to inventory"),
            );
            let add = if let Some(reason) = inventory_blocker {
                add.on_disabled_hover_text(reason)
            } else {
                add
            };
            if add.clicked() {
                *apply_requested = state.candidate.clone().map(CandidateAction::AddToInventory);
            }
        });
    });
}

fn roll_family(
    catalog: &Catalog,
    class_type: u64,
    show_dummy_items: bool,
    plug_mode: PlugSelectionMode,
    state: &mut WorkspaceState,
    family: ItemFamily,
) -> Result<(), String> {
    let selected_slot = match family {
        ItemFamily::Weapon => state.weapon_slot,
        ItemFamily::Armor => state.armor_slot,
    }
    .and_then(|index| family.slots().get(index).copied());
    let candidates = family
        .slots()
        .iter()
        .copied()
        .filter(|slot| selected_slot.is_none_or(|selected| selected == *slot));
    let candidates = random_item_candidates(catalog, class_type, show_dummy_items, candidates)
        .into_iter()
        .filter(|item| state.filter(family).matches(catalog, item))
        .collect::<Vec<_>>();
    let item = Rng::from_clock()
        .pick(&candidates)
        .copied()
        .ok_or_else(|| {
            format!(
                "No usable {} definitions match the selection and filters",
                family.label()
            )
        })?;
    state.candidate = Some(rolled_candidate(catalog, item, plug_mode)?);
    state.selected_socket = 0;
    state.socket_query.clear();
    state.feedback = None;
    state.armor_stat_allocation.clear_feedback();
    Ok(())
}

fn select_base(
    catalog: &Catalog,
    plug_mode: PlugSelectionMode,
    state: &mut WorkspaceState,
    hash: u64,
) -> Result<(), String> {
    let item = catalog
        .item(hash)
        .filter(|item| item_can_be_authored(item))
        .ok_or("The selected base item cannot be authored safely")?;
    state.candidate = Some(rolled_candidate(catalog, item, plug_mode)?);
    state.selected_socket = 0;
    state.socket_query.clear();
    state.feedback = None;
    state.armor_stat_allocation.clear_feedback();
    Ok(())
}

fn open_builder_request(
    catalog: &Catalog,
    state: &mut WorkspaceState,
    request: &ItemBuilderRequest,
) -> Result<(), String> {
    let (family, slot_index, candidate) = candidate_from_builder_request(catalog, request)?;
    state.active_family = family;
    match family {
        ItemFamily::Weapon => state.weapon_slot = Some(slot_index),
        ItemFamily::Armor => state.armor_slot = Some(slot_index),
    }
    state.base_query.clear();
    state.socket_query.clear();
    state.selected_socket = 0;
    state.candidate = Some(candidate);
    state.pending_destructive_equip = None;
    state.feedback = None;
    state.armor_stat_allocation.clear_feedback();
    Ok(())
}

fn candidate_from_builder_request(
    catalog: &Catalog,
    request: &ItemBuilderRequest,
) -> Result<(ItemFamily, usize, Candidate), String> {
    let item = catalog
        .item(request.item_hash)
        .filter(|item| item_can_be_authored(item))
        .ok_or("This item cannot be opened safely in Random Item Builder")?;
    let (slot, _) = slot_for_bucket(item.bucket_hash)
        .ok_or("Random Item Builder only supports weapons and armor")?;
    let (family, slot_index) = if let Some(index) = WEAPON_SLOTS
        .iter()
        .position(|candidate_slot| *candidate_slot == slot)
    {
        (ItemFamily::Weapon, index)
    } else if let Some(index) = ARMOR_SLOTS
        .iter()
        .position(|candidate_slot| *candidate_slot == slot)
    {
        (ItemFamily::Armor, index)
    } else {
        return Err("Random Item Builder only supports weapons and armor".to_owned());
    };

    let mut candidate = default_candidate(item)?;
    if let Some(authored_plugs) = request.authored_plugs.as_ref() {
        let (plug_values, _) = displayed_plugs(Some(authored_plugs), &item.default_plugs);
        let plugs = plug_values
            .iter()
            .map(|value| {
                if value.is_null() {
                    Ok(None)
                } else {
                    parse_unsigned_value(value)
                        .filter(|hash| valid_definition_hash(*hash))
                        .map(Some)
                        .ok_or_else(|| "This item's authored plugs are malformed".to_owned())
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        let authored_count = item
            .sockets
            .len()
            .max(plugs.len())
            .min(inventory::MAX_ITEM_PLUGS);
        candidate.plugs = plugs;
        candidate.plugs.resize(authored_count, None);
        candidate.plugs.truncate(inventory::MAX_ITEM_PLUGS);
    }
    Ok((family, slot_index, candidate))
}

fn rolled_candidate(
    catalog: &Catalog,
    item: &ItemDef,
    plug_mode: PlugSelectionMode,
) -> Result<Candidate, String> {
    let mut candidate = default_candidate(item)?;
    let mut rng = Rng::from_clock();
    for socket_index in 0..item.sockets.len().min(inventory::MAX_ITEM_PLUGS) {
        if let Some(hash) = random_plug(catalog, item, socket_index, plug_mode, &mut rng) {
            candidate.plugs[socket_index] = Some(hash);
        }
    }
    Ok(candidate)
}

fn default_candidate(item: &ItemDef) -> Result<Candidate, String> {
    let mut plugs = item
        .default_plugs
        .iter()
        .map(|plug| {
            plug.as_deref()
                .map(|text| {
                    parse_hash_hex(text)
                        .filter(|hash| valid_definition_hash(*hash))
                        .ok_or_else(|| {
                            format!("{} contains an invalid package-default plug", item.name)
                        })
                })
                .transpose()
        })
        .collect::<Result<Vec<_>, _>>()?;
    let authored_count = item
        .sockets
        .len()
        .max(plugs.len())
        .min(inventory::MAX_ITEM_PLUGS);
    plugs.resize(authored_count, None);
    Ok(Candidate {
        item_hash: item.hash,
        plugs,
    })
}

fn default_plug(item: &ItemDef, socket_index: usize) -> Result<Option<u64>, String> {
    item.default_plugs
        .get(socket_index)
        .and_then(|plug| plug.as_deref())
        .map(|text| {
            parse_hash_hex(text)
                .filter(|hash| valid_definition_hash(*hash))
                .ok_or_else(|| format!("{} contains an invalid package-default plug", item.name))
        })
        .transpose()
}

fn random_plug(
    catalog: &Catalog,
    item: &ItemDef,
    socket_index: usize,
    plug_mode: PlugSelectionMode,
    rng: &mut Rng,
) -> Option<u64> {
    let socket = item.sockets.get(socket_index)?;
    match plug_mode {
        PlugSelectionMode::Supported => rng.pick_valid_hash(catalog.socket_options(socket)),
        PlugSelectionMode::MatchingSocketType => {
            rng.pick_valid_hash(catalog.socket_type_options(socket.socket_type))
        }
        PlugSelectionMode::GearType => {
            let options = catalog.gear_type_options(item, socket_index);
            rng.pick_valid_hash(&options)
        }
        PlugSelectionMode::AnyPlug => rng.pick_valid_hash(catalog.all_plug_options()),
    }
}

fn apply_candidate(
    document: &mut Value,
    catalog: &Catalog,
    character_index: usize,
    candidate: &Candidate,
    discard_replaced: bool,
) -> Result<String, String> {
    if !inventory::schema_mode(document).can_mutate_equipment() {
        return Err("Randomizing requires a writable equipment schema".to_owned());
    }
    if candidate.plugs.len() > inventory::MAX_ITEM_PLUGS {
        return Err("The generated item contains too many authored plugs".to_owned());
    }
    let item = catalog
        .item(candidate.item_hash)
        .filter(|item| item_can_be_authored(item))
        .ok_or("The generated base item is no longer available")?;
    let (slot, slot_label) = slot_for_bucket(item.bucket_hash)
        .ok_or("The generated item does not belong to a weapon or armor slot")?;
    let class_type = character_class(document, character_index);
    if class_type > 2 {
        return Err(format!(
            "Character {} has no valid class",
            character_index + 1
        ));
    }
    if item.class_type != 3 && item.class_type != class_type {
        return Err(format!(
            "{} is not compatible with {}",
            item.name,
            class_name(class_type)
        ));
    }

    let equipped_item = equipped_item_row(document, character_index, slot)?;
    let mut updated = document.clone();
    let mut previous_item_preserved = false;
    let mut previous_item_discarded = false;
    if let Some(equipped_item) = equipped_item.as_ref() {
        if let Some(reason) = equipped_item_preservation_blocker(
            document,
            catalog,
            character_index,
            slot,
            equipped_item,
        ) {
            if !discard_replaced {
                return Err(format!(
                    "The currently equipped item cannot be moved to inventory: {reason}"
                ));
            }
            previous_item_discarded = true;
        } else {
            inventory::move_equipment_item_to_inventory(&mut updated, character_index, slot)
                .map_err(|error| {
                    format!("The currently equipped item could not be moved to inventory: {error}")
                })?;
            previous_item_preserved = true;
        }
    }
    equip_definition(
        &mut updated,
        character_index,
        slot,
        item.hash,
        &item.default_plugs,
    )?;
    for (socket_index, hash) in candidate.plugs.iter().copied().enumerate() {
        set_equipment_item_plug(
            &mut updated,
            character_index,
            slot,
            socket_index,
            &item.default_plugs,
            hash,
        )?;
    }
    settings::validate_document(&updated)
        .map_err(|error| format!("The generated item did not pass validation: {error}"))?;
    *document = updated;
    let result = if previous_item_preserved {
        format!(
            "Equipped {} in {slot_label}; moved the previous item to character inventory",
            item.name
        )
    } else if previous_item_discarded {
        format!(
            "Equipped {} in {slot_label}; deleted the previous equipped item",
            item.name
        )
    } else {
        format!("Equipped {} in {slot_label}", item.name)
    };
    Ok(result)
}

fn equip_replacement_warning(
    document: &Value,
    catalog: &Catalog,
    character_index: usize,
    candidate: Option<&Candidate>,
) -> Option<EquipReplacementWarning> {
    let candidate = candidate?;
    let candidate_item = catalog.item(candidate.item_hash)?;
    let (slot, slot_label) = slot_for_bucket(candidate_item.bucket_hash)?;
    let equipped_item = equipped_item_row(document, character_index, slot)
        .ok()
        .flatten()?;
    let current_item_name = equipped_item
        .get("definition_hash")
        .and_then(parse_unsigned_value)
        .and_then(|hash| catalog.item(hash).map(|item| item.name.clone()))
        .unwrap_or_else(|| format!("The currently equipped {slot_label} item"));
    let reason = equipped_item_preservation_blocker(
        document,
        catalog,
        character_index,
        slot,
        &equipped_item,
    )?;
    Some(EquipReplacementWarning {
        current_item_name,
        candidate_item_name: candidate_item.name.clone(),
        reason,
    })
}

fn equipped_item_row(
    document: &Value,
    character_index: usize,
    slot: &str,
) -> Result<Option<Value>, String> {
    let equipment = document
        .get("state")
        .and_then(|state| state.get("characters"))
        .and_then(Value::as_array)
        .and_then(|characters| characters.get(character_index))
        .and_then(|character| character.get("equipment"))
        .and_then(Value::as_object)
        .ok_or("Character equipment is unavailable")?;
    match equipment.get(slot) {
        Some(Value::Object(_)) => Ok(equipment.get(slot).cloned()),
        Some(Value::Null) | None => Ok(None),
        Some(_) => Err(format!("The equipped {slot} item is malformed")),
    }
}

fn equipped_item_preservation_blocker(
    document: &Value,
    catalog: &Catalog,
    character_index: usize,
    slot: &str,
    equipped_item: &Value,
) -> Option<String> {
    let Some(definition_hash) = equipped_item
        .get("definition_hash")
        .and_then(parse_unsigned_value)
    else {
        return Some("Its definition hash is unreadable".to_owned());
    };
    if let Some(reason) = definition_inventory_add_blocker(
        document,
        catalog,
        character_index,
        definition_hash,
        "The equipped item cannot be stored in character inventory",
    ) {
        return Some(reason);
    }

    let mut preview = document.clone();
    inventory::move_equipment_item_to_inventory(&mut preview, character_index, slot)
        .err()
        .map(|error| error.to_string())
}

fn inventory_add_blocker(
    document: &Value,
    catalog: &Catalog,
    character_index: usize,
    candidate: Option<&Candidate>,
) -> Option<String> {
    let candidate = candidate?;
    let Some(item) = catalog.item(candidate.item_hash) else {
        return Some("The generated base item is unavailable".to_owned());
    };
    definition_inventory_add_blocker(
        document,
        catalog,
        character_index,
        item.hash,
        "This definition cannot be added to character inventory",
    )
}

fn definition_inventory_add_blocker(
    document: &Value,
    catalog: &Catalog,
    character_index: usize,
    definition_hash: u64,
    unavailable_message: &str,
) -> Option<String> {
    if !inventory::schema_mode(document).can_mutate_character_inventory() {
        return Some("Requires writable character inventory".to_owned());
    }
    let inventory = match inventory::character_inventory(document, character_index) {
        Ok(inventory) => inventory,
        Err(error) => return Some(format!("Character inventory is unavailable: {error}")),
    };
    if inventory
        .as_ref()
        .is_some_and(|items| items.len() >= inventory::CHARACTER_INVENTORY_CAPACITY)
    {
        return Some("Character inventory is full".to_owned());
    }
    let Some(metadata) = catalog
        .inventory_metadata(definition_hash)
        .filter(|metadata| metadata.is_character_inventory_candidate())
    else {
        return Some(unavailable_message.to_owned());
    };
    if let Some(reason) = character_bucket_add_blocker(
        document,
        catalog,
        character_index,
        inventory.as_deref().unwrap_or_default(),
        metadata,
    ) {
        return Some(reason);
    }
    None
}

fn character_bucket_add_blocker(
    document: &Value,
    catalog: &Catalog,
    character_index: usize,
    inventory: &[inventory::InventoryItemSnapshot],
    candidate: &InventoryMetadata,
) -> Option<String> {
    let Some(capacity) = candidate.authored_row_capacity().map(usize::from) else {
        return Some("This inventory bucket has no safe capacity".to_owned());
    };
    let Some(equipment) = document
        .get("state")
        .and_then(|state| state.get("characters"))
        .and_then(Value::as_array)
        .and_then(|characters| characters.get(character_index))
        .and_then(|character| character.get("equipment"))
        .and_then(Value::as_object)
    else {
        return Some("Character equipment is unavailable".to_owned());
    };
    let mut occupied = 0usize;
    let mut unresolved = 0usize;
    let mut count = |hash: Option<u64>| match hash.and_then(|hash| catalog.inventory_metadata(hash))
    {
        Some(metadata)
            if metadata.scope == InventoryScope::Character
                && metadata.native_bucket_id == candidate.native_bucket_id =>
        {
            occupied += 1;
        }
        Some(metadata) if metadata.scope == InventoryScope::Character => {}
        Some(_) | None => unresolved += 1,
    };
    for item in equipment.values().filter(|item| !item.is_null()) {
        count(item.get("definition_hash").and_then(parse_unsigned_value));
    }
    for item in inventory {
        count(Some(u64::from(item.definition_hash)));
    }

    let bucket_label = candidate.bucket_label();
    if occupied >= capacity {
        Some(format!("{bucket_label} is full"))
    } else if bucket_has_room_for_add(occupied, unresolved, capacity) {
        None
    } else {
        Some(format!(
            "Cannot verify room in {bucket_label} because existing inventory placement is unresolved"
        ))
    }
}

const fn bucket_has_room_for_add(occupied: usize, unresolved: usize, capacity: usize) -> bool {
    occupied.saturating_add(unresolved) < capacity
}

fn add_candidate_to_inventory(
    document: &mut Value,
    catalog: &Catalog,
    character_index: usize,
    candidate: &Candidate,
) -> Result<String, String> {
    if let Some(reason) = inventory_add_blocker(document, catalog, character_index, Some(candidate))
    {
        return Err(reason);
    }
    if candidate.plugs.len() > inventory::MAX_ITEM_PLUGS {
        return Err("The generated item contains too many authored plugs".to_owned());
    }
    let item = catalog
        .item(candidate.item_hash)
        .filter(|item| item_can_be_authored(item))
        .ok_or("The generated base item is no longer available")?;
    let class_type = character_class(document, character_index);
    if class_type > 2 {
        return Err(format!(
            "Character {} has no valid class",
            character_index + 1
        ));
    }
    if item.class_type != 3 && item.class_type != class_type {
        return Err(format!(
            "{} is not compatible with {}",
            item.name,
            class_name(class_type)
        ));
    }
    let definition_hash = u32::try_from(item.hash)
        .map_err(|_| format!("{} has an invalid definition hash", item.name))?;
    let item_level = capped_inventory_item_level(catalog, item)?;
    let plugs = candidate
        .plugs
        .iter()
        .copied()
        .map(|hash| {
            hash.map(|hash| {
                u32::try_from(hash).map_err(|_| format!("{} generated an invalid plug", item.name))
            })
            .transpose()
        })
        .collect::<Result<Vec<_>, _>>()?;

    let mut updated = document.clone();
    let location = inventory::add_inventory_item(
        &mut updated,
        character_index,
        inventory::NewInventoryItem::single(definition_hash, item_level),
    )
    .map_err(|error| error.to_string())?;
    inventory::apply_inventory_item_action(
        &mut updated,
        location,
        inventory::InventoryItemAction::SetPlugs(inventory::ItemPlugs::Authored(plugs)),
    )
    .map_err(|error| error.to_string())?;
    settings::validate_document(&updated)
        .map_err(|error| format!("The generated item did not pass validation: {error}"))?;
    *document = updated;
    Ok(format!("Added {} to character inventory", item.name))
}

fn randomize_full_loadout(
    document: &mut Value,
    catalog: &Catalog,
    character_index: usize,
    plug_mode: PlugSelectionMode,
    show_dummy_items: bool,
    options: LoadoutOptions,
) -> Result<String, String> {
    if !options.any() {
        return Err("Select at least one loadout section".to_owned());
    }
    let schema_mode = inventory::schema_mode(document);
    if !schema_mode.can_mutate_equipment() {
        return Err("Randomizing a loadout requires writable equipment".to_owned());
    }
    if !schema_mode.can_mutate_character_inventory() {
        return Err("Randomizing a loadout requires writable character inventory".to_owned());
    }

    let class_type = character_class(document, character_index);
    if class_type > 2 {
        return Err(format!(
            "Character {} has no valid class",
            character_index + 1
        ));
    }
    inventory::character_inventory(document, character_index).map_err(|error| error.to_string())?;

    let mut updated = document.clone();
    clear_selected_inventory(&mut updated, catalog, character_index, options)?;
    let mut rng = Rng::from_clock();
    let mut exotic_weapon_equipped = false;
    let mut exotic_armor_equipped = false;
    let mut generated_items = 0usize;
    let mut generated_slots = 0usize;
    let mut held_items = inventory::character_inventory(&updated, character_index)
        .map_err(|error| error.to_string())?
        .map_or(0, |items| items.len());

    for &(slot, _, bucket_hash) in SLOTS {
        let scope = loadout_scope_for_slot(slot);
        if !options.includes(scope) {
            continue;
        }
        let candidates = catalog
            .browse(bucket_hash, class_type, show_dummy_items)
            .into_iter()
            .filter(|item| item_can_be_authored(item))
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            continue;
        }
        let ordinary = candidates
            .iter()
            .copied()
            .filter(|item| !is_exotic(catalog, item))
            .collect::<Vec<_>>();
        let equipped_candidates = if (WEAPON_SLOTS.contains(&slot) && exotic_weapon_equipped)
            || (ARMOR_SLOTS.contains(&slot) && exotic_armor_equipped)
        {
            ordinary.as_slice()
        } else {
            candidates.as_slice()
        };
        let mut used_hashes = Vec::new();
        let equipped = pick_avoiding(&mut rng, equipped_candidates, &used_hashes)
            .ok_or_else(|| format!("No usable non-exotic item is available for the {slot} slot"))?;
        if slot == SUBCLASS_SLOT {
            equip_subclass_with_default_abilities(&mut updated, character_index, equipped)?;
        } else {
            install_random_equipped(
                &mut updated,
                catalog,
                character_index,
                slot,
                equipped,
                plug_mode,
                &mut rng,
            )?;
        }
        if WEAPON_SLOTS.contains(&slot) && is_exotic(catalog, equipped) {
            exotic_weapon_equipped = true;
        }
        if ARMOR_SLOTS.contains(&slot) && is_exotic(catalog, equipped) {
            exotic_armor_equipped = true;
        }
        used_hashes.push(equipped.hash);
        generated_items += 1;
        generated_slots += 1;

        let held_target = if matches!(slot, SUBCLASS_SLOT | CLAN_BANNER_SLOT) {
            0
        } else {
            HELD_ITEMS_PER_SLOT
                .min(inventory::CHARACTER_INVENTORY_CAPACITY.saturating_sub(held_items))
        };
        for _ in 0..held_target {
            let held = pick_avoiding(&mut rng, &candidates, &used_hashes)
                .ok_or_else(|| format!("No usable held item is available for the {slot} slot"))?;
            install_random_held(
                &mut updated,
                catalog,
                character_index,
                held,
                plug_mode,
                &mut rng,
            )?;
            used_hashes.push(held.hash);
            held_items += 1;
            generated_items += 1;
        }
    }

    if generated_slots == 0 {
        return Err("No usable item definitions were found for this character".to_owned());
    }
    settings::validate_document(&updated)
        .map_err(|error| format!("The generated loadout did not pass validation: {error}"))?;
    *document = updated;
    Ok(format!(
        "Randomized {generated_items} items across {generated_slots} slots"
    ))
}

fn clear_selected_inventory(
    document: &mut Value,
    catalog: &Catalog,
    character_index: usize,
    options: LoadoutOptions,
) -> Result<(), String> {
    let character = document
        .pointer_mut("/state/characters")
        .and_then(Value::as_array_mut)
        .and_then(|characters| characters.get_mut(character_index))
        .and_then(Value::as_object_mut)
        .ok_or_else(|| format!("Character {} must be an object", character_index + 1))?;
    let inventory = character
        .get_mut("inventory")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| {
            format!(
                "Character {} inventory must be an array",
                character_index + 1
            )
        })?;
    inventory.retain(|value| {
        let scope = value
            .get("definition_hash")
            .and_then(|value| {
                value
                    .as_u64()
                    .or_else(|| value.as_str().and_then(parse_hash_hex))
            })
            .and_then(|hash| catalog.item(hash))
            .map(|item| loadout_scope_for_bucket(item.bucket_hash))
            .unwrap_or(LoadoutScope::EquipmentFlair);
        !options.includes(scope)
    });
    Ok(())
}

fn loadout_scope_for_bucket(bucket_hash: u64) -> LoadoutScope {
    SLOTS
        .iter()
        .find_map(|(slot, _, bucket)| {
            (*bucket == bucket_hash).then(|| loadout_scope_for_slot(slot))
        })
        .unwrap_or(LoadoutScope::EquipmentFlair)
}

fn loadout_scope_for_slot(slot: &str) -> LoadoutScope {
    if WEAPON_SLOTS.contains(&slot) {
        LoadoutScope::Weapons
    } else if ARMOR_SLOTS.contains(&slot) {
        LoadoutScope::Armor
    } else if slot == SUBCLASS_SLOT {
        LoadoutScope::Subclass
    } else {
        LoadoutScope::EquipmentFlair
    }
}

#[allow(clippy::too_many_arguments)]
fn install_random_equipped(
    document: &mut Value,
    catalog: &Catalog,
    character_index: usize,
    slot: &str,
    item: &ItemDef,
    plug_mode: PlugSelectionMode,
    rng: &mut Rng,
) -> Result<(), String> {
    equip_definition(
        document,
        character_index,
        slot,
        item.hash,
        &item.default_plugs,
    )?;
    for socket_index in 0..item.sockets.len().min(inventory::MAX_ITEM_PLUGS) {
        if let Some(hash) = random_plug(catalog, item, socket_index, plug_mode, rng) {
            set_equipment_item_plug(
                document,
                character_index,
                slot,
                socket_index,
                &item.default_plugs,
                Some(hash),
            )?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn install_random_held(
    document: &mut Value,
    catalog: &Catalog,
    character_index: usize,
    item: &ItemDef,
    plug_mode: PlugSelectionMode,
    rng: &mut Rng,
) -> Result<(), String> {
    let definition_hash = u32::try_from(item.hash)
        .map_err(|_| format!("{} has an invalid definition hash", item.name))?;
    let item_level = capped_inventory_item_level(catalog, item)?;
    let location = inventory::add_inventory_item(
        document,
        character_index,
        inventory::NewInventoryItem::single(definition_hash, item_level),
    )
    .map_err(|error| error.to_string())?;
    let mut plugs = default_candidate(item)?
        .plugs
        .into_iter()
        .map(|hash| {
            hash.map(|hash| {
                u32::try_from(hash)
                    .map_err(|_| format!("{} has an invalid default plug", item.name))
            })
            .transpose()
        })
        .collect::<Result<Vec<_>, _>>()?;
    for (socket_index, plug) in plugs
        .iter_mut()
        .enumerate()
        .take(item.sockets.len().min(inventory::MAX_ITEM_PLUGS))
    {
        if let Some(hash) = random_plug(catalog, item, socket_index, plug_mode, rng) {
            *plug = Some(
                u32::try_from(hash)
                    .map_err(|_| format!("{} generated an invalid plug", item.name))?,
            );
        }
    }
    inventory::apply_inventory_item_action(
        document,
        location,
        inventory::InventoryItemAction::SetPlugs(inventory::ItemPlugs::Authored(plugs)),
    )
    .map_err(|error| error.to_string())
}

fn capped_inventory_item_level(catalog: &Catalog, item: &ItemDef) -> Result<i32, String> {
    let native_bucket_id = catalog
        .inventory_metadata(item.hash)
        .map(|metadata| metadata.native_bucket_id)
        .ok_or_else(|| format!("{} has no inventory placement metadata", item.name))?;
    i32::try_from(item_editor::new_inventory_item_level(
        native_bucket_id,
        catalog.item_power_cap(item.hash),
    ))
    .map_err(|_| format!("{} has an invalid maximum item level", item.name))
}

fn is_exotic(catalog: &Catalog, item: &ItemDef) -> bool {
    item.sockets.iter().any(|socket| {
        EXOTIC_TRAIT_SOCKET_TYPES.contains(&socket.socket_type)
            || catalog.socket_options(socket).iter().any(|hash| {
                catalog.names.get(hash).is_some_and(|name| {
                    name == "Empty Catalyst Socket" || name.ends_with(" Catalyst")
                })
            })
    })
}

fn pick_avoiding<'a>(
    rng: &mut Rng,
    candidates: &[&'a ItemDef],
    used_hashes: &[u64],
) -> Option<&'a ItemDef> {
    let unused = candidates
        .iter()
        .copied()
        .filter(|item| !used_hashes.contains(&item.hash))
        .collect::<Vec<_>>();
    rng.pick(&unused)
        .copied()
        .or_else(|| rng.pick(candidates).copied())
}

fn matching_item_instances(
    document: &Value,
    character_index: usize,
    matching_hashes: &HashSet<u64>,
) -> Vec<ItemInstanceChoice> {
    let mut choices = Vec::new();
    if let Ok(equipped) = equipped_item_snapshots(document, character_index) {
        choices.extend(equipped.into_iter().filter_map(|snapshot| {
            let item_hash = snapshot.definition_hash?;
            if !matching_hashes.contains(&item_hash) {
                return None;
            }
            Some(ItemInstanceChoice {
                request: ItemBuilderRequest {
                    item_hash,
                    authored_plugs: Some(equipped_plugs_value(&snapshot.plugs)?),
                },
                location: format!("Equipped · {}", snapshot.slot_label),
            })
        }));
    }
    if let Ok(Some(stored)) = inventory::character_inventory(document, character_index) {
        choices.extend(stored.into_iter().filter_map(|snapshot| {
            let item_hash = u64::from(snapshot.definition_hash);
            matching_hashes
                .contains(&item_hash)
                .then(|| ItemInstanceChoice {
                    request: ItemBuilderRequest {
                        item_hash,
                        authored_plugs: Some(inventory_plugs_value(&snapshot.plugs)),
                    },
                    location: format!("Inventory · item {}", snapshot.location.item_index + 1),
                })
        }));
    }
    choices
}

fn equipped_plugs_value(plugs: &EquippedItemPlugs) -> Option<Value> {
    match plugs {
        EquippedItemPlugs::NativeDefaults => Some(Value::Null),
        EquippedItemPlugs::Authored(plugs) => plugs
            .iter()
            .map(|plug| match plug {
                EquippedPlugValue::Empty => Some(Value::Null),
                EquippedPlugValue::Hash(hash) if valid_definition_hash(*hash) => {
                    Some(Value::from(*hash))
                }
                EquippedPlugValue::Hash(_) | EquippedPlugValue::Malformed(_) => None,
            })
            .collect::<Option<Vec<_>>>()
            .map(Value::Array),
        EquippedItemPlugs::Missing | EquippedItemPlugs::Malformed(_) => None,
    }
}

fn matching_items<'a>(
    catalog: &'a Catalog,
    class_type: u64,
    show_dummy_items: bool,
    query: &str,
    family: ItemFamily,
    filter: &item_editor::ItemFilter,
) -> Vec<&'a ItemDef> {
    if query.is_empty() || class_type > 2 {
        return Vec::new();
    }
    let mut items = family
        .slots()
        .iter()
        .copied()
        .filter_map(slot_definition)
        .flat_map(|(_, _, bucket)| catalog.search(query, bucket, class_type, show_dummy_items))
        .filter(|item| item_can_be_authored(item))
        .filter(|item| filter.matches(catalog, item))
        .collect::<Vec<_>>();
    items.sort_by_cached_key(|item| (item.name.to_ascii_lowercase(), item.hash));
    items.dedup_by_key(|item| item.hash);
    items
}

fn random_item_candidates(
    catalog: &Catalog,
    class_type: u64,
    show_dummy_items: bool,
    slots: impl Iterator<Item = &'static str>,
) -> Vec<&ItemDef> {
    if class_type > 2 {
        return Vec::new();
    }
    let mut items = slots
        .filter_map(slot_definition)
        .flat_map(|(_, _, bucket)| catalog.browse(bucket, class_type, show_dummy_items))
        .filter(|item| item_can_be_authored(item))
        .collect::<Vec<_>>();
    items.sort_by_key(|item| item.hash);
    items.dedup_by_key(|item| item.hash);
    items
}

#[cfg(test)]
fn random_slots() -> impl Iterator<Item = &'static str> {
    WEAPON_SLOTS.iter().chain(ARMOR_SLOTS).copied()
}

fn slot_definition(slot: &str) -> Option<(&'static str, &'static str, u64)> {
    SLOTS
        .iter()
        .find(|(known_slot, _, _)| *known_slot == slot)
        .copied()
}

fn slot_for_bucket(bucket_hash: u64) -> Option<(&'static str, &'static str)> {
    SLOTS
        .iter()
        .filter(|(slot, _, _)| WEAPON_SLOTS.contains(slot) || ARMOR_SLOTS.contains(slot))
        .find_map(|(slot, label, bucket)| (*bucket == bucket_hash).then_some((*slot, *label)))
}

fn character_class(document: &Value, character_index: usize) -> u64 {
    document
        .pointer("/state/characters")
        .and_then(Value::as_array)
        .and_then(|characters| characters.get(character_index))
        .and_then(|character| character.get("class"))
        .and_then(Value::as_u64)
        .unwrap_or(99)
}

fn item_can_be_authored(item: &ItemDef) -> bool {
    valid_definition_hash(item.hash)
        && item.default_plugs.len() <= inventory::MAX_ITEM_PLUGS
        && item.default_plugs.iter().all(|plug| {
            plug.as_deref()
                .is_none_or(|text| parse_hash_hex(text).is_some_and(valid_definition_hash))
        })
}

fn valid_definition_hash(hash: u64) -> bool {
    hash != NO_DEFINITION_HASH && u32::try_from(hash).is_ok()
}

/// A dependency-free xorshift generator. UI variety does not require a
/// cryptographic random source.
struct Rng(u64);

impl Rng {
    fn from_clock() -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| {
                since
                    .as_secs()
                    .wrapping_mul(1_000_000_000)
                    .wrapping_add(u64::from(since.subsec_nanos()))
            });
        Self::from_seed(seed)
    }

    fn from_seed(seed: u64) -> Self {
        Self(seed | 1)
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn pick<'a, T>(&mut self, options: &'a [T]) -> Option<&'a T> {
        let count = u64::try_from(options.len())
            .ok()
            .filter(|count| *count > 0)?;
        let index = usize::try_from(self.next() % count).ok()?;
        options.get(index)
    }

    fn pick_valid_hash(&mut self, options: &[u64]) -> Option<u64> {
        let count = options.len();
        let count_u64 = u64::try_from(count).ok().filter(|count| *count > 0)?;
        let start = usize::try_from(self.next() % count_u64).ok()?;
        (0..count).find_map(|offset| {
            let hash = options[(start + offset) % count];
            valid_definition_hash(hash).then_some(hash)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn randomizer_covers_only_weapon_and_armor_slots() {
        let slots = random_slots().collect::<Vec<_>>();
        assert_eq!(slots.len(), 8);
        assert!(slots.contains(&"kinetic"));
        assert!(slots.contains(&"class_item"));
        assert!(!slots.contains(&"ghost"));
    }

    #[test]
    fn seeded_random_choices_are_repeatable() {
        let mut first = Rng::from_seed(42);
        let mut second = Rng::from_seed(42);
        let options = [3, 5, 7, 11];
        assert_eq!(first.pick(&options), second.pick(&options));
        assert_eq!(first.pick(&options), second.pick(&options));
    }

    #[test]
    fn hash_choices_skip_the_engine_empty_marker() {
        let mut rng = Rng::from_seed(7);
        assert_eq!(rng.pick_valid_hash(&[NO_DEFINITION_HASH, 7]), Some(7));
        assert_eq!(rng.pick_valid_hash(&[NO_DEFINITION_HASH]), None);
    }

    #[test]
    fn loadout_inventory_plan_matches_panoptes_distribution() {
        let options = LoadoutOptions::default();
        let equipment_slots = SLOTS
            .iter()
            .filter(|(slot, _, _)| options.includes(loadout_scope_for_slot(slot)))
            .count();
        let held_items = SLOTS
            .iter()
            .filter(|(slot, _, _)| {
                options.includes(loadout_scope_for_slot(slot))
                    && !matches!(*slot, SUBCLASS_SLOT | CLAN_BANNER_SLOT)
            })
            .count()
            * HELD_ITEMS_PER_SLOT;
        assert_eq!(equipment_slots, 16);
        assert_eq!(held_items, 126);
        assert!(held_items <= inventory::CHARACTER_INVENTORY_CAPACITY);
    }

    #[test]
    fn loadout_scopes_default_on_and_partition_every_slot() {
        let options = LoadoutOptions::default();
        assert!(options.weapons && options.armor && options.equipment_flair && options.subclass);
        assert_eq!(loadout_scope_for_slot("kinetic"), LoadoutScope::Weapons);
        assert_eq!(loadout_scope_for_slot("helmet"), LoadoutScope::Armor);
        assert_eq!(loadout_scope_for_slot("subclass"), LoadoutScope::Subclass);
        assert_eq!(loadout_scope_for_slot("ship"), LoadoutScope::EquipmentFlair);
        assert!(
            SLOTS
                .iter()
                .all(|(slot, _, _)| options.includes(loadout_scope_for_slot(slot)))
        );
    }

    #[test]
    fn unchecked_loadout_scopes_are_excluded_without_affecting_the_others() {
        let options = LoadoutOptions {
            weapons: false,
            armor: true,
            equipment_flair: false,
            subclass: true,
        };
        let selected = SLOTS
            .iter()
            .filter(|(slot, _, _)| options.includes(loadout_scope_for_slot(slot)))
            .map(|(slot, _, _)| *slot)
            .collect::<Vec<_>>();
        assert_eq!(selected.len(), ARMOR_SLOTS.len() + 1);
        assert!(selected.contains(&"helmet"));
        assert!(selected.contains(&SUBCLASS_SLOT));
        assert!(!selected.contains(&"kinetic"));
        assert!(!selected.contains(&"ship"));
    }

    #[test]
    fn loadout_confirmation_uses_the_visible_safety_labels() {
        assert_eq!(PlugSelectionMode::Supported.label(), "Compatible");
        assert_eq!(PlugSelectionMode::MatchingSocketType.label(), "Socket type");
        assert_eq!(PlugSelectionMode::GearType.label(), "Gear type");
        assert_eq!(PlugSelectionMode::AnyPlug.label(), "All");
    }

    #[test]
    fn random_item_add_respects_native_bucket_capacity() {
        assert!(bucket_has_room_for_add(9, 0, 10));
        assert!(!bucket_has_room_for_add(10, 0, 10));
        assert!(!bucket_has_room_for_add(9, 1, 10));
        assert!(bucket_has_room_for_add(8, 1, 10));
    }

    #[test]
    fn random_item_search_preserves_equipped_and_inventory_rolls() {
        let document = serde_json::json!({
            "version": 6,
            "state": {
                "characters": [{
                    "equipment": {
                        "kinetic": {
                            "instance_soid": 1,
                            "definition_hash": 11,
                            "level": 100,
                            "quantity": 1,
                            "plugs": [101, null, 103]
                        }
                    },
                    "inventory": [{
                        "instance_soid": 2,
                        "definition_hash": 22,
                        "level": 100,
                        "quantity": 1,
                        "plugs": [201, 202]
                    }]
                }]
            }
        });
        let matching_hashes = HashSet::from([11, 22]);

        let choices = matching_item_instances(&document, 0, &matching_hashes);

        assert_eq!(choices.len(), 2);
        assert_eq!(choices[0].request.item_hash, 11);
        assert_eq!(
            choices[0].request.authored_plugs,
            Some(serde_json::json!([101, null, 103]))
        );
        assert_eq!(choices[0].location, "Equipped · Kinetic");
        assert_eq!(choices[1].request.item_hash, 22);
        assert_eq!(
            choices[1].request.authored_plugs,
            Some(serde_json::json!([201, 202]))
        );
        assert_eq!(choices[1].location, "Inventory · item 1");
    }
}

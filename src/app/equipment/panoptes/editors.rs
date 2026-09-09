//! Compact equipped and stored-item editors owned by the Panoptes presentation.

use eframe::egui;
use serde_json::Value;
use sundial_account::NO_DEFINITION_HASH;

use crate::{
    app::{
        ARMOR_SLOTS, ITEM_PICKER_MAX_HEIGHT, ITEM_PICKER_MIN_HEIGHT, PLUG_PICKER_MAX_HEIGHT,
        PLUG_PICKER_MIN_HEIGHT, PlugSelectionMode, SundialApp, WEAPON_SLOTS,
        inspector::DefinitionInspectionContext,
        inventory::{self, InventoryItemAction, InventoryItemSnapshot, ItemPlugs},
        inventory_page::{
            CharacterInventoryEditorContext, InventoryItemUiId, displayed_inventory_plugs,
            inventory_item_state_key,
        },
        item_editor::{
            self, ClearDefinitionChoice, DefinitionPickerChoices, ItemEditorAction,
            NumericItemFields, PickerHeight,
        },
    },
    hash::{format_hash_hex, parse_unsigned_value},
};

use super::{layout, widgets};
use crate::app::equipment::{
    EquippedItemPlugs, EquippedItemSnapshot, EquippedPlugValue, displayed_plugs,
    equipment_definition_choices, native_plug_default,
};

pub(super) struct EquippedEditor<'a> {
    pub(super) character_index: usize,
    pub(super) slot: &'static str,
    pub(super) label: &'a str,
    pub(super) bucket_hash: u64,
    pub(super) class_type: u64,
    pub(super) editable: bool,
    pub(super) snapshot: Option<&'a EquippedItemSnapshot>,
    pub(super) group_sockets: bool,
}

pub(super) struct StoredEditor<'a> {
    pub(super) snapshot: &'a InventoryItemSnapshot,
    pub(super) ui_identity: InventoryItemUiId,
    pub(super) context: &'a CharacterInventoryEditorContext,
    pub(super) heading: &'a str,
    pub(super) slot: &'static str,
    pub(super) bucket_hash: u64,
    pub(super) group_sockets: bool,
}

fn panoptes_socket_is_visible(
    mode: PlugSelectionMode,
    current_hash: Option<u64>,
    is_mod_socket: bool,
) -> bool {
    current_hash.is_some_and(|hash| hash != u64::from(NO_DEFINITION_HASH.get()))
        || is_mod_socket
        || matches!(
            mode,
            PlugSelectionMode::GearType | PlugSelectionMode::AnyPlug
        )
}

impl SundialApp {
    pub(super) fn draw_panoptes_equipped_editor(
        &mut self,
        ui: &mut egui::Ui,
        editor: EquippedEditor<'_>,
    ) {
        let EquippedEditor {
            character_index,
            slot,
            label,
            bucket_hash,
            class_type,
            editable,
            snapshot,
            group_sockets,
        } = editor;
        let current_hash = snapshot.and_then(|item| item.definition_hash);
        let is_empty = snapshot.is_none();
        let current_level = snapshot.and_then(|item| item.level);
        let current_hash_display_text =
            snapshot.map_or_else(|| "<empty>".to_owned(), |item| item.definition_text.clone());
        let authored_plugs = snapshot.and_then(|item| match &item.plugs {
            EquippedItemPlugs::NativeDefaults => Some(Value::Null),
            EquippedItemPlugs::Authored(plugs) => Some(Value::Array(
                plugs
                    .iter()
                    .map(|plug| match plug {
                        EquippedPlugValue::Empty => Value::Null,
                        EquippedPlugValue::Hash(hash) => Value::from(*hash),
                        EquippedPlugValue::Malformed(value) => Value::String(value.clone()),
                    })
                    .collect(),
            )),
            EquippedItemPlugs::Missing | EquippedItemPlugs::Malformed(_) => None,
        });
        let current =
            current_hash.and_then(|hash| self.manifest.item_handle_for_bucket(hash, bucket_hash));
        let definition_valid = is_empty
            || current
                .as_ref()
                .is_some_and(|item| item.class_type == 3 || item.class_type == class_type);
        let snapshot_valid = snapshot.is_none_or(|snapshot| snapshot.issues.is_empty());
        let guided_editable = editable && snapshot_valid;
        let valid = definition_valid && snapshot_valid;
        let (title, type_name) = if is_empty {
            ("Empty", None)
        } else if let Some(item) = current.as_ref() {
            (item.name.as_str(), Some(item.type_name.as_str()))
        } else {
            ("Unknown item", None)
        };
        let hash_display_text = (!is_empty).then_some(current_hash_display_text.as_str());
        let armor_generation = current.as_ref().and_then(|item| {
            ARMOR_SLOTS.contains(&slot).then_some(
                if item.sockets.iter().any(|socket| socket.socket_type == 643) {
                    "Armor 2.0"
                } else {
                    "Armor 1.0"
                },
            )
        });
        let default_plugs_equipped = current.as_ref().is_some_and(|item| {
            let (plugs, native_defaults) =
                displayed_plugs(authored_plugs.as_ref(), &item.default_plugs);
            native_defaults && (!item.sockets.is_empty() || !plugs.is_empty())
        });
        let mut level_change = None;
        let mut flags_change = None;

        ui.push_id(("panoptes-equipped-editor", character_index, slot), |ui| {
            let header_response = widgets::draw_compact_item_header(
                ui,
                &self.manifest,
                widgets::CompactItemHeader {
                    heading: label,
                    title,
                    type_name,
                    armor_generation,
                    hash: current_hash,
                    inspection_context: DefinitionInspectionContext {
                        source: format!("Character {} Equipment · {label}", character_index + 1),
                        instance_id: snapshot.and_then(|item| {
                            item.instance_soid.map(|_| item.instance_soid_text.clone())
                        }),
                        authored_level: current_level,
                        flags: snapshot.and_then(|item| item.flags),
                        quantity: snapshot.and_then(|item| item.quantity),
                        plug_count: authored_plugs
                            .as_ref()
                            .and_then(Value::as_array)
                            .map(Vec::len),
                        plugs: authored_plugs.clone(),
                    },
                    hash_display_text,
                    default_plugs_equipped,
                    valid,
                    invalid_message: if definition_valid {
                        "invalid equipped item"
                    } else {
                        "invalid for slot/class"
                    },
                },
                |ui| {
                    if !is_empty {
                        ui.add_enabled_ui(guided_editable, |ui| {
                            if let Some(level) = current_level {
                                ui.horizontal(|ui| {
                                    for action in item_editor::draw_level_and_quantity(
                                        ui,
                                        ("panoptes-equipment-numeric", character_index, slot),
                                        NumericItemFields {
                                            level: Some(level),
                                            power_max: current_hash.and_then(|hash| {
                                                self.manifest.item_power_cap(hash)
                                            }),
                                            allow_power_above_cap: self
                                                .preferences
                                                .experimental_power_above_cap,
                                            quantity: None,
                                            quantity_max: None,
                                        },
                                    ) {
                                        if let ItemEditorAction::SetLevel { level } = action {
                                            level_change = Some(level);
                                        }
                                    }
                                });
                            } else {
                                ui.horizontal(|ui| {
                                    ui.label("Power");
                                    ui.label(egui::RichText::new("<invalid or missing>").weak());
                                });
                            }
                        });
                    }
                },
                |ui| {
                    ui.add_enabled_ui(guided_editable && !is_empty, |ui| {
                        flags_change = item_editor::draw_masterwork_flag(
                            ui,
                            snapshot.and_then(|item| item.flags),
                            self.document.supports_v13_account(),
                        );
                    });
                },
            );

            if let Some(snapshot) = snapshot
                && !snapshot.issues.is_empty()
            {
                ui.colored_label(ui.visuals().error_fg_color, snapshot.issues.join(" · "))
                    .on_hover_text(format!("Authored item: {}", snapshot.raw_item_text));
            }

            let picker_key = format!("panoptes-equipment:{character_index}:{slot}");
            let picker_action = {
                let manifest = &self.manifest;
                let show_dummy_items = self.show_dummy_items;
                let query = self.searches.entry(picker_key.clone()).or_default();
                ui.add_enabled_ui(guided_editable, |ui| {
                    item_editor::draw_definition_picker_with_open_request(
                        ui,
                        manifest,
                        ("panoptes-equipment-definition", character_index, slot),
                        query,
                        PickerHeight {
                            min: ITEM_PICKER_MIN_HEIGHT,
                            max: ITEM_PICKER_MAX_HEIGHT,
                        },
                        (Some(&header_response), false),
                        |query_value| {
                            let candidates = if query_value.trim().is_empty() {
                                manifest.browse(
                                    bucket_hash,
                                    class_type,
                                    show_dummy_items,
                                    self.preferences.experimental_cross_class_subclasses,
                                )
                            } else {
                                manifest.search(
                                    query_value,
                                    bucket_hash,
                                    class_type,
                                    show_dummy_items,
                                    self.preferences.experimental_cross_class_subclasses,
                                )
                            };
                            let needle = query_value.to_lowercase();
                            DefinitionPickerChoices {
                                definitions: equipment_definition_choices(
                                    candidates,
                                    self.document.supports_v13_account(),
                                ),
                                existing_inventory: Vec::new(),
                                clear: (WEAPON_SLOTS.contains(&slot)
                                    && (query_value.trim().is_empty()
                                        || "empty weapon".contains(&needle)))
                                .then(|| ClearDefinitionChoice {
                                    label: "Empty weapon".to_owned(),
                                    tooltip: "Sets this equipment slot to empty.".to_owned(),
                                    selected: is_empty,
                                }),
                                random_item_builder_hash: current_hash.filter(|_| {
                                    WEAPON_SLOTS.contains(&slot) || ARMOR_SLOTS.contains(&slot)
                                }),
                                empty_message: "No compatible installed items found".to_owned(),
                            }
                        },
                    )
                })
                .inner
            };

            if let Some(item) = current.as_ref() {
                self.draw_panoptes_equipped_sockets(
                    ui,
                    character_index,
                    slot,
                    item,
                    authored_plugs.as_ref(),
                    guided_editable,
                    group_sockets,
                );
            }

            if let Some(flags) = flags_change {
                self.select_equipment_flags(character_index, slot, flags);
            }
            if let Some(level) = level_change {
                self.select_equipment_level(character_index, slot, level);
            }
            match picker_action {
                Some(ItemEditorAction::ClearDefinition) => {
                    self.empty_weapon(character_index, slot);
                    self.searches.insert(picker_key, String::new());
                }
                Some(ItemEditorAction::SetDefinition { hash }) => {
                    if let Some(item) = self.manifest.item_handle_for_bucket(hash, bucket_hash) {
                        self.select_item(character_index, slot, &item);
                        self.searches.insert(picker_key, String::new());
                    }
                }
                Some(ItemEditorAction::OpenInRandomItemBuilder { hash }) => {
                    super::super::randomize::request_item_builder(
                        ui.ctx(),
                        character_index,
                        hash,
                        authored_plugs.clone(),
                    );
                }
                _ => {}
            }
        });
    }

    pub(super) fn draw_panoptes_stored_editor(
        &mut self,
        ui: &mut egui::Ui,
        editor: StoredEditor<'_>,
    ) -> bool {
        let StoredEditor {
            snapshot,
            ui_identity,
            context,
            heading,
            slot,
            bucket_hash,
            group_sockets,
        } = editor;
        let hash = u64::from(snapshot.definition_hash);
        let hash_display_text = format_hash_hex(hash);
        let current = self.manifest.item_handle_for_bucket(hash, bucket_hash);
        let valid = current
            .as_ref()
            .is_some_and(|item| item.class_type == 3 || item.class_type == context.class_type());
        let (title, type_name) = current.as_ref().map_or(("Unknown item", None), |item| {
            (item.name.as_str(), Some(item.type_name.as_str()))
        });
        let armor_generation = current.as_ref().and_then(|item| {
            ARMOR_SLOTS.contains(&slot).then_some(
                if item.sockets.iter().any(|socket| socket.socket_type == 643) {
                    "Armor 2.0"
                } else {
                    "Armor 1.0"
                },
            )
        });
        let default_plugs_equipped = current.as_ref().is_some_and(|item| {
            let (plugs, native_defaults) = displayed_inventory_plugs(snapshot, item);
            native_defaults && (!item.sockets.is_empty() || !plugs.is_empty())
        });
        let editable = context.editable();
        let mut requested = Vec::new();
        let mut equip_requested = false;
        let mut flags_change = None;

        ui.push_id(("panoptes-stored-editor", ui_identity), |ui| {
            let header_response = widgets::draw_compact_item_header(
                ui,
                &self.manifest,
                widgets::CompactItemHeader {
                    heading,
                    title,
                    type_name,
                    armor_generation,
                    hash: Some(hash),
                    inspection_context: DefinitionInspectionContext {
                        source: format!(
                            "Character {} Inventory · Item {}",
                            snapshot.location.character_index + 1,
                            snapshot.location.item_index + 1
                        ),
                        instance_id: Some(format!("0x{:016X}", snapshot.instance_soid)),
                        authored_level: Some(i64::from(snapshot.level)),
                        flags: snapshot.flags,
                        quantity: Some(i64::from(snapshot.quantity)),
                        plug_count: match &snapshot.plugs {
                            ItemPlugs::NativeDefaults => None,
                            ItemPlugs::Authored(plugs) => Some(plugs.len()),
                        },
                        plugs: Some(match &snapshot.plugs {
                            ItemPlugs::NativeDefaults => Value::Null,
                            ItemPlugs::Authored(plugs) => serde_json::json!(plugs),
                        }),
                    },
                    hash_display_text: Some(&hash_display_text),
                    default_plugs_equipped,
                    valid,
                    invalid_message: "invalid for slot/class",
                },
                |ui| {
                    ui.add_enabled_ui(editable, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            for action in item_editor::draw_level_and_quantity(
                                ui,
                                ("panoptes-inventory-numeric", ui_identity),
                                NumericItemFields {
                                    level: Some(i64::from(snapshot.level)),
                                    power_max: self.manifest.item_power_cap(hash),
                                    allow_power_above_cap: self
                                        .preferences
                                        .experimental_power_above_cap,
                                    quantity: None,
                                    quantity_max: None,
                                },
                            ) {
                                if let ItemEditorAction::SetLevel { level } = action
                                    && let Ok(level) = i32::try_from(level)
                                {
                                    requested.push(InventoryItemAction::SetLevel(level));
                                }
                            }
                        });
                    });
                    let equip_enabled = editable && snapshot.quantity == 1;
                    let equip = ui
                        .add_enabled(equip_enabled, egui::Button::new("Equip"))
                        .on_hover_text("Swap this stored item with the equipped item")
                        .on_disabled_hover_text(if editable {
                            "Only one editable stored item can be equipped"
                        } else {
                            "Character inventory is read-only"
                        });
                    equip_requested = equip.clicked();
                },
                |ui| {
                    ui.add_enabled_ui(editable, |ui| {
                        flags_change = item_editor::draw_masterwork_flag(
                            ui,
                            snapshot.flags,
                            self.document.supports_v13_account(),
                        );
                    });
                },
            );
            if let Some(flags) = flags_change {
                requested.push(InventoryItemAction::SetFlags(flags));
            }

            let picker_key = inventory_item_state_key(ui_identity);
            let picker_action = {
                let manifest = &self.manifest;
                let show_dummy_items = self.show_dummy_items;
                let query = self.searches.entry(picker_key.clone()).or_default();
                ui.add_enabled_ui(editable, |ui| {
                    item_editor::draw_definition_picker_with_open_request(
                        ui,
                        manifest,
                        ("panoptes-inventory-definition", ui_identity),
                        query,
                        PickerHeight {
                            min: ITEM_PICKER_MIN_HEIGHT,
                            max: ITEM_PICKER_MAX_HEIGHT,
                        },
                        (Some(&header_response), false),
                        |query_value| {
                            let candidates = if query_value.trim().is_empty() {
                                manifest.browse(
                                    bucket_hash,
                                    context.class_type(),
                                    show_dummy_items,
                                    self.preferences.experimental_cross_class_subclasses,
                                )
                            } else {
                                manifest.search(
                                    query_value,
                                    bucket_hash,
                                    context.class_type(),
                                    show_dummy_items,
                                    self.preferences.experimental_cross_class_subclasses,
                                )
                            };
                            DefinitionPickerChoices {
                                definitions: equipment_definition_choices(
                                    candidates,
                                    self.document.supports_v13_account(),
                                ),
                                existing_inventory: Vec::new(),
                                clear: None,
                                random_item_builder_hash: (WEAPON_SLOTS.contains(&slot)
                                    || ARMOR_SLOTS.contains(&slot))
                                .then_some(hash),
                                empty_message: "No compatible installed items found".to_owned(),
                            }
                        },
                    )
                })
                .inner
            };
            if let Some(ItemEditorAction::OpenInRandomItemBuilder { hash }) = picker_action {
                super::super::randomize::request_inventory_item_builder(
                    ui.ctx(),
                    snapshot.location.character_index,
                    hash,
                    &snapshot.plugs,
                );
            }
            if let Some(ItemEditorAction::SetDefinition { hash }) = picker_action
                && let Ok(hash) = u32::try_from(hash)
            {
                requested.push(InventoryItemAction::SetDefinitionHash(hash));
                requested.push(InventoryItemAction::SetPlugs(ItemPlugs::NativeDefaults));
                if let Some(maximum) = self
                    .manifest
                    .inventory_metadata(u64::from(hash))
                    .and_then(|metadata| metadata.max_stack_size)
                    .map(|maximum| maximum.min(i32::MAX as u32) as i32)
                    && snapshot.quantity > maximum
                {
                    requested.push(InventoryItemAction::SetQuantity(maximum.max(1)));
                }
                self.searches.insert(picker_key, String::new());
            }

            if let Some(item) = current.as_ref() {
                self.draw_panoptes_stored_sockets(
                    ui,
                    snapshot,
                    ui_identity,
                    slot,
                    item,
                    editable,
                    group_sockets,
                    &mut requested,
                );
            }
        });

        if !requested.is_empty() {
            self.apply_character_inventory_item_actions(snapshot, ui_identity, requested);
        }
        equip_requested
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_panoptes_equipped_sockets(
        &mut self,
        ui: &mut egui::Ui,
        character_index: usize,
        slot: &'static str,
        item: &crate::catalog::ItemDef,
        authored_plugs: Option<&Value>,
        editable: bool,
        group_sockets: bool,
    ) {
        let (current_plugs, _) = displayed_plugs(authored_plugs, &item.default_plugs);
        let socket_count = item.sockets.len().max(current_plugs.len());
        if socket_count == 0 {
            return;
        }
        let socket_lines = layout::socket_lines(
            &self.manifest,
            item,
            slot,
            socket_count,
            group_sockets,
            |socket_index| {
                panoptes_socket_is_visible(
                    self.plug_selection_mode,
                    current_plugs
                        .get(socket_index)
                        .and_then(parse_unsigned_value),
                    layout::is_mod_socket(item, socket_index),
                )
            },
        );
        if socket_lines.is_empty() {
            return;
        }
        ui.add_space(4.0);
        widgets::draw_socket_rows(ui, socket_lines, |ui, socket_index| {
            let current_hash = current_plugs
                .get(socket_index)
                .and_then(parse_unsigned_value);
            let native_default = native_plug_default(&item.default_plugs, socket_index);
            let current_label = current_hash.map_or_else(
                || "None".to_owned(),
                |hash| {
                    self.manifest
                        .plug_label(hash, self.preferences.show_plug_hashes)
                },
            );
            let picker_snapshot = item_editor::plug_picker_snapshot(
                &self.manifest,
                item,
                socket_index,
                current_hash,
                current_label.clone(),
                native_default,
                self.plug_selection_mode,
            );
            let query_key =
                format!("panoptes-equipment:{character_index}:{slot}:plug:{socket_index}");
            let mut query = self
                .plug_searches
                .get(&query_key)
                .cloned()
                .unwrap_or_default();
            let searchable = picker_snapshot.choices.len() > 12;
            let action = ui
                .add_enabled_ui(editable, |ui| {
                    let button = widgets::draw_socket_button(
                        ui,
                        &self.manifest,
                        current_hash,
                        layout::is_mod_socket(item, socket_index),
                        &format!("{}\n{}", picker_snapshot.socket_label, current_label),
                    );
                    item_editor::draw_plug_icon_picker(
                        ui,
                        &self.manifest,
                        (
                            "panoptes-equipment-plug",
                            character_index,
                            slot,
                            socket_index,
                        ),
                        &mut query,
                        &picker_snapshot,
                        PickerHeight {
                            min: PLUG_PICKER_MIN_HEIGHT,
                            max: PLUG_PICKER_MAX_HEIGHT,
                        },
                        &button,
                    )
                })
                .inner;
            if let Some(ItemEditorAction::SetPlug { socket_index, hash }) = action {
                self.select_plug(
                    character_index,
                    slot,
                    socket_index,
                    &picker_snapshot.socket_label,
                    &item.default_plugs,
                    hash,
                );
            }
            if searchable {
                self.plug_searches.insert(query_key, query);
            } else {
                self.plug_searches.remove(&query_key);
            }
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_panoptes_stored_sockets(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &InventoryItemSnapshot,
        ui_identity: InventoryItemUiId,
        slot: &'static str,
        item: &crate::catalog::ItemDef,
        editable: bool,
        group_sockets: bool,
        requested: &mut Vec<InventoryItemAction>,
    ) {
        let (current_plugs, _) = displayed_inventory_plugs(snapshot, item);
        let socket_count = item
            .sockets
            .len()
            .max(current_plugs.len())
            .min(inventory::MAX_ITEM_PLUGS);
        if socket_count == 0 {
            return;
        }
        let socket_lines = layout::socket_lines(
            &self.manifest,
            item,
            slot,
            socket_count,
            group_sockets,
            |socket_index| {
                panoptes_socket_is_visible(
                    self.plug_selection_mode,
                    current_plugs
                        .get(socket_index)
                        .copied()
                        .flatten()
                        .map(u64::from),
                    layout::is_mod_socket(item, socket_index),
                )
            },
        );
        if socket_lines.is_empty() {
            return;
        }
        ui.add_space(4.0);
        widgets::draw_socket_rows(ui, socket_lines, |ui, socket_index| {
            let current_hash = current_plugs
                .get(socket_index)
                .copied()
                .flatten()
                .map(u64::from);
            let native_default = native_plug_default(&item.default_plugs, socket_index);
            let current_label = current_hash.map_or_else(
                || "None".to_owned(),
                |hash| {
                    self.manifest
                        .plug_label(hash, self.preferences.show_plug_hashes)
                },
            );
            let picker_snapshot = item_editor::plug_picker_snapshot(
                &self.manifest,
                item,
                socket_index,
                current_hash,
                current_label.clone(),
                native_default,
                self.plug_selection_mode,
            );
            let query_key = format!(
                "{}:plug:{socket_index}",
                inventory_item_state_key(ui_identity)
            );
            let mut query = self
                .plug_searches
                .get(&query_key)
                .cloned()
                .unwrap_or_default();
            let searchable = picker_snapshot.choices.len() > 12;
            let action = ui
                .add_enabled_ui(editable, |ui| {
                    let button = widgets::draw_socket_button(
                        ui,
                        &self.manifest,
                        current_hash,
                        layout::is_mod_socket(item, socket_index),
                        &format!("{}\n{}", picker_snapshot.socket_label, current_label),
                    );
                    item_editor::draw_plug_icon_picker(
                        ui,
                        &self.manifest,
                        ("panoptes-inventory-plug", ui_identity, socket_index),
                        &mut query,
                        &picker_snapshot,
                        PickerHeight {
                            min: PLUG_PICKER_MIN_HEIGHT,
                            max: PLUG_PICKER_MAX_HEIGHT,
                        },
                        &button,
                    )
                })
                .inner;
            if let Some(ItemEditorAction::SetPlug { socket_index, hash }) = action {
                let mut plugs = current_plugs.clone();
                while plugs.len() <= socket_index {
                    plugs.push(None);
                }
                plugs[socket_index] = hash.and_then(|hash| u32::try_from(hash).ok());
                requested.push(InventoryItemAction::SetPlugs(ItemPlugs::Authored(plugs)));
            }
            if searchable {
                self.plug_searches.insert(query_key, query);
            } else {
                self.plug_searches.remove(&query_key);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_panoptes_sockets_only_surface_for_broad_safety_modes() {
        assert!(!panoptes_socket_is_visible(
            PlugSelectionMode::Supported,
            None,
            false
        ));
        assert!(!panoptes_socket_is_visible(
            PlugSelectionMode::SocketAndGearType,
            None,
            false
        ));
        assert!(!panoptes_socket_is_visible(
            PlugSelectionMode::MatchingSocketType,
            None,
            false
        ));
        assert!(panoptes_socket_is_visible(
            PlugSelectionMode::GearType,
            None,
            false
        ));
        assert!(panoptes_socket_is_visible(
            PlugSelectionMode::AnyPlug,
            None,
            false
        ));
    }

    #[test]
    fn populated_and_explicitly_empty_panoptes_sockets_remain_visible() {
        assert!(panoptes_socket_is_visible(
            PlugSelectionMode::Supported,
            Some(123),
            false
        ));
        assert!(!panoptes_socket_is_visible(
            PlugSelectionMode::Supported,
            Some(u64::from(NO_DEFINITION_HASH.get())),
            false
        ));
        assert!(panoptes_socket_is_visible(
            PlugSelectionMode::Supported,
            None,
            true
        ));
    }
}

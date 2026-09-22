//! Socket presentation shared by equipped and stored Panoptes items.

use eframe::egui;
use serde_json::Value;
use sundial_account::NO_DEFINITION_HASH;

use super::{layout, widgets};
use crate::{
    app::{
        PLUG_PICKER_MAX_HEIGHT, PLUG_PICKER_MIN_HEIGHT, PlugSelectionMode, SundialApp,
        equipment::{displayed_plugs, native_plug_default},
        inventory::{self, InventoryItemAction, InventoryItemSnapshot},
        inventory_page::{InventoryItemUiId, displayed_inventory_plugs, inventory_item_state_key},
        item_editor::{self, ItemEditorAction, PickerHeight},
    },
    catalog::ItemDef,
    hash::parse_unsigned_value,
};

enum SocketLocation {
    Equipped {
        character_index: usize,
        slot: &'static str,
    },
    Stored(InventoryItemUiId),
}

impl SocketLocation {
    fn search_key(&self, socket_index: usize) -> String {
        match self {
            Self::Equipped {
                character_index,
                slot,
            } => format!("panoptes-equipment:{character_index}:{slot}:plug:{socket_index}"),
            Self::Stored(identity) => format!(
                "{}:plug:{socket_index}",
                inventory_item_state_key(*identity)
            ),
        }
    }
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
    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw_panoptes_equipped_sockets(
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
            let selection = ui
                .add_enabled_ui(editable, |ui| {
                    self.draw_panoptes_socket_picker(
                        ui,
                        item,
                        socket_index,
                        current_hash,
                        current_plugs
                            .iter()
                            .map(|value| {
                                parse_unsigned_value(value)
                                    .and_then(|hash| u32::try_from(hash).ok())
                            })
                            .collect(),
                        SocketLocation::Equipped {
                            character_index,
                            slot,
                        },
                    )
                })
                .inner;
            if let Some((socket_label, hash)) = selection {
                self.select_plug(
                    character_index,
                    slot,
                    socket_index,
                    &socket_label,
                    &item.default_plugs,
                    hash,
                );
            }
        });
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw_panoptes_stored_sockets(
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
            let selection = ui
                .add_enabled_ui(editable, |ui| {
                    self.draw_panoptes_socket_picker(
                        ui,
                        item,
                        socket_index,
                        current_hash,
                        current_plugs.clone(),
                        SocketLocation::Stored(ui_identity),
                    )
                })
                .inner;
            if let Some((_, hash)) = selection {
                requested.push(InventoryItemAction::set_plug(
                    &current_plugs,
                    socket_index,
                    hash,
                ));
            }
        });
    }
    fn draw_panoptes_socket_picker(
        &mut self,
        ui: &mut egui::Ui,
        item: &ItemDef,
        socket_index: usize,
        current_hash: Option<u64>,
        plugs: Vec<Option<u32>>,
        location: SocketLocation,
    ) -> Option<(String, Option<u64>)> {
        let current_label = current_hash.map_or_else(
            || "None".to_owned(),
            |hash| {
                self.manifest
                    .plug_label(hash, self.preferences.show_plug_hashes)
            },
        );
        let snapshot = item_editor::plug_picker_snapshot(
            &self.manifest,
            item,
            socket_index,
            current_hash,
            current_label,
            native_plug_default(&item.default_plugs, socket_index),
            self.plug_selection_mode,
        )
        .with_preview_plugs(plugs.into_iter());
        let query_key = location.search_key(socket_index);
        let mut query = self.plug_searches.remove(&query_key).unwrap_or_default();
        let button = widgets::draw_socket_button(
            ui,
            &self.manifest,
            current_hash,
            layout::is_mod_socket(item, socket_index),
            &format!("{}\n{}", snapshot.socket_label, snapshot.current_label),
        );
        let action = item_editor::draw_plug_icon_picker(
            ui,
            &self.manifest,
            ("panoptes-plug", &query_key),
            &mut query,
            &snapshot,
            PickerHeight {
                min: PLUG_PICKER_MIN_HEIGHT,
                max: PLUG_PICKER_MAX_HEIGHT,
            },
            &button,
        );
        if snapshot.choices.len() > 12 {
            self.plug_searches.insert(query_key, query);
        }
        match action {
            Some(ItemEditorAction::SetPlug { hash, .. }) => Some((snapshot.socket_label, hash)),
            _ => None,
        }
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

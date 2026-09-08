//! Optional Panoptes-inspired loadout presentation.
//!
//! Panoptes introduced the useful idea of editing one selected item beside an
//! equipped-plus-inventory icon grid. This module ports that presentation only:
//! Sundial's package-scanned catalog, stable inventory identities, adapter capability gates,
//! and atomic equipment/inventory actions remain authoritative.

use crate::app::account_workspace as account;

mod editors;
mod icons;
mod layout;
mod widgets;

use eframe::egui;

use super::EquippedItemSnapshot;
use crate::app::{
    SundialApp, ViewMode,
    inventory::InventoryItemSnapshot,
    inventory_page::{
        CharacterInventoryEditorContext, InventoryItemUiId, inventory_item_ui_identities,
    },
    item_editor,
};
use editors::{EquippedEditor, StoredEditor};

const GRID_COLUMNS: usize = 3;
const GRID_VISIBLE_ITEMS: usize = 9;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum PanoptesSelection {
    #[default]
    Equipped,
    Stored(InventoryItemUiId),
}

#[derive(Clone, Copy)]
struct StoredSlotItem<'a> {
    snapshot: &'a InventoryItemSnapshot,
    ui_identity: InventoryItemUiId,
}

struct PanoptesGrid<'a> {
    slot_label: &'static str,
    equipped_hash: Option<u64>,
    stored_items: &'a [StoredSlotItem<'a>],
    selection: &'a mut PanoptesSelection,
}

impl SundialApp {
    pub(in crate::app) fn draw_panoptes_layout_preview(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &EquippedItemSnapshot,
        class_type: u64,
    ) {
        let inventory_context = self.character_inventory_editor_context(false, class_type);
        self.draw_panoptes_slot(
            ui,
            0,
            snapshot.slot,
            snapshot.slot_label,
            snapshot.bucket_hash,
            class_type,
            false,
            true,
            Some(snapshot),
            snapshot.definition_hash,
            &[],
            &inventory_context,
        );
    }

    pub(super) fn draw_panoptes_equipment(&mut self, ui: &mut egui::Ui, character_index: usize) {
        let group_sockets_id = ui.make_persistent_id("panoptes-group-sockets");
        let mut group_sockets = ui
            .data_mut(|data| data.get_temp::<bool>(group_sockets_id))
            .unwrap_or(true);
        let equipment_editable = account::can_mutate_equipment(&self.document);
        let inventory_editable = account::can_mutate_character_inventory(&self.document);
        let class_type = account::character_metadata(&self.document, character_index)
            .ok()
            .map(|metadata| u64::from(metadata.class_type))
            .unwrap_or(99);
        let (inventory_items, inventory_error) =
            match account::character_inventory(&self.document, character_index) {
                Ok(items) => (items.unwrap_or_default(), None),
                Err(error) => (Vec::new(), Some(error.to_string())),
            };
        let inventory_ui_identities = inventory_item_ui_identities(&inventory_items);
        let inventory_context =
            self.character_inventory_editor_context(inventory_editable, class_type);
        let (equipped_items, equipment_error) =
            match account::equipped_item_snapshots(&self.document, character_index) {
                Ok(items) => (items, None),
                Err(error) => (Vec::new(), Some(error)),
            };

        ui.add_space(10.0);
        ui.heading("Loadout");
        let randomize_request = self.draw_panoptes_toolbar(
            ui,
            character_index,
            equipment_editable,
            inventory_editable,
            &mut group_sockets,
        );
        super::randomize::draw_dialogs(self, ui.ctx(), character_index, randomize_request);
        super::armor_stats_adjuster::draw_window(self, ui.ctx(), character_index);
        self.draw_equipped_armor_stat_row(ui, character_index);
        ui.data_mut(|data| data.insert_temp(group_sockets_id, group_sockets));
        if !inventory_editable {
            let reason = self.document.account_editing_blocked().unwrap_or(
                "Character inventory editing is unavailable for this settings.json schema.",
            );
            ui.label(egui::RichText::new(format!("Stored items are read-only. {reason}")).weak());
        }
        if let Some(error) = inventory_error {
            ui.colored_label(
                ui.visuals().error_fg_color,
                format!("Stored inventory could not be read: {error}"),
            );
        }
        if let Some(error) = equipment_error {
            ui.colored_label(
                ui.visuals().error_fg_color,
                format!("Equipped items could not be read: {error}"),
            );
        }

        let unmatched_count = inventory_items
            .iter()
            .filter(|item| {
                !self
                    .document
                    .equipment_slots()
                    .iter()
                    .any(|(_, _, bucket_hash)| {
                        self.manifest
                            .item_handle_for_bucket(u64::from(item.definition_hash), *bucket_hash)
                            .is_some()
                    })
            })
            .count();
        if unmatched_count > 0 {
            ui.label(
                egui::RichText::new(format!(
                    "{unmatched_count} stored item(s) do not map to a loadout slot and remain available in Character inventory."
                ))
                .weak(),
            );
        }
        ui.add_space(8.0);

        for &(slot, label, bucket_hash) in self
            .document
            .equipment_slots()
            .iter()
            .filter(|(slot, _, _)| *slot != "subclass")
        {
            let equipped_snapshot = equipped_items.iter().find(|snapshot| snapshot.slot == slot);
            let equipped_hash = equipped_snapshot.and_then(|snapshot| snapshot.definition_hash);
            let stored_items = inventory_items
                .iter()
                .zip(inventory_ui_identities.iter().copied())
                .filter(|(snapshot, _)| {
                    self.manifest
                        .item_handle_for_bucket(u64::from(snapshot.definition_hash), bucket_hash)
                        .is_some()
                })
                .map(|(snapshot, ui_identity)| StoredSlotItem {
                    snapshot,
                    ui_identity,
                })
                .collect::<Vec<_>>();

            self.draw_panoptes_slot(
                ui,
                character_index,
                slot,
                label,
                bucket_hash,
                class_type,
                equipment_editable,
                group_sockets,
                equipped_snapshot,
                equipped_hash,
                &stored_items,
                &inventory_context,
            );
            ui.add_space(4.0);
        }
    }

    fn draw_panoptes_toolbar(
        &mut self,
        ui: &mut egui::Ui,
        character_index: usize,
        equipment_editable: bool,
        inventory_editable: bool,
        group_sockets: &mut bool,
    ) -> Option<super::randomize::Request> {
        let mut randomize_request = None;
        ui.horizontal_wrapped(|ui| {
            ui.add_enabled_ui(equipment_editable, |ui| {
                self.draw_plug_safety_choice(ui, true);
                ui.separator();
                randomize_request =
                    super::randomize::draw_menu(ui, equipment_editable, inventory_editable);
                if super::armor_stats_adjuster::draw_entry_button(ui, equipment_editable).clicked()
                {
                    self.armor_stats_adjuster.open(character_index);
                }
            });
            ui.separator();
            ui.checkbox(group_sockets, "Group Sockets")
                .on_hover_text("Arrange sockets by their role, matching Panoptes loadout rows");
        });

        if self.preferences.show_safety_warnings {
            super::super::draw_plug_selection_warning(ui, self.plug_selection_mode);
        }
        randomize_request
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_panoptes_slot(
        &mut self,
        ui: &mut egui::Ui,
        character_index: usize,
        slot: &'static str,
        label: &'static str,
        bucket_hash: u64,
        class_type: u64,
        equipment_editable: bool,
        group_sockets: bool,
        equipped_snapshot: Option<&EquippedItemSnapshot>,
        equipped_hash: Option<u64>,
        stored_items: &[StoredSlotItem<'_>],
        inventory_context: &CharacterInventoryEditorContext,
    ) {
        ui.push_id(("panoptes-slot", character_index, slot), |ui| {
            let selection_id = ui.make_persistent_id("selection");
            let stored_visible = &stored_items[..stored_items.len().min(GRID_VISIBLE_ITEMS)];
            let mut selection = ui
                .data_mut(|data| data.get_temp::<PanoptesSelection>(selection_id))
                .unwrap_or_default();
            selection = valid_selection(selection, stored_visible);
            let mut equip_request = None;

            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_width(ui.available_width());
                let editor_selection = selection;
                let mut draw_editor = |app: &mut Self, ui: &mut egui::Ui| match editor_selection {
                    PanoptesSelection::Equipped => app.draw_panoptes_equipped_editor(
                        ui,
                        EquippedEditor {
                            character_index,
                            slot,
                            label,
                            bucket_hash,
                            class_type,
                            editable: equipment_editable,
                            snapshot: equipped_snapshot,
                            group_sockets,
                        },
                    ),
                    PanoptesSelection::Stored(ui_identity) => {
                        if let Some((inventory_index, stored)) = stored_visible
                            .iter()
                            .enumerate()
                            .find(|(_, stored)| stored.ui_identity == ui_identity)
                        {
                            if app.draw_panoptes_stored_editor(
                                ui,
                                StoredEditor {
                                    snapshot: stored.snapshot,
                                    ui_identity: stored.ui_identity,
                                    context: inventory_context,
                                    heading: &format!(
                                        "{label} · inventory {}",
                                        inventory_index + 1
                                    ),
                                    slot,
                                    bucket_hash,
                                    group_sockets,
                                },
                            ) {
                                equip_request = Some(stored.snapshot.location);
                            }
                        }
                    }
                };

                let editor_width = widgets::editor_width(ui);
                let gap = ui.spacing().item_spacing.x;
                let side_by_side_min_width =
                    editor_width + widgets::inventory_width(ui) + 3.0 * gap;
                if slot == "clan_banner" {
                    draw_editor(self, ui);
                } else if ui.available_width() >= side_by_side_min_width {
                    ui.horizontal_top(|ui| {
                        let editor = ui.allocate_ui_with_layout(
                            egui::vec2(editor_width, 0.0),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.set_min_width(editor_width);
                                draw_editor(self, ui);
                            },
                        );
                        ui.add_space(gap);
                        let divider_x = ui.cursor().left();
                        ui.add_space(gap);
                        let grid = ui.vertical(|ui| {
                            self.draw_panoptes_grid(
                                ui,
                                PanoptesGrid {
                                    slot_label: label,
                                    equipped_hash,
                                    stored_items,
                                    selection: &mut selection,
                                },
                            );
                        });

                        ui.painter().vline(
                            divider_x,
                            editor.response.rect.top()
                                ..=editor
                                    .response
                                    .rect
                                    .bottom()
                                    .max(grid.response.rect.bottom()),
                            ui.visuals().widgets.noninteractive.bg_stroke,
                        );
                    });
                } else {
                    draw_editor(self, ui);
                    ui.add_space(8.0);
                    ui.separator();
                    self.draw_panoptes_grid(
                        ui,
                        PanoptesGrid {
                            slot_label: label,
                            equipped_hash,
                            stored_items,
                            selection: &mut selection,
                        },
                    );
                }
            });

            if let Some(location) = equip_request
                && self.equip_stored_item(location, slot)
            {
                selection = PanoptesSelection::Equipped;
            }

            ui.data_mut(|data| data.insert_temp(selection_id, selection));
        });
    }

    fn draw_panoptes_grid(&mut self, ui: &mut egui::Ui, grid: PanoptesGrid<'_>) {
        let PanoptesGrid {
            slot_label,
            equipped_hash,
            stored_items,
            selection,
        } = grid;
        ui.horizontal_top(|ui| {
            let response = self.draw_panoptes_item_button(
                ui,
                equipped_hash,
                matches!(selection, PanoptesSelection::Equipped),
                &format!("Equipped {slot_label}"),
            );
            if response.clicked() {
                *selection = PanoptesSelection::Equipped;
            }

            widgets::draw_fixed_height_divider(ui, widgets::inventory_matrix_height(ui));
            ui.vertical(|ui| {
                for row in 0..(GRID_VISIBLE_ITEMS / GRID_COLUMNS) {
                    ui.horizontal(|ui| {
                        for column in 0..GRID_COLUMNS {
                            let index = row * GRID_COLUMNS + column;
                            let stored = stored_items.get(index);
                            let hash =
                                stored.map(|stored| u64::from(stored.snapshot.definition_hash));
                            let selected = stored.is_some_and(|stored| {
                                *selection == PanoptesSelection::Stored(stored.ui_identity)
                            });
                            let response = self.draw_panoptes_item_button(
                                ui,
                                hash,
                                selected,
                                &format!("Inventory {}", index + 1),
                            );
                            if response.clicked()
                                && let Some(stored) = stored
                            {
                                *selection = PanoptesSelection::Stored(stored.ui_identity);
                            }
                        }
                    });
                }
            });
        });

        let overflow = stored_items.len().saturating_sub(GRID_VISIBLE_ITEMS);
        if overflow > 0
            && ui
                .small_button(format!("+{overflow} more in Inventory ›"))
                .on_hover_text("Open the complete character inventory without hiding overflow")
                .clicked()
        {
            self.select_view(ViewMode::CharacterInventory);
        }
    }

    fn draw_panoptes_item_button(
        &self,
        ui: &mut egui::Ui,
        hash: Option<u64>,
        selected: bool,
        context_label: &str,
    ) -> egui::Response {
        let button_side = widgets::icon_button_side(ui, widgets::GEAR_ICON);
        let response = match icons::gear(&self.manifest, ui.ctx(), hash) {
            Some(texture) => ui.add(
                egui::ImageButton::new((
                    texture.id(),
                    egui::vec2(widgets::GEAR_ICON, widgets::GEAR_ICON),
                ))
                .corner_radius(3),
            ),
            None => ui.add_sized([button_side, button_side], egui::Button::new("")),
        };
        if selected {
            ui.painter().rect_stroke(
                response.rect,
                3.0,
                ui.visuals().selection.stroke,
                egui::StrokeKind::Inside,
            );
        }
        if let Some(hash) = hash {
            item_editor::catalog_item_tooltip(response, &self.manifest, hash)
        } else {
            response.on_hover_text(format!("{context_label}\nEmpty"))
        }
    }
}

fn valid_selection(
    selection: PanoptesSelection,
    stored_items: &[StoredSlotItem<'_>],
) -> PanoptesSelection {
    match selection {
        PanoptesSelection::Stored(ui_identity)
            if !stored_items
                .iter()
                .any(|stored| stored.ui_identity == ui_identity) =>
        {
            PanoptesSelection::Equipped
        }
        selection => selection,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::inventory::{InventoryItemLocation, ItemPlugs};

    fn identity(instance_soid: u64) -> InventoryItemUiId {
        InventoryItemUiId::new(0, instance_soid, None)
    }

    fn snapshot(instance_soid: u64) -> InventoryItemSnapshot {
        InventoryItemSnapshot {
            location: InventoryItemLocation {
                character_index: 0,
                item_index: 0,
            },
            instance_soid,
            definition_hash: 1,
            level: 0,
            quantity: 1,
            plugs: ItemPlugs::NativeDefaults,
            flags: None,
        }
    }

    #[test]
    fn stale_stored_selection_falls_back_to_equipped() {
        let item = snapshot(1);
        let stored = [StoredSlotItem {
            snapshot: &item,
            ui_identity: identity(1),
        }];

        assert_eq!(
            valid_selection(PanoptesSelection::Stored(identity(2)), &stored),
            PanoptesSelection::Equipped
        );
    }

    #[test]
    fn stable_stored_selection_survives_position_changes() {
        let first = snapshot(1);
        let selected = snapshot(2);
        let stored = [
            StoredSlotItem {
                snapshot: &first,
                ui_identity: identity(1),
            },
            StoredSlotItem {
                snapshot: &selected,
                ui_identity: identity(2),
            },
        ];

        assert_eq!(
            valid_selection(PanoptesSelection::Stored(identity(2)), &stored),
            PanoptesSelection::Stored(identity(2))
        );
    }
}

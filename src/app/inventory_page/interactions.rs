//! Shared item interactions, transient picker state, transfers, and atomic edits.

use std::collections::HashMap;

use eframe::egui;

use crate::{
    catalog::{InventoryMetadata, ItemDef},
    hash::parse_hash_hex,
};

use super::{
    super::{
        SundialApp,
        inventory::{
            self, InventoryItemAction, InventoryItemLocation, InventoryItemSnapshot, ItemPlugs,
        },
        ui::single_line_galley as transfer_menu_galley,
    },
    model::{
        BucketUsage, CharacterTransferDestination, InventoryItemUiId,
        TRANSFER_DESTINATION_ROW_HEIGHT, TRANSFER_DESTINATION_ROW_SPACING,
    },
};

impl SundialApp {
    pub(super) fn open_bucket_picker(&mut self, key: &str, prefix: &str) {
        self.searches
            .retain(|stored, _| !stored.starts_with(prefix));
        self.searches.insert(key.to_owned(), String::new());
        self.searches
            .insert(bucket_picker_open_request_key(key), String::new());
    }

    pub(super) fn clear_inventory_item_picker_state(
        &mut self,
        ui_identity: InventoryItemUiId,
        removed: bool,
    ) {
        let key = inventory_item_state_key(ui_identity);
        if removed {
            self.searches.remove(&key);
        }
        let plug_prefix = format!("{key}:plug:");
        self.plug_searches
            .retain(|stored, _| !stored.starts_with(&plug_prefix));
    }

    pub(super) fn mark_inventory_changed(&mut self, status: &str) {
        self.dirty = true;
        self.set_status(format!("{status}; click Save to write it"), false);
    }
}
pub(super) fn bucket_picker_open_request_key(picker_key: &str) -> String {
    format!("{picker_key}:request-open")
}

pub(super) fn take_bucket_picker_open_request(
    searches: &mut HashMap<String, String>,
    picker_key: &str,
    pointer_clicked: bool,
) -> bool {
    if pointer_clicked {
        return false;
    }
    searches
        .remove(&bucket_picker_open_request_key(picker_key))
        .is_some()
}
pub(super) fn draw_character_transfer_destinations(
    ui: &mut egui::Ui,
    destinations: &[CharacterTransferDestination],
) -> Option<usize> {
    if destinations.is_empty() {
        return None;
    }

    let mut selected = None;
    ui.separator();
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = TRANSFER_DESTINATION_ROW_SPACING;
        ui.label(egui::RichText::new("Move to another character").strong());
        for destination in destinations {
            let response = ui
                .add_enabled_ui(destination.enabled, |ui| {
                    draw_character_transfer_destination(ui, destination)
                })
                .inner;
            let response = if destination.enabled {
                response.on_hover_text(&destination.tooltip)
            } else {
                response.on_disabled_hover_text(&destination.tooltip)
            };
            if response.clicked() {
                selected = Some(destination.character_index);
            }
        }
    });
    selected
}

pub(super) fn draw_character_transfer_destination(
    ui: &mut egui::Ui,
    destination: &CharacterTransferDestination,
) -> egui::Response {
    draw_inventory_item_menu_text(ui, &destination.label, &destination.detail)
}

pub(super) fn character_bucket_usage_detail(
    metadata: InventoryMetadata,
    usage: &BucketUsage,
) -> Option<String> {
    let capacity = metadata.authored_row_capacity()?;
    let occupied = usage
        .counts
        .get(&metadata.native_bucket_id)
        .copied()
        .unwrap_or_default();
    Some(format!(
        "{} · {occupied} / {capacity} slots used",
        metadata.bucket_label()
    ))
}

pub(super) fn draw_inventory_item_menu_text(
    ui: &mut egui::Ui,
    primary_text: &str,
    secondary_text: &str,
) -> egui::Response {
    const HORIZONTAL_PADDING: f32 = 4.0;
    const TEXT_GAP: f32 = 8.0;
    const PRIMARY_WIDTH_SHARE: f32 = 0.45;
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), TRANSFER_DESTINATION_ROW_HEIGHT),
        egui::Sense::click(),
    );
    if !ui.is_rect_visible(rect) {
        return response;
    }

    let visuals = ui.style().interact(&response);
    if response.hovered() || response.has_focus() {
        ui.painter().rect(
            rect,
            visuals.corner_radius,
            visuals.weak_bg_fill,
            visuals.bg_stroke,
            egui::StrokeKind::Inside,
        );
    }

    let text_width = (rect.width() - HORIZONTAL_PADDING * 2.0).max(0.0);
    let available_text_width = (text_width - TEXT_GAP).max(0.0);
    let primary_font = egui::TextStyle::Button.resolve(ui.style());
    let secondary_font = egui::TextStyle::Body.resolve(ui.style());
    let primary_color = visuals.text_color();
    let secondary_color = ui.visuals().weak_text_color();
    let natural_primary = transfer_menu_galley(
        ui,
        primary_text,
        primary_font.clone(),
        primary_color,
        f32::INFINITY,
    );
    let natural_secondary = transfer_menu_galley(
        ui,
        secondary_text,
        secondary_font.clone(),
        secondary_color,
        f32::INFINITY,
    );
    let reserved_primary_width = natural_primary
        .size()
        .x
        .min(available_text_width * PRIMARY_WIDTH_SHARE);
    let secondary_width = natural_secondary
        .size()
        .x
        .min((available_text_width - reserved_primary_width).max(0.0));
    let primary_width = natural_primary
        .size()
        .x
        .min((available_text_width - secondary_width).max(0.0));
    let primary =
        transfer_menu_galley(ui, primary_text, primary_font, primary_color, primary_width);
    let secondary = transfer_menu_galley(
        ui,
        secondary_text,
        secondary_font,
        secondary_color,
        secondary_width,
    );
    let primary_position = egui::pos2(
        rect.left() + HORIZONTAL_PADDING,
        rect.center().y - primary.size().y / 2.0,
    );
    let secondary_position = egui::pos2(
        rect.right() - HORIZONTAL_PADDING - secondary.size().x,
        rect.center().y - secondary.size().y / 2.0,
    );
    ui.painter()
        .galley(primary_position, primary, primary_color);
    ui.painter()
        .galley(secondary_position, secondary, secondary_color);
    response
}

pub(in crate::app) fn inventory_item_ui_identities(
    items: &[InventoryItemSnapshot],
) -> Vec<InventoryItemUiId> {
    let mut totals = HashMap::<u64, usize>::new();
    for item in items {
        *totals.entry(item.instance_soid).or_default() += 1;
    }
    let mut seen = HashMap::<u64, usize>::new();
    items
        .iter()
        .map(|item| {
            let duplicate_ordinal = (totals[&item.instance_soid] > 1).then(|| {
                let ordinal = seen.entry(item.instance_soid).or_default();
                let current = *ordinal;
                *ordinal += 1;
                current
            });
            InventoryItemUiId {
                character_index: item.location.character_index,
                instance_soid: item.instance_soid,
                duplicate_ordinal,
            }
        })
        .collect()
}

pub(in crate::app) fn inventory_item_state_key(identity: InventoryItemUiId) -> String {
    let duplicate = identity
        .duplicate_ordinal
        .map_or_else(String::new, |ordinal| format!(":duplicate-{ordinal}"));
    format!(
        "character-inventory:{}:{}{}",
        identity.character_index, identity.instance_soid, duplicate
    )
}

pub(in crate::app) fn displayed_inventory_plugs(
    inventory: &InventoryItemSnapshot,
    item: &ItemDef,
) -> (Vec<Option<u32>>, bool) {
    match &inventory.plugs {
        ItemPlugs::NativeDefaults => (
            item.default_plugs
                .iter()
                .map(|hash| {
                    hash.as_deref()
                        .and_then(parse_hash_hex)
                        .and_then(|hash| u32::try_from(hash).ok())
                })
                .collect(),
            true,
        ),
        ItemPlugs::Authored(plugs) => (plugs.clone(), false),
    }
}

pub(super) fn apply_inventory_actions_atomic(
    document: &mut serde_json::Value,
    location: InventoryItemLocation,
    actions: Vec<InventoryItemAction>,
) -> Result<(), String> {
    let mut candidate = document.clone();
    for action in actions {
        inventory::apply_inventory_item_action(&mut candidate, location, action)
            .map_err(|error| error.to_string())?;
    }
    *document = candidate;
    Ok(())
}

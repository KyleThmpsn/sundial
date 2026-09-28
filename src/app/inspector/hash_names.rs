use eframe::egui;

use crate::catalog::Catalog;

use super::super::ui::TABLE_CELL_HEIGHT;
use super::metadata::draw_named_catalog_hash_link;

/// Resolve the most specific catalog label available for an item-definition
/// hash.
///
/// Package item names take precedence because they describe the exact package
/// record represented by item, plug, and material hashes. This intentionally
/// must not be used for sandbox-perk hashes: that hash space has same-value item
/// collisions but no validated localization bridge in the supported build.
fn resolved_item_definition_name(catalog: &Catalog, hash: u64) -> Option<&str> {
    catalog
        .package_item_name(hash)
        .or_else(|| catalog.display_name(hash))
}

/// A fixed-width table cell holding the item's name as a link to its definition.
pub(in crate::app) fn item_definition_name_cell(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    hash: u64,
    width: f32,
) -> egui::Response {
    ui.allocate_ui_with_layout(
        egui::vec2(width, TABLE_CELL_HEIGHT),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_size(egui::vec2(width, TABLE_CELL_HEIGHT));
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
            if hash == 0 {
                return ui.weak("-");
            }
            let name = resolved_item_definition_name(catalog, hash).unwrap_or(super::UNNAMED);
            draw_named_catalog_hash_link(ui, catalog, hash, name)
        },
    )
    .inner
}

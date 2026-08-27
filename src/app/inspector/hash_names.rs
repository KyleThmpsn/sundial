use eframe::egui;

use crate::catalog::Catalog;

use super::super::ui::table_cell;

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

pub(in crate::app) fn item_definition_name_cell(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    hash: u64,
    width: f32,
) -> egui::Response {
    if let Some(name) = resolved_item_definition_name(catalog, hash) {
        crate::app::item_editor::catalog_item_tooltip(table_cell(ui, width, name), catalog, hash)
    } else {
        let response = table_cell(
            ui,
            width,
            egui::RichText::new("<not resolved>").weak().italics(),
        );
        if crate::app::item_editor::catalog_item_tooltip_available(catalog, hash) {
            crate::app::item_editor::catalog_item_tooltip(response, catalog, hash)
        } else {
            response.on_hover_text(format!(
                "No package item name resolves for definition hash 0x{hash:08X}"
            ))
        }
    }
}

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
        table_cell(ui, width, name).on_hover_text(name)
    } else {
        table_cell(
            ui,
            width,
            egui::RichText::new("<not resolved>").weak().italics(),
        )
        .on_hover_text(format!(
            "No package item name resolves for definition hash 0x{hash:08X}"
        ))
    }
}

pub(in crate::app) fn unresolved_name_cell(
    ui: &mut egui::Ui,
    width: f32,
    explanation: &'static str,
) -> egui::Response {
    table_cell(
        ui,
        width,
        egui::RichText::new("<not resolved>").weak().italics(),
    )
    .on_hover_text(explanation)
}

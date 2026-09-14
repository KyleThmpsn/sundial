//! One item menu opened by either the header or its small menu button.

use eframe::egui;

use super::{DefinitionInspectionContext, format_hash_hex, request_hash_inspection_with_context};

/// Lock control aligned immediately before the header's item menu.
pub(crate) fn draw_header_lock(
    ui: &mut egui::Ui,
    header: &egui::Response,
    flags: Option<u8>,
    enabled: bool,
) -> Option<Option<u8>> {
    let locked = flags.unwrap_or_default() & crate::account_contract::INVENTORY_FLAG_LOCKED != 0;
    let rect = egui::Rect::from_min_size(
        header.rect.right_bottom() - egui::vec2(47.0, 19.0),
        egui::vec2(22.0, 18.0),
    );
    let response = ui
        .scope_builder(
            egui::UiBuilder::new()
                .id_salt(header.id.with("item_lock_button"))
                .max_rect(rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
            |ui| {
                if locked {
                    super::draw_lock_button(ui, enabled, "Unlock Item")
                        .on_hover_text("Unlock this item")
                } else {
                    super::draw_unlock_button(ui, enabled, "Lock Item")
                        .on_hover_text("Lock this item")
                }
            },
        )
        .inner;
    response
        .clicked()
        .then(|| crate::app::inventory::set_inventory_locked_flag(flags, !locked))
}

pub(crate) fn draw_context_menu(
    ui: &mut egui::Ui,
    header: &egui::Response,
    item: Option<(u64, DefinitionInspectionContext)>,
    contents: impl FnOnce(&mut egui::Ui),
) {
    let rect = egui::Rect::from_min_size(
        header.rect.right_bottom() - egui::vec2(23.0, 19.0),
        egui::vec2(22.0, 18.0),
    );
    let button = ui
        .push_id(header.id.with("item_menu_button"), |ui| {
            ui.put(
                rect,
                egui::Button::new(egui::RichText::new("…").size(13.0))
                    .small()
                    .frame(false),
            )
            .on_hover_text("Item Menu")
        })
        .inner;
    let menu_id = egui::Id::new("item_header_context_menu");
    let mut state = egui::menu::BarState::load(ui.ctx(), menu_id);
    egui::menu::MenuRoot::context_click_interaction(header, &mut state);
    if button.clicked() {
        let mut position = button.rect.left_bottom();
        if let Some(transform) = ui.ctx().layer_transform_to_global(header.layer_id) {
            position = transform * position;
        }
        egui::menu::MenuRoot::handle_menu_response(
            &mut state,
            egui::menu::MenuResponse::Create(position, header.id),
        );
    }
    state.show(header, |ui| {
        if let Some((hash, context)) = &item
            && ui.button("Inspect Item").clicked()
        {
            request_hash_inspection_with_context(ui.ctx(), *hash, context.clone());
            ui.close_menu();
        }
        contents(ui);
        if let Some((hash, _)) = item {
            ui.separator();
            if ui.button("Copy Hash (Hex)").clicked() {
                ui.ctx().copy_text(format_hash_hex(hash));
                ui.close_menu();
            }
            if ui.button("Copy Hash (Decimal)").clicked() {
                ui.ctx().copy_text(hash.to_string());
                ui.close_menu();
            }
        }
    });
    state.store(ui.ctx(), menu_id);
}

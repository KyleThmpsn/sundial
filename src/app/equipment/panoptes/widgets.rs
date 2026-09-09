//! Compact widgets that visually match the Panoptes loadout editor.

use eframe::egui;

use crate::{
    app::{
        inspector::{
            DefinitionInspectionContext, request_definition, request_definition_with_context,
        },
        item_editor,
    },
    catalog::Catalog,
    hash::format_hash_hex,
};

use super::{icons, layout::SocketLine};

pub(super) const SOCKET_ICON: f32 = 44.0;
pub(super) const GEAR_ICON: f32 = SOCKET_ICON * 1.5;

pub(super) fn icon_button_side(ui: &egui::Ui, icon: f32) -> f32 {
    icon + 2.0 * ui.spacing().button_padding.x
}

pub(super) fn pin_column_width(ui: &egui::Ui) -> f32 {
    icon_button_side(ui, SOCKET_ICON) + 3.0
}

pub(super) fn editor_width(ui: &egui::Ui) -> f32 {
    let gap = ui.spacing().item_spacing.x;
    let icon = icon_button_side(ui, SOCKET_ICON);
    let row = super::layout::MAX_ROW_WIDTH as f32;
    pin_column_width(ui) + 3.0 * gap + row * icon + (row - 1.0) * gap + 2.0
}

pub(super) fn inventory_width(ui: &egui::Ui) -> f32 {
    let gap = ui.spacing().item_spacing.x;
    let gear = icon_button_side(ui, GEAR_ICON);
    gear + 3.0 * gap + 3.0 * gear + 2.0 * gap
}

pub(super) fn inventory_matrix_height(ui: &egui::Ui) -> f32 {
    let gear = icon_button_side(ui, GEAR_ICON);
    3.0 * gear + 2.0 * ui.spacing().item_spacing.y
}

pub(super) struct CompactItemHeader<'a> {
    pub(super) heading: &'a str,
    pub(super) title: &'a str,
    pub(super) type_name: Option<&'a str>,
    pub(super) armor_generation: Option<&'static str>,
    pub(super) hash: Option<u64>,
    pub(super) inspection_context: DefinitionInspectionContext,
    pub(super) hash_display_text: Option<&'a str>,
    pub(super) default_plugs_equipped: bool,
    pub(super) valid: bool,
    pub(super) invalid_message: &'a str,
}

pub(super) fn draw_compact_item_header(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    header: CompactItemHeader<'_>,
    trailing: impl FnOnce(&mut egui::Ui),
    context_menu: impl FnOnce(&mut egui::Ui),
) -> egui::Response {
    let mut icon_response = None;
    let mut hash_rect = None;
    let header_area = ui.horizontal(|ui| {
        let texture = icons::gear(catalog, ui.ctx(), header.hash);
        let button_side = icon_button_side(ui, GEAR_ICON);
        let response = match texture {
            None => ui.add_sized([button_side, button_side], egui::Button::new("")),
            Some(icon) => ui.add(
                egui::ImageButton::new((icon.id(), egui::vec2(GEAR_ICON, GEAR_ICON)))
                    .corner_radius(3),
            ),
        };
        icon_response = Some(if header.hash.is_some() {
            response
        } else {
            response.on_hover_text(format!(
                "{}: {}\nClick to change",
                header.heading, header.title
            ))
        });

        ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(header.heading).strong().size(13.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(hash_text) = header.hash_display_text {
                        let hash_response =
                            ui.label(egui::RichText::new(hash_text).monospace().weak());
                        hash_rect = Some(hash_response.rect);
                    }
                });
            });
            if header.default_plugs_equipped {
                ui.label(egui::RichText::new("Default Plugs Equipped").weak());
            }
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    egui::RichText::new(header.title)
                        .size(15.0)
                        .color(if header.valid {
                            ui.visuals().strong_text_color()
                        } else {
                            ui.visuals().error_fg_color
                        }),
                );
                if header.hash.is_some_and(crate::dummy_items::contains) {
                    item_editor::draw_item_badge(ui, "Dummy")
                        .on_hover_text("Display-only dummy definition");
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let mut drew_metadata = false;
                if let Some(type_name) = header.type_name {
                    ui.label(egui::RichText::new(type_name).weak());
                    drew_metadata = true;
                }
                if let Some(generation) = header.armor_generation {
                    if drew_metadata {
                        ui.label(egui::RichText::new("·").weak());
                    }
                    ui.label(egui::RichText::new(generation).weak());
                }
            });
            if !header.valid {
                ui.label(
                    egui::RichText::new(header.invalid_message)
                        .small()
                        .color(ui.visuals().error_fg_color),
                );
            }
            trailing(ui);
        });
    });

    let response = (icon_response.expect("a compact item header always draws its icon")
        | header_area.response)
        .interact(egui::Sense::click());
    if let Some(hash) = header.hash {
        let card_response = ui.interact(
            response.rect,
            response.id.with(("item_card", hash)),
            egui::Sense::click(),
        );
        let tooltip_response =
            item_editor::catalog_item_tooltip(card_response.clone(), catalog, hash);
        if let Some(hash_rect) = hash_rect {
            let hash_response = ui
                .interact(
                    hash_rect.expand2(egui::vec2(4.0, 2.0)),
                    response.id.with(("definition_hash", hash)),
                    egui::Sense::click(),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text("Inspect Item");
            if hash_response.clicked() {
                request_definition_with_context(ui.ctx(), hash, header.inspection_context.clone());
            }
        }
        card_response.context_menu(|ui| {
            if ui.button("Inspect Item").clicked() {
                request_definition_with_context(ui.ctx(), hash, header.inspection_context.clone());
                ui.close_menu();
            }
            context_menu(ui);
            ui.separator();
            if ui.button("Copy Hash (Hex)").clicked() {
                ui.ctx().copy_text(format_hash_hex(hash));
                ui.close_menu();
            }
            if ui.button("Copy Hash (Decimal)").clicked() {
                ui.ctx().copy_text(hash.to_string());
                ui.close_menu();
            }
        });
        response | card_response | tooltip_response
    } else {
        response
    }
}

pub(super) fn draw_socket_rows(
    ui: &mut egui::Ui,
    lines: Vec<SocketLine>,
    mut draw_socket: impl FnMut(&mut egui::Ui, usize),
) {
    for line in lines {
        ui.horizontal(|ui| {
            let button_side = icon_button_side(ui, SOCKET_ICON);
            let cell = egui::vec2(pin_column_width(ui), button_side);
            ui.allocate_ui_with_layout(
                cell,
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_min_size(cell);
                    if let Some(socket_index) = line.pinned {
                        draw_socket(ui, socket_index);
                    }
                },
            );
            draw_fixed_height_divider(ui, button_side);
            for socket_index in line.row {
                draw_socket(ui, socket_index);
            }
        });
    }
}

pub(super) fn draw_socket_button(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    hash: Option<u64>,
    empty_is_mod: bool,
    label: &str,
) -> egui::Response {
    let texture = icons::socket(catalog, ui.ctx(), hash, empty_is_mod);
    let button_side = icon_button_side(ui, SOCKET_ICON);
    let response = match texture {
        None => ui.add_sized([button_side, button_side], egui::Button::new("")),
        Some(texture) => ui.add(
            egui::ImageButton::new(
                egui::Image::new((texture.id(), egui::vec2(SOCKET_ICON, SOCKET_ICON)))
                    .bg_fill(crate::app::ui::package_icon_backdrop(ui)),
            )
            .corner_radius(3),
        ),
    };
    let response = if let Some(hash) = hash {
        item_editor::catalog_item_tooltip(response, catalog, hash)
    } else {
        response.on_hover_text(label)
    };
    if let Some(hash) = hash {
        response.context_menu(|ui| {
            if ui.button("Inspect definition").clicked() {
                request_definition(ui.ctx(), hash);
                ui.close_menu();
            }
            if ui.button("Copy hash (hex)").clicked() {
                ui.ctx().copy_text(format_hash_hex(hash));
                ui.close_menu();
            }
            if ui.button("Copy hash (decimal)").clicked() {
                ui.ctx().copy_text(hash.to_string());
                ui.close_menu();
            }
        });
    }
    response
}

pub(super) fn draw_fixed_height_divider(ui: &mut egui::Ui, height: f32) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.spacing().item_spacing.x, height),
        egui::Sense::hover(),
    );
    ui.painter().vline(
        rect.center().x,
        rect.y_range(),
        ui.visuals().widgets.noninteractive.bg_stroke,
    );
}

use eframe::egui;

use super::super::glyphs::{self, Glyph};

const TRASH_HOVER_COLOR: egui::Color32 = egui::Color32::from_rgb(166, 111, 114);

/// Draws a compact, neutral delete button.
pub(crate) fn draw_trash_button(
    ui: &mut egui::Ui,
    enabled: bool,
    accessible_label: &str,
) -> egui::Response {
    draw_action_button(
        ui,
        enabled,
        accessible_label,
        Glyph::Trash,
        None,
        Some(TRASH_HOVER_COLOR),
        true,
    )
}

/// Draws the active lock state in a muted green.
pub(crate) fn draw_lock_button(
    ui: &mut egui::Ui,
    enabled: bool,
    accessible_label: &str,
) -> egui::Response {
    draw_action_button(
        ui,
        enabled,
        accessible_label,
        Glyph::Lock,
        Some(egui::Color32::from_rgb(102, 153, 113)),
        None,
        false,
    )
}

/// Draws the inactive lock state with the theme's subdued text color.
pub(crate) fn draw_unlock_button(
    ui: &mut egui::Ui,
    enabled: bool,
    accessible_label: &str,
) -> egui::Response {
    let color = ui.visuals().weak_text_color();
    draw_action_button(
        ui,
        enabled,
        accessible_label,
        Glyph::Unlock,
        Some(color),
        None,
        false,
    )
}

fn draw_action_button(
    ui: &mut egui::Ui,
    enabled: bool,
    accessible_label: &str,
    icon: Glyph,
    icon_color: Option<egui::Color32>,
    hover_icon_color: Option<egui::Color32>,
    framed: bool,
) -> egui::Response {
    let use_icon_color = enabled && ui.is_enabled();
    let text_button_height = ui.text_style_height(&egui::TextStyle::Body);
    let side = if framed {
        text_button_height
    } else {
        (text_button_height + 2.0).max(16.0)
    };
    let response = if framed {
        ui.scope(|ui| {
            ui.style_mut().visuals.widgets.hovered.expansion = 0.0;
            ui.style_mut().visuals.widgets.active.expansion = 0.0;
            ui.add_enabled(
                enabled,
                egui::Button::new("")
                    .small()
                    .min_size(egui::vec2(side, side)),
            )
        })
        .inner
    } else {
        ui.add_enabled_ui(enabled, |ui| {
            ui.allocate_response(egui::vec2(side, side), egui::Sense::click())
        })
        .inner
    };
    let response = if !framed && use_icon_color {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    };
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, accessible_label)
    });

    if ui.is_rect_visible(response.rect) {
        let base_icon_size = (response.rect.height() - 6.0).clamp(10.0, 12.0);
        let icon_size = match icon {
            Glyph::Trash => base_icon_size + 1.0,
            Glyph::Lock | Glyph::Unlock => base_icon_size + 2.0,
            Glyph::ChevronUp | Glyph::ChevronDown | Glyph::ChevronLeft | Glyph::ChevronRight => {
                base_icon_size
            }
        };
        let icon_rect =
            egui::Rect::from_center_size(response.rect.center(), egui::vec2(icon_size, icon_size));
        let interaction_color = ui.style().interact(&response).fg_stroke.color;
        let color = if use_icon_color && response.hovered() {
            hover_icon_color.or(icon_color).unwrap_or(interaction_color)
        } else if use_icon_color {
            icon_color.unwrap_or(interaction_color)
        } else {
            interaction_color
        };
        let stroke = egui::Stroke::new((icon_size / 12.0).max(1.0), color);
        glyphs::paint_with_stroke(ui, icon_rect, icon, stroke);
    }

    response
}

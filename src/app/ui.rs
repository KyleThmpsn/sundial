use eframe::egui;

use super::glyphs::{self, Glyph};

const DESTINY_TEXT_FONT_FAMILY: &str = "Sundial Destiny text";

pub(super) fn section_heading(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let style = egui::TextStyle::Name("Section Heading".into());
    let text = egui::RichText::new(text).strong();
    let text = if ui.style().text_styles.contains_key(&style) {
        text.text_style(style)
    } else {
        text
    };
    ui.label(text)
}

pub(super) fn field_label(ui: &mut egui::Ui, text: &str, width: f32) -> egui::Response {
    ui.allocate_ui_with_layout(
        egui::vec2(width, ui.spacing().interact_size.y),
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            ui.set_min_width(width);
            ui.label(crate::ui_help::emphasized_text(ui, text))
        },
    )
    .inner
}

pub(super) fn secondary_text_color(ui: &egui::Ui) -> egui::Color32 {
    egui::Color32::from_gray(if ui.visuals().dark_mode { 175 } else { 100 })
}

pub(super) fn configure_contrast(ctx: &egui::Context) {
    ctx.all_styles_mut(|style| {
        if style.visuals.dark_mode {
            style.visuals.override_text_color = Some(egui::Color32::from_gray(240));
            style.visuals.error_fg_color = egui::Color32::from_rgb(255, 128, 128);
            style.visuals.warn_fg_color = egui::Color32::from_rgb(255, 180, 84);
        } else {
            style.visuals.error_fg_color = egui::Color32::from_rgb(175, 0, 0);
            style.visuals.warn_fg_color = egui::Color32::from_rgb(143, 74, 0);
        }
    });
}

pub(super) const TABLE_CELL_HEIGHT: f32 = 24.0;
pub(super) const TABLE_COLUMN_GAP: f32 = 12.0;
pub(super) const HIERARCHY_INDENT: f32 = 14.0;

// Destiny's transparent perk glyphs are authored in white. Keep their native
// colors, but provide a dark plate when the surrounding application is light.
pub(super) fn package_icon_backdrop(ui: &egui::Ui) -> egui::Color32 {
    if ui.visuals().dark_mode {
        egui::Color32::TRANSPARENT
    } else {
        egui::Color32::from_gray(55)
    }
}

pub(super) fn destiny_text_font_family() -> egui::FontFamily {
    egui::FontFamily::Name(DESTINY_TEXT_FONT_FAMILY.into())
}

pub(super) fn destiny_text(ui: &egui::Ui, text: impl Into<String>) -> egui::RichText {
    let mut font_id = egui::TextStyle::Body.resolve(ui.style());
    font_id.family = destiny_text_font_family();
    egui::RichText::new(text.into()).font(font_id)
}

pub(super) fn toolbar<R>(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::NONE
        .fill(ui.visuals().faint_bg_color)
        .corner_radius(egui::CornerRadius::same(4))
        .inner_margin(egui::Margin::symmetric(8, 3))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(add_contents).inner
        })
        .inner
}

pub(super) fn sortable_header_cell(
    ui: &mut egui::Ui,
    width: f32,
    label: &str,
    marker: Option<Glyph>,
) -> egui::Response {
    let cell = ui.allocate_ui_with_layout(
        egui::vec2(width, TABLE_CELL_HEIGHT),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_size(egui::vec2(width, TABLE_CELL_HEIGHT));
            ui.spacing_mut().item_spacing.x = 3.0;
            let label_response = ui.add(
                egui::Label::new(egui::RichText::new(label).strong())
                    .truncate()
                    .sense(egui::Sense::click()),
            );
            if let Some(direction) = marker {
                let (rect, _) = ui
                    .allocate_exact_size(egui::vec2(10.0, TABLE_CELL_HEIGHT), egui::Sense::hover());
                glyphs::paint(ui, rect, direction);
            }
            label_response
        },
    );
    let cell_response = cell
        .response
        .interact(egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text("Sort");
    let response = cell_response
        .union(cell.inner)
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text("Sort");
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Sort by {label}"))
    });
    response
}

pub(super) fn glyph_button(
    ui: &mut egui::Ui,
    glyph: Glyph,
    accessible_label: &str,
) -> egui::Response {
    let side = ui
        .text_style_height(&egui::TextStyle::Body)
        .max(ui.spacing().interact_size.y);
    let response = ui
        .scope(|ui| {
            ui.style_mut().visuals.widgets.hovered.expansion = 0.0;
            ui.style_mut().visuals.widgets.active.expansion = 0.0;
            ui.add(
                egui::Button::new("")
                    .small()
                    .min_size(egui::Vec2::splat(side)),
            )
        })
        .inner
        .on_hover_text(accessible_label);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, accessible_label)
    });

    if ui.is_rect_visible(response.rect) {
        let icon_size = (response.rect.height() - 8.0).clamp(10.0, 12.0);
        let icon_rect =
            egui::Rect::from_center_size(response.rect.center(), egui::Vec2::splat(icon_size));
        glyphs::paint_with_stroke(
            ui,
            icon_rect,
            glyph,
            ui.style().interact(&response).fg_stroke,
        );
    }

    response
}

pub(super) fn table_cell(
    ui: &mut egui::Ui,
    width: f32,
    text: impl Into<egui::WidgetText>,
) -> egui::Response {
    ui.allocate_ui_with_layout(
        egui::vec2(width, TABLE_CELL_HEIGHT),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_size(egui::vec2(width, TABLE_CELL_HEIGHT));
            ui.add(egui::Label::new(text).truncate())
        },
    )
    .inner
}

pub(super) fn hierarchy_branch_cell(
    ui: &mut egui::Ui,
    width: f32,
    depth: usize,
    label: &str,
    expanded: bool,
    interactive: bool,
) -> egui::Response {
    let cell = ui.allocate_ui_with_layout(
        egui::vec2(width, TABLE_CELL_HEIGHT),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_size(egui::vec2(width, TABLE_CELL_HEIGHT));
            ui.add_space(depth as f32 * HIERARCHY_INDENT);
            ui.spacing_mut().item_spacing.x = 4.0;
            let (rect, _) =
                ui.allocate_exact_size(egui::vec2(10.0, TABLE_CELL_HEIGHT), egui::Sense::hover());
            glyphs::paint(
                ui,
                rect,
                if expanded {
                    Glyph::ChevronDown
                } else {
                    Glyph::ChevronRight
                },
            );
            ui.add(egui::Label::new(egui::RichText::new(label).strong()).truncate());
        },
    );
    let response = cell.response.interact(if interactive {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    });
    if interactive {
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                true,
                format!("{} {label}", if expanded { "Collapse" } else { "Expand" }),
            )
        });
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}

pub(super) fn hierarchy_leaf_cell(
    ui: &mut egui::Ui,
    width: f32,
    depth: usize,
    text: impl Into<egui::WidgetText>,
) -> egui::Response {
    ui.allocate_ui_with_layout(
        egui::vec2(width, TABLE_CELL_HEIGHT),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_size(egui::vec2(width, TABLE_CELL_HEIGHT));
            ui.add_space((depth as f32 + 1.0) * HIERARCHY_INDENT);
            ui.add(egui::Label::new(text).truncate())
        },
    )
    .inner
}

pub(super) fn single_line_galley(
    ui: &egui::Ui,
    text: &str,
    font_id: egui::FontId,
    color: egui::Color32,
    max_width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::single_section(
        text.to_owned(),
        egui::TextFormat {
            font_id,
            color,
            ..Default::default()
        },
    );
    job.wrap.max_width = max_width;
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    ui.fonts(|fonts| fonts.layout_job(job))
}

#[cfg(test)]
mod contrast_tests {
    use super::*;

    fn luminance(color: egui::Color32) -> f32 {
        let linear = |value: u8| {
            let value = f32::from(value) / 255.0;
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        linear(color.r()) * 0.2126 + linear(color.g()) * 0.7152 + linear(color.b()) * 0.0722
    }

    #[test]
    fn guidance_and_status_colors_remain_readable_after_theme_changes() {
        let ctx = egui::Context::default();
        configure_contrast(&ctx);
        for theme in [egui::Theme::Dark, egui::Theme::Light, egui::Theme::Dark] {
            ctx.set_theme(theme);
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let visuals = ui.visuals();
                    for foreground in [
                        secondary_text_color(ui),
                        visuals.warn_fg_color,
                        visuals.error_fg_color,
                    ] {
                        for background in [
                            visuals.panel_fill,
                            visuals.window_fill(),
                            visuals.extreme_bg_color,
                        ] {
                            let a = luminance(foreground);
                            let b = luminance(background);
                            let contrast = (a.max(b) + 0.05) / (a.min(b) + 0.05);
                            assert!(contrast >= 4.5, "contrast {contrast}, theme {theme:?}");
                        }
                    }
                });
            });
        }
    }
}

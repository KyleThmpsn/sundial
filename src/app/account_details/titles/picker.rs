use crate::{catalog::Catalog, investment::titles::Title};
use eframe::egui;

fn paint_icon(ui: &egui::Ui, catalog: &Catalog, title: &Title, rect: egui::Rect) {
    if let Some(container) = title.icon_container
        && let Some(icon) = catalog.icon_texture_with_native_size(
            ui.ctx(),
            0x3_0000_0000 | u64::from(container),
            container,
        )
    {
        let size = icon.size_vec2();
        let scale = (rect.width() / size.x).min(rect.height() / size.y);
        let rect = egui::Rect::from_center_size(rect.center(), size * scale);
        ui.painter()
            .rect_filled(rect, 2.0, crate::app::ui::package_icon_backdrop(ui));
        ui.painter().image(
            icon.id(),
            rect,
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    }
}

pub(super) fn thumbnail(ui: &mut egui::Ui, catalog: &Catalog, title: &Title, size: f32) {
    if title.icon_container.is_none() {
        return;
    }
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    paint_icon(ui, catalog, title, rect);
}

pub(super) fn row(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    title: Option<&Title>,
    selected: bool,
) -> egui::Response {
    const ICON_SIZE: f32 = 32.0;
    const PADDING: f32 = 4.0;
    let label = title.map_or("None", |title| title.name.as_str());
    let font = egui::TextStyle::Button.resolve(ui.style());
    let text = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font, egui::Color32::PLACEHOLDER);
    let description = title
        .map(|title| title.description.trim())
        .filter(|description| !description.is_empty());
    let secondary = description.map(|description| {
        ui.painter().layout_no_wrap(
            description.to_owned(),
            egui::TextStyle::Small.resolve(ui.style()),
            egui::Color32::PLACEHOLDER,
        )
    });
    let secondary_height = secondary.as_ref().map_or(0.0, |text| text.size().y + 2.0);
    let text_height = text.size().y + secondary_height;
    let text_width = text
        .size()
        .x
        .max(secondary.as_ref().map_or(0.0, |text| text.size().x));
    let text_offset = PADDING + ICON_SIZE + ui.spacing().icon_spacing;
    let size = egui::vec2(
        ui.available_width().max(text_offset + text_width + PADDING),
        ICON_SIZE.max(text_height) + PADDING * 2.0,
    );
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            ui.is_enabled(),
            selected,
            description.map_or_else(
                || label.to_owned(),
                |description| format!("{label}, {description}"),
            ),
        )
    });
    if response.gained_focus() {
        response.scroll_to_me(None);
    }
    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact_selectable(&response, selected);
        if selected || response.hovered() || response.has_focus() {
            ui.painter().rect(
                rect,
                visuals.corner_radius,
                visuals.weak_bg_fill,
                visuals.bg_stroke,
                egui::StrokeKind::Inside,
            );
        }
        if let Some(title) = title {
            let icon = egui::Rect::from_min_size(
                egui::pos2(rect.left() + PADDING, rect.center().y - ICON_SIZE / 2.0),
                egui::vec2(ICON_SIZE, ICON_SIZE),
            );
            paint_icon(ui, catalog, title, icon);
        }
        ui.painter().galley(
            egui::pos2(
                rect.left() + text_offset,
                rect.center().y - text_height / 2.0,
            ),
            text,
            visuals.text_color(),
        );
        if let Some(secondary) = secondary {
            ui.painter().galley(
                egui::pos2(
                    rect.left() + text_offset,
                    rect.center().y + text_height / 2.0 - secondary.size().y,
                ),
                secondary,
                if selected {
                    visuals.text_color()
                } else {
                    ui.visuals().weak_text_color()
                },
            );
        }
    }
    response
}

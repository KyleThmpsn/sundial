use super::*;

pub(super) fn catalog_button<'a>(
    ui: &egui::Ui,
    catalog: &Catalog,
    hash: u64,
    label: &'a str,
    icon_size: f32,
) -> egui::Button<'a> {
    catalog.icon_texture(ui.ctx(), hash).map_or_else(
        || egui::Button::new(label),
        |texture| {
            egui::Button::image_and_text((texture.id(), egui::vec2(icon_size, icon_size)), label)
        },
    )
}

pub(crate) struct CatalogPickerRow<'a> {
    pub(crate) hash: u64,
    pub(crate) primary: &'a str,
    pub(crate) primary_max_rows: usize,
    pub(crate) secondary: Option<&'a str>,
    pub(crate) icon_size: f32,
    pub(crate) row_height: f32,
    pub(crate) selected: bool,
}

pub(crate) fn draw_catalog_picker_row(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    row: CatalogPickerRow<'_>,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), row.row_height),
        egui::Sense::click(),
    );
    if !ui.is_rect_visible(rect) {
        return response;
    }

    let visuals = ui.style().interact_selectable(&response, row.selected);
    if row.selected || response.hovered() || response.has_focus() {
        ui.painter().rect(
            rect,
            visuals.corner_radius,
            visuals.weak_bg_fill,
            visuals.bg_stroke,
            egui::StrokeKind::Inside,
        );
    }

    const PADDING: f32 = 4.0;
    let icon_size = row.icon_size.min((row.row_height - PADDING * 2.0).max(0.0));
    let icon_rect = egui::Rect::from_min_size(
        egui::pos2(rect.left() + PADDING, rect.center().y - icon_size / 2.0),
        egui::vec2(icon_size, icon_size),
    );
    if let Some(texture) = catalog.icon_texture(ui.ctx(), row.hash) {
        ui.painter().image(
            texture.id(),
            icon_rect,
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    }

    let text_left = icon_rect.right() + ui.spacing().icon_spacing;
    let text_width = (rect.right() - PADDING - text_left).max(0.0);
    let primary_font = egui::TextStyle::Button.resolve(ui.style());
    let secondary_font = egui::TextStyle::Body.resolve(ui.style());
    let primary_galley = limited_line_galley(
        ui,
        row.primary,
        primary_font,
        visuals.text_color(),
        text_width,
        row.primary_max_rows,
    );
    let secondary_galley = row.secondary.map(|secondary| {
        single_line_galley(
            ui,
            secondary,
            secondary_font,
            visuals.text_color(),
            text_width,
        )
    });
    let content_height = primary_galley.size().y
        + secondary_galley
            .as_ref()
            .map_or(0.0, |galley| 1.0 + galley.size().y);
    let mut text_top = rect.center().y - content_height / 2.0;
    ui.painter().galley(
        egui::pos2(text_left, text_top),
        primary_galley,
        visuals.text_color(),
    );
    if let Some(secondary_galley) = secondary_galley {
        text_top += content_height - secondary_galley.size().y;
        ui.painter().galley(
            egui::pos2(text_left, text_top),
            secondary_galley,
            visuals.text_color(),
        );
    }
    response
}

fn limited_line_galley(
    ui: &egui::Ui,
    text: &str,
    font_id: egui::FontId,
    color: egui::Color32,
    max_width: f32,
    max_rows: usize,
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
    job.wrap.max_rows = max_rows.max(1);
    job.wrap.break_anywhere = true;
    ui.fonts(|fonts| fonts.layout_job(job))
}

pub(super) fn single_line_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(super) fn picker_secondary_text(type_name: &str, description: &str) -> String {
    match (type_name.trim(), description.trim()) {
        ("", description) => description.to_owned(),
        (type_name, "") => type_name.to_owned(),
        (type_name, description) => format!("{type_name} · {description}"),
    }
}

pub(crate) fn catalog_item_tooltip(
    response: egui::Response,
    catalog: &Catalog,
    hash: u64,
) -> egui::Response {
    if !catalog_item_tooltip_available(catalog, hash) {
        return response;
    }
    response.on_hover_ui(|ui| draw_catalog_item_tooltip(ui, catalog, hash))
}

pub(crate) fn catalog_item_tooltip_immediate(
    response: egui::Response,
    catalog: &Catalog,
    hash: u64,
) -> egui::Response {
    if response.hovered() && catalog_item_tooltip_available(catalog, hash) {
        response.show_tooltip_ui(|ui| draw_catalog_item_tooltip(ui, catalog, hash));
    }
    response
}

fn catalog_item_tooltip_available(catalog: &Catalog, hash: u64) -> bool {
    catalog.display_name(hash).is_some()
        || catalog
            .plug_type_name(hash)
            .is_some_and(|name| !name.trim().is_empty())
        || catalog
            .description(hash)
            .is_some_and(|description| !description.trim().is_empty())
        || catalog.icon_diagnostic(hash).is_some()
}

fn draw_catalog_item_tooltip(ui: &mut egui::Ui, catalog: &Catalog, hash: u64) {
    let name = catalog.display_name(hash);
    let type_name = catalog
        .plug_type_name(hash)
        .filter(|name| !name.trim().is_empty());
    let description = catalog
        .description(hash)
        .filter(|description| !description.trim().is_empty());
    let icon_diagnostic = catalog.icon_diagnostic(hash);
    ui.set_max_width(320.0);
    let icon = catalog.icon_texture(ui.ctx(), hash);
    ui.horizontal_top(|ui| {
        if let Some(icon) = icon {
            ui.add(egui::Image::new(&icon));
        }
        ui.vertical(|ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                if let Some(name) = name {
                    ui.label(egui::RichText::new(name).strong());
                }
                ui.horizontal_wrapped(|ui| {
                    if let Some(type_name) = type_name {
                        ui.label(type_name);
                        ui.label(egui::RichText::new("·").small().weak());
                    }
                    ui.label(
                        egui::RichText::new(format_hash_hex(hash))
                            .small()
                            .monospace()
                            .weak(),
                    );
                });
            });
            if let Some(description) = description {
                ui.separator();
                ui.label(description);
            }
            if let Some(diagnostic) = icon_diagnostic {
                ui.separator();
                ui.label(
                    egui::RichText::new(format!("Icon: {diagnostic}"))
                        .small()
                        .color(ui.visuals().warn_fg_color),
                );
            }
        });
    });
}

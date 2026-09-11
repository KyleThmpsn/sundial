//! Native mod artwork with vertically centered names.
use std::sync::Arc;

use crate::{catalog::Catalog, investment::seasonal::ArtifactMod};
use eframe::egui;

pub(super) const ACCENT: egui::Color32 = egui::Color32::from_rgb(25, 213, 219);
const PADDING: f32 = 12.0;

pub(super) struct Layout {
    name: String,
    title: Arc<egui::Galley>,
    icon_size: f32,
    pub height: f32,
}

pub(super) fn layout(
    ui: &egui::Ui,
    catalog: &Catalog,
    entry: &ArtifactMod,
    width: f32,
    owned: bool,
    available: bool,
) -> Layout {
    let name = catalog
        .package_item_name(entry.item_hash)
        .unwrap_or("Artifact Mod")
        .to_owned();
    let icon_size = if width >= 190.0 { 48.0 } else { 36.0 };
    let text_width = (width - PADDING * 2.0 - icon_size - 10.0).max(1.0);
    let title_color = if owned || available {
        egui::Color32::from_rgb(232, 242, 244)
    } else {
        egui::Color32::from_rgb(167, 189, 195)
    };
    let title = ui.fonts(|fonts| {
        let mut job = egui::text::LayoutJob::simple(
            name.clone(),
            egui::FontId::proportional(14.0),
            title_color,
            text_width,
        );
        job.wrap.max_rows = 3;
        fonts.layout_job(job)
    });
    let height = (PADDING * 2.0 + icon_size.max(title.size().y)).max(80.0);
    Layout {
        name,
        title,
        icon_size,
        height,
    }
}

pub(super) fn draw(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    entry: &ArtifactMod,
    rect: egui::Rect,
    content: &Layout,
    owned: bool,
    available: bool,
) -> egui::Response {
    let response = ui.interact(
        rect,
        ui.make_persistent_id(("artifact_mod", entry.sale_index)),
        egui::Sense::click(),
    );
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, available, owned, &content.name)
    });
    let painter = ui.painter_at(rect.intersect(ui.clip_rect()));
    let card = rect.shrink2(egui::vec2(6.0, 5.0));
    if owned {
        painter.rect(
            card,
            2.0,
            egui::Color32::from_rgb(24, 75, 82),
            egui::Stroke::new(1.0, egui::Color32::from_rgb(48, 112, 121)),
            egui::StrokeKind::Inside,
        );
    }
    if available && (response.hovered() || response.has_focus()) {
        painter.rect_stroke(
            card,
            2.0,
            egui::Stroke::new(1.0, ACCENT),
            egui::StrokeKind::Inside,
        );
    }
    let icon = egui::Rect::from_min_size(
        egui::pos2(
            rect.left() + PADDING,
            rect.center().y - content.icon_size / 2.0,
        ),
        egui::vec2(content.icon_size, content.icon_size),
    );
    painter.rect_filled(
        icon,
        3.0,
        if owned {
            egui::Color32::from_rgb(31, 109, 114)
        } else {
            egui::Color32::from_rgb(8, 29, 35)
        },
    );
    if let Some(texture) = catalog.icon_texture(ui.ctx(), entry.item_hash) {
        painter.image(
            texture.id(),
            icon,
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
            if owned {
                egui::Color32::from_rgb(61, 244, 244)
            } else if available {
                egui::Color32::from_gray(220)
            } else {
                egui::Color32::from_gray(135)
            },
        );
    }
    if owned {
        painter.rect_stroke(
            icon,
            3.0,
            egui::Stroke::new(1.0, ACCENT),
            egui::StrokeKind::Inside,
        );
    }
    painter.galley(
        egui::pos2(
            icon.right() + 10.0,
            rect.center().y - content.title.size().y / 2.0,
        ),
        content.title.clone(),
        egui::Color32::WHITE,
    );
    if available {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}

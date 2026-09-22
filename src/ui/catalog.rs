//! Reusable read-only catalog views. Callers own selection and authoring policy.
pub mod runtime;
pub mod scan;

mod list;
pub use list::BrowserList;
pub mod assets;
pub mod content;
pub mod labels;

use eframe::egui;

/// Fill a previously reserved toolbar slot without moving the body's layout cursor.
pub fn toolbar_status(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    text: impl Into<String>,
) -> egui::Response {
    ui.new_child(
        egui::UiBuilder::new()
            .id_salt("catalog-status")
            .max_rect(rect),
    )
    .add_sized(
        rect.size(),
        egui::Label::new(egui::RichText::new(text).weak())
            .halign(egui::Align::Min)
            .truncate(),
    )
}

pub fn search(ui: &mut egui::Ui, query: &mut String, opened: bool, width: f32, hint: &str) -> bool {
    let response = ui.add_sized(
        [width.max(80.0), ui.spacing().interact_size.y],
        egui::TextEdit::singleline(query).hint_text(hint),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, hint));
    if opened {
        response.request_focus();
    }
    let mut changed = response.changed();
    if ui.button("Clear").clicked() {
        query.clear();
        changed = true;
        response.request_focus();
    }
    changed
}

pub mod kinds;
use crate::investment::PerkSources;
pub fn draw_sources(ui: &mut egui::Ui, sources: &PerkSources, index: usize) {
    let entries = sources.get(index);
    ui.add_space(8.0);
    egui::CollapsingHeader::new(format!("Source Items and Plugs ({})", entries.len()))
        .id_salt(("effect-sources", index))
        .show(ui, |ui| {
            ui.small("These names belong to items and plugs that reference this effect. A source can carry other effects, and its description may cover more than this effect.");
            if entries.is_empty() {
                ui.weak("No item or plug references found in the catalog.");
            }
            for source in entries {
                ui.label(format!("{} · {:08X}", source.name, source.hash));
                if !source.type_name.is_empty() { ui.small(&source.type_name); }
            }
        });
}

/// Native kind support, shared by inspection and the local authoring canvas.
pub fn support_badge(ui: &mut egui::Ui, support: crate::sandbox_perk::nodes::Support) {
    use crate::sandbox_perk::nodes::Support;
    let visuals = ui.visuals();
    let color = match support {
        Support::Authorable => {
            if visuals.dark_mode {
                egui::Color32::from_rgb(126, 215, 133)
            } else {
                egui::Color32::from_rgb(25, 105, 40)
            }
        }
        Support::Readable => visuals.weak_text_color(),
        Support::Structural | Support::Unobserved => visuals.warn_fg_color,
    };
    ui.label(egui::RichText::new(support.label()).small().color(color))
        .on_hover_text(support.detail());
}

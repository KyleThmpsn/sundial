//! Five native-style artifact columns with perk tooltips and spend thresholds.
mod tile;

use super::{super::CollectionStateSnapshot, Edit};
use crate::{
    catalog::Catalog,
    investment::seasonal::{ArtifactMod, Definition},
};
use eframe::egui;

pub(super) fn draw(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    definition: &Definition,
    snapshot: &CollectionStateSnapshot,
    editable: bool,
    requested: &mut Option<Edit>,
) {
    let Ok(experience) = snapshot.seasonal_experience(definition) else {
        ui.label("Set a valid seasonal XP total before editing artifact mods.");
        return;
    };
    let mask = snapshot.artifact_mask(definition, true);
    let used = mask.count_ones();
    header(ui, experience.points_earned, used, editable, requested);
    if used > u32::from(experience.points_earned) {
        ui.colored_label(ui.visuals().warn_fg_color, "Owned mods exceed the earned point budget. Remove or reset mods, or increase seasonal XP.");
    }
    let columns: [Vec<&ArtifactMod>; 5] = std::array::from_fn(|column| {
        definition
            .mods
            .iter()
            .filter(|entry| entry.column() == column)
            .collect()
    });
    let width = ui.available_width() / 5.0;
    let layouts: [Vec<tile::Layout>; 5] = std::array::from_fn(|column| {
        columns[column]
            .iter()
            .map(|entry| {
                let owned = mask & entry.bit() != 0;
                let available = definition
                    .unlock(mask, entry.sale_index, experience.points_earned)
                    .is_ok();
                tile::layout(ui, catalog, entry, width, owned, available)
            })
            .collect()
    });
    let slot_height = layouts
        .iter()
        .flatten()
        .map(|layout| layout.height)
        .fold(80.0, f32::max);
    let height = slot_height * 5.0;
    let (bounds, _) =
        ui.allocate_exact_size(egui::vec2(width * 5.0, height + 44.0), egui::Sense::hover());
    for (column, entries) in columns.iter().enumerate() {
        let required = entries.first().map_or(0, |entry| entry.points_required());
        let open = used >= u32::from(required);
        let left = bounds.left() + column as f32 * width;
        let panel =
            egui::Rect::from_min_size(egui::pos2(left, bounds.top()), egui::vec2(width, height));
        draw_column_background(ui, panel, open);
        let mut top = panel.top();
        for row in 0..5 {
            if let (Some(entry), Some(content)) = (entries.get(row), layouts[column].get(row)) {
                let owned = mask & entry.bit() != 0;
                let eligibility =
                    definition.unlock(mask, entry.sale_index, experience.points_earned);
                let clickable = editable && (owned || eligibility.is_ok());
                let rect = egui::Rect::from_min_size(
                    egui::pos2(left, top),
                    egui::vec2(width, slot_height),
                );
                let response = tile::draw(ui, catalog, entry, rect, content, owned, clickable);
                if response.clicked() && clickable {
                    *requested = Some(Edit::Mod {
                        sale_index: entry.sale_index,
                        owned: !owned,
                    });
                }
                details(response, catalog, entry, owned, eligibility.as_ref().err());
            }
            top += slot_height;
        }
        threshold(ui, panel, required, used);
    }
}

fn draw_column_background(ui: &egui::Ui, panel: egui::Rect, open: bool) {
    ui.painter().rect_filled(
        panel,
        0.0,
        if open {
            egui::Color32::from_rgb(17, 48, 57)
        } else {
            egui::Color32::from_rgb(13, 38, 47)
        },
    );
    ui.painter().line_segment(
        [panel.right_top(), panel.right_bottom()],
        egui::Stroke::new(
            if open { 2.0 } else { 1.0 },
            if open {
                tile::ACCENT.gamma_multiply(0.65)
            } else {
                egui::Color32::from_rgb(33, 72, 82)
            },
        ),
    );
}

fn header(ui: &mut egui::Ui, earned: u16, used: u32, editable: bool, requested: &mut Option<Edit>) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Available").size(16.0));
        ui.label(
            egui::RichText::new(u32::from(earned).saturating_sub(used).to_string())
                .size(16.0)
                .strong(),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add_enabled(editable && used != 0, egui::Button::new("Reset Artifact"))
                .on_hover_text("Remove this character's mods and refund all spent points.")
                .clicked()
            {
                *requested = Some(Edit::Reset);
            }
        });
    });
    ui.add_space(2.0);
    let (line, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(line, 0.0, ui.visuals().text_color().gamma_multiply(0.5));
    ui.add_space(4.0);
}

fn threshold(ui: &mut egui::Ui, panel: egui::Rect, required: u16, used: u32) {
    let open = used >= u32::from(required);
    let bar = egui::Rect::from_min_size(
        egui::pos2(panel.left() + 6.0, panel.bottom() + 10.0),
        egui::vec2((panel.width() - 12.0).max(0.0), 6.0),
    );
    ui.painter().rect_filled(
        bar,
        0.0,
        if open {
            ui.visuals().text_color()
        } else {
            ui.visuals().text_color().gamma_multiply(0.4)
        },
    );
    ui.painter().text(
        egui::pos2(panel.right() - 9.0, bar.bottom() + 4.0),
        egui::Align2::RIGHT_TOP,
        required.to_string(),
        egui::FontId::proportional(20.0),
        ui.visuals().text_color(),
    );
    ui.interact(
        egui::Rect::from_min_max(bar.min, egui::pos2(panel.right(), panel.bottom() + 44.0)),
        ui.make_persistent_id(("artifact_threshold", required)),
        egui::Sense::hover(),
    )
    .on_hover_text(if required == 0 {
        "This column is available immediately.".into()
    } else {
        format!("Spend {required} artifact points to unlock this column. Currently spent: {used}.")
    });
}

fn details(
    response: egui::Response,
    catalog: &Catalog,
    entry: &ArtifactMod,
    owned: bool,
    reason: Option<&String>,
) {
    response.context_menu(|ui| {
        ui.strong("Mod Details");
        crate::app::inspector::draw_catalog_hash_link(ui, catalog, entry.item_hash, "Inspect Mod");
        crate::app::inspector::draw_catalog_hash_link(
            ui,
            catalog,
            entry.collectible_hash,
            "Inspect Collectible",
        );
        if let Some(flag) = catalog.unlock_flag_definition(usize::from(entry.flag_definition)) {
            crate::app::inspector::draw_catalog_hash_link(
                ui,
                catalog,
                flag.hash,
                "Inspect Unlock Flag",
            );
        }
    });
    response.on_hover_ui(|ui| {
        crate::app::item_editor::draw_catalog_item_tooltip(ui, catalog, entry.item_hash);
        ui.separator();
        ui.label(if owned {
            "Click to remove this mod and refund one point."
        } else {
            reason.map_or("Click to unlock this mod for one point.", String::as_str)
        });
    });
}

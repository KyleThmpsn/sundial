//! Shared metadata presentation used by the focused inspectors.

use eframe::egui;

use crate::{
    catalog::{Catalog, ObjectiveOwnerKind, ProgressionContextKind, ProgressionScope},
    hash::{format_hash_decimal, format_hash_hex, format_hash_hex_and_decimal, parse_hash_hex},
};

use super::request_definition;
use crate::app::ui::TABLE_CELL_HEIGHT;

pub(in crate::app) fn hash_detail_field(
    ui: &mut egui::Ui,
    label: &str,
    value: impl Into<String>,
    monospace: bool,
) {
    ui.label(metadata_label_text(ui, label));
    let value = value.into();
    let absent = value.starts_with('<') && value.ends_with('>');
    let parsed_hash = label
        .to_ascii_lowercase()
        .contains("hash")
        .then(|| parse_hash_hex(hash_hex_component(&value)))
        .flatten()
        .filter(|hash| *hash != 0);
    let text = egui::RichText::new(&value);
    let text = if absent { text.weak().italics() } else { text };
    let text = if monospace { text.monospace() } else { text };
    let text = if parsed_hash.is_some() || label == "Instance ID" {
        text.weak()
    } else {
        text
    };
    if let Some(parsed_hash) = parsed_hash {
        let response = ui
            .add(egui::Button::new(text).frame(false))
            .on_hover_text(format!("Open details for 0x{parsed_hash:08X}"));
        if response.clicked() {
            request_definition(ui.ctx(), parsed_hash);
        }
    } else {
        ui.add(egui::Label::new(text).wrap());
    }
    ui.end_row();
}

pub(in crate::app) fn draw_hash_wrapped_detail(
    ui: &mut egui::Ui,
    label: &str,
    value: impl Into<String>,
) {
    ui.label(metadata_label_text(ui, label));
    let value = value.into();
    let absent = value.starts_with('<') && value.ends_with('>');
    let text = egui::RichText::new(value);
    ui.add(egui::Label::new(if absent { text.weak().italics() } else { text }).wrap());
}

pub(in crate::app) fn draw_hash_hex_cell(ui: &mut egui::Ui, width: f32, hash: Option<u64>) {
    ui.allocate_ui_with_layout(
        egui::vec2(width, TABLE_CELL_HEIGHT),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_size(egui::vec2(width, TABLE_CELL_HEIGHT));
            if let Some(hash) = hash.filter(|hash| *hash != 0) {
                draw_hash_link(ui, hash, format_hash_hex(hash));
            } else {
                ui.label(egui::RichText::new("-").weak());
            }
        },
    );
}

pub(in crate::app) fn draw_metadata_paths(ui: &mut egui::Ui, paths: &[Vec<String>]) {
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(format!("Package paths ({})", paths.len()))
            .strong()
            .small(),
    );
    if paths.is_empty() {
        ui.label(egui::RichText::new("<none>").weak().monospace());
        return;
    }
    for (index, path) in paths.iter().enumerate() {
        let path = metadata_path_text(path);
        ui.add(
            egui::Label::new(egui::RichText::new(format!("{}. {path}", index + 1)).monospace())
                .wrap(),
        );
    }
}

pub(in crate::app) fn metadata_path_text(path: &[String]) -> String {
    if path.is_empty() {
        "<empty path>".into()
    } else {
        path.iter()
            .rev()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(" > ")
    }
}

pub(in crate::app) fn metadata_section<R>(
    ui: &mut egui::Ui,
    title: &str,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    ui.label(egui::RichText::new(title).heading().strong());
    ui.add_space(4.0);
    add_contents(ui)
}

pub(in crate::app) fn hash_metadata_section(
    ui: &mut egui::Ui,
    title: &str,
    default_open: bool,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    ui.add_space(6.0);
    egui::CollapsingHeader::new(egui::RichText::new(title).strong())
        .id_salt(("hash_metadata_section", title))
        .default_open(default_open)
        .show(ui, add_contents);
}

pub(in crate::app) fn metadata_subsection<R>(
    ui: &mut egui::Ui,
    title: &str,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    ui.label(egui::RichText::new(title).strong());
    ui.add_space(2.0);
    add_contents(ui)
}

pub(in crate::app) fn metadata_field(
    ui: &mut egui::Ui,
    label: &'static str,
    value: impl Into<String>,
    monospace: bool,
) {
    ui.label(metadata_label_text(ui, label));
    let value = value.into();
    let absent = value.starts_with('<') && value.ends_with('>');
    let text = egui::RichText::new(&value);
    let text = if absent { text.weak().italics() } else { text };
    let text = if monospace { text.monospace() } else { text };
    ui.add(egui::Label::new(text).wrap());
    ui.end_row();
}

pub(in crate::app) fn hash_hex_and_decimal_field(
    ui: &mut egui::Ui,
    label: &'static str,
    hash: u64,
) {
    ui.label(metadata_label_text(ui, label));
    if hash == 0 || hash == u64::from(u32::MAX) {
        ui.label(egui::RichText::new("<not present>").weak().italics());
    } else {
        draw_hash_link(ui, hash, format_hash_hex_and_decimal(hash));
    }
    ui.end_row();
}

pub(in crate::app) fn catalog_hash_hex_and_decimal_field(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    label: &'static str,
    hash: u64,
) {
    ui.label(metadata_label_text(ui, label));
    if hash == 0 || hash == u64::from(u32::MAX) {
        ui.label(egui::RichText::new("<not present>").weak().italics());
    } else {
        draw_catalog_hash_link(ui, catalog, hash, format_hash_hex_and_decimal(hash));
    }
    ui.end_row();
}

pub(in crate::app) const fn item_class_type_label(class_type: u64) -> &'static str {
    match class_type {
        0 => "Titan",
        1 => "Hunter",
        2 => "Warlock",
        3 => "Any",
        _ => "Unknown",
    }
}

pub(in crate::app) fn metadata_label_text(
    ui: &egui::Ui,
    label: impl Into<String>,
) -> egui::RichText {
    egui::RichText::new(label.into()).color(ui.visuals().text_color().gamma_multiply(0.92))
}

pub(in crate::app) fn draw_hash_link(
    ui: &mut egui::Ui,
    hash: u64,
    text: impl Into<String>,
) -> egui::Response {
    draw_hash_button(
        ui,
        hash,
        egui::RichText::new(text.into()).monospace().weak(),
    )
    .on_hover_text(format!("Open details for {}", format_hash_hex(hash)))
}

pub(in crate::app) fn draw_catalog_hash_link(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    hash: u64,
    text: impl Into<String>,
) -> egui::Response {
    let response = draw_hash_button(
        ui,
        hash,
        egui::RichText::new(text.into()).monospace().weak(),
    );
    catalog_hash_tooltip(response, catalog, hash)
}

pub(in crate::app) fn draw_named_catalog_hash_link(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    hash: u64,
    name: impl Into<String>,
) -> egui::Response {
    let color = ui.visuals().hyperlink_color;
    let response = draw_hash_button(
        ui,
        hash,
        crate::app::ui::destiny_text(ui, name).color(color),
    );
    catalog_hash_tooltip(response, catalog, hash)
}

fn draw_hash_button(ui: &mut egui::Ui, hash: u64, text: egui::RichText) -> egui::Response {
    let response = ui.add(egui::Button::new(text).frame(false));
    if response.clicked() {
        request_definition(ui.ctx(), hash);
    }
    response
}

fn catalog_hash_tooltip(response: egui::Response, catalog: &Catalog, hash: u64) -> egui::Response {
    if crate::app::item_editor::catalog_item_tooltip_available(catalog, hash) {
        crate::app::item_editor::catalog_item_tooltip(response, catalog, hash)
    } else {
        response.on_hover_text(format!("Open details for {}", format_hash_hex(hash)))
    }
}

pub(in crate::app) fn draw_hash_hex_and_decimal_cells(ui: &mut egui::Ui, hash: u64) {
    draw_hash_link(ui, hash, format_hash_hex(hash));
    ui.monospace(format_hash_decimal(hash));
}

pub(in crate::app) fn hash_hex_component(value: &str) -> &str {
    value.split_once(" · ").map_or(value, |(hex, _)| hex)
}

pub(in crate::app) fn metadata_text(value: &str) -> &str {
    if value.is_empty() { "<empty>" } else { value }
}

pub(in crate::app) const fn yes_no(value: bool) -> &'static str {
    if value { "Yes" } else { "No" }
}

pub(in crate::app) const fn objective_owner_kind_label(kind: ObjectiveOwnerKind) -> &'static str {
    match kind {
        ObjectiveOwnerKind::InventoryItem => "Inventory item",
        ObjectiveOwnerKind::Milestone => "Milestone",
        ObjectiveOwnerKind::Metric => "Metric",
        ObjectiveOwnerKind::Record => "Record",
        ObjectiveOwnerKind::PresentationNode => "Presentation node",
    }
}

pub(in crate::app) const fn progression_context_kind_label(
    kind: ProgressionContextKind,
) -> &'static str {
    match kind {
        ProgressionContextKind::InventoryItem => "Inventory item",
        ProgressionContextKind::Collectible => "Collectible",
        ProgressionContextKind::Record => "Record",
        ProgressionContextKind::Objective => "Objective",
        ProgressionContextKind::PresentationNode => "Presentation node",
        ProgressionContextKind::Activity => "Activity",
        ProgressionContextKind::ActivityAvailability => "Activity availability",
        ProgressionContextKind::Location => "Location",
        ProgressionContextKind::LocationRelease => "Location release",
        ProgressionContextKind::ExpressionMapping => "Expression mapping",
        ProgressionContextKind::Progression => "Progression",
        ProgressionContextKind::Achievement => "Achievement",
        ProgressionContextKind::Requirement => "Requirement",
        ProgressionContextKind::ValueCounter => "Value Counter",
        ProgressionContextKind::PackageExpression => "Package Expression",
    }
}

pub(in crate::app) const fn progression_scope_label(scope: ProgressionScope) -> &'static str {
    match scope {
        ProgressionScope::Account => "Account",
        ProgressionScope::Character => "Character",
        ProgressionScope::Unreplicated => "Unreplicated",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_paths_render_root_first() {
        let package_path = vec![
            "Aspirant Suit".to_owned(),
            "Leveling".to_owned(),
            "Warlock".to_owned(),
            "Armor".to_owned(),
            "Items".to_owned(),
        ];

        assert_eq!(
            metadata_path_text(&package_path),
            "Items > Armor > Warlock > Leveling > Aspirant Suit"
        );
        assert_eq!(metadata_path_text(&[]), "<empty path>");
    }

    #[test]
    fn parser_accepts_the_hex_component_of_a_hash_display_pair() {
        let displayed = "0x574E0A2A · 1464732202";
        assert_eq!(hash_hex_component(displayed), "0x574E0A2A");
        assert_eq!(
            parse_hash_hex(hash_hex_component(displayed)),
            Some(0x574E_0A2A)
        );
        assert_eq!(parse_hash_hex("1464732202"), None);
        assert_eq!(parse_hash_hex("0X574e0a2a"), Some(0x574E_0A2A));
    }
}

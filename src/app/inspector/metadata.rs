//! Shared metadata presentation used by the focused inspectors.

use eframe::egui;

use crate::{
    catalog::{Catalog, ObjectiveOwnerKind, ProgressionContextKind, ProgressionScope},
    hash::{format_hash_decimal, format_hash_hex, format_hash_hex_and_decimal, parse_hash_hex},
};

use super::{UNNAMED, request_definition};

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
    ui.strong(title);
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

pub(in crate::app) fn draw_catalog_hash_link(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    hash: u64,
    text: impl Into<String>,
) -> egui::Response {
    let name = catalog
        .display_name(hash)
        .or_else(|| catalog.package_item_name(hash));
    let response = draw_hash_button(
        ui,
        hash,
        egui::RichText::new(text.into()).monospace().weak(),
        name,
    );
    catalog_hash_tooltip(response, catalog, hash)
}

pub(in crate::app) fn draw_named_catalog_hash_link(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    hash: u64,
    name: impl Into<String>,
) -> egui::Response {
    let name = name.into();
    let color = ui.visuals().hyperlink_color;
    let copied_name =
        Some(name.as_str()).filter(|name| *name != UNNAMED && !name.trim().is_empty());
    let response = draw_hash_button(
        ui,
        hash,
        crate::app::ui::destiny_text(ui, &name).color(color),
        copied_name,
    );
    catalog_hash_tooltip(response, catalog, hash)
}

/// A frameless hash link that opens its definition, with a context menu for copying it.
fn draw_hash_button(
    ui: &mut egui::Ui,
    hash: u64,
    text: egui::RichText,
    name: Option<&str>,
) -> egui::Response {
    let response = ui.add(egui::Button::new(text).frame(false));
    if response.clicked() {
        request_definition(ui.ctx(), hash);
    }
    response.context_menu(|ui| hash_context_menu(ui, hash, name));
    response
}

fn hash_context_menu(ui: &mut egui::Ui, hash: u64, name: Option<&str>) {
    inspect_menu_button(ui, "Open", hash);
    copy_menu_buttons(ui, hash, name);
}

/// Right-click actions for a definition shown as plain text: open it and copy its hash.
pub(in crate::app) fn definition_context_menu(response: &egui::Response, label: &str, hash: u64) {
    if hash == 0 || hash == u64::from(u32::MAX) {
        return;
    }
    response.context_menu(|ui| {
        inspect_menu_button(ui, label, hash);
        copy_menu_buttons(ui, hash, None);
    });
}

/// A menu button that opens a definition.
pub(in crate::app) fn inspect_menu_button(ui: &mut egui::Ui, label: &str, hash: u64) {
    if ui.button(label).clicked() {
        request_definition(ui.ctx(), hash);
        ui.close_menu();
    }
}

/// Menu buttons that copy a hash, and the name when there is one.
pub(in crate::app) fn copy_menu_buttons(ui: &mut egui::Ui, hash: u64, name: Option<&str>) {
    if ui.button("Copy Hash").clicked() {
        ui.ctx().copy_text(format_hash_hex(hash));
        ui.close_menu();
    }
    if ui.button("Copy Decimal").clicked() {
        ui.ctx().copy_text(format_hash_decimal(hash));
        ui.close_menu();
    }
    if let Some(name) = name
        && ui.button("Copy Name").clicked()
    {
        ui.ctx().copy_text(name.to_owned());
        ui.close_menu();
    }
}

fn catalog_hash_tooltip(response: egui::Response, catalog: &Catalog, hash: u64) -> egui::Response {
    if crate::app::item_editor::catalog_item_tooltip_available(catalog, hash) {
        crate::app::item_editor::catalog_item_tooltip(response, catalog, hash)
    } else {
        response.on_hover_text(format!("Open details for {}", format_hash_hex(hash)))
    }
}

pub(in crate::app) fn hash_hex_component(value: &str) -> &str {
    value.split_once(" · ").map_or(value, |(hex, _)| hex)
}

/// Reads a definition hash in any form the inspector shows or a user pastes: `0x` hexadecimal,
/// bare hexadecimal with a letter or exactly eight digits, decimal, or a "0x… · decimal" pair.
pub(in crate::app) fn parse_hash_text(text: &str) -> Option<u64> {
    let text = hash_hex_component(text.trim()).trim();
    let digits = |test: fn(&u8) -> bool| !text.is_empty() && text.bytes().all(|byte| test(&byte));
    let bare_hex = text.len() <= 16
        && digits(u8::is_ascii_hexdigit)
        && (text.len() == 8 || text.bytes().any(|byte| byte.is_ascii_alphabetic()));
    let parsed = if text.starts_with("0x") || text.starts_with("0X") {
        parse_hash_hex(text)
    } else if bare_hex {
        u64::from_str_radix(text, 16).ok()
    } else if digits(u8::is_ascii_digit) {
        text.parse::<u32>().ok().map(u64::from)
    } else {
        None
    };
    parsed.filter(|hash| *hash != 0)
}

pub(in crate::app) fn metadata_text(value: &str) -> &str {
    if value.is_empty() { "<empty>" } else { value }
}

pub(in crate::app) const fn yes_no(value: bool) -> &'static str {
    if value { "Yes" } else { "No" }
}

pub(in crate::app) const fn objective_owner_kind_label(kind: ObjectiveOwnerKind) -> &'static str {
    match kind {
        ObjectiveOwnerKind::InventoryItem => "Inventory Item",
        ObjectiveOwnerKind::Milestone => "Milestone",
        ObjectiveOwnerKind::Metric => "Metric",
        ObjectiveOwnerKind::Record => "Record",
        ObjectiveOwnerKind::PresentationNode => "Presentation Node",
    }
}

pub(in crate::app) const fn progression_context_kind_label(
    kind: ProgressionContextKind,
) -> &'static str {
    match kind {
        ProgressionContextKind::InventoryItem => "Inventory Item",
        ProgressionContextKind::Collectible => "Collectible",
        ProgressionContextKind::Record => "Record",
        ProgressionContextKind::Objective => "Objective",
        ProgressionContextKind::PresentationNode => "Presentation Node",
        ProgressionContextKind::Activity => "Activity",
        ProgressionContextKind::ActivityAvailability => "Activity Availability",
        ProgressionContextKind::Location => "Location",
        ProgressionContextKind::LocationRelease => "Location Release",
        ProgressionContextKind::ExpressionMapping => "Expression Mapping",
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
    fn typed_hashes_are_read_in_every_displayed_form() {
        for text in [
            "0x574E0A2A",
            " 0X574e0a2a ",
            "574E0A2A",
            "574e0a2a",
            "1464732202",
            "0x574E0A2A · 1464732202",
        ] {
            assert_eq!(parse_hash_text(text), Some(0x574E_0A2A), "{text}");
        }
        assert_eq!(parse_hash_text("12345678"), Some(0x1234_5678));
        assert_eq!(parse_hash_text("ace"), Some(0xACE));
        assert_eq!(parse_hash_text("1234"), Some(1234));
        for text in ["", "0", "0x0", "ace rifle", "4294967296", "0xnope", "x12"] {
            assert_eq!(parse_hash_text(text), None, "{text}");
        }
    }
}

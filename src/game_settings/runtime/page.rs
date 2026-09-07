//! Compact runtime groups; advanced activity forms stay collapsed by default.

use super::{
    fields::{FIELDS, Field, Kind},
    optional_value, set_field, write_value,
};
use eframe::egui;
use serde_json::Value;

pub(in crate::game_settings) fn draw(
    ui: &mut egui::Ui,
    document: &mut Value,
    json_account: bool,
) -> bool {
    if !super::available(document) {
        return false;
    }
    ui.horizontal(|ui| {
        ui.strong("Sunrise Runtime Settings");
        crate::ui_help::info(
            ui,
            "Omitted settings use Sunrise's runtime defaults. Opening this page does not add them.",
        );
    });
    ui.label("Save, then fully restart Destiny 2 to apply.");
    let mut changed = false;
    for group in [
        "Profile & Catalysts",
        "Presentation",
        "Activities & Scripting",
        "Advanced Runtime",
    ] {
        egui::CollapsingHeader::new(group).default_open(group == "Profile & Catalysts").show(ui, |ui| {
            for &field in FIELDS.iter().filter(|field| field.group == group) {
                ui.push_id(field.path, |ui| {
                    ui.add_enabled_ui(json_account || !field.account_owned(), |ui| {
                        changed |= draw_field(ui, document, field, json_account);
                    }).response.on_disabled_hover_text("This field belongs to a JSON account; the active SQLite account is unchanged.");
                });
            }
        });
    }
    egui::CollapsingHeader::new("Activity Destinations (Advanced)").show(ui, |ui| {
        changed |= super::activity_page::draw(ui, document);
    });
    changed
}

pub(super) fn draw_field(
    ui: &mut egui::Ui,
    document: &mut Value,
    field: Field,
    json_account: bool,
) -> bool {
    let mut value = match optional_value(document, field.path) {
        Ok(value) => value.cloned().unwrap_or_else(|| field.default_value()),
        Err(error) => {
            ui.colored_label(ui.visuals().error_fg_color, error);
            return false;
        }
    };
    let text_field = matches!(field.kind, Kind::Text(..) | Kind::Ipv4(_));
    if let Err(error) = field.validate(&value) {
        ui.colored_label(ui.visuals().error_fg_color, error);
        if !text_field || !value.is_string() {
            return false;
        }
    }
    let text_edit_id = ui.make_persistent_id(("runtime-field", field.path));
    let mut requested = false;
    ui.horizontal_wrapped(|ui| {
        match field.kind {
            Kind::Bool(_) => {
                let mut enabled = value.as_bool().unwrap_or(false);
                requested = ui
                    .checkbox(&mut enabled, field.label)
                    .on_hover_text(field.help)
                    .changed();
                if requested {
                    value = Value::Bool(enabled);
                }
            }
            Kind::UInt(_, min, max) => {
                ui.label(field.label);
                let mut number = value.as_u64().unwrap_or(min);
                requested = ui
                    .add(egui::DragValue::new(&mut number).range(min..=max))
                    .changed();
                value = Value::from(number);
            }
            Kind::Text(_, _, _) | Kind::Ipv4(_) => {
                ui.label(field.label);
                let mut text = value.as_str().unwrap_or_default().to_owned();
                requested = ui
                    .add(egui::TextEdit::singleline(&mut text).id(text_edit_id))
                    .changed();
                value = Value::from(text);
            }
            Kind::Choice(choices) => {
                ui.label(field.label).on_hover_text(field.help);
                let mut selected = value.as_str().unwrap_or(choices[0]).to_owned();
                egui::ComboBox::from_id_salt(field.path)
                    .selected_text(&selected)
                    .show_ui(ui, |ui| {
                        for choice in choices {
                            requested |= ui
                                .selectable_value(&mut selected, (*choice).to_owned(), *choice)
                                .changed();
                        }
                    });
                if requested {
                    value = Value::from(selected);
                }
            }
        }
        crate::ui_help::info(ui, field.help);
        if document.pointer(field.path).is_none() {
            ui.label(egui::RichText::new("Runtime Default").small().weak());
        }
    });
    let error_id = ui.make_persistent_id(("runtime-error", field.path));
    if requested {
        // Keep incomplete text in the document so dirty state, undo and reload see it.
        // The save boundary validates the complete value; no hidden draft can outlive a file.
        let result = if text_field {
            write_value(document, field.path, value).map(|()| true)
        } else {
            set_field(document, field.path, value, json_account)
        };
        match result {
            Ok(changed) => {
                ui.data_mut(|data| data.remove::<String>(error_id));
                return changed;
            }
            Err(error) => ui.data_mut(|data| data.insert_temp(error_id, error)),
        }
    }
    if let Some(error) = ui.data_mut(|data| data.get_temp::<String>(error_id)) {
        ui.colored_label(ui.visuals().warn_fg_color, error);
    }
    false
}

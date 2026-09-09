//! Per-character Sunrise preferences; metadata rules live with the account adapter.
use crate::{
    hash::parse_unsigned_value,
    persistence::{json_account::character_runtime::validate_details, json_fields::optional_value},
};
use eframe::egui;
use serde_json::Value;

pub(super) fn validate(document: &Value) -> Result<(), String> {
    let Some(value) = optional_value(document, "/state/characters")? else {
        return Ok(());
    };
    let rows = value
        .as_array()
        .ok_or("state.characters must be an array")?;
    for (index, row) in rows.iter().enumerate() {
        let character = row
            .as_object()
            .ok_or_else(|| format!("Character {} must be an object", index + 1))?;
        validate_details(character).map_err(|error| format!("Character {}: {error}", index + 1))?;
    }
    Ok(())
}

pub(super) fn draw(ui: &mut egui::Ui, document: &mut Value, json_account: bool) -> bool {
    let mut changed = false;
    egui::CollapsingHeader::new("Character Runtime Details").show(ui, |ui| {
        if !json_account {
            ui.label("Character runtime details require a JSON account.");
            return;
        }
        ui.label("Per-character Sunrise fields. Destination is the last orbited destination hash.");
        let Some(rows) = document
            .pointer_mut("/state/characters")
            .and_then(Value::as_array_mut)
        else {
            ui.label("No character rows available.");
            return;
        };
        for (index, row) in rows.iter_mut().enumerate() {
            ui.push_id(("character-runtime", index), |ui| {
                egui::CollapsingHeader::new(format!("Character {}", index + 1)).show(ui, |ui| {
                    let Some(object) = row.as_object_mut() else {
                        ui.label("Invalid character row");
                        return;
                    };
                    for (key, label) in [
                        ("preview_available", "Preview available"),
                        ("content_bypass", "Content bypass"),
                    ] {
                        let mut enabled = object.get(key).and_then(Value::as_bool).unwrap_or(false);
                        if ui.checkbox(&mut enabled, label).changed() {
                            object.insert(key.into(), Value::Bool(enabled));
                            changed = true;
                        }
                    }
                    ui.horizontal(|ui| {
                        ui.label("Appearance value");
                        let mut number = object
                            .get("appearance_value")
                            .and_then(Value::as_f64)
                            .unwrap_or(0.0);
                        if ui
                            .add(
                                egui::DragValue::new(&mut number)
                                    .speed(0.01)
                                    .range(-(f32::MAX as f64)..=f32::MAX as f64),
                            )
                            .changed()
                        {
                            object.insert("appearance_value".into(), Value::from(number));
                            changed = true;
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("Last orbited destination");
                        let value = object.get("last_orbited_destination");
                        let mut text = value
                            .and_then(Value::as_str)
                            .map(str::to_owned)
                            .unwrap_or_else(|| {
                                format!(
                                    "0x{:08X}",
                                    value.and_then(parse_unsigned_value).unwrap_or(0)
                                )
                            });
                        if ui.text_edit_singleline(&mut text).changed() {
                            object.insert("last_orbited_destination".into(), Value::from(text));
                            changed = true;
                        }
                    });
                });
            });
        }
    });
    changed
}

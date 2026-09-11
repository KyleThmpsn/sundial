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

pub(super) fn draw(ui: &mut egui::Ui, document: &mut Value, account_available: bool) -> bool {
    let mut changed = false;
    if !account_available {
        ui.label("The active account is unavailable.");
        return false;
    }
    let Some(rows) = document
        .pointer_mut("/state/characters")
        .and_then(Value::as_array_mut)
    else {
        ui.label("No character rows available.");
        return false;
    };
    if rows.is_empty() {
        ui.label("No character rows available.");
        return false;
    }
    let id = ui.make_persistent_id("sunrise_character");
    let mut selected = ui
        .data_mut(|data| data.get_temp::<usize>(id))
        .unwrap_or(0)
        .min(rows.len() - 1);
    ui.horizontal_wrapped(|ui| {
        for index in 0..rows.len() {
            ui.selectable_value(&mut selected, index, format!("Character {}", index + 1));
        }
    });
    ui.data_mut(|data| data.insert_temp(id, selected));
    ui.add_space(8.0);
    let row = &mut rows[selected];
    ui.push_id(("character-runtime", selected), |ui| {
        let Some(object) = row.as_object_mut() else {
            ui.label("Invalid character row");
            return;
        };
        for (key, label) in [
            ("preview_available", "Preview Available"),
            ("content_bypass", "Content Bypass"),
        ] {
            let mut enabled = object.get(key).and_then(Value::as_bool).unwrap_or(false);
            if ui.checkbox(&mut enabled, label).changed() {
                object.insert(key.into(), Value::Bool(enabled));
                changed = true;
            }
        }
        ui.horizontal(|ui| {
            ui.label("Appearance Value");
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
            ui.label("Last Orbited Destination");
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
    changed
}

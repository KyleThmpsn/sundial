//! Authored server ownership table, including Sunrise's bounded names and application IDs.
use crate::persistence::json_fields::{optional_value, write_value};
use eframe::egui;
use serde_json::{Map, Value, json};

const PATH: &str = "/server/entitlements";
const MAX_ENTITLEMENTS: usize = 128;
const MAX_NAME_BYTES: usize = 31;
const OWNERSHIP: &[&str] = &["none", "handle", "application"];

pub(crate) fn validate(document: &Value) -> Result<(), String> {
    validate_rows(document, false)
}

#[cfg(feature = "sqlite-account")]
pub(crate) fn validate_native(document: &Value) -> Result<(), String> {
    validate_rows(document, true)
}

fn validate_rows(document: &Value, native: bool) -> Result<(), String> {
    let Some(value) = optional_value(document, PATH)? else {
        return Ok(());
    };
    let rows = value
        .as_array()
        .ok_or("Server entitlements must be an array")?;
    if rows.len() > MAX_ENTITLEMENTS {
        return Err(format!(
            "At most {MAX_ENTITLEMENTS} server entitlements are supported"
        ));
    }
    let mut names = std::collections::HashSet::new();
    for (index, row) in rows.iter().enumerate() {
        let name = row.get("name").and_then(Value::as_str).unwrap_or_default();
        let owned = row.get("owned").and_then(Value::as_str).unwrap_or("none");
        if name.is_empty()
            || name.len() > MAX_NAME_BYTES
            || !name.bytes().all(|byte| {
                (32..=126).contains(&byte) && (native || (byte != b'\\' && byte != b'"'))
            })
            || !names.insert(name)
        {
            return Err(format!(
                "Entitlement {} needs a unique name of 1–{MAX_NAME_BYTES} printable ASCII characters",
                index + 1
            ));
        }
        if !OWNERSHIP.contains(&owned)
            || row.get("owned").is_some_and(|value| !value.is_string())
            || (owned == "application"
                && (!name.bytes().all(|byte| byte.is_ascii_digit())
                    || name.parse::<u32>().is_err()))
        {
            return Err(format!(
                "Entitlement {} has invalid ownership; application ownership requires a decimal 32-bit application ID",
                index + 1
            ));
        }
    }
    Ok(())
}

pub(super) fn draw(ui: &mut egui::Ui, document: &mut Value) -> bool {
    egui::CollapsingHeader::new("Server Entitlements")
        .show(ui, |ui| draw_table(ui, document))
        .body_returned
        .unwrap_or(false)
}

fn draw_table(ui: &mut egui::Ui, document: &mut Value) -> bool {
    ui.label("Ownership may use a manifest handle or a numeric application ID. Save validates the complete table.");
    match optional_value(document, PATH) {
        Err(error) => {
            ui.colored_label(ui.visuals().error_fg_color, error);
            return false;
        }
        Ok(None) => {
            ui.label("Using Sunrise's bundled entitlements.");
            if ui.button("Customize bundled entitlements").clicked() {
                let defaults = serde_json::from_str(include_str!("entitlements.json"))
                    .expect("bundled entitlements");
                return write_value(document, PATH, defaults).is_ok();
            }
            return false;
        }
        Ok(Some(value)) if !value.is_array() => {
            ui.colored_label(ui.visuals().error_fg_color, "Entitlements must be an array");
            return false;
        }
        _ => {}
    }
    let rows = document
        .pointer_mut(PATH)
        .and_then(Value::as_array_mut)
        .expect("checked array");
    let mut changed = false;
    let mut remove = None;
    for (index, row) in rows.iter_mut().enumerate() {
        ui.push_id(("entitlement", index), |ui| {
            ui.horizontal(|ui| {
                if let Some(object) = row.as_object_mut() {
                    changed |= draw_row(ui, object);
                } else {
                    ui.label("Invalid entitlement row");
                }
                if ui.small_button("Remove").clicked() {
                    remove = Some(index);
                }
            });
        });
    }
    if let Some(index) = remove {
        rows.remove(index);
        changed = true;
    }
    if ui
        .add_enabled(
            rows.len() < MAX_ENTITLEMENTS,
            egui::Button::new("Add entitlement"),
        )
        .clicked()
    {
        rows.push(json!({"name": "", "owned": "none"}));
        changed = true;
    }
    changed
}

fn draw_row(ui: &mut egui::Ui, object: &mut Map<String, Value>) -> bool {
    let mut changed = false;
    let mut name = object
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    if ui
        .text_edit_singleline(&mut name)
        .on_hover_text("Entitlement name")
        .changed()
    {
        object.insert("name".into(), Value::from(name));
        changed = true;
    }
    let mut owned = object
        .get("owned")
        .and_then(Value::as_str)
        .unwrap_or("none")
        .to_owned();
    egui::ComboBox::from_id_salt("owned")
        .selected_text(&owned)
        .show_ui(ui, |ui| {
            for &choice in OWNERSHIP {
                if ui
                    .selectable_value(&mut owned, choice.to_owned(), choice)
                    .changed()
                {
                    object.insert("owned".into(), Value::from(owned.clone()));
                    changed = true;
                }
            }
        });
    changed
}

//! Shared staged object forms: rendering never normalizes or writes the source document.

use eframe::egui;
use serde_json::Value;

#[derive(Clone, Copy)]
pub(crate) enum Input {
    Text,
    Integer(i64, i64),
    Unsigned(u64),
    Bool,
}
pub(crate) struct Field {
    pub key: &'static str,
    pub label: &'static str,
    pub input: Input,
    pub optional: bool,
}
#[derive(Clone)]
struct Draft {
    source: Value,
    value: Value,
}
pub(crate) enum Action {
    Apply(Value),
    Remove,
}

pub(crate) fn draw(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash,
    source: &Value,
    fields: &[Field],
    removable: bool,
    validate: impl Fn(&Value) -> Result<(), String>,
) -> Option<Action> {
    let id = ui.make_persistent_id(id);
    let mut draft = ui
        .data_mut(|data| data.get_temp::<Draft>(id))
        .filter(|draft| draft.source == *source)
        .unwrap_or_else(|| Draft {
            source: source.clone(),
            value: source.clone(),
        });
    let Some(object) = draft.value.as_object_mut() else {
        ui.colored_label(
            ui.visuals().error_fg_color,
            "This row must be an object; use Raw JSON to inspect it.",
        );
        return None;
    };
    let stacked = ui.available_width() < 480.0;
    let input_width = if stacked {
        ui.available_width().min(280.0)
    } else {
        240.0
    };
    egui::Grid::new(id.with("fields"))
        .num_columns(if stacked { 1 } else { 2 })
        .spacing([16.0, 8.0])
        .show(ui, |ui| {
            for field in fields {
                let mut present = object.contains_key(field.key);
                if field.optional {
                    if ui
                        .checkbox(&mut present, field.label)
                        .on_hover_text("Enable to author an override; disable to omit it.")
                        .changed()
                    {
                        if present {
                            object.insert(field.key.into(), default_value(field.input));
                        } else {
                            object.remove(field.key);
                        }
                    }
                } else {
                    ui.label(field.label);
                }
                if stacked {
                    ui.end_row();
                }
                if !present {
                    if !field.optional && ui.small_button("Set value").clicked() {
                        object.insert(field.key.into(), default_value(field.input));
                    } else {
                        ui.label(egui::RichText::new("Not authored").weak());
                    }
                } else if let Some(value) = object.get_mut(field.key) {
                    draw_input(ui, field.input, value, input_width);
                }
                ui.end_row();
            }
        });
    let error = validate(&draft.value).err();
    if let Some(error) = &error {
        ui.colored_label(ui.visuals().warn_fg_color, error);
    }
    let mut action = None;
    ui.horizontal(|ui| {
        if ui
            .add_enabled(error.is_none(), egui::Button::new("Apply"))
            .clicked()
        {
            action = Some(Action::Apply(draft.value.clone()));
        }
        if ui.small_button("Cancel").clicked() {
            draft.value = draft.source.clone();
        }
        if removable && ui.small_button("Remove").clicked() {
            action = Some(Action::Remove);
        }
    });
    ui.data_mut(|data| data.insert_temp(id, draft));
    action
}

fn default_value(input: Input) -> Value {
    match input {
        Input::Text => Value::from(""),
        Input::Integer(min, _) => Value::from(min.max(0)),
        Input::Unsigned(_) => Value::from(0),
        Input::Bool => Value::Bool(false),
    }
}

fn draw_input(ui: &mut egui::Ui, input: Input, value: &mut Value, width: f32) {
    match input {
        Input::Text => {
            let mut text = value.as_str().unwrap_or_default().to_owned();
            if ui
                .add(egui::TextEdit::singleline(&mut text).desired_width(width))
                .changed()
            {
                *value = Value::from(text);
            }
        }
        Input::Unsigned(max) => {
            let mut text = value
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| value.to_string());
            if ui
                .add(egui::TextEdit::singleline(&mut text).desired_width(width.min(160.0)))
                .changed()
            {
                // Keep even a partial hex string in the draft; Apply validates the complete row.
                *value = text
                    .parse::<u64>()
                    .map_or_else(|_| Value::from(text), Value::from);
            }
            if crate::hash::parse_unsigned_value(value).is_none_or(|n| n > max) {
                ui.colored_label(ui.visuals().warn_fg_color, "Invalid unsigned value");
            }
        }
        Input::Integer(min, max) => {
            let mut number = value.as_i64().unwrap_or(min);
            if ui
                .add(
                    egui::DragValue::new(&mut number)
                        .range(min..=max)
                        .clamp_existing_to_range(false),
                )
                .changed()
            {
                *value = Value::from(number);
            }
        }
        Input::Bool => {
            let mut enabled = value.as_bool().unwrap_or(false);
            if ui.checkbox(&mut enabled, "Enabled").changed() {
                *value = Value::Bool(enabled);
            }
        }
    }
}

#[cfg(test)]
mod tests;

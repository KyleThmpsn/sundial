//! Staged activity configuration controls, separate from the common runtime switches.

use super::{activity, optional_value};
use crate::app::components::object_form::{self as form, Action, Field, Input};
use eframe::egui;
use serde_json::{Value, json};

const DESTINATION_FIELDS: &[Field] = &[
    Field {
        key: "package_name",
        label: "Package Name",
        input: Input::Text,
        optional: false,
    },
    Field {
        key: "reason",
        label: "Selection Reason",
        input: Input::Integer(-1, 14),
        optional: false,
    },
    Field {
        key: "source_activity_index",
        label: "Source Activity Index",
        input: Input::Integer(-1, 4094),
        optional: false,
    },
    Field {
        key: "activity_index",
        label: "Destination Activity Index",
        input: Input::Integer(0, 4094),
        optional: false,
    },
    Field {
        key: "bubble_count",
        label: "Bubble Count",
        input: Input::Integer(1, 64),
        optional: false,
    },
    Field {
        key: "stateful_bubble_mask",
        label: "Stateful Bubble Mask",
        input: Input::Unsigned(u64::MAX),
        optional: false,
    },
    Field {
        key: "initial_slice_set",
        label: "Initial Slice Set",
        input: Input::Integer(0, 511),
        optional: false,
    },
    Field {
        key: "spawn_set_hash",
        label: "Spawn Set Hash",
        input: Input::Unsigned(u32::MAX as u64),
        optional: false,
    },
];
const ARRIVAL_FIELDS: &[Field] = &[
    Field {
        key: "package_name",
        label: "Package Name",
        input: Input::Text,
        optional: false,
    },
    Field {
        key: "bubble",
        label: "Bubble Override",
        input: Input::Integer(0, 63),
        optional: true,
    },
    Field {
        key: "slice_set",
        label: "Slice Set Override",
        input: Input::Integer(0, 511),
        optional: true,
    },
    Field {
        key: "spawn_set_hash",
        label: "Spawn Set Override",
        input: Input::Unsigned(u32::MAX as u64),
        optional: true,
    },
    Field {
        key: "current_activity_from_launch",
        label: "Set Current Activity on Launch",
        input: Input::Bool,
        optional: true,
    },
];

pub(super) fn draw(ui: &mut egui::Ui, document: &mut Value) -> bool {
    ui.horizontal(|ui| { ui.strong("Activity Destinations"); crate::ui_help::info(ui, "Use indices and package names from the installed game. Choose Apply to keep your changes or Cancel to discard them."); });
    let mut changed = false;
    egui::CollapsingHeader::new("Default Destination").show(ui, |ui| {
        match optional_value(document, activity::DESTINATION) {
            Err(error) => { ui.colored_label(ui.visuals().error_fg_color, error); }
            Ok(value) => {
                let fallback = json!({"reason":-1,"source_activity_index":-1,"activity_index":20,"package_name":"city_tower_social_d2","bubble_count":8,"stateful_bubble_mask":"0xCF","initial_slice_set":48,"spawn_set_hash":"0x811C9DC5"});
                let source = value.unwrap_or(&fallback);
                if value.is_none() { ui.label("No override is authored. Apply to create a destination override."); }
                if let Some(Action::Apply(value)) = form::draw(ui, "destination", source, DESTINATION_FIELDS, false, activity::validate_destination) {
                    changed |= report(ui, activity::set_destination(document, value));
                }
            }
        }
    });
    let rows = match optional_value(document, activity::ARRIVALS) {
        Ok(None) => Vec::new(),
        Ok(Some(value)) => match value.as_array() {
            Some(rows) => rows.clone(),
            None => {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    "Arrival overrides must be an array",
                );
                return changed;
            }
        },
        Err(error) => {
            ui.colored_label(ui.visuals().error_fg_color, error);
            return changed;
        }
    };
    ui.label(format!(
        "Arrival overrides: {} / {}",
        rows.len(),
        activity::ARRIVAL_CAPACITY
    ));
    let mut pending = None;
    for (index, row) in rows.iter().enumerate() {
        let label = row
            .get("package_name")
            .and_then(Value::as_str)
            .unwrap_or("Invalid arrival row");
        egui::CollapsingHeader::new(label)
            .id_salt(("arrival", index))
            .show(ui, |ui| {
                if let Some(action) = form::draw(
                    ui,
                    ("arrival", index),
                    row,
                    ARRIVAL_FIELDS,
                    true,
                    activity::validate_arrival,
                ) {
                    pending = Some((
                        index,
                        match action {
                            Action::Apply(row) => Some(row),
                            Action::Remove => None,
                        },
                    ));
                }
            });
    }
    if rows.len() < activity::ARRIVAL_CAPACITY {
        egui::CollapsingHeader::new("Add Arrival Override").show(ui, |ui| {
            if let Some(Action::Apply(row)) = form::draw(
                ui,
                ("arrival-new", rows.len()),
                &json!({"package_name":"","bubble":0}),
                ARRIVAL_FIELDS,
                false,
                activity::validate_arrival,
            ) {
                pending = Some((rows.len(), Some(row)));
            }
        });
    }
    if let Some((index, row)) = pending {
        changed |= report(ui, activity::set_arrival(document, index, row));
    }
    changed
}

fn report(ui: &mut egui::Ui, result: Result<bool, String>) -> bool {
    match result {
        Ok(changed) => changed,
        Err(error) => {
            ui.colored_label(ui.visuals().error_fg_color, error);
            false
        }
    }
}

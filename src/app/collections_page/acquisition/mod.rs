use eframe::egui;

use crate::{
    catalog::{
        Catalog, CollectibleDef, CollectionConditionDef, CollectionConditionTokenDef,
        UnlockDefinition,
    },
    hash::format_hash_hex,
};

use super::super::{
    inspector::request_definition as request_hash_inspection,
    progression::{
        CollectionStateSnapshot, collection_flag_state_text, collection_value_state_text,
    },
};

mod edits;
mod expression;

pub(super) use edits::draw_collection_acquisition_action;
pub(in crate::app) use edits::{
    collectible_acquisition_edit_available, set_collectible_acquisition_state,
};
use expression::{
    AND_INSTRUCTION, EQUAL_INSTRUCTION, GREATER_OR_EQUAL_INSTRUCTION, GREATER_THAN_INSTRUCTION,
    LEGACY_LITERAL_ENCODING_INSTRUCTION, LITERAL_INSTRUCTION, NOT_EQUAL_INSTRUCTION,
    NOT_INSTRUCTION, OR_INSTRUCTION, evaluate_expression_with,
};
pub(super) use expression::{FLAG_INSTRUCTION, OBJECTIVE_INSTRUCTION, VALUE_INSTRUCTION};

pub(super) const ACQUISITION_CONDITION_FIELD: u8 = 3;

#[derive(Clone)]
pub(super) struct StateLine {
    pub(super) text: String,
    pub(super) tooltip: String,
    pub(super) state: AcquisitionState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AcquisitionState {
    Acquired,
    Missing,
    NoRule,
    Unknown,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct AcquisitionCounts {
    pub(super) acquired: usize,
    pub(super) missing: usize,
    pub(super) no_rule: usize,
    pub(super) unknown: usize,
}

impl AcquisitionCounts {
    pub(super) const fn total(self) -> usize {
        self.acquired + self.missing + self.no_rule + self.unknown
    }

    pub(super) fn add(&mut self, state: AcquisitionState) {
        match state {
            AcquisitionState::Acquired => self.acquired += 1,
            AcquisitionState::Missing => self.missing += 1,
            AcquisitionState::NoRule => self.no_rule += 1,
            AcquisitionState::Unknown => self.unknown += 1,
        }
    }
}

pub(super) fn state_lines(
    definition: &CollectibleDef,
    snapshot: &CollectionStateSnapshot,
    catalog: &Catalog,
) -> Vec<String> {
    let mut lines = Vec::new();
    for condition in &definition.conditions {
        for token in &condition.tokens {
            let (label, state) = match token.kind {
                FLAG_INSTRUCTION => {
                    let (scope, state) = flag_reference(token, snapshot, catalog);
                    (
                        format!("{} · {scope}", condition_token_metadata(token, catalog)),
                        state,
                    )
                }
                VALUE_INSTRUCTION => {
                    let (scope, state) = value_reference(token, snapshot, catalog);
                    (
                        format!("{} · {scope}", condition_token_metadata(token, catalog)),
                        state,
                    )
                }
                OBJECTIVE_INSTRUCTION => objective_reference(token, snapshot, catalog),
                _ => continue,
            };
            let text = format!(
                "{} - {label}: {state}",
                condition_field_label(condition.field)
            );
            if !lines.contains(&text) {
                lines.push(text);
            }
        }
    }
    lines
}

pub(super) fn condition_field_label(field: u8) -> String {
    if field == ACQUISITION_CONDITION_FIELD {
        "Acquisition (field 3)".into()
    } else {
        format!("Field {field}")
    }
}

pub(super) fn condition_metadata_lines(definition: &CollectibleDef) -> Vec<String> {
    definition
        .conditions
        .iter()
        .map(|condition| {
            format!(
                "{}: {}",
                condition_field_label(condition.field),
                condition_program(condition)
            )
        })
        .collect()
}

pub(super) fn acquisition_status(
    definition: &CollectibleDef,
    snapshot: &CollectionStateSnapshot,
    catalog: &Catalog,
) -> StateLine {
    let conditions = definition
        .conditions
        .iter()
        .filter(|condition| condition.field == ACQUISITION_CONDITION_FIELD)
        .collect::<Vec<_>>();
    let program = conditions
        .first()
        .map(|condition| condition_program(condition))
        .unwrap_or_default();
    let value = (conditions.len() == 1)
        .then(|| evaluate_expression(&conditions[0].tokens, snapshot, catalog))
        .flatten();
    match value {
        Some(true) => StateLine {
            text: "Acquired".into(),
            tooltip: format!("Acquisition condition: true\nProgram: {program}"),
            state: AcquisitionState::Acquired,
        },
        Some(false) => StateLine {
            text: "Missing".into(),
            tooltip: format!("Acquisition condition: false\nProgram: {program}"),
            state: AcquisitionState::Missing,
        },
        None if program.is_empty() => StateLine {
            text: "No condition program".into(),
            tooltip: "No acquisition condition".into(),
            state: AcquisitionState::NoRule,
        },
        None => {
            let mut unsupported = conditions
                .iter()
                .flat_map(|condition| condition.tokens.iter())
                .filter_map(|token| {
                    (!(matches!(token.kind, 1 | 2 | 3 | 4 | 8 | 9 | 10 | 11 | 12 | 13 | 14)
                        || token.kind == LEGACY_LITERAL_ENCODING_INSTRUCTION && token.operand == 0))
                        .then_some(token.kind)
                })
                .collect::<Vec<_>>();
            unsupported.sort_unstable();
            unsupported.dedup();
            let unavailable = conditions.first().map_or_else(Vec::new, |condition| {
                unavailable_acquisition_references(condition, snapshot, catalog)
            });
            let (text, reason) = if !unsupported.is_empty() {
                (
                    "Unsupported package operation".to_owned(),
                    format!(
                        "Unsupported package operation(s): {}",
                        unsupported
                            .iter()
                            .map(u32::to_string)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                )
            } else if !unavailable.is_empty() {
                (
                    "State unavailable".to_owned(),
                    format!(
                        "Referenced Sunrise state not present:\n{}",
                        unavailable.join("\n")
                    ),
                )
            } else {
                (
                    "Invalid condition program".to_owned(),
                    "Package condition program does not produce one result".to_owned(),
                )
            };
            StateLine {
                text,
                tooltip: format!("{reason}\nProgram: {program}"),
                state: AcquisitionState::Unknown,
            }
        }
    }
}

pub(in crate::app) fn collectible_state(
    definition: &CollectibleDef,
    snapshot: &CollectionStateSnapshot,
    catalog: &Catalog,
) -> (String, String) {
    let state = acquisition_status(definition, snapshot, catalog);
    (state.text, state.tooltip)
}

pub(in crate::app) fn collectible_acquired_state(
    definition: &CollectibleDef,
    snapshot: &CollectionStateSnapshot,
    catalog: &Catalog,
) -> Option<bool> {
    match acquisition_status(definition, snapshot, catalog).state {
        AcquisitionState::Acquired => Some(true),
        AcquisitionState::Missing => Some(false),
        AcquisitionState::NoRule | AcquisitionState::Unknown => None,
    }
}

fn unavailable_acquisition_references(
    condition: &CollectionConditionDef,
    snapshot: &CollectionStateSnapshot,
    catalog: &Catalog,
) -> Vec<String> {
    let mut unavailable = Vec::new();
    for token in &condition.tokens {
        let missing = match token.kind {
            FLAG_INSTRUCTION => catalog
                .unlock_flag_definition(token.operand as usize)
                .is_none_or(|definition| {
                    snapshot
                        .flag_value(token.operand as usize, definition)
                        .is_none()
                }),
            VALUE_INSTRUCTION => catalog
                .unlock_value_definition(token.operand as usize)
                .is_none_or(|definition| {
                    snapshot.value(token.operand as usize, definition).is_none()
                }),
            OBJECTIVE_INSTRUCTION => {
                objective_completion(token.operand as usize, snapshot, catalog).is_none()
            }
            _ => false,
        };
        if missing {
            let label = match token.kind {
                FLAG_INSTRUCTION => format!("Flag #{}", token.operand),
                VALUE_INSTRUCTION => format!("Value #{}", token.operand),
                OBJECTIVE_INSTRUCTION => format!("Objective #{}", token.operand),
                _ => continue,
            };
            if !unavailable.contains(&label) {
                unavailable.push(label);
            }
        }
    }
    unavailable
}

pub(super) fn evaluate_expression(
    tokens: &[CollectionConditionTokenDef],
    snapshot: &CollectionStateSnapshot,
    catalog: &Catalog,
) -> Option<bool> {
    evaluate_expression_with(
        tokens,
        |index| {
            let definition = catalog.unlock_flag_definition(index)?;
            snapshot.flag_value(index, definition)
        },
        |index| {
            let definition = catalog.unlock_value_definition(index)?;
            snapshot.value(index, definition)
        },
        |index| objective_completion(index, snapshot, catalog),
    )
}

fn objective_completion(
    index: usize,
    snapshot: &CollectionStateSnapshot,
    catalog: &Catalog,
) -> Option<bool> {
    let objective = catalog.objective_definition(index)?;
    let definition_index = usize::from(objective.related_unlock_value_definition_index?);
    let definition = catalog.unlock_value_definition(definition_index)?;
    let current = snapshot.value(definition_index, definition)?;
    Some(if objective.is_counting_downward {
        current <= objective.completion_value
    } else {
        current >= objective.completion_value
    })
}

fn flag_reference(
    token: &CollectionConditionTokenDef,
    snapshot: &CollectionStateSnapshot,
    catalog: &Catalog,
) -> (String, String) {
    let index = token.operand as usize;
    let Some(definition) = catalog.unlock_flag_definition(index) else {
        return (format!("Flag #{index}"), "Unavailable".into());
    };
    (
        definition_state_label("flag", index, definition),
        collection_flag_state_text(snapshot, index, definition),
    )
}

fn value_reference(
    token: &CollectionConditionTokenDef,
    snapshot: &CollectionStateSnapshot,
    catalog: &Catalog,
) -> (String, String) {
    let index = token.operand as usize;
    let Some(definition) = catalog.unlock_value_definition(index) else {
        return (format!("Value #{index}"), "Unavailable".into());
    };
    (
        definition_state_label("value", index, definition),
        collection_value_state_text(snapshot, index, definition),
    )
}

fn objective_reference(
    token: &CollectionConditionTokenDef,
    snapshot: &CollectionStateSnapshot,
    catalog: &Catalog,
) -> (String, String) {
    let index = token.operand as usize;
    let Some(objective) = catalog.objective_definition(index) else {
        return (format!("Objective #{index}"), "Unavailable".into());
    };
    let label = objective_display_name(objective).map_or_else(
        || format!("Objective #{index} · 0x{:08X}", objective.hash),
        |name| format!("Objective #{index} · {name} · 0x{:08X}", objective.hash),
    );
    let state = objective_completion(index, snapshot, catalog).map_or_else(
        || "State not present in Sunrise settings".into(),
        |completed| {
            if completed {
                "Complete".into()
            } else {
                "Incomplete".into()
            }
        },
    );
    (label, state)
}

fn objective_display_name(objective: &crate::catalog::ObjectiveDef) -> Option<&str> {
    [
        objective.name.as_str(),
        objective.progress_description.as_str(),
        objective.display_description.as_str(),
        objective.description.as_str(),
    ]
    .into_iter()
    .find(|text| !text.trim().is_empty())
}

fn definition_state_label(kind: &str, index: usize, definition: &UnlockDefinition) -> String {
    let Some(slot) = definition.compact_slot else {
        return format!("{kind} #{index}");
    };
    let scope = match (kind, definition.bank()) {
        ("flag", 1) => "Account flag",
        ("flag", 2) => "Profile flag",
        ("flag", 3) => "Selected-character flag",
        ("flag", 6) => "Per-character flag",
        ("value", 1) => "Account value",
        ("value", 2) => "Selected-character value",
        _ => return format!("{kind} #{index}"),
    };
    format!("{scope} {slot}")
}

pub(super) fn condition_program(condition: &CollectionConditionDef) -> String {
    condition
        .tokens
        .iter()
        .map(|token| format!("{}:{}", token.kind, token.operand))
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn condition_token_state(
    token: &CollectionConditionTokenDef,
    snapshot: &CollectionStateSnapshot,
    catalog: &Catalog,
) -> String {
    match token.kind {
        FLAG_INSTRUCTION => {
            let (scope, state) = flag_reference(token, snapshot, catalog);
            format!("{scope}: {state}")
        }
        VALUE_INSTRUCTION => {
            let (scope, state) = value_reference(token, snapshot, catalog);
            format!("{scope}: {state}")
        }
        OBJECTIVE_INSTRUCTION => objective_reference(token, snapshot, catalog).1,
        _ => "-".into(),
    }
}

pub(super) fn condition_token_label(kind: u32) -> String {
    match kind {
        FLAG_INSTRUCTION => "Flag reference".into(),
        NOT_INSTRUCTION => "Not".into(),
        OR_INSTRUCTION => "Or".into(),
        AND_INSTRUCTION => "And".into(),
        EQUAL_INSTRUCTION => "Equal".into(),
        NOT_EQUAL_INSTRUCTION => "Not equal".into(),
        VALUE_INSTRUCTION => "Value reference".into(),
        LITERAL_INSTRUCTION => "Literal".into(),
        OBJECTIVE_INSTRUCTION => "Objective reference".into(),
        GREATER_THAN_INSTRUCTION => "Greater than".into(),
        GREATER_OR_EQUAL_INSTRUCTION => "Greater than or equal".into(),
        LEGACY_LITERAL_ENCODING_INSTRUCTION => "Literal encoding".into(),
        _ => format!("Opcode {kind}"),
    }
}

pub(super) fn condition_token_metadata(
    token: &CollectionConditionTokenDef,
    catalog: &Catalog,
) -> String {
    let index = token.operand as usize;
    if token.kind == OBJECTIVE_INSTRUCTION {
        let Some(objective) = catalog.objective_definition(index) else {
            return "Objective unavailable".into();
        };
        return objective_display_name(objective).map_or_else(
            || format!("Objective #{index} · 0x{:08X}", objective.hash),
            |name| format!("{name} · 0x{:08X}", objective.hash),
        );
    }
    let definition = match token.kind {
        FLAG_INSTRUCTION => catalog.unlock_flag_definition(index),
        VALUE_INSTRUCTION => catalog.unlock_value_definition(index),
        _ => return String::new(),
    };
    let Some(definition) = definition else {
        return "Definition unavailable".into();
    };
    definition
        .name
        .as_deref()
        .filter(|name| !name.trim().is_empty())
        .map_or_else(
            || format_hash_hex(definition.hash),
            |name| format!("{name} · 0x{:08X}", definition.hash),
        )
}

pub(super) fn draw_condition_token_metadata(
    ui: &mut egui::Ui,
    token: &CollectionConditionTokenDef,
    catalog: &Catalog,
) {
    if token.kind == OBJECTIVE_INSTRUCTION {
        let text = condition_token_metadata(token, catalog);
        let Some(objective) = catalog.objective_definition(token.operand as usize) else {
            ui.label(text);
            return;
        };
        let canonical = format_hash_hex(objective.hash);
        let response = ui
            .add(egui::Button::new(egui::RichText::new(text)).frame(false))
            .on_hover_text(format!("Open details for {canonical}"));
        if response.clicked() {
            request_hash_inspection(ui.ctx(), objective.hash);
        }
        return;
    }
    let definition = match token.kind {
        FLAG_INSTRUCTION => catalog.unlock_flag_definition(token.operand as usize),
        VALUE_INSTRUCTION => catalog.unlock_value_definition(token.operand as usize),
        _ => None,
    };
    let text = condition_token_metadata(token, catalog);
    let Some(definition) = definition else {
        ui.label(text);
        return;
    };
    let canonical = format_hash_hex(definition.hash);
    let response = ui
        .add(egui::Button::new(egui::RichText::new(text)).frame(false))
        .on_hover_text(format!("Open details for {canonical}"));
    if response.clicked() {
        request_hash_inspection(ui.ctx(), definition.hash);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_lists_every_package_condition_field_and_raw_opcode() {
        let definition = CollectibleDef {
            index: 1,
            hash: 2,
            item_definition_index: 4,
            item_hash: 3,
            material_requirement_set_index: None,
            material_requirement_set_hash: 0,
            material_requirements: Vec::new(),
            name: "Test".into(),
            type_name: "Record".into(),
            paths: Vec::new(),
            conditions: (0..=3)
                .map(|field| CollectionConditionDef {
                    field,
                    tokens: vec![CollectionConditionTokenDef {
                        kind: 12 + u32::from(field),
                        operand: 40 + u32::from(field),
                    }],
                })
                .collect(),
        };

        assert_eq!(
            condition_metadata_lines(&definition),
            [
                "Field 0: 12:40",
                "Field 1: 13:41",
                "Field 2: 14:42",
                "Acquisition (field 3): 15:43",
            ]
        );
    }
}

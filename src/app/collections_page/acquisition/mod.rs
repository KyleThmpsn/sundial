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
    ADD_INSTRUCTION, AND_INSTRUCTION, BITWISE_AND_INSTRUCTION, BITWISE_NOT_INSTRUCTION,
    BITWISE_OR_INSTRUCTION, BITWISE_XOR_INSTRUCTION, DIVIDE_INSTRUCTION, EQUAL_INSTRUCTION,
    GREATER_OR_EQUAL_INSTRUCTION, GREATER_THAN_INSTRUCTION, HASH_COMBINE_INSTRUCTION,
    HASH_NUMBER_INSTRUCTION, LESS_OR_EQUAL_INSTRUCTION, LESS_THAN_INSTRUCTION, LITERAL_INSTRUCTION,
    MODULO_INSTRUCTION, MULTIPLY_INSTRUCTION, NAND_INSTRUCTION, NEGATE_NUMBER_INSTRUCTION,
    NOR_INSTRUCTION, NOT_EQUAL_ALTERNATE_INSTRUCTION, NOT_EQUAL_INSTRUCTION, NOT_INSTRUCTION,
    OR_INSTRUCTION, SUBTRACT_INSTRUCTION, evaluate_expression_with, is_supported_instruction,
};
pub(in crate::app) use expression::{
    ExpressionValue, evaluate_expression_value_with,
    is_supported_instruction as is_supported_condition_instruction,
};
pub(super) use expression::{FLAG_INSTRUCTION, POOL_INSTRUCTION, VALUE_INSTRUCTION};

pub(super) const ACQUISITION_CONDITION_FIELD: u8 =
    crate::catalog::COLLECTIBLE_ACQUIRED_CONDITION_FIELD;

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
    Unknown,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct AcquisitionCounts {
    pub(super) acquired: usize,
    pub(super) missing: usize,
    pub(super) unknown: usize,
}

impl AcquisitionCounts {
    pub(super) const fn total(self) -> usize {
        self.acquired + self.missing + self.unknown
    }

    pub(super) fn add(&mut self, state: AcquisitionState) {
        match state {
            AcquisitionState::Acquired => self.acquired += 1,
            AcquisitionState::Missing => self.missing += 1,
            AcquisitionState::Unknown => self.unknown += 1,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AcquisitionEvaluation {
    Acquired,
    Missing,
    Unconditional,
    Unknown,
}

impl AcquisitionEvaluation {
    const fn state(self) -> AcquisitionState {
        match self {
            Self::Acquired | Self::Unconditional => AcquisitionState::Acquired,
            Self::Missing => AcquisitionState::Missing,
            Self::Unknown => AcquisitionState::Unknown,
        }
    }
}

fn acquisition_evaluation(
    conditions: &[&CollectionConditionDef],
    value: Option<bool>,
) -> AcquisitionEvaluation {
    match value {
        Some(true) => AcquisitionEvaluation::Acquired,
        Some(false) => AcquisitionEvaluation::Missing,
        None if conditions.is_empty() => AcquisitionEvaluation::Unconditional,
        None => AcquisitionEvaluation::Unknown,
    }
}

pub(super) fn for_each_expression_token(
    tokens: &[CollectionConditionTokenDef],
    catalog: &Catalog,
    mut visit: impl FnMut(&CollectionConditionTokenDef),
) -> bool {
    fn walk(
        tokens: &[CollectionConditionTokenDef],
        catalog: &Catalog,
        active_pool_rows: &mut [bool],
        visit: &mut impl FnMut(&CollectionConditionTokenDef),
    ) -> bool {
        for token in tokens {
            visit(token);
            if token.kind != POOL_INSTRUCTION {
                continue;
            }
            let index = token.operand as usize;
            let Some(program) = catalog.shared_expression(index) else {
                return false;
            };
            let Some(active) = active_pool_rows.get_mut(index) else {
                return false;
            };
            if *active {
                return false;
            }
            *active = true;
            let complete = walk(program, catalog, active_pool_rows, visit);
            active_pool_rows[index] = false;
            if !complete {
                return false;
            }
        }
        true
    }

    let mut active_pool_rows = vec![false; catalog.shared_expression_pool().len()];
    walk(tokens, catalog, &mut active_pool_rows, &mut visit)
}

pub(super) fn state_lines(
    definition: &CollectibleDef,
    snapshot: &CollectionStateSnapshot,
    catalog: &Catalog,
) -> Vec<String> {
    let mut lines = Vec::new();
    for condition in &definition.conditions {
        for_each_expression_token(&condition.tokens, catalog, |token| {
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
                _ => return,
            };
            let text = format!(
                "{} - {label}: {state}",
                condition_field_label(condition.field)
            );
            if !lines.contains(&text) {
                lines.push(text);
            }
        });
    }
    lines
}

pub(super) fn condition_field_label(field: u8) -> String {
    if field == ACQUISITION_CONDITION_FIELD {
        "Acquisition (field 4)".into()
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
    let evaluation = acquisition_evaluation(&conditions, value);
    match evaluation {
        AcquisitionEvaluation::Acquired => StateLine {
            text: "Acquired".into(),
            tooltip: format!(
                "Acquisition condition from available state: true\nProgram: {program}"
            ),
            state: evaluation.state(),
        },
        AcquisitionEvaluation::Missing => StateLine {
            text: "Missing".into(),
            tooltip: format!(
                "Acquisition condition from available state: false\nProgram: {program}"
            ),
            state: evaluation.state(),
        },
        AcquisitionEvaluation::Unconditional => StateLine {
            text: "Acquired".into(),
            tooltip: "No acquisition condition. This collectible is unconditionally acquired"
                .into(),
            state: evaluation.state(),
        },
        AcquisitionEvaluation::Unknown => {
            let mut unsupported = Vec::new();
            for condition in &conditions {
                for_each_expression_token(&condition.tokens, catalog, |token| {
                    if !is_supported_instruction(token.kind) {
                        unsupported.push(token.kind);
                    }
                });
            }
            unsupported.sort_unstable();
            unsupported.dedup();
            let unavailable = conditions.first().map_or_else(Vec::new, |condition| {
                unavailable_acquisition_references(condition, snapshot, catalog)
            });
            let (text, reason) = if !unsupported.is_empty() {
                (
                    "Unsupported unlock-expression opcode".to_owned(),
                    format!(
                        "Unsupported unlock-expression opcode(s): {}",
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
        AcquisitionState::Unknown => None,
    }
}

fn unavailable_acquisition_references(
    condition: &CollectionConditionDef,
    snapshot: &CollectionStateSnapshot,
    catalog: &Catalog,
) -> Vec<String> {
    let mut unavailable = Vec::new();
    let complete = for_each_expression_token(&condition.tokens, catalog, |token| {
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
            POOL_INSTRUCTION => catalog.shared_expression(token.operand as usize).is_none(),
            _ => false,
        };
        if missing {
            let label = match token.kind {
                FLAG_INSTRUCTION => format!("Flag #{}", token.operand),
                VALUE_INSTRUCTION => format!("Value #{}", token.operand),
                POOL_INSTRUCTION => format!("Shared expression #{}", token.operand),
                _ => return,
            };
            if !unavailable.contains(&label) {
                unavailable.push(label);
            }
        }
    });
    if !complete
        && !unavailable
            .iter()
            .any(|label| label.starts_with("Shared expression #"))
    {
        unavailable.push("Shared expression cycle".into());
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
        catalog.shared_expression_pool(),
        |index| snapshot.evaluated_flag(index, catalog),
        |index| snapshot.evaluated_value(index, catalog),
    )
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
        POOL_INSTRUCTION => catalog
            .shared_expression(token.operand as usize)
            .map_or_else(|| "Unavailable".into(), |_| "Evaluated recursively".into()),
        _ => "-".into(),
    }
}

pub(super) fn condition_token_label(kind: u32) -> String {
    match kind {
        FLAG_INSTRUCTION => "Flag reference".into(),
        NOT_INSTRUCTION => "Not".into(),
        OR_INSTRUCTION => "Or".into(),
        AND_INSTRUCTION => "And".into(),
        NOR_INSTRUCTION => "Nor".into(),
        NOT_EQUAL_ALTERNATE_INSTRUCTION => "Not equal".into(),
        NAND_INSTRUCTION => "Nand".into(),
        EQUAL_INSTRUCTION => "Equal".into(),
        NOT_EQUAL_INSTRUCTION => "Not equal".into(),
        VALUE_INSTRUCTION => "Value reference".into(),
        LITERAL_INSTRUCTION => "Literal".into(),
        POOL_INSTRUCTION => "Shared expression".into(),
        GREATER_THAN_INSTRUCTION => "Greater than".into(),
        GREATER_OR_EQUAL_INSTRUCTION => "Greater than or equal".into(),
        LESS_THAN_INSTRUCTION => "Less than".into(),
        LESS_OR_EQUAL_INSTRUCTION => "Less than or equal".into(),
        ADD_INSTRUCTION => "Add".into(),
        SUBTRACT_INSTRUCTION => "Subtract".into(),
        MULTIPLY_INSTRUCTION => "Multiply".into(),
        DIVIDE_INSTRUCTION => "Divide".into(),
        MODULO_INSTRUCTION => "Modulo".into(),
        NEGATE_NUMBER_INSTRUCTION => "Negate number".into(),
        HASH_NUMBER_INSTRUCTION => "Hash number".into(),
        HASH_COMBINE_INSTRUCTION => "Combine hash".into(),
        BITWISE_NOT_INSTRUCTION => "Bitwise NOT".into(),
        BITWISE_AND_INSTRUCTION => "Bitwise and".into(),
        BITWISE_OR_INSTRUCTION => "Bitwise or".into(),
        BITWISE_XOR_INSTRUCTION => "Bitwise xor".into(),
        _ => format!("Opcode {kind}"),
    }
}

pub(super) fn condition_token_metadata(
    token: &CollectionConditionTokenDef,
    catalog: &Catalog,
) -> String {
    let index = token.operand as usize;
    if token.kind == POOL_INSTRUCTION {
        return if catalog.shared_expression(index).is_some() {
            format!("Pool row #{index}")
        } else {
            format!("Pool row #{index} unavailable")
        };
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
    fn absent_acquisition_condition_is_unconditionally_acquired() {
        let evaluation = acquisition_evaluation(&[], None);

        assert_eq!(evaluation, AcquisitionEvaluation::Unconditional);
        assert_eq!(evaluation.state(), AcquisitionState::Acquired);
    }

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
            conditions: (0..=4)
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
                "Field 3: 15:43",
                "Acquisition (field 4): 16:44",
            ]
        );
    }
}

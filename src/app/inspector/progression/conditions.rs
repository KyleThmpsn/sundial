//! Condition-program decoding and inspector navigation.

use eframe::egui;

use crate::app::progression::CollectionStateSnapshot;
use crate::catalog::{Catalog, UnlockDefinition};

use super::{
    definitions::definition_name,
    objectives::resolved_objective_table_text,
    state::{MetadataSelection, ProgressionInspectorState},
};

pub(in crate::app) fn definition_has_undecoded_opcodes(definition: &UnlockDefinition) -> bool {
    definition
        .tested_by
        .iter()
        .flat_map(|context| context.condition_programs.iter())
        .flatten()
        .any(|token| !decoded_condition_opcode(token[0]))
}

pub(super) const fn decoded_condition_opcode(opcode: u32) -> bool {
    matches!(
        opcode,
        1 | 2 | 3 | 4 | 8 | 9 | 10 | 11 | 12 | 13 | 14 | 15 | 22
    )
}

pub(super) fn direct_value_comparison(
    program: &[[u32; 2]],
    definition_index: usize,
    forced_value: i32,
) -> Option<(String, bool)> {
    let (left, right, operator) = match program {
        [left, right, operator] => (*left, *right, *operator),
        [left, right, encoding, operator] if *encoding == [22, 0] => (*left, *right, *operator),
        _ => return None,
    };
    let index = u32::try_from(definition_index).ok()?;
    let (literal, reference_first) = match (left[0], right[0]) {
        (10, 11) if left[1] == index => (right[1] as i32, true),
        (11, 10) if right[1] == index => (left[1] as i32, false),
        _ => return None,
    };
    let (label, result) = match (operator[0], reference_first) {
        (8, _) => (format!("= {literal}"), forced_value == literal),
        (9, _) => (format!("≠ {literal}"), forced_value != literal),
        (13, true) => (format!("> {literal}"), forced_value > literal),
        (13, false) => (format!("< {literal}"), forced_value < literal),
        (14, true) => (format!("≥ {literal}"), forced_value >= literal),
        (14, false) => (format!("≤ {literal}"), forced_value <= literal),
        (15, true) => (format!("< {literal}"), forced_value < literal),
        (15, false) => (format!("> {literal}"), forced_value > literal),
        _ => return None,
    };
    Some((label, result))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ConditionEvaluation {
    Passed,
    Failed,
    Value(i32),
    Unresolved(String),
}

impl ConditionEvaluation {
    pub(super) fn label(&self) -> String {
        match self {
            Self::Passed => "Pass".into(),
            Self::Failed => "Fail".into(),
            Self::Value(value) => format!("Value {value}"),
            Self::Unresolved(reason) => format!("Unresolved · {reason}"),
        }
    }

    pub(super) const fn is_resolved(&self) -> bool {
        !matches!(self, Self::Unresolved(_))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum StackValue {
    Unknown(String),
    Bool(bool),
    Int(i32),
}

impl StackValue {
    fn truthy(&self) -> Option<bool> {
        match self {
            Self::Unknown(_) => None,
            Self::Bool(value) => Some(*value),
            Self::Int(value) => Some(*value != 0),
        }
    }

    fn number(&self) -> Option<i32> {
        match self {
            Self::Unknown(_) => None,
            Self::Bool(value) => Some(i32::from(*value)),
            Self::Int(value) => Some(*value),
        }
    }

    fn unresolved_reason(&self) -> String {
        match self {
            Self::Unknown(reason) => reason.clone(),
            Self::Bool(_) | Self::Int(_) => "operation received incompatible values".into(),
        }
    }
}

pub(super) fn evaluate_condition_program(
    program: &[[u32; 2]],
    catalog: &Catalog,
    snapshot: Option<&CollectionStateSnapshot>,
) -> ConditionEvaluation {
    let mut stack = Vec::<StackValue>::new();
    for &[opcode, operand] in program {
        let value = match opcode {
            1 => catalog
                .unlock_flag_definition(operand as usize)
                .and_then(|definition| {
                    snapshot.and_then(|snapshot| {
                        snapshot
                            .flag_value(operand as usize, definition)
                            .map(StackValue::Bool)
                    })
                })
                .unwrap_or_else(|| {
                    StackValue::Unknown(format!("flag #{operand} or save state is unavailable"))
                }),
            10 => catalog
                .unlock_value_definition(operand as usize)
                .and_then(|definition| {
                    snapshot.and_then(|snapshot| {
                        snapshot
                            .value(operand as usize, definition)
                            .map(StackValue::Int)
                    })
                })
                .unwrap_or_else(|| {
                    StackValue::Unknown(format!("value #{operand} or save state is unavailable"))
                }),
            11 => StackValue::Int(operand as i32),
            12 => objective_condition_value(operand as usize, catalog, snapshot),
            22 if operand == 0 => match stack.pop() {
                Some(value) => value,
                None => {
                    return ConditionEvaluation::Unresolved(
                        "literal encoding requires one value".into(),
                    );
                }
            },
            2 => match stack.pop() {
                Some(value) => value.truthy().map_or_else(
                    || StackValue::Unknown(value.unresolved_reason()),
                    |value| StackValue::Bool(!value),
                ),
                None => return ConditionEvaluation::Unresolved("Not requires one value".into()),
            },
            3 | 4 => match pop_two_values(&mut stack) {
                Some((left, right)) => logical_values(opcode, left, right),
                None => {
                    return ConditionEvaluation::Unresolved(
                        "logical operation requires two values".into(),
                    );
                }
            },
            8 | 9 | 13 | 14 | 15 => match pop_two_values(&mut stack) {
                Some((left, right)) => compare_values(opcode, left, right).map_or_else(
                    || StackValue::Unknown("comparison is unresolved".into()),
                    StackValue::Bool,
                ),
                None => {
                    return ConditionEvaluation::Unresolved(
                        "comparison requires two values".into(),
                    );
                }
            },
            22 => StackValue::Unknown(format!("literal encoding mode {operand} is not decoded")),
            _ => StackValue::Unknown(format!("opcode {opcode} is not decoded")),
        };
        stack.push(value);
    }
    match stack.as_slice() {
        [StackValue::Bool(true)] => ConditionEvaluation::Passed,
        [StackValue::Bool(false)] => ConditionEvaluation::Failed,
        [StackValue::Int(value)] => ConditionEvaluation::Value(*value),
        [StackValue::Unknown(reason)] => ConditionEvaluation::Unresolved(reason.clone()),
        [] => ConditionEvaluation::Unresolved("program produced no result".into()),
        _ => ConditionEvaluation::Unresolved(format!(
            "program left {} values on the evaluation stack",
            stack.len()
        )),
    }
}

fn pop_two_values(stack: &mut Vec<StackValue>) -> Option<(StackValue, StackValue)> {
    let right = stack.pop()?;
    let left = stack.pop()?;
    Some((left, right))
}

fn logical_values(opcode: u32, left: StackValue, right: StackValue) -> StackValue {
    match (opcode, left.truthy(), right.truthy()) {
        (3, Some(true), _) | (3, _, Some(true)) => StackValue::Bool(true),
        (3, Some(false), Some(false)) => StackValue::Bool(false),
        (4, Some(false), _) | (4, _, Some(false)) => StackValue::Bool(false),
        (4, Some(true), Some(true)) => StackValue::Bool(true),
        _ => StackValue::Unknown(format!(
            "{}; {}",
            left.unresolved_reason(),
            right.unresolved_reason()
        )),
    }
}

fn compare_values(opcode: u32, left: StackValue, right: StackValue) -> Option<bool> {
    match (left.number(), right.number()) {
        (Some(left), Some(right)) => match opcode {
            8 => Some(left == right),
            9 => Some(left != right),
            13 => Some(left > right),
            14 => Some(left >= right),
            15 => Some(left < right),
            _ => None,
        },
        (None, _) | (_, None) => None,
    }
}

fn objective_condition_value(
    objective_index: usize,
    catalog: &Catalog,
    snapshot: Option<&CollectionStateSnapshot>,
) -> StackValue {
    let Some(objective) = catalog.objective_definition(objective_index) else {
        return StackValue::Unknown(format!("objective #{objective_index} is unavailable"));
    };
    let Some(definition_index) = objective
        .related_unlock_value_definition_index
        .map(usize::from)
    else {
        return StackValue::Unknown(format!(
            "objective #{objective_index} has no related unlock value"
        ));
    };
    let current = catalog
        .unlock_value_definition(definition_index)
        .and_then(|definition| {
            snapshot.and_then(|snapshot| snapshot.value(definition_index, definition))
        });
    current.map_or_else(
        || {
            StackValue::Unknown(format!(
                "objective #{objective_index} or save state is unavailable"
            ))
        },
        |current| {
            StackValue::Bool(if objective.is_counting_downward {
                current <= objective.completion_value
            } else {
                current >= objective.completion_value
            })
        },
    )
}

pub(super) fn draw_condition_programs(
    ui: &mut egui::Ui,
    id_source: &'static str,
    owner_hash: u64,
    programs: &[Vec<[u32; 2]>],
    catalog: &Catalog,
    snapshot: Option<&CollectionStateSnapshot>,
    state: &mut ProgressionInspectorState,
) {
    for (program_index, program) in programs.iter().enumerate() {
        egui::CollapsingHeader::new(format!("Condition program {}", program_index + 1))
            .id_salt((id_source, owner_hash, program_index))
            .show(ui, |ui| {
                let evaluation = evaluate_condition_program(program, catalog, snapshot);
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new("Effective result").strong());
                    let color = match evaluation {
                        ConditionEvaluation::Passed => ui.visuals().selection.bg_fill,
                        ConditionEvaluation::Failed => ui.visuals().error_fg_color,
                        ConditionEvaluation::Value(_) | ConditionEvaluation::Unresolved(_) => {
                            ui.visuals().warn_fg_color
                        }
                    };
                    ui.colored_label(color, evaluation.label());
                });
                draw_condition_dependencies(ui, program, catalog, snapshot, state);
                ui.add_space(4.0);
                if ui.available_width() >= 700.0 {
                    egui::Grid::new((id_source, "condition_tokens", owner_hash, program_index))
                        .num_columns(4)
                        .spacing([16.0, 3.0])
                        .show(ui, |ui| {
                            ui.strong("#");
                            ui.strong("Operation");
                            ui.strong("Operand");
                            ui.strong("Referenced entry");
                            ui.end_row();
                            for (token_index, token) in program.iter().enumerate() {
                                ui.monospace((token_index + 1).to_string());
                                ui.label(condition_opcode_label(token[0]));
                                ui.monospace(token[1].to_string());
                                draw_condition_token_resolution(ui, token, catalog, state);
                                ui.end_row();
                            }
                        });
                } else {
                    for (token_index, token) in program.iter().enumerate() {
                        if token_index > 0 {
                            ui.add_space(4.0);
                        }
                        ui.group(|ui| {
                            ui.set_min_width(ui.available_width());
                            ui.horizontal_wrapped(|ui| {
                                ui.strong(format!(
                                    "{}. {}",
                                    token_index + 1,
                                    condition_opcode_label(token[0])
                                ));
                                ui.label(egui::RichText::new("Operand").weak());
                                ui.monospace(token[1].to_string());
                            });
                            ui.horizontal_wrapped(|ui| {
                                ui.label(egui::RichText::new("Referenced entry").weak());
                                draw_condition_token_resolution(ui, token, catalog, state);
                            });
                        });
                    }
                }
                egui::CollapsingHeader::new("Raw opcodes")
                    .id_salt((id_source, "raw_condition", owner_hash, program_index))
                    .show(ui, |ui| {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(
                                    program
                                        .iter()
                                        .map(|token| format!("{}:{}", token[0], token[1]))
                                        .collect::<Vec<_>>()
                                        .join(" "),
                                )
                                .monospace(),
                            )
                            .wrap(),
                        );
                    });
            });
    }
}

fn draw_condition_dependencies(
    ui: &mut egui::Ui,
    program: &[[u32; 2]],
    catalog: &Catalog,
    snapshot: Option<&CollectionStateSnapshot>,
    state: &mut ProgressionInspectorState,
) {
    let dependencies = program
        .iter()
        .filter(|token| matches!(token[0], 1 | 10 | 12))
        .collect::<Vec<_>>();
    if dependencies.is_empty() {
        return;
    }
    egui::CollapsingHeader::new(format!("Evaluated dependencies ({})", dependencies.len()))
        .default_open(true)
        .show(ui, |ui| {
            for token in dependencies {
                ui.horizontal_wrapped(|ui| {
                    draw_condition_token_resolution(ui, token, catalog, state);
                    ui.label(egui::RichText::new("Current:").weak());
                    ui.monospace(condition_dependency_value(token, catalog, snapshot));
                });
            }
        });
}

fn condition_dependency_value(
    token: &[u32; 2],
    catalog: &Catalog,
    snapshot: Option<&CollectionStateSnapshot>,
) -> String {
    let Some(snapshot) = snapshot else {
        return "save state unavailable".into();
    };
    let index = token[1] as usize;
    match token[0] {
        1 => catalog.unlock_flag_definition(index).map_or_else(
            || "definition unavailable".into(),
            |definition| snapshot.flag_text(index, definition),
        ),
        10 => catalog.unlock_value_definition(index).map_or_else(
            || "definition unavailable".into(),
            |definition| snapshot.value_text(index, definition),
        ),
        12 => catalog.objective_definition(index).map_or_else(
            || "objective unavailable".into(),
            |objective| {
                objective
                    .related_unlock_value_definition_index
                    .map(usize::from)
                    .and_then(|definition_index| {
                        catalog
                            .unlock_value_definition(definition_index)
                            .map(|definition| snapshot.value_text(definition_index, definition))
                    })
                    .unwrap_or_else(|| "objective value unavailable".into())
            },
        ),
        _ => "not a dependency".into(),
    }
}

fn draw_condition_token_resolution(
    ui: &mut egui::Ui,
    token: &[u32; 2],
    catalog: &Catalog,
    state: &mut ProgressionInspectorState,
) {
    let resolution = condition_token_resolution(token[0], token[1], catalog);
    if let Some(selection) = condition_token_selection(token[0], token[1], catalog) {
        if ui
            .add(egui::Button::new(crate::app::ui::destiny_text(ui, resolution)).frame(false))
            .on_hover_text("Open referenced metadata")
            .clicked()
        {
            state.open(selection);
        }
    } else {
        ui.label(crate::app::ui::destiny_text(ui, resolution));
    }
}

fn condition_token_selection(
    kind: u32,
    operand: u32,
    catalog: &Catalog,
) -> Option<MetadataSelection> {
    let index = operand as usize;
    match kind {
        1 => catalog
            .unlock_flag_definition(index)
            .map(|_| MetadataSelection::FlagDefinition(index)),
        10 => catalog
            .unlock_value_definition(index)
            .map(|_| MetadataSelection::ValueDefinition(index)),
        12 => catalog
            .objective_definition(index)
            .and_then(|objective| objective.related_unlock_value_definition_index)
            .map(usize::from)
            .and_then(|definition_index| {
                catalog
                    .unlock_value_definition(definition_index)
                    .map(|_| MetadataSelection::ValueDefinition(definition_index))
            }),
        _ => None,
    }
}

pub(in crate::app) fn condition_opcode_label(kind: u32) -> String {
    match kind {
        1 => "Flag reference (1)".into(),
        2 => "Not (2)".into(),
        3 => "Or (3)".into(),
        4 => "And (4)".into(),
        8 => "Equal (8)".into(),
        9 => "Not equal (9)".into(),
        10 => "Value reference (10)".into(),
        11 => "Literal (11)".into(),
        12 => "Objective reference (12)".into(),
        13 => "Greater than (13)".into(),
        14 => "Greater than or equal (14)".into(),
        15 => "Less than (15)".into(),
        22 => "Literal encoding (22)".into(),
        _ => format!("Undecoded ({kind})"),
    }
}

pub(in crate::app) fn condition_token_resolution(
    kind: u32,
    operand: u32,
    catalog: &Catalog,
) -> String {
    let index = operand as usize;
    if kind == 12 {
        let Some(objective) = catalog.objective_definition(index) else {
            return format!("Objective #{index} unavailable");
        };
        return format!(
            "Objective #{index} · {} · 0x{:08X}",
            resolved_objective_table_text(catalog, objective, None),
            objective.hash
        );
    }
    let definition = match kind {
        1 => catalog.unlock_flag_definition(index),
        10 => catalog.unlock_value_definition(index),
        _ => return "-".into(),
    };
    let Some(definition) = definition else {
        return format!("Definition #{index} unavailable");
    };
    let identity = definition_name(definition)
        .or_else(|| catalog.display_name(definition.hash))
        .unwrap_or("<not resolved>");
    let slot = definition.compact_slot.map_or_else(
        || "unbanked".into(),
        |slot| format!("bank {} · slot {slot}", definition.bank()),
    );
    format!("#{index} · {identity} · 0x{:08X} · {slot}", definition.hash)
}

#[cfg(test)]
mod tests {
    use crate::catalog::{ProgressionContextDef, ProgressionContextKind, UnlockDefinition};

    use super::{
        StackValue, compare_values, condition_opcode_label, definition_has_undecoded_opcodes,
        logical_values,
    };

    #[test]
    fn objective_reference_opcode_is_decoded() {
        let definition = UnlockDefinition {
            tested_by: vec![ProgressionContextDef {
                hash: 0,
                kind: ProgressionContextKind::ExpressionMapping,
                name: String::new(),
                type_name: String::new(),
                description: String::new(),
                paths: Vec::new(),
                condition_programs: vec![vec![[12, 91]]],
            }],
            ..UnlockDefinition::default()
        };

        assert!(!definition_has_undecoded_opcodes(&definition));
        assert_eq!(condition_opcode_label(15), "Less than (15)");
        assert_eq!(condition_opcode_label(4), "And (4)");
        assert_eq!(condition_opcode_label(9), "Not equal (9)");
    }

    #[test]
    fn condition_comparisons_preserve_rpn_operand_order() {
        assert_eq!(
            compare_values(13, StackValue::Int(10), StackValue::Int(4)),
            Some(true)
        );
        assert_eq!(
            compare_values(15, StackValue::Int(10), StackValue::Int(4)),
            Some(false)
        );
        assert_eq!(
            compare_values(8, StackValue::Bool(true), StackValue::Bool(true)),
            Some(true)
        );
        assert_eq!(
            compare_values(13, StackValue::Bool(true), StackValue::Bool(false)),
            Some(true)
        );
    }

    #[test]
    fn logical_evaluation_short_circuits_unknown_dependencies() {
        assert_eq!(
            logical_values(
                3,
                StackValue::Unknown("missing flag".into()),
                StackValue::Bool(true),
            ),
            StackValue::Bool(true)
        );
        assert_eq!(
            logical_values(
                4,
                StackValue::Unknown("missing flag".into()),
                StackValue::Bool(false),
            ),
            StackValue::Bool(false)
        );
    }
}

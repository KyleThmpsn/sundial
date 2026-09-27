//! Condition-program decoding and inspector navigation.

use eframe::egui;

use crate::app::{
    collections_page::{
        ExpressionValue, evaluate_expression_value_with, is_supported_condition_instruction,
    },
    progression::CollectionStateSnapshot,
};
use crate::catalog::{Catalog, CollectionConditionTokenDef, UnlockDefinition};

use super::{
    definitions::{definition_name, storage_text},
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
    is_supported_condition_instruction(opcode)
}

pub(super) fn direct_value_comparison(
    program: &[[u32; 2]],
    definition_index: usize,
    forced_value: i32,
) -> Option<(String, bool)> {
    let (left, right, operator) = match program {
        [left, right, operator] => (*left, *right, *operator),
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
        (6 | 9, _) => (format!("≠ {literal}"), forced_value != literal),
        (13, true) => (format!("> {literal}"), forced_value > literal),
        (13, false) => (format!("< {literal}"), forced_value < literal),
        (14, true) => (format!("≥ {literal}"), forced_value >= literal),
        (14, false) => (format!("≤ {literal}"), forced_value <= literal),
        (15, true) => (format!("< {literal}"), forced_value < literal),
        (15, false) => (format!("> {literal}"), forced_value > literal),
        (16, true) => (format!("≤ {literal}"), forced_value <= literal),
        (16, false) => (format!("≥ {literal}"), forced_value >= literal),
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

pub(super) fn evaluate_condition_program(
    program: &[[u32; 2]],
    catalog: &Catalog,
    snapshot: Option<&CollectionStateSnapshot>,
) -> ConditionEvaluation {
    let tokens = program
        .iter()
        .map(|&[kind, operand]| CollectionConditionTokenDef { kind, operand })
        .collect::<Vec<_>>();
    match evaluate_expression_value_with(
        &tokens,
        catalog.shared_expression_pool(),
        |index| {
            catalog
                .unlock_flag_definition(index)
                .and_then(|_| snapshot.and_then(|state| state.evaluated_flag(index, catalog)))
        },
        |index| {
            catalog
                .unlock_value_definition(index)
                .and_then(|_| snapshot.and_then(|state| state.evaluated_value(index, catalog)))
        },
    ) {
        Some(ExpressionValue::Boolean(true)) => ConditionEvaluation::Passed,
        Some(ExpressionValue::Boolean(false)) => ConditionEvaluation::Failed,
        Some(ExpressionValue::Number(value)) => ConditionEvaluation::Value(value),
        Some(ExpressionValue::Unknown) => {
            ConditionEvaluation::Unresolved("Referenced state unavailable".into())
        }
        None => ConditionEvaluation::Unresolved("Unsupported or malformed".into()),
    }
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
        egui::CollapsingHeader::new(format!("Condition Program {}", program_index + 1))
            .id_salt((id_source, owner_hash, program_index))
            .show(ui, |ui| {
                let evaluation = evaluate_condition_program(program, catalog, snapshot);
                ui.horizontal_wrapped(|ui| {
                    ui.strong("Effective Result");
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
                            ui.strong("Referenced Entry");
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
                                ui.weak("Operand");
                                ui.monospace(token[1].to_string());
                            });
                            ui.horizontal_wrapped(|ui| {
                                ui.weak("Referenced Entry");
                                draw_condition_token_resolution(ui, token, catalog, state);
                            });
                        });
                    }
                }
                egui::CollapsingHeader::new("Raw Opcodes")
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
    egui::CollapsingHeader::new(format!("Evaluated Dependencies ({})", dependencies.len()))
        .default_open(true)
        .show(ui, |ui| {
            for token in dependencies {
                ui.horizontal_wrapped(|ui| {
                    draw_condition_token_resolution(ui, token, catalog, state);
                    ui.weak("Current:");
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
            |definition| {
                format!(
                    "{} · Saved: {}",
                    snapshot
                        .evaluated_flag(index, catalog)
                        .map_or_else(|| "Unresolved".into(), |value| value.to_string()),
                    snapshot.flag_text(index, definition)
                )
            },
        ),
        10 => catalog.unlock_value_definition(index).map_or_else(
            || "definition unavailable".into(),
            |definition| {
                format!(
                    "{} · Saved: {}",
                    snapshot
                        .evaluated_value(index, catalog)
                        .map_or_else(|| "Unresolved".into(), |value| value.to_string()),
                    snapshot.value_text(index, definition)
                )
            },
        ),
        12 => {
            let Some(program) = catalog.shared_expression(index) else {
                return "shared expression unavailable".into();
            };
            match evaluate_expression_value_with(
                program,
                catalog.shared_expression_pool(),
                |definition_index| {
                    catalog
                        .unlock_flag_definition(definition_index)
                        .and_then(|_| snapshot.evaluated_flag(definition_index, catalog))
                },
                |definition_index| {
                    catalog
                        .unlock_value_definition(definition_index)
                        .and_then(|_| snapshot.evaluated_value(definition_index, catalog))
                },
            ) {
                Some(ExpressionValue::Boolean(value)) => value.to_string(),
                Some(ExpressionValue::Number(value)) => value.to_string(),
                Some(ExpressionValue::Unknown) => "referenced state unavailable".into(),
                None => "unsupported or malformed".into(),
            }
        }
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
        _ => None,
    }
}

pub(in crate::app) fn condition_opcode_label(kind: u32) -> String {
    match kind {
        1 => "Flag Reference (1)".into(),
        2 => "Not (2)".into(),
        3 => "Or (3)".into(),
        4 => "And (4)".into(),
        5 => "Nor (5)".into(),
        6 => "Not Equal (6)".into(),
        7 => "Nand (7)".into(),
        8 => "Equal (8)".into(),
        9 => "Not Equal (9)".into(),
        10 => "Value Reference (10)".into(),
        11 => "Literal (11)".into(),
        12 => "Shared Expression (12)".into(),
        13 => "Greater Than (13)".into(),
        14 => "Greater Than or Equal (14)".into(),
        15 => "Less Than (15)".into(),
        16 => "Less Than or Equal (16)".into(),
        17 => "Add (17)".into(),
        18 => "Subtract (18)".into(),
        19 => "Multiply (19)".into(),
        20 => "Divide (20)".into(),
        21 => "Modulo (21)".into(),
        22 => "Negate Number (22)".into(),
        23 => "FNV-1a Hash (23)".into(),
        24 => "FNV-1a Combine (24)".into(),
        28 => "Bitwise Not (28)".into(),
        25 => "Bitwise And (25)".into(),
        26 => "Bitwise Or (26)".into(),
        27 => "Bitwise Xor (27)".into(),
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
        return if catalog.shared_expression(index).is_some() {
            format!("Shared expression pool row #{index}")
        } else {
            format!("Shared expression pool row #{index} unavailable")
        };
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
        .unwrap_or(crate::app::inspector::UNNAMED);
    let storage = storage_text(definition, kind == 10);
    format!(
        "#{index} · {identity} · 0x{:08X} · {storage}",
        definition.hash
    )
}

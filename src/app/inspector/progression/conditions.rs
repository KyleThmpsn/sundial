//! Condition-program decoding and inspector navigation.

use eframe::egui;

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

pub(super) fn draw_condition_programs(
    ui: &mut egui::Ui,
    id_source: &'static str,
    owner_hash: u64,
    programs: &[Vec<[u32; 2]>],
    catalog: &Catalog,
    state: &mut ProgressionInspectorState,
) {
    for (program_index, program) in programs.iter().enumerate() {
        egui::CollapsingHeader::new(format!("Condition program {}", program_index + 1))
            .id_salt((id_source, owner_hash, program_index))
            .show(ui, |ui| {
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

fn draw_condition_token_resolution(
    ui: &mut egui::Ui,
    token: &[u32; 2],
    catalog: &Catalog,
    state: &mut ProgressionInspectorState,
) {
    let resolution = condition_token_resolution(token[0], token[1], catalog);
    if let Some(selection) = condition_token_selection(token[0], token[1], catalog) {
        if ui
            .add(egui::Button::new(resolution).frame(false))
            .on_hover_text("Open referenced metadata")
            .clicked()
        {
            state.open(selection);
        }
    } else {
        ui.label(resolution);
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

    use super::{condition_opcode_label, definition_has_undecoded_opcodes};

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
}

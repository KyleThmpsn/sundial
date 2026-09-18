use super::*;
use super::{hierarchy::*, mutations::*, state::*};

pub(super) fn add_definition_label(
    catalog: &Catalog,
    definition: &UnlockDefinition,
    objective: Option<&ObjectiveDef>,
) -> String {
    if let Some(name) =
        definition_name(definition).or_else(|| catalog.display_name(definition.hash))
    {
        return name.to_owned();
    }
    if let Some(objective) = objective {
        return resolved_objective_table_text(catalog, objective, Some(definition));
    }
    if let Some(context) = definition_context_lines(definition).first() {
        return context.text();
    }
    definition_hash_hex_text(definition)
}

pub(super) fn add_definition_tooltip(
    definition_index: usize,
    definition: &UnlockDefinition,
    objective: Option<&ObjectiveDef>,
) -> String {
    let mut lines = vec![definition_identity(definition_index, definition)];
    if let Some(name) = definition_name(definition) {
        lines.push(format!("Name: {name}"));
    }
    if let Some(objective) = objective {
        lines.push(format!("Objective: {}", objective_description(objective)));
        lines.push(format!(
            "Completion value: {}",
            objective_target_text(objective)
        ));
    }
    lines.push(definition_metadata_tooltip(definition));
    lines.join("\n")
}

pub(super) fn draw_add_investment_window(
    ctx: &egui::Context,
    document: &mut Value,
    investment: &InvestmentPolicy,
    catalog: &Catalog,
    state: &mut UiState,
) -> bool {
    if state.read_only {
        state.add_open = false;
        return false;
    }
    if !state.add_open {
        return false;
    }
    let is_value = state.investment_table == InvestmentTable::ValueOverrides;
    let definitions = if is_value {
        catalog.unlock_value_definitions()
    } else {
        catalog.unlock_flag_definitions()
    };
    let occupied = if is_value {
        investment
            .value_overrides
            .iter()
            .map(|row| row.definition_index)
            .collect::<HashSet<_>>()
    } else {
        investment
            .flag_overrides
            .iter()
            .map(|row| row.definition_index)
            .collect::<HashSet<_>>()
    };
    let query = state.add_query.trim().to_lowercase();
    let candidates =
        definitions
            .iter()
            .enumerate()
            .filter(|(index, definition)| {
                !occupied.contains(index)
                    && (query.is_empty()
                        || definition_matches(&query, *index, definition)
                        || (is_value
                            && catalog.objective_for_unlock_value(*index).is_some_and(
                                |objective| resolved_objective_matches(catalog, &query, objective),
                            )))
            })
            .collect::<Vec<_>>();

    let mut open = state.add_open;
    let mut selection = None;
    egui::Window::new(format!("Add {}", state.investment_table.label()))
        .id(egui::Id::new("progression_add_investment"))
        .open(&mut open)
        .collapsible(false)
        .default_width(560.0)
        .default_height(480.0)
        .show(ctx, |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut state.add_query)
                    .hint_text("Filter package definitions…")
                    .desired_width(f32::INFINITY),
            );
            ui.horizontal(|ui| {
                ui.label(if is_value {
                    "Value"
                } else {
                    "Logical flag value"
                });
                let drag = egui::DragValue::new(&mut state.add_value).speed(1.0);
                ui.add(if is_value {
                    drag.range(i32::MIN..=i32::MAX)
                } else {
                    drag.range(0..=i32::from(FAMILY5_FLAG_VALUE_MAXIMUM))
                });
            });
            ui.weak(format!("{} available", candidates.len()));
            ui.separator();
            if candidates.is_empty() {
                ui.weak("No matching definitions");
                return;
            }
            egui::ScrollArea::vertical()
                .id_salt("progression_add_investment_rows")
                .auto_shrink([false, false])
                .show_rows(ui, 34.0, candidates.len(), |ui, range| {
                    for row in range {
                        let (definition_index, definition) = candidates[row];
                        ui.allocate_ui_with_layout(
                            egui::vec2(ui.available_width(), 30.0),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                let button_width = 44.0;
                                let index_width = 70.0;
                                let label_width = (ui.available_width()
                                    - button_width
                                    - index_width
                                    - ui.spacing().item_spacing.x * 2.0)
                                    .max(120.0);
                                let objective = if is_value {
                                    catalog.objective_for_unlock_value(definition_index)
                                } else {
                                    None
                                };
                                ui.add_sized(
                                    [label_width, 24.0],
                                    egui::Label::new(add_definition_label(
                                        catalog, definition, objective,
                                    ))
                                    .truncate(),
                                )
                                .on_hover_text(
                                    add_definition_tooltip(definition_index, definition, objective),
                                );
                                ui.add_sized(
                                    [index_width, 24.0],
                                    egui::Label::new(
                                        egui::RichText::new(format!("#{definition_index}"))
                                            .monospace(),
                                    ),
                                );
                                if ui.small_button("Add").clicked() {
                                    selection = Some(definition_index);
                                }
                            },
                        );
                    }
                });
        });
    state.add_open = open;
    let Some(definition_index) = selection else {
        return false;
    };
    state.add_open = false;
    let changed = set_investment_override(
        document,
        state.investment_table,
        definition_index,
        state.add_value,
    )
    .changed();
    if changed {
        state.last_investment_change = Some(match state.investment_table {
            InvestmentTable::FlagOverrides => InvestmentUndo::Flag {
                definition_index,
                previous: None,
            },
            InvestmentTable::ValueOverrides => InvestmentUndo::Value {
                definition_index,
                previous: None,
            },
        });
    }
    changed
}

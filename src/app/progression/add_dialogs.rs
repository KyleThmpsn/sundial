use super::*;
use super::{hierarchy::*, mutations::*, state::*};

#[derive(Clone, Copy)]
pub(super) struct AddTableSpec {
    id: &'static str,
    bank: u8,
    capacity: usize,
    value: bool,
}

pub(super) fn add_table_spec(table: UnlockTable) -> AddTableSpec {
    match table {
        UnlockTable::AccountFlagRuns => AddTableSpec {
            id: "account_flag_runs",
            bank: ACCOUNT_FLAG_BANK,
            capacity: ACCOUNT_FLAG_CAPACITY,
            value: false,
        },
        UnlockTable::ProfileFlagRuns => AddTableSpec {
            id: "profile_flag_runs",
            bank: PROFILE_FLAG_BANK,
            capacity: PROFILE_FLAG_CAPACITY,
            value: false,
        },
        UnlockTable::CharacterFlags => AddTableSpec {
            id: "character_flags",
            bank: CHARACTER_FLAG_BANK,
            capacity: CHARACTER_FLAG_CAPACITY,
            value: false,
        },
        UnlockTable::ObjectiveValues => AddTableSpec {
            id: "objective_values",
            bank: ACCOUNT_OBJECTIVE_BANK,
            capacity: OBJECTIVE_VALUE_CAPACITY,
            value: true,
        },
        UnlockTable::CharacterObjectFlagRuns => AddTableSpec {
            id: "character_object_flag_runs",
            bank: CHARACTER_OBJECT_FLAG_BANK,
            capacity: CHARACTER_OBJECT_FLAG_CAPACITY,
            value: false,
        },
        UnlockTable::CharacterObjectObjectiveValues => AddTableSpec {
            id: "character_object_objective_values",
            bank: CHARACTER_OBJECTIVE_BANK,
            capacity: CHARACTER_OBJECT_VALUE_CAPACITY,
            value: true,
        },
        UnlockTable::AccountProgressions
        | UnlockTable::CharacterProgressions
        | UnlockTable::UnreplicatedProgressions
        | UnlockTable::FlagDefinitions
        | UnlockTable::ValueDefinitions
        | UnlockTable::StoredValues => {
            unreachable!("progression tables use their package definition picker")
        }
    }
}

pub(super) fn draw_add_unlock_window(
    ctx: &egui::Context,
    document: &mut Value,
    unlocks: &UnlockPolicy,
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
    if state.unlock_table.field_name().is_none() {
        state.add_open = false;
        return false;
    }
    if matches!(
        state.unlock_table,
        UnlockTable::AccountProgressions | UnlockTable::CharacterProgressions
    ) {
        return draw_add_progression_window(ctx, document, unlocks, catalog, state);
    }
    let spec = add_table_spec(state.unlock_table);
    let occupied = occupied_slots(unlocks, state.unlock_table, spec.capacity);
    let definitions = if spec.value {
        catalog.unlock_value_definitions()
    } else {
        catalog.unlock_flag_definitions()
    };
    let query = state.add_query.trim().to_lowercase();
    let candidates =
        definitions
            .iter()
            .enumerate()
            .filter_map(|(index, definition)| {
                let slot = usize::from(definition.compact_slot?);
                (definition.bank() == spec.bank
                    && slot < spec.capacity
                    && !occupied.get(slot).copied().unwrap_or(false)
                    && (query.is_empty()
                        || definition_matches(&query, index, definition)
                        || slot.to_string().contains(&query)
                        || (spec.value
                            && catalog.objective_for_unlock_value(index).is_some_and(
                                |objective| resolved_objective_matches(catalog, &query, objective),
                            ))))
                .then_some((index, slot, definition))
            })
            .collect::<Vec<_>>();

    let mut open = state.add_open;
    let mut selection = None;
    egui::Window::new(format!("Add {}", state.unlock_table.label()))
        .id(egui::Id::new("progression_add_unlock"))
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
            if spec.value {
                ui.horizontal(|ui| {
                    ui.label("Initial value");
                    ui.add(
                        egui::DragValue::new(&mut state.add_value)
                            .speed(1.0)
                            .range(i32::MIN..=i32::MAX),
                    );
                });
            }
            ui.label(egui::RichText::new(format!("{} available", candidates.len())).weak());
            ui.separator();
            if candidates.is_empty() {
                ui.label(egui::RichText::new("No matching definitions").weak());
                return;
            }
            egui::ScrollArea::vertical()
                .id_salt("progression_add_unlock_rows")
                .auto_shrink([false, false])
                .show_rows(ui, 34.0, candidates.len(), |ui, range| {
                    for row in range {
                        let (definition_index, slot, definition) = candidates[row];
                        ui.allocate_ui_with_layout(
                            egui::vec2(ui.available_width(), 30.0),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                let button_width = 44.0;
                                let slot_width = 62.0;
                                let label_width = (ui.available_width()
                                    - button_width
                                    - slot_width
                                    - ui.spacing().item_spacing.x * 2.0)
                                    .max(120.0);
                                let label = add_definition_label(
                                    catalog,
                                    definition,
                                    if spec.value {
                                        catalog.objective_for_unlock_value(definition_index)
                                    } else {
                                        None
                                    },
                                );
                                ui.add_sized(
                                    [label_width, 24.0],
                                    egui::Label::new(label).truncate(),
                                )
                                .on_hover_text(
                                    add_definition_tooltip(
                                        definition_index,
                                        definition,
                                        if spec.value {
                                            catalog.objective_for_unlock_value(definition_index)
                                        } else {
                                            None
                                        },
                                    ),
                                );
                                ui.add_sized(
                                    [slot_width, 24.0],
                                    egui::Label::new(
                                        egui::RichText::new(format!("Slot {slot}")).monospace(),
                                    ),
                                );
                                if ui.small_button("Add").clicked() {
                                    selection = Some(slot);
                                }
                            },
                        );
                    }
                });
        });
    state.add_open = open;
    let Some(slot) = selection else {
        return false;
    };
    state.add_open = false;
    if spec.value {
        set_unlock_value(document, spec.id, slot, state.add_value)
    } else {
        set_unlock_flag(document, spec.id, slot, true)
    }
}

pub(super) fn draw_add_progression_window(
    ctx: &egui::Context,
    document: &mut Value,
    unlocks: &UnlockPolicy,
    catalog: &Catalog,
    state: &mut UiState,
) -> bool {
    let (id, scope, rows) = match state.unlock_table {
        UnlockTable::AccountProgressions => (
            "account_progressions",
            ProgressionScope::Account,
            &unlocks.account_progressions,
        ),
        UnlockTable::CharacterProgressions => (
            "character_progressions",
            ProgressionScope::Character,
            &unlocks.character_progressions,
        ),
        _ => return false,
    };
    let occupied = rows
        .iter()
        .map(|row| row.definition_index)
        .collect::<HashSet<_>>();
    let query = state.add_query.trim().to_lowercase();
    let candidates = catalog
        .progression_definitions()
        .iter()
        .filter(|definition| {
            definition.scope == scope
                && !occupied.contains(&usize::from(definition.definition_index))
                && (query.is_empty() || progression_definition_matches(&query, definition))
        })
        .collect::<Vec<_>>();
    let mut open = state.add_open;
    let mut selection = None;
    egui::Window::new(format!("Add {}", state.unlock_table.label()))
        .id(egui::Id::new("progression_add_progression"))
        .open(&mut open)
        .collapsible(false)
        .default_width(520.0)
        .default_height(440.0)
        .show(ctx, |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut state.add_query)
                    .hint_text("Filter progression definitions…")
                    .desired_width(f32::INFINITY),
            );
            ui.horizontal(|ui| {
                for lane in 0..3 {
                    ui.label(if lane == 0 {
                        "Progress"
                    } else if lane == 1 {
                        "Lane 1"
                    } else {
                        "Lane 2"
                    });
                    ui.add(
                        egui::DragValue::new(&mut state.add_progression_lanes[lane])
                            .speed(1.0)
                            .range(i32::MIN..=i32::MAX),
                    );
                }
            });
            ui.label(egui::RichText::new(format!("{} available", candidates.len())).weak());
            ui.separator();
            egui::ScrollArea::vertical()
                .id_salt("progression_add_progression_rows")
                .show_rows(ui, 30.0, candidates.len(), |ui, range| {
                    for row in range {
                        let definition = candidates[row];
                        ui.horizontal(|ui| {
                            ui.monospace(format!("#{}", definition.definition_index));
                            draw_hash_link(ui, definition.hash, format_hash_hex(definition.hash));
                            ui.label(progression_display_name(definition).map_or_else(
                                || egui::RichText::new("-").weak(),
                                egui::RichText::new,
                            ));
                            ui.label(definition.scope_slot.map_or_else(
                                || "<unreplicated>".into(),
                                |slot| format!("Slot {slot}"),
                            ));
                            if ui.small_button("Add").clicked() {
                                selection = Some(usize::from(definition.definition_index));
                            }
                        });
                    }
                });
        });
    state.add_open = open;
    let Some(definition_index) = selection else {
        return false;
    };
    state.add_open = false;
    set_progression_value(document, id, definition_index, state.add_progression_lanes)
}

pub(super) fn occupied_slots(
    unlocks: &UnlockPolicy,
    table: UnlockTable,
    capacity: usize,
) -> Vec<bool> {
    let mut occupied = vec![false; capacity];
    let slots = match table {
        UnlockTable::AccountFlagRuns => expanded_flag_slots(&unlocks.account_flag_runs, capacity),
        UnlockTable::ProfileFlagRuns => expanded_flag_slots(&unlocks.profile_flag_runs, capacity),
        UnlockTable::CharacterFlags => unlocks
            .character_flags
            .iter()
            .map(|row| row.index)
            .collect(),
        UnlockTable::ObjectiveValues => unlocks
            .objective_values
            .iter()
            .map(|row| row.index)
            .collect(),
        UnlockTable::CharacterObjectFlagRuns => {
            expanded_flag_slots(&unlocks.character_object_flag_runs, capacity)
        }
        UnlockTable::CharacterObjectObjectiveValues => unlocks
            .character_objective_values
            .iter()
            .map(|row| row.index)
            .collect(),
        UnlockTable::AccountProgressions => unlocks
            .account_progressions
            .iter()
            .map(|row| row.definition_index)
            .collect(),
        UnlockTable::CharacterProgressions => unlocks
            .character_progressions
            .iter()
            .map(|row| row.definition_index)
            .collect(),
        UnlockTable::UnreplicatedProgressions
        | UnlockTable::FlagDefinitions
        | UnlockTable::ValueDefinitions
        | UnlockTable::StoredValues => {
            unreachable!("read-only tables have no editable settings bank")
        }
    };
    for slot in slots {
        if let Some(value) = occupied.get_mut(slot) {
            *value = true;
        }
    }
    occupied
}

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
            ui.label(egui::RichText::new(format!("{} available", candidates.len())).weak());
            ui.separator();
            if candidates.is_empty() {
                ui.label(egui::RichText::new("No matching definitions").weak());
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
    );
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

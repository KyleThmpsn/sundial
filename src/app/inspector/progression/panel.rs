//! Progression metadata inspector workspace and navigation controller.

use eframe::egui;
use serde_json::Value;

use crate::{
    app::{
        glyphs::Glyph,
        inspector::{
            heading as inspector_heading, metadata_field, metadata_section, request_definition,
            uses_side_workspace, workspace as inspector_workspace,
        },
        progression::{CollectionStateSnapshot, collection_state_snapshot},
        ui::{glyph_button, toolbar},
    },
    catalog::{Catalog, ObjectiveDef, UnlockDefinition},
    hash::format_hash_hex,
};

use super::{
    conditions::{ConditionEvaluation, condition_token_resolution, evaluate_condition_program},
    definition_details::draw_unlock_definition_metadata,
    definitions::definition_name,
    objective_details::draw_objective_metadata,
    objectives::{objective_owner_display_label, preferred_objective_owner},
    overrides::draw_override_metadata,
    state::{MetadataSelection, ProgressionInspectorState},
};

pub(in crate::app) fn draw_progression_metadata_workspace(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    document: &Value,
    state: &mut ProgressionInspectorState,
    hash_inspector_open: bool,
) -> bool {
    if !state.is_open() {
        return false;
    }
    if !hash_inspector_open && ui.ctx().input(|input| input.key_pressed(egui::Key::Escape)) {
        state.close();
        return false;
    }

    let snapshot = collection_state_snapshot(document);
    let compact_width = !uses_side_workspace(ui.available_width());
    if state.full_width || compact_width {
        egui::Frame::group(ui.style())
            .inner_margin(egui::Margin::symmetric(12, 8))
            .show(ui, |ui| {
                draw_metadata_panel(ui, catalog, snapshot.as_ref(), state, !compact_width)
            });
        state.is_open()
    } else {
        inspector_workspace(
            ui,
            "progression_inspection_workspace",
            "progression_inspection_workspace_compact",
            |ui, _placement| draw_metadata_panel(ui, catalog, snapshot.as_ref(), state, true),
        );
        false
    }
}

fn draw_metadata_panel(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    snapshot: Option<&CollectionStateSnapshot>,
    state: &mut ProgressionInspectorState,
    allow_layout_toggle: bool,
) {
    let Some(selection) = state.selection() else {
        return;
    };
    let (title, definition, objectives) = selection_metadata(selection, catalog);
    let semantic_name = definition
        .and_then(definition_name)
        .map(str::to_owned)
        .or_else(|| {
            objectives.first().and_then(|objective| {
                (!objective.name.trim().is_empty())
                    .then(|| objective.name.trim().to_owned())
                    .or_else(|| {
                        preferred_objective_owner(objective).and_then(objective_owner_display_label)
                    })
            })
        });
    let title = semantic_name.map_or(title.clone(), |name| format!("{title} · {name}"));
    let mut navigate_back = state.can_go_back()
        && ui.input(|input| input.modifiers.alt && input.key_pressed(egui::Key::ArrowLeft));
    let close = inspector_heading(ui, title);
    let mut navigate_forward = state.next().is_some()
        && ui.input(|input| input.modifiers.alt && input.key_pressed(egui::Key::ArrowRight));
    ui.add_space(6.0);
    toolbar(ui, |ui| {
        let back_label = state.previous().map_or_else(
            || "No previous inspection".to_owned(),
            |previous| {
                format!(
                    "Back to {} (Alt+Left)",
                    metadata_selection_short_label(previous, catalog)
                )
            },
        );
        if ui
            .add_enabled_ui(state.can_go_back(), |ui| {
                glyph_button(ui, Glyph::ChevronLeft, &back_label)
            })
            .inner
            .clicked()
        {
            navigate_back = true;
        }
        let forward_label = state.next().map_or_else(
            || "No next inspection".to_owned(),
            |next| {
                format!(
                    "Forward to {} (Alt+Right)",
                    metadata_selection_short_label(next, catalog)
                )
            },
        );
        if ui
            .add_enabled_ui(state.next().is_some(), |ui| {
                glyph_button(ui, Glyph::ChevronRight, &forward_label)
            })
            .inner
            .clicked()
        {
            navigate_forward = true;
        }
        if allow_layout_toggle
            && ui
                .button(if state.full_width {
                    "Split View"
                } else {
                    "Expand"
                })
                .clicked()
        {
            state.full_width = !state.full_width;
        }
        if ui.button("Reveal in Table").clicked() {
            state.request_reveal();
        }
        ui.menu_button("More", |ui| {
            if let Some(definition) = definition
                && ui.button("Open Definition Inspector").clicked()
            {
                request_definition(ui.ctx(), definition.hash);
                ui.close_menu();
            }
            if ui.button("Copy Technical Report").clicked() {
                ui.ctx().copy_text(progression_inspector_report(
                    selection,
                    definition,
                    &objectives,
                    catalog,
                    snapshot,
                ));
                ui.close_menu();
            }
        });
    });
    ui.separator();

    egui::ScrollArea::vertical()
        .id_salt("progression_metadata_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            metadata_section(ui, "Current State", |ui| {
                draw_effective_state_summary(
                    ui,
                    selection,
                    definition,
                    &objectives,
                    catalog,
                    snapshot,
                );
            });

            ui.add_space(10.0);
            egui::CollapsingHeader::new("Definition Details")
                .id_salt(("progression_definition_details", selection))
                .default_open(false)
                .show(ui, |ui| {
                    let Some(definition) = definition else {
                        ui.colored_label(
                            ui.visuals().warn_fg_color,
                            "This definition is not present in the scanned package table.",
                        );
                        return;
                    };
                    if matches!(
                        selection,
                        MetadataSelection::FlagOverride(_, _)
                            | MetadataSelection::ValueOverride(_, _)
                    ) {
                        metadata_section(ui, "Saved Override", |ui| {
                            draw_override_metadata(ui, selection, definition);
                        });
                        ui.add_space(10.0);
                    }
                    metadata_section(ui, "Definition and Dependencies", |ui| {
                        draw_unlock_definition_metadata(
                            ui,
                            selection.definition_index(),
                            definition,
                            catalog,
                            snapshot,
                            state,
                        );
                    });
                    for (objective_index, objective) in objectives.iter().enumerate() {
                        ui.add_space(10.0);
                        let heading = if objectives.len() == 1 {
                            "Objective".to_owned()
                        } else {
                            format!("Objective {}", objective_index + 1)
                        };
                        metadata_section(ui, &heading, |ui| {
                            draw_objective_metadata(
                                ui, objective, definition, catalog, snapshot, state,
                            );
                        });
                    }
                    if objectives.is_empty() && selection.is_value() {
                        ui.label(egui::RichText::new("No objective uses this value.").weak());
                    }
                });
        });
    if navigate_back {
        state.back();
    } else if navigate_forward {
        state.forward();
    } else if close {
        state.close();
    }
}

fn selection_metadata(
    selection: MetadataSelection,
    catalog: &Catalog,
) -> (String, Option<&UnlockDefinition>, Vec<&ObjectiveDef>) {
    match selection {
        MetadataSelection::FlagDefinition(index) | MetadataSelection::FlagOverride(index, _) => (
            format!("Unlock Flag Definition #{index}"),
            catalog.unlock_flag_definition(index),
            Vec::new(),
        ),
        MetadataSelection::ValueDefinition(index) | MetadataSelection::ValueOverride(index, _) => (
            format!("Unlock Value Definition #{index}"),
            catalog.unlock_value_definition(index),
            catalog.objectives_for_unlock_value(index),
        ),
    }
}

fn draw_effective_state_summary(
    ui: &mut egui::Ui,
    selection: MetadataSelection,
    definition: Option<&UnlockDefinition>,
    objectives: &[&ObjectiveDef],
    catalog: &Catalog,
    snapshot: Option<&CollectionStateSnapshot>,
) {
    let index = selection.definition_index();
    let current_state = definition.map_or_else(
        || "Definition unavailable".into(),
        |definition| selection_state_text(selection, definition, snapshot),
    );
    let evaluations = definition
        .into_iter()
        .flat_map(|definition| definition.tested_by.iter())
        .flat_map(|context| context.condition_programs.iter())
        .chain(
            objectives
                .iter()
                .flat_map(|objective| objective.condition_programs.iter()),
        )
        .map(|program| evaluate_condition_program(program, catalog, snapshot))
        .collect::<Vec<_>>();
    let resolved = evaluations
        .iter()
        .filter(|evaluation| evaluation.is_resolved())
        .count();
    let evaluation_coverage = if definition.is_none() {
        "Definition unavailable".to_owned()
    } else if snapshot.is_none() {
        "Save state is not loaded".to_owned()
    } else if evaluations.is_empty() {
        "No conditions to evaluate".to_owned()
    } else if resolved == evaluations.len() {
        format!("All {} conditions evaluated", evaluations.len())
    } else {
        format!("{resolved}/{} conditions evaluated", evaluations.len())
    };
    let outcome = if selection.is_value() {
        objective_completion_summary(selection, definition, objectives, snapshot)
    } else if current_state == "Set" || current_state == "Override 2" {
        "Stored unlock flag is set".into()
    } else if current_state == "Unset" || current_state == "Override 0" {
        "Stored unlock flag is unset".into()
    } else {
        "Stored unlock state cannot be stated conclusively".into()
    };

    ui.label(egui::RichText::new(&outcome).strong().size(17.0));
    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        draw_summary_fact(ui, "State", &current_state, true);
        draw_summary_fact(
            ui,
            "Conditions",
            &condition_evaluation_summary(&evaluations),
            false,
        );
    });
    ui.add_space(4.0);
    ui.label(egui::RichText::new(evaluation_coverage).weak());
    ui.label(
        egui::RichText::new(format!(
            "Source: {}",
            selection_provenance(selection, definition)
        ))
        .weak(),
    );

    ui.add_space(8.0);
    egui::Grid::new(("progression_effective_summary", selection))
        .num_columns(2)
        .spacing([16.0, 4.0])
        .show(ui, |ui| {
            metadata_field(ui, "Definition Index", index.to_string(), true);
            if let Some(definition) = definition {
                metadata_field(
                    ui,
                    "Definition Hash",
                    format_hash_hex(definition.hash),
                    true,
                );
                metadata_field(
                    ui,
                    "Storage",
                    definition.compact_slot.map_or_else(
                        || "Unbanked override index".into(),
                        |slot| format!("Bank {} · slot {slot}", definition.bank()),
                    ),
                    true,
                );
            }
        });
}

fn draw_summary_fact(ui: &mut egui::Ui, label: &str, value: &str, monospace: bool) {
    egui::Frame::NONE
        .inner_margin(egui::Margin::symmetric(6, 3))
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.label(egui::RichText::new(label).small().weak());
                let value = egui::RichText::new(value).strong();
                ui.label(if monospace { value.monospace() } else { value });
            });
        });
}

fn objective_completion_summary(
    selection: MetadataSelection,
    definition: Option<&UnlockDefinition>,
    objectives: &[&ObjectiveDef],
    snapshot: Option<&CollectionStateSnapshot>,
) -> String {
    let Some(definition) = definition else {
        return "Objective state unavailable".into();
    };
    let current =
        snapshot.and_then(|snapshot| snapshot.value(selection.definition_index(), definition));
    let Some(current) = current else {
        return "Objective value is not available in the current save".into();
    };
    if objectives.is_empty() {
        return format!("Current value is {current}; no completion target is linked");
    }
    let complete = objectives
        .iter()
        .filter(|objective| {
            if objective.is_counting_downward {
                current <= objective.completion_value
            } else {
                current >= objective.completion_value
            }
        })
        .count();
    let targets = objectives
        .iter()
        .map(|objective| objective.completion_value.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "Current value {current}; {complete}/{} linked objectives complete (targets: {targets})",
        objectives.len()
    )
}

fn condition_evaluation_summary(evaluations: &[ConditionEvaluation]) -> String {
    if evaluations.is_empty() {
        return "No conditions".into();
    }
    let passed = evaluations
        .iter()
        .filter(|result| matches!(result, ConditionEvaluation::Passed))
        .count();
    let failed = evaluations
        .iter()
        .filter(|result| matches!(result, ConditionEvaluation::Failed))
        .count();
    let unresolved = evaluations
        .iter()
        .filter(|result| matches!(result, ConditionEvaluation::Unresolved(_)))
        .count();
    format!("{passed} pass · {failed} fail · {unresolved} unresolved")
}

fn selection_state_text(
    selection: MetadataSelection,
    definition: &UnlockDefinition,
    snapshot: Option<&CollectionStateSnapshot>,
) -> String {
    let Some(snapshot) = snapshot else {
        return "Save state unavailable".into();
    };
    if selection.is_value() {
        snapshot.value_text(selection.definition_index(), definition)
    } else {
        snapshot.flag_text(selection.definition_index(), definition)
    }
}

fn selection_provenance(
    selection: MetadataSelection,
    definition: Option<&UnlockDefinition>,
) -> String {
    match selection {
        MetadataSelection::FlagOverride(_, value) => {
            format!("saved flag override · value {value}")
        }
        MetadataSelection::ValueOverride(_, value) => {
            format!("saved value override · value {value}")
        }
        MetadataSelection::FlagDefinition(_) | MetadataSelection::ValueDefinition(_) => definition
            .map_or_else(
                || "package definition; storage location unresolved".into(),
                |definition| {
                    definition.compact_slot.map_or_else(
                        || "package definition · unbanked Family 5 slot".into(),
                        |slot| format!("save state · bank {}, slot {slot}", definition.bank()),
                    )
                },
            ),
    }
}

fn progression_inspector_report(
    selection: MetadataSelection,
    definition: Option<&UnlockDefinition>,
    objectives: &[&ObjectiveDef],
    catalog: &Catalog,
    snapshot: Option<&CollectionStateSnapshot>,
) -> String {
    let mut lines = vec![
        "Sundial progression inspector".to_owned(),
        format!(
            "Selection: {}",
            metadata_selection_short_label(selection, catalog)
        ),
    ];
    if let Some(definition) = definition {
        lines.push(format!("Hash: {}", format_hash_hex(definition.hash)));
        lines.push(format!(
            "Current state: {}",
            selection_state_text(selection, definition, snapshot)
        ));
        lines.push(format!(
            "Provenance: {}",
            selection_provenance(selection, Some(definition))
        ));
        let evaluations = definition
            .tested_by
            .iter()
            .flat_map(|context| context.condition_programs.iter())
            .chain(
                objectives
                    .iter()
                    .flat_map(|objective| objective.condition_programs.iter()),
            )
            .map(|program| evaluate_condition_program(program, catalog, snapshot))
            .collect::<Vec<_>>();
        lines.push(format!(
            "Condition results: {}",
            condition_evaluation_summary(&evaluations)
        ));
        let unresolved = evaluations
            .iter()
            .filter(|evaluation| !evaluation.is_resolved())
            .count();
        lines.push(format!(
            "Evaluation coverage: {}",
            if snapshot.is_none() {
                "save state unavailable"
            } else if unresolved == 0 {
                "all conditions evaluated"
            } else {
                "unresolved dependencies remain"
            }
        ));
        for context in &definition.tested_by {
            for program in &context.condition_programs {
                for token in program {
                    if matches!(token[0], 1 | 10 | 12) {
                        lines.push(format!(
                            "Dependency: {}",
                            condition_token_resolution(token[0], token[1], catalog)
                        ));
                    }
                }
            }
        }
    } else {
        lines.push("Definition: unavailable".into());
    }
    lines.push(format!("Related objectives: {}", objectives.len()));
    for (index, objective) in objectives.iter().enumerate() {
        lines.push(format!(
            "Objective {}: {} · target {}",
            index + 1,
            if objective.name.trim().is_empty() {
                "<unnamed>"
            } else {
                &objective.name
            },
            objective.completion_value
        ));
        if selection.is_value()
            && let Some(definition) = definition
        {
            lines.push(format!(
                "Objective {} state: {}",
                index + 1,
                objective_completion_summary(selection, Some(definition), &[*objective], snapshot,)
            ));
        }
    }
    lines.join("\n")
}

fn metadata_selection_short_label(selection: MetadataSelection, catalog: &Catalog) -> String {
    let (kind, definition) = if selection.is_value() {
        (
            "Value",
            catalog.unlock_value_definition(selection.definition_index()),
        )
    } else {
        (
            "Flag",
            catalog.unlock_flag_definition(selection.definition_index()),
        )
    };
    let index = selection.definition_index();
    definition
        .and_then(|definition| {
            definition_name(definition).or_else(|| catalog.display_name(definition.hash))
        })
        .map_or_else(
            || format!("{kind} #{index}"),
            |name| format!("{kind} #{index} · {name}"),
        )
}

//! Progression metadata inspector workspace and navigation controller.

use eframe::egui;

use crate::catalog::Catalog;

use crate::app::{
    inspector::{heading as inspector_heading, metadata_section, workspace as inspector_workspace},
    ui::back_button,
};

use super::{
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
    state: &mut ProgressionInspectorState,
    hash_inspector_open: bool,
) {
    if !state.is_open() {
        return;
    }
    if !hash_inspector_open && ui.ctx().input(|input| input.key_pressed(egui::Key::Escape)) {
        state.close();
        return;
    }

    inspector_workspace(
        ui,
        "progression_inspection_workspace",
        "progression_inspection_workspace_compact",
        |ui, _placement| draw_metadata_panel(ui, catalog, state),
    );
}

fn draw_metadata_panel(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    state: &mut ProgressionInspectorState,
) {
    let Some(selection) = state.selection() else {
        return;
    };
    let (title, definition, objectives) = match selection {
        MetadataSelection::FlagDefinition(index) | MetadataSelection::FlagOverride(index, _) => (
            format!("Unlock flag definition #{index}"),
            catalog.unlock_flag_definition(index),
            Vec::new(),
        ),
        MetadataSelection::ValueDefinition(index) | MetadataSelection::ValueOverride(index, _) => (
            format!("Unlock value definition #{index}"),
            catalog.unlock_value_definition(index),
            catalog.objectives_for_unlock_value(index),
        ),
    };
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
    ui.add_space(6.0);
    if let Some(previous) = state.previous() {
        let previous_label = metadata_selection_short_label(previous, catalog);
        if back_button(ui, &previous_label).clicked() {
            navigate_back = true;
        }
    }
    ui.separator();
    egui::ScrollArea::vertical()
        .id_salt("progression_metadata_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            match definition {
                Some(definition) => {
                    let index = selection.definition_index();
                    if matches!(
                        selection,
                        MetadataSelection::FlagOverride(_, _)
                            | MetadataSelection::ValueOverride(_, _)
                    ) {
                        metadata_section(ui, "Account override", |ui| {
                            draw_override_metadata(ui, selection, definition);
                        });
                        ui.add_space(8.0);
                    }
                    metadata_section(ui, "Unlock definition", |ui| {
                        draw_unlock_definition_metadata(ui, index, definition, catalog, state);
                    });
                    for (objective_index, objective) in objectives.iter().enumerate() {
                        ui.add_space(8.0);
                        let heading = if objectives.len() == 1 {
                            "Related objective".to_owned()
                        } else {
                            format!("Related objective {}", objective_index + 1)
                        };
                        metadata_section(ui, &heading, |ui| {
                            draw_objective_metadata(ui, objective, definition, catalog, state);
                        });
                    }
                    if objectives.is_empty() && selection.is_value() {
                        ui.add_space(8.0);
                        ui.label(egui::RichText::new("No related objective definition").weak());
                    }
                }
                None => {
                    ui.colored_label(
                        ui.visuals().warn_fg_color,
                        "Definition index is not present in the scanned package table",
                    );
                }
            }
        });
    if navigate_back {
        state.back();
    } else if close {
        state.close();
    }
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

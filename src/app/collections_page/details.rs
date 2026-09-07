use eframe::egui;
use serde_json::Value;

use crate::{
    catalog::Catalog,
    hash::{format_hash_hex, format_hash_hex_and_decimal},
};

use super::{
    super::{
        inspector::{
            heading as inspector_heading, item_definition_name_cell,
            request_definition as request_hash_inspection, workspace as inspector_workspace,
        },
        progression::CollectionStateSnapshot,
    },
    CollectionStatusFilter, UiState,
    acquisition::{
        ACQUISITION_CONDITION_FIELD, acquisition_status, condition_field_label, condition_program,
        condition_token_label, condition_token_metadata, condition_token_state,
        draw_collection_acquisition_action, draw_condition_token_metadata, evaluate_expression,
    },
    collection_hash_cell, collection_matches,
    hierarchy::root_first_path,
};

pub(super) fn draw_collection_metadata_workspace(
    ui: &mut egui::Ui,
    document: &mut Value,
    catalog: &Catalog,
    snapshot: &CollectionStateSnapshot,
    state: &mut UiState,
) -> bool {
    if state.metadata_index.is_none() {
        return false;
    }
    if !state.hash_inspection.is_open()
        && ui.ctx().input(|input| input.key_pressed(egui::Key::Escape))
    {
        state.metadata_index = None;
        return false;
    }

    inspector_workspace(
        ui,
        "collection_inspection_workspace",
        "collection_inspection_workspace_compact",
        |ui, _placement| draw_collection_metadata_panel(ui, document, catalog, snapshot, state),
    )
}

fn draw_collection_metadata_panel(
    ui: &mut egui::Ui,
    document: &mut Value,
    catalog: &Catalog,
    snapshot: &CollectionStateSnapshot,
    state: &mut UiState,
) -> bool {
    let Some(index) = state.metadata_index else {
        return false;
    };
    let definition = catalog
        .collectibles()
        .iter()
        .find(|definition| definition.index == index);
    let title = definition
        .filter(|definition| !definition.name.trim().is_empty())
        .map_or_else(
            || format!("Collectible #{index}"),
            |definition| format!("Collectible #{index} · {}", definition.name),
        );
    let close = inspector_heading(ui, title);

    let Some(definition) = definition else {
        ui.separator();
        ui.label("Collectible definition is no longer available");
        if close {
            state.metadata_index = None;
        }
        return false;
    };
    let query = state.query.trim().to_lowercase();
    let visible_in_current_table = collection_matches(&query, definition, catalog)
        && state
            .status_filter
            .matches(acquisition_status(definition, snapshot, catalog).state);
    if !visible_in_current_table
        && ui
            .button("Reveal in Table")
            .on_hover_text("Clear collection filters and scroll to this collectible")
            .clicked()
    {
        state.query.clear();
        state.status_filter = CollectionStatusFilter::All;
        state.reveal_selection = true;
    }
    ui.separator();

    let mut changed = false;
    egui::ScrollArea::both()
        .id_salt("collection_metadata_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            egui::Grid::new("collection_metadata_summary")
                .num_columns(2)
                .spacing([16.0, 4.0])
                .show(ui, |ui| {
                    collection_metadata_field(ui, "Type", &definition.type_name);
                    collection_metadata_field(
                        ui,
                        "Current acquisition state",
                        acquisition_status(definition, snapshot, catalog).text,
                    );
                });
            changed |= draw_collection_acquisition_action(
                ui, document, definition, snapshot, catalog, state,
            );
            egui::CollapsingHeader::new("Definition identifiers")
                .id_salt(("collection_definition_identifiers", definition.index))
                .show(ui, |ui| {
                    egui::Grid::new(("collection_metadata_identifiers", definition.index))
                        .num_columns(2)
                        .spacing([16.0, 4.0])
                        .show(ui, |ui| {
                            collection_metadata_field(
                                ui,
                                "Collectible index",
                                definition.index.to_string(),
                            );
                            collection_hash_field(ui, "Collectible hash", definition.hash);
                            collection_metadata_field(
                                ui,
                                "Item definition index",
                                if definition.item_definition_index == u16::MAX {
                                    "<unavailable>".into()
                                } else {
                                    definition.item_definition_index.to_string()
                                },
                            );
                            collection_hash_field(ui, "Item definition hash", definition.item_hash);
                            collection_metadata_field(
                                ui,
                                "Material requirement set index",
                                definition.material_requirement_set_index.map_or_else(
                                    || "<unavailable>".into(),
                                    |index| index.to_string(),
                                ),
                            );
                            collection_hash_field(
                                ui,
                                "Material requirement set hash",
                                definition.material_requirement_set_hash,
                            );
                        });
                });
            if !definition.material_requirements.is_empty() {
                ui.add_space(6.0);
                egui::CollapsingHeader::new(format!(
                    "Material requirements ({})",
                    definition.material_requirements.len()
                ))
                .id_salt(("collection_material_requirements", definition.index))
                .default_open(true)
                .show(ui, |ui| {
                    if ui.available_width() >= 700.0 {
                        egui::Grid::new(("collection_material_requirement_rows", definition.index))
                            .num_columns(7)
                            .spacing([16.0, 3.0])
                            .striped(true)
                            .show(ui, |ui| {
                                ui.strong("Index");
                                ui.strong("Hash");
                                ui.strong("Name");
                                ui.strong("Quantity");
                                ui.strong("Delete").on_hover_text("Delete on action");
                                ui.strong("Omit").on_hover_text("Omit from requirements");
                                ui.strong("Condition");
                                ui.end_row();
                                for requirement in &definition.material_requirements {
                                    ui.monospace(requirement.item_definition_index.to_string());
                                    collection_hash_cell(ui, 104.0, requirement.item_hash);
                                    item_definition_name_cell(
                                        ui,
                                        catalog,
                                        requirement.item_hash,
                                        190.0,
                                    );
                                    ui.monospace(requirement.quantity.to_string());
                                    ui.label(if requirement.delete_on_action {
                                        "True"
                                    } else {
                                        "False"
                                    });
                                    ui.label(if requirement.omit_from_requirements {
                                        "True"
                                    } else {
                                        "False"
                                    });
                                    ui.monospace(format!("0x{:04X}", requirement.condition));
                                    ui.end_row();
                                }
                            });
                    } else {
                        for (requirement_index, requirement) in
                            definition.material_requirements.iter().enumerate()
                        {
                            if requirement_index > 0 {
                                ui.add_space(4.0);
                            }
                            ui.group(|ui| {
                                ui.set_min_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    ui.strong(format!(
                                        "Item definition #{}",
                                        requirement.item_definition_index
                                    ));
                                    collection_hash_cell(ui, 104.0, requirement.item_hash);
                                });
                                egui::Grid::new((
                                    "collection_material_requirement_compact",
                                    definition.index,
                                    requirement_index,
                                ))
                                .num_columns(2)
                                .spacing([16.0, 3.0])
                                .show(ui, |ui| {
                                    ui.label(egui::RichText::new("Name").weak());
                                    item_definition_name_cell(
                                        ui,
                                        catalog,
                                        requirement.item_hash,
                                        190.0,
                                    );
                                    ui.end_row();
                                    ui.label(egui::RichText::new("Quantity").weak());
                                    ui.monospace(requirement.quantity.to_string());
                                    ui.end_row();
                                    ui.label(egui::RichText::new("Condition").weak());
                                    ui.monospace(format!("0x{:04X}", requirement.condition));
                                    ui.end_row();
                                    ui.label(egui::RichText::new("Delete on action").weak());
                                    ui.label(yes_no(requirement.delete_on_action));
                                    ui.end_row();
                                    ui.label(egui::RichText::new("Omit from requirements").weak());
                                    ui.label(yes_no(requirement.omit_from_requirements));
                                    ui.end_row();
                                });
                            });
                        }
                    }
                });
            }
            if !definition.paths.is_empty() {
                ui.add_space(4.0);
                egui::CollapsingHeader::new(format!("Package paths ({})", definition.paths.len()))
                    .id_salt(("collection_package_paths", definition.index))
                    .show(ui, |ui| {
                        for path in &definition.paths {
                            ui.label(egui::RichText::new(root_first_path(path).join(" > ")).weak());
                        }
                    });
            }
            for condition in &definition.conditions {
                ui.add_space(6.0);
                let result = evaluate_expression(&condition.tokens, snapshot, catalog)
                    .map_or("Unknown", |value| if value { "True" } else { "False" });
                egui::CollapsingHeader::new(format!(
                    "{} · {result}",
                    condition_field_label(condition.field)
                ))
                .id_salt(("collection_condition", condition.field))
                .default_open(
                    definition.conditions.len() == 1
                        || condition.field == ACQUISITION_CONDITION_FIELD,
                )
                .show(ui, |ui| {
                    if ui.available_width() >= 700.0 {
                        egui::Grid::new(("collection_condition_tokens", condition.field))
                            .num_columns(5)
                            .spacing([16.0, 3.0])
                            .show(ui, |ui| {
                                ui.strong("#");
                                ui.strong("Operation");
                                ui.strong("Operand");
                                ui.strong("Referenced entry");
                                ui.strong("Current State");
                                ui.end_row();
                                for (token_index, token) in condition.tokens.iter().enumerate() {
                                    ui.monospace((token_index + 1).to_string());
                                    ui.label(condition_token_label(token.kind));
                                    ui.monospace(token.operand.to_string());
                                    draw_condition_token_metadata(ui, token, catalog);
                                    ui.label(condition_token_state(token, snapshot, catalog));
                                    ui.end_row();
                                }
                            });
                    } else {
                        for (token_index, token) in condition.tokens.iter().enumerate() {
                            if token_index > 0 {
                                ui.add_space(4.0);
                            }
                            ui.group(|ui| {
                                ui.set_min_width(ui.available_width());
                                ui.strong(format!(
                                    "{}. {}",
                                    token_index + 1,
                                    condition_token_label(token.kind)
                                ));
                                egui::Grid::new((
                                    "collection_condition_token_compact",
                                    condition.field,
                                    token_index,
                                ))
                                .num_columns(4)
                                .spacing([16.0, 3.0])
                                .show(ui, |ui| {
                                    ui.label(egui::RichText::new("Operand").weak());
                                    ui.monospace(token.operand.to_string());
                                    ui.label(egui::RichText::new("Current State").weak());
                                    ui.label(condition_token_state(token, snapshot, catalog));
                                    ui.end_row();
                                    if !condition_token_metadata(token, catalog).is_empty() {
                                        ui.label(egui::RichText::new("Referenced entry").weak());
                                        draw_condition_token_metadata(ui, token, catalog);
                                        ui.end_row();
                                    }
                                });
                            });
                        }
                    }
                    egui::CollapsingHeader::new("Raw package program")
                        .id_salt(("collection_raw_condition", condition.field))
                        .show(ui, |ui| {
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(condition_program(condition)).monospace(),
                                )
                                .wrap(),
                            );
                        });
                });
            }
        });
    if close {
        state.metadata_index = None;
    }
    changed
}

const fn yes_no(value: bool) -> &'static str {
    if value { "Yes" } else { "No" }
}

fn collection_hash_field(ui: &mut egui::Ui, label: &str, hash: u64) {
    ui.label(egui::RichText::new(label).weak());
    if hash == 0 {
        ui.label(egui::RichText::new("<not present>").weak().italics());
        ui.end_row();
        return;
    }
    let canonical = format_hash_hex(hash);
    let response = ui
        .add(
            egui::Button::new(egui::RichText::new(format_hash_hex_and_decimal(hash)).monospace())
                .frame(false),
        )
        .on_hover_text(format!("Open details for {canonical}"));
    if response.clicked() {
        request_hash_inspection(ui.ctx(), hash);
    }
    ui.end_row();
}

fn collection_metadata_field(ui: &mut egui::Ui, label: &str, value: impl Into<String>) {
    ui.label(egui::RichText::new(label).weak());
    let value = value.into();
    ui.label(if value.trim().is_empty() {
        egui::RichText::new("<not present>").weak()
    } else {
        egui::RichText::new(value)
    });
    ui.end_row();
}

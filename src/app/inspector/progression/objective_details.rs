//! Objective, relationship, perk, and owner detail sections.

use eframe::egui;

use crate::{
    app::progression::CollectionStateSnapshot,
    catalog::{Catalog, ObjectiveDef, ObjectiveOwnerDef, UnlockDefinition},
    hash::format_hash_hex,
};

use crate::app::inspector::{
    catalog_hash_hex_and_decimal_field, draw_catalog_hash_link, draw_metadata_paths,
    draw_named_catalog_hash_link, metadata_field, metadata_subsection, metadata_text,
    objective_owner_kind_label, yes_no,
};

use super::{
    conditions::draw_condition_programs,
    definitions::definition_name,
    objectives::{meaningful_definition_contexts, resolved_objective_table_text},
    state::ProgressionInspectorState,
};

pub(super) fn draw_objective_metadata(
    ui: &mut egui::Ui,
    objective: &ObjectiveDef,
    definition: &UnlockDefinition,
    catalog: &Catalog,
    snapshot: Option<&CollectionStateSnapshot>,
    state: &mut ProgressionInspectorState,
) {
    let external_context_count = meaningful_definition_contexts(definition).len();
    let ownership_source = if !objective.owners.is_empty() {
        format!(
            "{} direct package {}",
            objective.owners.len(),
            if objective.owners.len() == 1 {
                "owner"
            } else {
                "owners"
            }
        )
    } else if !objective.referenced_objective_indices.is_empty() {
        format!(
            "{} linked {}",
            objective.referenced_objective_indices.len(),
            if objective.referenced_objective_indices.len() == 1 {
                "objective"
            } else {
                "objectives"
            }
        )
    } else if external_context_count > 0 {
        format!(
            "{external_context_count} reverse package {}",
            if external_context_count == 1 {
                "reference"
            } else {
                "references"
            }
        )
    } else if let Some(index) = objective.related_unlock_value_definition_index {
        format!("Unlock Value Definition #{index}")
    } else {
        "Objective definition".to_owned()
    };
    egui::Grid::new("progression_objective_metadata")
        .num_columns(2)
        .spacing([16.0, 4.0])
        .show(ui, |ui| {
            metadata_field(
                ui,
                "Resolved label",
                resolved_objective_table_text(catalog, objective, Some(definition)),
                false,
            );
            metadata_field(ui, "Ownership source", ownership_source, false);
            catalog_hash_hex_and_decimal_field(ui, catalog, "Definition Hash", objective.hash);
            catalog_hash_hex_and_decimal_field(
                ui,
                catalog,
                "Unlock definition hash",
                definition.hash,
            );
            metadata_field(ui, "Name", metadata_text(&objective.name), false);
            metadata_field(
                ui,
                "Display description",
                metadata_text(&objective.display_description),
                false,
            );
            metadata_field(
                ui,
                "Progress description",
                metadata_text(&objective.progress_description),
                false,
            );
            metadata_field(
                ui,
                "Completion value",
                objective.completion_value.to_string(),
                true,
            );
            metadata_field(
                ui,
                "Related unlock definition",
                objective.related_unlock_value_definition_index.map_or_else(
                    || "<not present>".into(),
                    |index| format!("#{index} · current account value"),
                ),
                true,
            );
            metadata_field(ui, "Unlock bank", definition.bank().to_string(), true);
            metadata_field(
                ui,
                "Account slot",
                definition
                    .compact_slot
                    .map_or_else(|| "<unbanked>".into(), |slot| slot.to_string()),
                true,
            );
            metadata_field(
                ui,
                "Allows over-completion",
                yes_no(objective.allow_overcompletion),
                false,
            );
            metadata_field(
                ui,
                "Allows negative values",
                yes_no(objective.allow_negative_value),
                false,
            );
            metadata_field(
                ui,
                "Allows changes after completion",
                yes_no(objective.allow_value_change_when_completed),
                false,
            );
            metadata_field(
                ui,
                "Counts downward",
                yes_no(objective.is_counting_downward),
                false,
            );
            metadata_field(
                ui,
                "Condition Programs",
                objective.condition_programs.len().to_string(),
                true,
            );
            metadata_field(
                ui,
                "Referenced objectives",
                objective.referenced_objective_indices.len().to_string(),
                true,
            );
            metadata_field(
                ui,
                "Intrinsic perk flags",
                objective
                    .intrinsic_perk_flag_definition_indices
                    .len()
                    .to_string(),
                true,
            );
            metadata_field(ui, "Owners", objective.owners.len().to_string(), true);
        });
    draw_condition_programs(
        ui,
        "objective",
        objective.hash,
        &objective.condition_programs,
        catalog,
        snapshot,
        state,
    );
    draw_objective_references(ui, objective, catalog);
    draw_objective_intrinsic_perks(ui, objective, catalog);
    for (owner_index, owner) in objective.owners.iter().enumerate() {
        let name = (!owner.name.trim().is_empty())
            .then_some(owner.name.as_str())
            .or_else(|| catalog.display_name(owner.hash));
        let label = name.map_or_else(
            || {
                format!(
                    "{}. {} · 0x{:08X}",
                    owner_index + 1,
                    objective_owner_kind_label(owner.kind),
                    owner.hash
                )
            },
            |name| {
                format!(
                    "{}. {name} · {}",
                    owner_index + 1,
                    objective_owner_kind_label(owner.kind)
                )
            },
        );
        egui::CollapsingHeader::new(label)
            .id_salt(("progression_owner_metadata", owner_index))
            .default_open(objective.owners.len() <= 3)
            .show(ui, |ui| draw_objective_owner_metadata(ui, owner, catalog));
    }
}

fn draw_objective_references(ui: &mut egui::Ui, objective: &ObjectiveDef, catalog: &Catalog) {
    if objective.referenced_objective_indices.is_empty() {
        return;
    }
    egui::CollapsingHeader::new(format!(
        "Referenced objectives ({})",
        objective.referenced_objective_indices.len()
    ))
    .id_salt(("objective_references", objective.hash))
    .default_open(true)
    .show(ui, |ui| {
        egui::Grid::new(("objective_reference_rows", objective.hash))
            .num_columns(4)
            .spacing([16.0, 3.0])
            .show(ui, |ui| {
                ui.strong("Index");
                ui.strong("Label");
                ui.strong("Hash");
                ui.strong("Relationship");
                ui.end_row();
                for &raw_index in &objective.referenced_objective_indices {
                    let index = usize::from(raw_index);
                    let target = catalog.objective_definition(index);
                    ui.monospace(format!("#{index}"));
                    if let Some(target) = target {
                        draw_named_catalog_hash_link(
                            ui,
                            catalog,
                            target.hash,
                            resolved_objective_table_text(catalog, target, None),
                        );
                    } else {
                        ui.label(egui::RichText::new("<unavailable>").weak().italics());
                    }
                    if let Some(target) = target {
                        draw_catalog_hash_link(
                            ui,
                            catalog,
                            target.hash,
                            format_hash_hex(target.hash),
                        );
                    } else {
                        ui.label(egui::RichText::new("<unavailable>").weak().italics());
                    }
                    ui.label("Condition program reference (opcode 12)");
                    ui.end_row();
                }
            });
    });
}

fn draw_objective_intrinsic_perks(ui: &mut egui::Ui, objective: &ObjectiveDef, catalog: &Catalog) {
    if objective.intrinsic_perk_flag_definition_indices.is_empty() {
        return;
    }
    egui::CollapsingHeader::new(format!(
        "Intrinsic perks ({})",
        objective.intrinsic_perk_flag_definition_indices.len()
    ))
    .id_salt(("objective_intrinsic_perks", objective.hash))
    .default_open(true)
    .show(ui, |ui| {
        egui::Grid::new(("objective_intrinsic_perk_rows", objective.hash))
            .num_columns(4)
            .spacing([16.0, 3.0])
            .show(ui, |ui| {
                ui.strong("Index");
                ui.strong("Perk");
                ui.strong("Hash");
                ui.strong("Effect");
                ui.end_row();
                for &raw_index in &objective.intrinsic_perk_flag_definition_indices {
                    let index = usize::from(raw_index);
                    let definition = catalog.unlock_flag_definition(index);
                    ui.monospace(format!("#{index}"));
                    if let Some(definition) = definition {
                        draw_named_catalog_hash_link(
                            ui,
                            catalog,
                            definition.hash,
                            definition_name(definition)
                                .or_else(|| catalog.display_name(definition.hash))
                                .unwrap_or("Package perk name not resolved"),
                        );
                    } else {
                        ui.label(egui::RichText::new("<unavailable>").weak().italics());
                    }
                    if let Some(definition) = definition {
                        draw_catalog_hash_link(
                            ui,
                            catalog,
                            definition.hash,
                            format_hash_hex(definition.hash),
                        );
                    } else {
                        ui.label(egui::RichText::new("<unavailable>").weak().italics());
                    }
                    ui.label("Enabled when this objective completes");
                    ui.end_row();
                }
            });
    });
}

fn draw_objective_owner_metadata(ui: &mut egui::Ui, owner: &ObjectiveOwnerDef, catalog: &Catalog) {
    egui::Grid::new(("progression_owner_fields", owner.hash, owner.kind as u8))
        .num_columns(2)
        .spacing([16.0, 4.0])
        .show(ui, |ui| {
            metadata_field(ui, "Kind", objective_owner_kind_label(owner.kind), false);
            catalog_hash_hex_and_decimal_field(ui, catalog, "Definition Hash", owner.hash);
            metadata_field(ui, "Name", metadata_text(&owner.name), false);
            metadata_field(ui, "Type", metadata_text(&owner.type_name), false);
            metadata_field(ui, "Description", metadata_text(&owner.description), false);
            metadata_field(ui, "Traits", owner.traits.len().to_string(), true);
        });
    draw_metadata_paths(ui, &owner.paths);
    for (trait_index, trait_definition) in owner.traits.iter().enumerate() {
        ui.add_space(6.0);
        let name = (!trait_definition.name.trim().is_empty())
            .then_some(trait_definition.name.as_str())
            .or_else(|| catalog.display_name(trait_definition.hash));
        let heading = name.map_or_else(
            || format!("Trait {}", trait_index + 1),
            |name| format!("{name} · Trait {}", trait_index + 1),
        );
        metadata_subsection(ui, &heading, |ui| {
            egui::Grid::new((
                "progression_trait_fields",
                owner.hash,
                trait_index,
                trait_definition.hash,
            ))
            .num_columns(2)
            .spacing([16.0, 4.0])
            .show(ui, |ui| {
                catalog_hash_hex_and_decimal_field(
                    ui,
                    catalog,
                    "Definition Hash",
                    trait_definition.hash,
                );
                metadata_field(ui, "Name", metadata_text(&trait_definition.name), false);
                metadata_field(
                    ui,
                    "Description",
                    metadata_text(&trait_definition.description),
                    false,
                );
            });
        });
    }
}

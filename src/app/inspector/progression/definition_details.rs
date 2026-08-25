//! Unlock-definition and reader-context detail sections.

use eframe::egui;

use crate::catalog::{Catalog, ProgressionContextDef, UnlockDefinition};

use crate::app::inspector::{
    draw_metadata_paths, hash_hex_and_decimal_field, metadata_field, metadata_text,
    progression_context_kind_label,
};

use super::{conditions::draw_condition_programs, state::ProgressionInspectorState};

pub(super) fn draw_unlock_definition_metadata(
    ui: &mut egui::Ui,
    index: usize,
    definition: &UnlockDefinition,
    catalog: &Catalog,
    state: &mut ProgressionInspectorState,
) {
    let [bank, code_high_byte] = definition.code.to_le_bytes();
    egui::Grid::new("progression_definition_metadata")
        .num_columns(2)
        .spacing([16.0, 4.0])
        .show(ui, |ui| {
            hash_hex_and_decimal_field(ui, "Definition hash", definition.hash);
            metadata_field(
                ui,
                "Description",
                definition.description.as_deref().unwrap_or("<not present>"),
                false,
            );
            metadata_field(
                ui,
                "Condition references",
                definition.tested_by.len().to_string(),
                true,
            );
        });
    egui::CollapsingHeader::new("Technical package fields")
        .id_salt(("progression_definition_technical", index))
        .show(ui, |ui| {
            egui::Grid::new(("progression_definition_technical_fields", index))
                .num_columns(2)
                .spacing([16.0, 4.0])
                .show(ui, |ui| {
                    metadata_field(
                        ui,
                        "Code",
                        format!("0x{:04X} · {}", definition.code, definition.code),
                        true,
                    );
                    metadata_field(ui, "Bank / code low byte", bank.to_string(), true);
                    metadata_field(ui, "Code high byte", code_high_byte.to_string(), true);
                    metadata_field(
                        ui,
                        "Compact slot",
                        definition.compact_slot.map_or_else(
                            || "Unbanked".into(),
                            |slot| format!("{slot} · 0x{slot:04X}"),
                        ),
                        true,
                    );
                });
        });
    for (context_index, context) in definition.tested_by.iter().enumerate() {
        let label = format!(
            "{}. {} · 0x{:08X}",
            context_index + 1,
            progression_context_kind_label(context.kind),
            context.hash
        );
        egui::CollapsingHeader::new(label)
            .id_salt(("progression_context_metadata", context_index))
            .default_open(context_index == 0)
            .show(ui, |ui| draw_context_metadata(ui, context, catalog, state));
    }
}

fn draw_context_metadata(
    ui: &mut egui::Ui,
    context: &ProgressionContextDef,
    catalog: &Catalog,
    state: &mut ProgressionInspectorState,
) {
    egui::Grid::new((
        "progression_context_fields",
        context.hash,
        context.kind as u8,
    ))
    .num_columns(2)
    .spacing([16.0, 4.0])
    .show(ui, |ui| {
        metadata_field(
            ui,
            "Kind",
            progression_context_kind_label(context.kind),
            false,
        );
        hash_hex_and_decimal_field(ui, "Definition hash", context.hash);
        metadata_field(ui, "Name", metadata_text(&context.name), false);
        metadata_field(ui, "Type", metadata_text(&context.type_name), false);
        metadata_field(
            ui,
            "Description",
            metadata_text(&context.description),
            false,
        );
        metadata_field(
            ui,
            "Condition programs",
            context.condition_programs.len().to_string(),
            true,
        );
    });
    draw_metadata_paths(ui, &context.paths);
    draw_condition_programs(
        ui,
        "context",
        context.hash,
        &context.condition_programs,
        catalog,
        state,
    );
}

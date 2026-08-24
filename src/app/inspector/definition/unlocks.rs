use super::*;

pub(super) fn draw_hash_unlock_matches(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    title: &str,
    matches: &[(usize, &UnlockDefinition)],
) {
    if matches.is_empty() {
        return;
    }
    ui.add_space(8.0);
    hash_metadata_section(
        ui,
        &format!("{title} ({})", matches.len()),
        matches.len() <= 3,
        |ui| {
            for (index, definition) in matches {
                metadata_subsection(ui, &format!("Definition #{index}"), |ui| {
                    egui::Grid::new(("hash_unlock_definition", title, *index))
                        .num_columns(2)
                        .spacing([16.0, 4.0])
                        .show(ui, |ui| {
                            let name = definition
                                .name
                                .as_deref()
                                .filter(|name| !name.trim().is_empty())
                                .or_else(|| catalog.display_name(definition.hash));
                            hash_detail_field(ui, "Name", name.unwrap_or("<not present>"), false);
                            hash_detail_field(
                                ui,
                                "Description",
                                definition
                                    .description
                                    .as_deref()
                                    .filter(|description| !description.trim().is_empty())
                                    .unwrap_or("<not present>"),
                                false,
                            );
                            hash_detail_field(
                                ui,
                                "Code",
                                format!("0x{:04X}", definition.code),
                                true,
                            );
                            hash_detail_field(ui, "Bank", definition.bank().to_string(), true);
                            hash_detail_field(
                                ui,
                                "Compact slot",
                                definition
                                    .compact_slot
                                    .map_or_else(|| "<none>".into(), |slot| slot.to_string()),
                                true,
                            );
                            hash_detail_field(
                                ui,
                                "Readers",
                                definition.tested_by.len().to_string(),
                                true,
                            );
                        });
                    draw_hash_unlock_readers(ui, catalog, title, *index, definition);
                });
            }
        },
    );
}

fn draw_hash_unlock_readers(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    definition_kind: &str,
    definition_index: usize,
    definition: &UnlockDefinition,
) {
    if definition.tested_by.is_empty() {
        return;
    }
    egui::CollapsingHeader::new(format!("Readers ({})", definition.tested_by.len()))
        .id_salt(("hash_unlock_readers", definition_kind, definition_index))
        .default_open(definition.tested_by.len() <= HASH_RELATIONSHIP_AUTO_EXPAND_LIMIT)
        .show(ui, |ui| {
            for (context_index, context) in definition.tested_by.iter().enumerate() {
                let semantic_name = (!context.name.trim().is_empty())
                    .then_some(context.name.trim())
                    .or_else(|| catalog.display_name(context.hash))
                    .or_else(|| {
                        (!context.type_name.trim().is_empty()).then_some(context.type_name.trim())
                    });
                let label = semantic_name.map_or_else(
                    || {
                        format!(
                            "{}. {} · 0x{:08X}",
                            context_index + 1,
                            progression_context_kind_label(context.kind),
                            context.hash
                        )
                    },
                    |name| {
                        format!(
                            "{}. {} · {name} · 0x{:08X}",
                            context_index + 1,
                            progression_context_kind_label(context.kind),
                            context.hash
                        )
                    },
                );
                egui::CollapsingHeader::new(label)
                    .id_salt((
                        "hash_unlock_reader",
                        definition_kind,
                        definition_index,
                        context_index,
                    ))
                    .default_open(definition.tested_by.len() == 1)
                    .show(ui, |ui| {
                        egui::Grid::new((
                            "hash_unlock_reader_fields",
                            definition_kind,
                            definition_index,
                            context_index,
                        ))
                        .num_columns(2)
                        .spacing([16.0, 4.0])
                        .show(ui, |ui| {
                            hash_detail_field(
                                ui,
                                "Definition hash",
                                format_hash_hex_and_decimal(context.hash),
                                true,
                            );
                            hash_detail_field(ui, "Name", metadata_text(&context.name), false);
                            hash_detail_field(ui, "Type", metadata_text(&context.type_name), false);
                            hash_detail_field(
                                ui,
                                "Description",
                                metadata_text(&context.description),
                                false,
                            );
                            hash_detail_field(
                                ui,
                                "Condition programs",
                                context.condition_programs.len().to_string(),
                                true,
                            );
                        });
                        draw_metadata_paths(ui, &context.paths);
                    });
            }
        });
}

use super::*;

pub(super) fn draw_hash_collection_matches(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    hash: u64,
    matches: &CatalogHashMatches<'_>,
) {
    let collectible_matches = &matches.collectible_matches;
    let material_requirement_set_matches = &matches.material_requirement_set_matches;

    if !collectible_matches.is_empty() {
        ui.add_space(8.0);
        hash_metadata_section(
            ui,
            &format!("Collectibles ({})", collectible_matches.len()),
            collectible_matches.len() <= 3,
            |ui| {
                for collectible in collectible_matches {
                    metadata_subsection(ui, &format!("Collectible #{}", collectible.index), |ui| {
                        egui::Grid::new(("hash_collectible", collectible.index))
                            .num_columns(2)
                            .spacing([16.0, 4.0])
                            .show(ui, |ui| {
                                let mut matched_as = Vec::new();
                                if collectible.hash == hash {
                                    matched_as.push("Collectible hash");
                                }
                                if collectible.item_hash == hash {
                                    matched_as.push("Definition hash");
                                }
                                if collectible.material_requirement_set_hash == hash {
                                    matched_as.push("Material requirement set hash");
                                }
                                if collectible
                                    .material_requirements
                                    .iter()
                                    .any(|requirement| requirement.item_hash == hash)
                                {
                                    matched_as.push("Material requirement definition hash");
                                }
                                hash_detail_field(ui, "Matched as", matched_as.join(" · "), false);
                                hash_detail_field(
                                    ui,
                                    "Collectible index",
                                    collectible.index.to_string(),
                                    true,
                                );
                                hash_hex_and_decimal_field(
                                    ui,
                                    "Collectible hash",
                                    collectible.hash,
                                );
                                hash_detail_field(
                                    ui,
                                    "Item definition index",
                                    if collectible.item_definition_index == u16::MAX {
                                        "<unavailable>".into()
                                    } else {
                                        collectible.item_definition_index.to_string()
                                    },
                                    true,
                                );
                                hash_hex_and_decimal_field(
                                    ui,
                                    "Item definition hash",
                                    collectible.item_hash,
                                );
                                hash_detail_field(
                                    ui,
                                    "Material requirement set index",
                                    collectible.material_requirement_set_index.map_or_else(
                                        || "<unavailable>".into(),
                                        |index| index.to_string(),
                                    ),
                                    true,
                                );
                                hash_hex_and_decimal_field(
                                    ui,
                                    "Material requirement set hash",
                                    collectible.material_requirement_set_hash,
                                );
                                hash_detail_field(
                                    ui,
                                    "Name",
                                    if collectible.name.trim().is_empty() {
                                        catalog.display_name(hash).unwrap_or("<not resolved>")
                                    } else {
                                        &collectible.name
                                    },
                                    false,
                                );
                                hash_detail_field(
                                    ui,
                                    "Type",
                                    metadata_text(&collectible.type_name),
                                    false,
                                );
                            });
                        let detail_id = egui::Id::new((
                            "hash_collectible_detail",
                            collectible.index,
                            collectible.hash,
                        ));
                        draw_hash_package_paths(ui, detail_id, &collectible.paths);
                        draw_hash_collection_conditions(
                            ui,
                            detail_id,
                            &collectible.conditions,
                            catalog,
                        );
                        draw_hash_material_requirements(
                            ui,
                            catalog,
                            detail_id,
                            &collectible.material_requirements,
                        );
                    });
                }
            },
        );
    }

    if !material_requirement_set_matches.is_empty() {
        ui.add_space(8.0);
        hash_metadata_section(
            ui,
            &format!(
                "Material requirement sets ({})",
                material_requirement_set_matches.len()
            ),
            material_requirement_set_matches.len() <= 3,
            |ui| {
                for set in material_requirement_set_matches {
                    draw_hash_material_requirement_set(ui, catalog, set, hash);
                }
            },
        );
    }
}

pub(super) fn draw_hash_package_paths(ui: &mut egui::Ui, id: egui::Id, paths: &[Vec<String>]) {
    if paths.is_empty() {
        return;
    }
    egui::CollapsingHeader::new(format!("Package paths ({})", paths.len()))
        .id_salt((id, "package_paths"))
        .default_open(false)
        .show(ui, |ui| {
            for (index, path) in paths.iter().enumerate() {
                ui.label(
                    egui::RichText::new(format!("{}. {}", index + 1, metadata_path_text(path)))
                        .monospace(),
                );
            }
        });
}

pub(super) fn draw_hash_condition_programs(
    ui: &mut egui::Ui,
    id: egui::Id,
    programs: &[Vec<[u32; 2]>],
    catalog: &Catalog,
) {
    if programs.is_empty() {
        return;
    }
    egui::CollapsingHeader::new(format!("Condition programs ({})", programs.len()))
        .id_salt((id, "condition_programs"))
        .default_open(false)
        .show(ui, |ui| {
            for (program_index, program) in programs.iter().enumerate() {
                draw_hash_condition_tokens(ui, (id, program_index), program, catalog);
            }
        });
}

fn draw_hash_collection_conditions(
    ui: &mut egui::Ui,
    id: egui::Id,
    conditions: &[CollectionConditionDef],
    catalog: &Catalog,
) {
    if conditions.is_empty() {
        return;
    }
    egui::CollapsingHeader::new(format!("Conditions ({})", conditions.len()))
        .id_salt((id, "collection_conditions"))
        .default_open(false)
        .show(ui, |ui| {
            for (condition_index, condition) in conditions.iter().enumerate() {
                ui.label(
                    egui::RichText::new(if condition.field == 3 {
                        "Acquisition (field 3)".to_owned()
                    } else {
                        format!("Field {}", condition.field)
                    })
                    .strong(),
                );
                let program = condition
                    .tokens
                    .iter()
                    .map(|token| [token.kind, token.operand])
                    .collect::<Vec<_>>();
                draw_hash_condition_tokens(ui, (id, condition_index), &program, catalog);
            }
        });
}

fn draw_hash_condition_tokens(
    ui: &mut egui::Ui,
    id: (egui::Id, usize),
    program: &[[u32; 2]],
    catalog: &Catalog,
) {
    egui::Grid::new(("hash_condition_tokens", id))
        .num_columns(4)
        .spacing([16.0, 3.0])
        .striped(true)
        .show(ui, |ui| {
            ui.strong("Index");
            ui.strong("Operation");
            ui.strong("Operand");
            ui.strong("Referenced entry");
            ui.end_row();
            for (token_index, token) in program.iter().enumerate() {
                ui.monospace((token_index + 1).to_string());
                ui.label(condition_opcode_label(token[0]));
                ui.monospace(token[1].to_string());
                draw_hash_condition_reference(ui, token[0], token[1], catalog);
                ui.end_row();
            }
        });
}

fn draw_hash_condition_reference(ui: &mut egui::Ui, kind: u32, operand: u32, catalog: &Catalog) {
    let index = operand as usize;
    let hash = match kind {
        1 => catalog
            .unlock_flag_definition(index)
            .map(|definition| definition.hash),
        10 => catalog
            .unlock_value_definition(index)
            .map(|definition| definition.hash),
        12 => catalog
            .objective_definition(index)
            .map(|objective| objective.hash),
        _ => None,
    };
    let text = condition_token_resolution(kind, operand, catalog);
    if let Some(hash) = hash {
        draw_hash_link(ui, hash, text);
    } else {
        ui.add(egui::Label::new(text).wrap());
    }
}

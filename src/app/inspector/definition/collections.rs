use super::*;

pub(super) fn draw_hash_collection_matches(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    hash: u64,
    matches: &CatalogHashMatches<'_>,
    snapshot: Option<&CollectionStateSnapshot>,
    progression_editable: bool,
    action: &mut HashInspectorAction,
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
                draw_hash_collectible_table(
                    ui,
                    catalog,
                    collectible_matches,
                    snapshot,
                    progression_editable,
                    action,
                );
                draw_hash_collectible_technical_details(ui, catalog, hash, collectible_matches);
            },
        );
    }

    if !material_requirement_set_matches.is_empty() {
        ui.add_space(8.0);
        hash_metadata_section(
            ui,
            &format!(
                "Material Requirement Sets ({})",
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

fn draw_hash_collectible_table(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    collectibles: &[&CollectibleDef],
    snapshot: Option<&CollectionStateSnapshot>,
    progression_editable: bool,
    action: &mut HashInspectorAction,
) {
    let show_actions = progression_editable && snapshot.is_some();
    egui::Grid::new("hash_collectible_rows")
        .num_columns(if show_actions { 6 } else { 5 })
        .spacing([16.0, 3.0])
        .striped(true)
        .show(ui, |ui| {
            ui.strong("Index");
            ui.strong("Item");
            ui.strong("Hash");
            ui.strong("Type");
            ui.strong("State");
            if show_actions {
                ui.strong("Action");
            }
            ui.end_row();
            for collectible in collectibles {
                ui.monospace(format!("#{}", collectible.index));
                let item_name = collectible_item_name(catalog, collectible);
                draw_named_catalog_hash_link(ui, catalog, collectible.item_hash, item_name);
                draw_catalog_hash_link(
                    ui,
                    catalog,
                    collectible.item_hash,
                    format_hash_hex(collectible.item_hash),
                );
                ui.label(metadata_text(&collectible.type_name));
                if let Some(snapshot) = snapshot {
                    let (state, tooltip) = crate::app::collections_page::collectible_state(
                        collectible,
                        snapshot,
                        catalog,
                    );
                    ui.label(state).on_hover_text(tooltip);
                } else {
                    ui.label(egui::RichText::new("Unavailable").weak());
                }
                if show_actions {
                    draw_collectible_state_action(ui, catalog, collectible, snapshot, action);
                }
                ui.end_row();
            }
        });
}

fn draw_collectible_state_action(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    collectible: &CollectibleDef,
    snapshot: Option<&CollectionStateSnapshot>,
    action: &mut HashInspectorAction,
) {
    let Some(snapshot) = snapshot else {
        ui.label(egui::RichText::new("-").weak());
        return;
    };
    let Some(acquired) =
        crate::app::collections_page::collectible_acquired_state(collectible, snapshot, catalog)
    else {
        ui.label(egui::RichText::new("-").weak())
            .on_hover_text("This collectible does not have a reversible acquisition condition");
        return;
    };
    let desired = !acquired;
    let available = crate::app::collections_page::collectible_acquisition_edit_available(
        collectible,
        snapshot,
        catalog,
        desired,
    );
    let label = if desired {
        "Mark Acquired"
    } else {
        "Mark Missing"
    };
    if ui
        .add_enabled(available, egui::Button::new(label).small())
        .on_disabled_hover_text("No validated reversible edit can produce this state")
        .clicked()
    {
        action.progression_edit = Some(InspectorProgressionEdit::Collectible {
            collectible_index: collectible.index,
            acquired: desired,
        });
    }
}

fn draw_hash_collectible_technical_details(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    inspected_hash: u64,
    collectibles: &[&CollectibleDef],
) {
    for collectible in collectibles {
        egui::CollapsingHeader::new(format!(
            "Technical Fields · Collectible #{}",
            collectible.index
        ))
        .id_salt(("hash_collectible_technical", collectible.index))
        .default_open(false)
        .show(ui, |ui| {
            egui::Grid::new(("hash_collectible_technical_fields", collectible.index))
                .num_columns(2)
                .spacing([16.0, 4.0])
                .show(ui, |ui| {
                    hash_detail_field(
                        ui,
                        "Relationship",
                        collectible_match_relationships(inspected_hash, collectible).join(" · "),
                        false,
                    );
                    hash_detail_field(ui, "Collectible Index", collectible.index.to_string(), true);
                    catalog_hash_hex_and_decimal_field(
                        ui,
                        catalog,
                        "Collectible Hash",
                        collectible.hash,
                    );
                    if collectible.item_definition_index != u16::MAX {
                        hash_detail_field(
                            ui,
                            "Item Definition Index",
                            collectible.item_definition_index.to_string(),
                            true,
                        );
                    }
                    catalog_hash_hex_and_decimal_field(
                        ui,
                        catalog,
                        "Item Definition Hash",
                        collectible.item_hash,
                    );
                    if let Some(index) = collectible.material_requirement_set_index {
                        hash_detail_field(
                            ui,
                            "Material Requirement Set Index",
                            index.to_string(),
                            true,
                        );
                    }
                    if collectible.material_requirement_set_hash != 0
                        && collectible.material_requirement_set_hash != u64::from(u32::MAX)
                    {
                        catalog_hash_hex_and_decimal_field(
                            ui,
                            catalog,
                            "Material Requirement Set Hash",
                            collectible.material_requirement_set_hash,
                        );
                    }
                    let item_name = collectible_item_name(catalog, collectible);
                    if !collectible.name.trim().is_empty() && collectible.name.trim() != item_name {
                        hash_detail_field(ui, "Collectible Name", collectible.name.trim(), false);
                    }
                });
            let detail_id = egui::Id::new((
                "hash_collectible_detail",
                collectible.index,
                collectible.hash,
            ));
            draw_hash_package_paths(ui, detail_id, &collectible.paths);
            draw_hash_collection_conditions(ui, detail_id, &collectible.conditions, catalog);
            draw_hash_material_requirements(
                ui,
                catalog,
                detail_id,
                &collectible.material_requirements,
            );
        });
    }
}

pub(super) fn collectible_item_name<'a>(
    catalog: &'a Catalog,
    collectible: &'a CollectibleDef,
) -> &'a str {
    catalog
        .package_item_name(collectible.item_hash)
        .or_else(|| catalog.display_name(collectible.item_hash))
        .or_else(|| (!collectible.name.trim().is_empty()).then_some(collectible.name.trim()))
        .unwrap_or("Name not resolved")
}

fn collectible_match_relationships(
    inspected_hash: u64,
    collectible: &CollectibleDef,
) -> Vec<&'static str> {
    let mut relationships = Vec::new();
    if collectible.hash == inspected_hash {
        relationships.push("Collectible Definition");
    }
    if collectible.item_hash == inspected_hash {
        relationships.push("Item Definition");
    }
    if collectible.material_requirement_set_hash == inspected_hash {
        relationships.push("Material Requirement Set");
    }
    if collectible
        .material_requirements
        .iter()
        .any(|requirement| requirement.item_hash == inspected_hash)
    {
        relationships.push("Material Requirement Item");
    }
    relationships
}

pub(super) fn draw_hash_package_paths(ui: &mut egui::Ui, id: egui::Id, paths: &[Vec<String>]) {
    if paths.is_empty() {
        return;
    }
    egui::CollapsingHeader::new(format!("Package Paths ({})", paths.len()))
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
    egui::CollapsingHeader::new(format!("Condition Programs ({})", programs.len()))
        .id_salt((id, "condition_programs"))
        .default_open(false)
        .show(ui, |ui| {
            draw_hash_condition_program_table(ui, id, programs, catalog);
        });
}

fn draw_hash_condition_program_table(
    ui: &mut egui::Ui,
    id: egui::Id,
    programs: &[Vec<[u32; 2]>],
    catalog: &Catalog,
) {
    egui::Grid::new(("hash_condition_program_rows", id))
        .num_columns(5)
        .spacing([16.0, 3.0])
        .striped(true)
        .show(ui, |ui| {
            ui.strong("Program");
            ui.strong("Step");
            ui.strong("Operation");
            ui.strong("Operand").on_hover_text(
                "Raw package operand. Hover a value to see how this operation uses it.",
            );
            ui.strong("Referenced Definition");
            ui.end_row();
            for (program_index, program) in programs.iter().enumerate() {
                for (token_index, token) in program.iter().enumerate() {
                    ui.monospace((program_index + 1).to_string());
                    ui.monospace((token_index + 1).to_string());
                    ui.label(condition_opcode_label(token[0]));
                    draw_hash_condition_operand(ui, token[0], token[1], catalog);
                    draw_hash_condition_reference(ui, token[0], token[1], catalog);
                    ui.end_row();
                }
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
            ui.strong("Operand").on_hover_text(
                "Raw package operand. Hover a value to see how this operation uses it.",
            );
            ui.strong("Referenced Definition");
            ui.end_row();
            for (token_index, token) in program.iter().enumerate() {
                ui.monospace((token_index + 1).to_string());
                ui.label(condition_opcode_label(token[0]));
                draw_hash_condition_operand(ui, token[0], token[1], catalog);
                draw_hash_condition_reference(ui, token[0], token[1], catalog);
                ui.end_row();
            }
        });
}

fn draw_hash_condition_operand(ui: &mut egui::Ui, kind: u32, operand: u32, catalog: &Catalog) {
    ui.monospace(operand.to_string())
        .on_hover_text(condition_operand_tooltip(kind, operand, catalog));
}

fn condition_operand_tooltip(kind: u32, operand: u32, catalog: &Catalog) -> String {
    let index = operand as usize;
    match kind {
        1 => catalog.unlock_flag_definition(index).map_or_else(
            || format!("Unlock flag definition index {index}. The referenced definition is unavailable."),
            |definition| {
                format!(
                    "Unlock flag definition index {index}. Reads {} and pushes its true/false state.",
                    definition
                        .name
                        .as_deref()
                        .filter(|name| !name.trim().is_empty())
                        .unwrap_or("the referenced unlock flag")
                )
            },
        ),
        10 => catalog.unlock_value_definition(index).map_or_else(
            || format!("Unlock value definition index {index}. The referenced definition is unavailable."),
            |definition| {
                format!(
                    "Unlock value definition index {index}. Reads {} and pushes its numeric value.",
                    definition
                        .name
                        .as_deref()
                        .filter(|name| !name.trim().is_empty())
                        .unwrap_or("the referenced unlock value")
                )
            },
        ),
        11 => format!("Literal numeric value {operand}. Pushes this value onto the condition stack."),
        12 => catalog.objective_definition(index).map_or_else(
            || format!("Objective definition index {index}. The referenced objective is unavailable."),
            |objective| {
                let name = if objective.name.trim().is_empty() {
                    format_hash_hex(objective.hash)
                } else {
                    objective.name.clone()
                };
                format!(
                    "Objective definition index {index}. Evaluates {name} and pushes whether it is complete."
                )
            },
        ),
        22 if operand == 0 => {
            "Legacy literal-encoding marker. Operand 0 preserves the preceding literal for the next comparison.".into()
        }
        2 | 3 | 4 | 8 | 9 | 13 | 14 | 15 => format!(
            "This operation does not read its operand. {operand} is retained as the raw package value."
        ),
        _ => format!("Raw operand {operand}; this operation has not been decoded."),
    }
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
        draw_named_catalog_hash_link(ui, catalog, hash, text);
    } else {
        ui.add(egui::Label::new(text).wrap());
    }
}

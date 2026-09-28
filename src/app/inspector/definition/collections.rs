use super::progression::{parent_nodes_row, paths_row, property_link};
use super::*;
use crate::app::inspector::look;

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
        let own_page =
            matches!(collectible_matches.as_slice(), [collectible] if collectible.hash == hash);
        look::section(
            ui,
            ("hash_collectibles", hash),
            if own_page {
                "Collectible"
            } else {
                "Collectibles"
            },
            (!own_page).then_some(collectible_matches.len()),
            own_page || collectible_matches.len() <= HASH_RELATIONSHIP_AUTO_EXPAND_LIMIT,
            |ui| {
                draw_hash_collectible_table(
                    ui,
                    catalog,
                    hash,
                    collectible_matches,
                    snapshot,
                    progression_editable,
                    action,
                );
                draw_hash_collectible_details(ui, catalog, hash, collectible_matches);
            },
        );
    }

    if !material_requirement_set_matches.is_empty() {
        let own_page =
            matches!(material_requirement_set_matches.as_slice(), [set] if set.hash == hash);
        look::section(
            ui,
            ("hash_material_requirement_sets", hash),
            if own_page {
                "Material Requirement Set"
            } else {
                "Material Requirement Sets"
            },
            (!own_page).then_some(material_requirement_set_matches.len()),
            own_page || material_requirement_set_matches.len() <= 3,
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
    current: u64,
    collectibles: &[&CollectibleDef],
    snapshot: Option<&CollectionStateSnapshot>,
    progression_editable: bool,
    action: &mut HashInspectorAction,
) {
    let show_actions = progression_editable && snapshot.is_some();
    egui::Grid::new(("hash_collectible_rows", current))
        .num_columns(if show_actions { 4 } else { 3 })
        .spacing([16.0, 4.0])
        .striped(true)
        .show(ui, |ui| {
            ui.strong("Item");
            ui.strong("Type");
            ui.strong("State");
            if show_actions {
                ui.strong("Action");
            }
            ui.end_row();
            for collectible in collectibles {
                let item_name = collectible_item_name(catalog, collectible);
                if collectible.item_hash == current {
                    ui.label(crate::app::ui::destiny_text(ui, item_name));
                } else {
                    draw_named_catalog_hash_link(ui, catalog, collectible.item_hash, item_name);
                }
                ui.label(collectible.type_name.trim());
                if let Some(snapshot) = snapshot {
                    let (state, tooltip) = crate::app::collections_page::collectible_state(
                        collectible,
                        snapshot,
                        catalog,
                    );
                    ui.label(state).on_hover_text(tooltip);
                } else {
                    ui.label(egui::RichText::new("No Account Loaded").color(look::muted(ui)));
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
        ui.weak("-");
        return;
    };
    let Some(acquired) =
        crate::app::collections_page::collectible_acquired_state(collectible, snapshot, catalog)
    else {
        ui.weak("-")
            .on_hover_text("No reversible acquisition condition");
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
        "Mark Not Acquired"
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

fn draw_hash_collectible_details(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    inspected_hash: u64,
    collectibles: &[&CollectibleDef],
) {
    for collectible in collectibles {
        let draw = |ui: &mut egui::Ui| {
            draw_hash_collectible_detail(ui, catalog, inspected_hash, collectible);
        };
        if collectibles.len() == 1 {
            ui.add_space(6.0);
            draw(ui);
        } else {
            look::section(
                ui,
                ("hash_collectible_detail", collectible.index),
                &format!("Collectible #{} Details", collectible.index),
                None,
                false,
                draw,
            );
        }
    }
}

fn draw_hash_collectible_detail(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    inspected_hash: u64,
    collectible: &CollectibleDef,
) {
    let relationships = collectible_match_relationships(inspected_hash, collectible);
    let item_name = collectible_item_name(catalog, collectible);
    look::properties(ui, ("hash_collectible_fields", collectible.index), |p| {
        if relationships != ["Collectible Definition"] {
            p.text("Relationship", relationships.join(" · "));
            property_link(
                p,
                "Collectible",
                catalog,
                collectible.hash,
                inspected_hash,
                if collectible.name.trim().is_empty() {
                    item_name
                } else {
                    collectible.name.trim()
                },
            );
        }
        p.mono("Collectible Index", collectible.index.to_string());
        if collectible.item_definition_index != u16::MAX {
            p.mono(
                "Item Definition Index",
                collectible.item_definition_index.to_string(),
            );
        }
        if collectible.material_requirement_set_hash != 0
            && collectible.material_requirement_set_hash != u64::from(u32::MAX)
        {
            property_link(
                p,
                "Material Requirement Set",
                catalog,
                collectible.material_requirement_set_hash,
                inspected_hash,
                collectible.material_requirement_set_index.map_or_else(
                    || format_hash_hex(collectible.material_requirement_set_hash),
                    |index| format!("Set #{index}"),
                ),
            );
        }
        if !collectible.name.trim().is_empty() && collectible.name.trim() != item_name {
            p.text("Collectible Name", collectible.name.trim());
        }
        parent_nodes_row(p, catalog, &collectible.parent_nodes);
        paths_row(p, "Paths", &collectible.paths);
    });
    let detail_id = egui::Id::new((
        "hash_collectible_detail",
        collectible.index,
        collectible.hash,
    ));
    draw_hash_collection_conditions(ui, detail_id, &collectible.conditions, catalog);
    draw_hash_material_requirements(ui, catalog, detail_id, &collectible.material_requirements);
}

pub(super) fn collectible_item_name<'a>(
    catalog: &'a Catalog,
    collectible: &'a CollectibleDef,
) -> &'a str {
    catalog
        .package_item_name(collectible.item_hash)
        .or_else(|| catalog.display_name(collectible.item_hash))
        .or_else(|| (!collectible.name.trim().is_empty()).then_some(collectible.name.trim()))
        .unwrap_or(UNNAMED)
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

pub(super) fn draw_hash_condition_programs(
    ui: &mut egui::Ui,
    id: egui::Id,
    programs: &[Vec<[u32; 2]>],
    catalog: &Catalog,
) {
    if programs.is_empty() {
        return;
    }
    look::section(
        ui,
        (id, "condition_programs"),
        "Condition Programs",
        Some(programs.len()),
        false,
        |ui| {
            draw_hash_condition_program_table(ui, id, programs, catalog);
        },
    );
}

fn draw_hash_condition_program_table(
    ui: &mut egui::Ui,
    id: egui::Id,
    programs: &[Vec<[u32; 2]>],
    catalog: &Catalog,
) {
    egui::Grid::new(("hash_condition_program_rows", id))
        .num_columns(5)
        .spacing([16.0, 4.0])
        .striped(true)
        .show(ui, |ui| {
            ui.strong("Program");
            ui.strong("Step");
            ui.strong("Operation");
            ui.strong("Operand");
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
    look::section(
        ui,
        (id, "collection_conditions"),
        "Conditions",
        Some(conditions.len()),
        false,
        |ui| {
            for (condition_index, condition) in conditions.iter().enumerate() {
                look::subheading(
                    ui,
                    &if condition.field == crate::catalog::COLLECTIBLE_ACQUIRED_CONDITION_FIELD {
                        format!("Acquisition · Field {}", condition.field)
                    } else {
                        format!("Field {}", condition.field)
                    },
                );
                let program = condition
                    .tokens
                    .iter()
                    .map(|token| [token.kind, token.operand])
                    .collect::<Vec<_>>();
                draw_hash_condition_tokens(ui, (id, condition_index), &program, catalog);
            }
        },
    );
}

fn draw_hash_condition_tokens(
    ui: &mut egui::Ui,
    id: (egui::Id, usize),
    program: &[[u32; 2]],
    catalog: &Catalog,
) {
    egui::Grid::new(("hash_condition_tokens", id))
        .num_columns(4)
        .spacing([16.0, 4.0])
        .striped(true)
        .show(ui, |ui| {
            ui.strong("Step");
            ui.strong("Operation");
            ui.strong("Operand");
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
        12 => format!("Shared expression pool row {index}."),
        22 if operand == 0 => {
            "Legacy literal-encoding marker. Operand 0 preserves the preceding literal for the next comparison.".into()
        }
        2 | 3 | 4 | 8 | 9 | 13 | 14 | 15 => format!(
            "This operation does not read its operand. {operand} is retained as the raw package value."
        ),
        _ => format!("Raw operand {operand}. This operation has not been decoded."),
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
        _ => None,
    };
    let text = condition_token_resolution(kind, operand, catalog);
    if let Some(hash) = hash {
        draw_named_catalog_hash_link(ui, catalog, hash, text);
    } else {
        ui.add(egui::Label::new(text).wrap());
    }
}

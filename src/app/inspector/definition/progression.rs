use super::*;

pub(super) fn draw_hash_progression_matches(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    document: Option<&Value>,
    inspected_hash: u64,
    matches: &CatalogHashMatches<'_>,
) {
    let progression_definitions = &matches.progression_definitions;
    let progression_reward_matches = &matches.progression_reward_matches;
    let progression_faction_matches = &matches.progression_faction_matches;
    let objectives = &matches.objectives;
    let owner_matches = &matches.owner_matches;
    let trait_matches = &matches.trait_matches;
    let context_matches = &matches.context_matches;

    if !progression_definitions.is_empty() {
        ui.add_space(8.0);
        let heading = if progression_definitions.len() == 1 {
            "Progression definition".to_owned()
        } else {
            format!(
                "Progression definitions ({})",
                progression_definitions.len()
            )
        };
        hash_metadata_section(ui, &heading, true, |ui| {
            for (index, definition) in progression_definitions {
                if progression_definitions.len() == 1 {
                    draw_hash_progression_definition(ui, catalog, document, *index, definition);
                } else {
                    let heading = progression_display_name(definition).map_or_else(
                        || format!("Definition #{index}"),
                        |name| format!("{name} · Definition #{index}"),
                    );
                    metadata_subsection(ui, &heading, |ui| {
                        draw_hash_progression_definition(ui, catalog, document, *index, definition);
                    });
                }
            }
        });
    }

    if !progression_reward_matches.is_empty() {
        ui.add_space(8.0);
        hash_metadata_section(
            ui,
            &format!(
                "Progression reward references ({})",
                progression_reward_matches.len()
            ),
            progression_reward_matches.len() <= 12,
            |ui| {
                let progression_width = 220.0;
                let hash_width = 126.0;
                let reward_index_width = 88.0;
                let level_width = 72.0;
                let quantity_width = 88.0;
                ui.horizontal(|ui| {
                    table_cell(
                        ui,
                        progression_width,
                        egui::RichText::new("Progression").strong(),
                    );
                    table_cell(ui, hash_width, egui::RichText::new("Hash").strong());
                    table_cell(
                        ui,
                        reward_index_width,
                        egui::RichText::new("Reward index").strong(),
                    );
                    table_cell(ui, level_width, egui::RichText::new("Level").strong());
                    table_cell(ui, quantity_width, egui::RichText::new("Quantity").strong());
                });
                egui::ScrollArea::vertical()
                    .id_salt(("hash_progression_reward_references", inspected_hash))
                    .max_height(280.0)
                    .auto_shrink([false, true])
                    .show_rows(
                        ui,
                        TABLE_CELL_HEIGHT,
                        progression_reward_matches.len(),
                        |ui, range| {
                            egui::Grid::new((
                                "hash_progression_reward_reference_rows",
                                inspected_hash,
                            ))
                            .num_columns(5)
                            .striped(true)
                            .spacing([TABLE_COLUMN_GAP, TABLE_ROW_GAP])
                            .show(ui, |ui| {
                                for row in range {
                                    let (definition_index, definition, reward_index) =
                                        progression_reward_matches[row];
                                    let reward = &definition.reward_items[reward_index];
                                    table_cell(
                                        ui,
                                        progression_width,
                                        if definition.name.trim().is_empty() {
                                            format!("Definition #{definition_index}")
                                        } else {
                                            format!(
                                                "Definition #{definition_index} · {}",
                                                definition.name
                                            )
                                        },
                                    );
                                    draw_hash_hex_cell(ui, hash_width, Some(definition.hash));
                                    table_cell(
                                        ui,
                                        reward_index_width,
                                        egui::RichText::new(reward_index.to_string()).monospace(),
                                    );
                                    table_cell(
                                        ui,
                                        level_width,
                                        egui::RichText::new(
                                            reward.rewarded_at_progression_level.to_string(),
                                        )
                                        .monospace(),
                                    );
                                    table_cell(
                                        ui,
                                        quantity_width,
                                        egui::RichText::new(reward.quantity.to_string())
                                            .monospace(),
                                    );
                                    ui.end_row();
                                }
                            });
                        },
                    );
            },
        );
    }

    if !progression_faction_matches.is_empty() {
        ui.add_space(8.0);
        hash_metadata_section(
            ui,
            &format!(
                "Faction progression references ({})",
                progression_faction_matches.len()
            ),
            true,
            |ui| {
                egui::Grid::new(("hash_progression_faction_references", inspected_hash))
                    .num_columns(4)
                    .striped(true)
                    .spacing([16.0, 4.0])
                    .show(ui, |ui| {
                        ui.strong("Faction");
                        ui.strong("Progression");
                        ui.strong("Index");
                        ui.strong("Hash");
                        ui.end_row();
                        for (definition_index, definition, _, faction) in
                            progression_faction_matches
                        {
                            ui.label(if faction.name.trim().is_empty() {
                                egui::RichText::new("-").weak()
                            } else {
                                egui::RichText::new(&faction.name)
                            });
                            let progression_name = if definition.name.trim().is_empty() {
                                format!("Definition #{definition_index}")
                            } else {
                                definition.name.clone()
                            };
                            draw_named_catalog_hash_link(
                                ui,
                                catalog,
                                definition.hash,
                                progression_name,
                            );
                            ui.monospace(definition_index.to_string());
                            draw_catalog_hash_link(
                                ui,
                                catalog,
                                definition.hash,
                                format_hash_hex(definition.hash),
                            );
                            ui.end_row();
                        }
                    });
            },
        );
    }

    if !objectives.is_empty() {
        ui.add_space(8.0);
        hash_metadata_section(
            ui,
            &format!("Objectives ({})", objectives.len()),
            objectives.len() <= 3,
            |ui| {
                for (index, objective) in objectives {
                    let name = if objective.name.trim().is_empty() {
                        catalog.display_name(objective.hash)
                    } else {
                        Some(objective.name.as_str())
                    };
                    let heading = name.map_or_else(
                        || format!("Objective #{index}"),
                        |name| format!("{name} · Objective #{index}"),
                    );
                    metadata_subsection(ui, &heading, |ui| {
                        egui::Grid::new(("hash_objective", *index))
                            .num_columns(2)
                            .spacing([16.0, 4.0])
                            .show(ui, |ui| {
                                hash_detail_field(
                                    ui,
                                    "Description",
                                    objective_description(objective),
                                    false,
                                );
                                hash_detail_field(
                                    ui,
                                    "Completion value",
                                    objective.completion_value.to_string(),
                                    true,
                                );
                                hash_detail_field(
                                    ui,
                                    "Owners",
                                    objective.owners.len().to_string(),
                                    true,
                                );
                            });
                        draw_hash_condition_programs(
                            ui,
                            egui::Id::new(("hash_objective_conditions", *index, objective.hash)),
                            &objective.condition_programs,
                            catalog,
                        );
                    });
                }
            },
        );
    }

    if !owner_matches.is_empty() {
        ui.add_space(8.0);
        hash_metadata_section(
            ui,
            &format!("Objective owners ({})", owner_matches.len()),
            owner_matches.len() <= HASH_RELATIONSHIP_AUTO_EXPAND_LIMIT,
            |ui| {
                for (objective_index, objective, owner) in owner_matches {
                    let name = if owner.name.trim().is_empty() {
                        catalog.display_name(owner.hash)
                    } else {
                        Some(owner.name.as_str())
                    };
                    let heading = name.map_or_else(
                        || {
                            format!(
                                "{} · Objective #{objective_index}",
                                objective_owner_kind_label(owner.kind)
                            )
                        },
                        |name| format!("{name} · {}", objective_owner_kind_label(owner.kind)),
                    );
                    metadata_subsection(ui, &heading, |ui| {
                        egui::Grid::new(("hash_objective_owner", *objective_index, owner.hash))
                            .num_columns(2)
                            .spacing([16.0, 4.0])
                            .show(ui, |ui| {
                                hash_detail_field(
                                    ui,
                                    "Objective",
                                    objective_description(objective),
                                    false,
                                );
                                hash_detail_field(
                                    ui,
                                    "Type",
                                    metadata_text(&owner.type_name),
                                    false,
                                );
                                hash_detail_field(
                                    ui,
                                    "Description",
                                    metadata_text(&owner.description),
                                    false,
                                );
                            });
                        let detail_id = egui::Id::new((
                            "hash_objective_owner_detail",
                            *objective_index,
                            owner.hash,
                        ));
                        draw_hash_package_paths(ui, detail_id, &owner.paths);
                        if !owner.traits.is_empty() {
                            egui::CollapsingHeader::new(format!("Traits ({})", owner.traits.len()))
                                .id_salt(detail_id.with("traits"))
                                .default_open(owner.traits.len() <= 4)
                                .show(ui, |ui| {
                                    egui::Grid::new(detail_id.with("trait_rows"))
                                        .num_columns(3)
                                        .striped(true)
                                        .spacing([16.0, 4.0])
                                        .show(ui, |ui| {
                                            ui.strong("Name");
                                            ui.strong("Hash");
                                            ui.strong("Description");
                                            ui.end_row();
                                            for trait_definition in &owner.traits {
                                                draw_named_catalog_hash_link(
                                                    ui,
                                                    catalog,
                                                    trait_definition.hash,
                                                    metadata_text(&trait_definition.name),
                                                );
                                                draw_catalog_hash_link(
                                                    ui,
                                                    catalog,
                                                    trait_definition.hash,
                                                    format_hash_hex(trait_definition.hash),
                                                );
                                                ui.label(metadata_text(
                                                    &trait_definition.description,
                                                ));
                                                ui.end_row();
                                            }
                                        });
                                });
                        }
                    });
                }
            },
        );
    }

    if !trait_matches.is_empty() {
        ui.add_space(8.0);
        hash_metadata_section(
            ui,
            &format!("Objective traits ({})", trait_matches.len()),
            trait_matches.len() <= HASH_RELATIONSHIP_AUTO_EXPAND_LIMIT && owner_matches.is_empty(),
            |ui| {
                for (objective_index, objective, owner, trait_definition) in trait_matches {
                    let name = if trait_definition.name.trim().is_empty() {
                        catalog.display_name(trait_definition.hash)
                    } else {
                        Some(trait_definition.name.as_str())
                    };
                    let heading = name.map_or_else(
                        || format!("Trait · Objective #{objective_index}"),
                        |name| format!("{name} · Trait"),
                    );
                    metadata_subsection(ui, &heading, |ui| {
                        egui::Grid::new((
                            "hash_objective_trait",
                            *objective_index,
                            trait_definition.hash,
                        ))
                        .num_columns(2)
                        .spacing([16.0, 4.0])
                        .show(ui, |ui| {
                            hash_detail_field(
                                ui,
                                "Objective",
                                objective_description(objective),
                                false,
                            );
                            hash_detail_field(
                                ui,
                                "Owner",
                                objective_owner_display_label(owner)
                                    .or_else(|| catalog.display_name(owner.hash).map(str::to_owned))
                                    .unwrap_or_else(|| "<not resolved>".into()),
                                false,
                            );
                            hash_detail_field(
                                ui,
                                "Description",
                                metadata_text(&trait_definition.description),
                                false,
                            );
                        });
                    });
                }
            },
        );
    }

    if !context_matches.is_empty() {
        ui.add_space(8.0);
        hash_metadata_section(
            ui,
            &format!("Progression readers ({})", context_matches.len()),
            context_matches.len() <= HASH_RELATIONSHIP_AUTO_EXPAND_LIMIT
                && owner_matches.is_empty()
                && trait_matches.is_empty(),
            |ui| {
                draw_hash_progression_reader_table(ui, catalog, context_matches);
                draw_hash_progression_reader_details(ui, catalog, context_matches);
            },
        );
    }
}

fn draw_hash_progression_reader_table(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    readers: &[(&'static str, usize, &ProgressionContextDef)],
) {
    egui::Grid::new("hash_progression_reader_rows")
        .num_columns(5)
        .spacing([16.0, 3.0])
        .striped(true)
        .show(ui, |ui| {
            ui.strong("Reader");
            ui.strong("Source");
            ui.strong("Type");
            ui.strong("Hash");
            ui.strong("Programs");
            ui.end_row();
            for (kind, definition_index, context) in readers {
                let name = progression_reader_name(catalog, context);
                draw_named_catalog_hash_link(ui, catalog, context.hash, name);
                ui.monospace(format!("{kind} #{definition_index}"));
                ui.label(progression_reader_type(context));
                draw_catalog_hash_link(ui, catalog, context.hash, format_hash_hex(context.hash));
                ui.monospace(context.condition_programs.len().to_string());
                ui.end_row();
            }
        });
}

fn draw_hash_progression_reader_details(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    readers: &[(&'static str, usize, &ProgressionContextDef)],
) {
    for (kind, definition_index, context) in readers {
        let name = progression_reader_name(catalog, context);
        egui::CollapsingHeader::new(format!("Reader details · {name}"))
            .id_salt((
                "hash_progression_reader_details",
                *kind,
                *definition_index,
                context.hash,
            ))
            .default_open(false)
            .show(ui, |ui| {
                if !context.description.trim().is_empty() {
                    ui.add(
                        egui::Label::new(crate::app::ui::destiny_text(
                            ui,
                            context.description.trim(),
                        ))
                        .wrap(),
                    );
                }
                let detail_id = egui::Id::new((
                    "hash_context_detail",
                    *kind,
                    *definition_index,
                    context.hash,
                ));
                draw_hash_package_paths(ui, detail_id, &context.paths);
                draw_hash_condition_programs(ui, detail_id, &context.condition_programs, catalog);
            });
    }
}

fn progression_reader_name<'a>(
    catalog: &'a Catalog,
    context: &'a ProgressionContextDef,
) -> &'a str {
    if context.name.trim().is_empty() {
        catalog
            .display_name(context.hash)
            .unwrap_or("Name not resolved")
    } else {
        context.name.trim()
    }
}

fn progression_reader_type(context: &ProgressionContextDef) -> String {
    let kind = progression_context_kind_label(context.kind);
    if context.type_name.trim().is_empty() {
        kind.to_owned()
    } else {
        format!("{} · {kind}", context.type_name.trim())
    }
}

fn draw_hash_progression_definition(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    document: Option<&Value>,
    index: usize,
    definition: &ProgressionDefinition,
) {
    if hash_inspector_uses_wide_summary(ui.available_width()) {
        ui.columns(2, |columns| {
            draw_hash_progression_identity_summary(&mut columns[0], catalog, index, definition);
            draw_hash_progression_persistence_summary(&mut columns[1], document, index, definition);
        });
    } else {
        draw_hash_progression_identity_summary(ui, catalog, index, definition);
        ui.add_space(8.0);
        draw_hash_progression_persistence_summary(ui, document, index, definition);
    }

    if !definition.factions.is_empty() {
        egui::CollapsingHeader::new(format!("Factions ({})", definition.factions.len()))
            .id_salt(("hash_progression_factions", index))
            .default_open(true)
            .show(ui, |ui| {
                for (faction_index, faction) in definition.factions.iter().enumerate() {
                    let heading = if faction.name.trim().is_empty() {
                        format!("Faction #{faction_index}")
                    } else {
                        faction.name.clone()
                    };
                    metadata_subsection(ui, &heading, |ui| {
                        egui::Grid::new(("hash_progression_faction", index, faction_index))
                            .num_columns(2)
                            .spacing([16.0, 4.0])
                            .show(ui, |ui| {
                                hash_hex_and_decimal_field(ui, "Hash", faction.hash);
                                hash_detail_field(
                                    ui,
                                    "Description",
                                    if faction.description.trim().is_empty() {
                                        "<not present>"
                                    } else {
                                        &faction.description
                                    },
                                    false,
                                );
                            });
                    });
                }
            });
    }

    if !definition.steps.is_empty() {
        let has_step_icons = definition
            .steps
            .iter()
            .any(|step| step.icon_container.is_some());
        egui::CollapsingHeader::new(format!("Steps ({})", definition.steps.len()))
            .id_salt(("hash_progression_steps", index))
            .default_open(definition.steps.len() <= 12)
            .show(ui, |ui| {
                egui::Grid::new(("hash_progression_step_rows", index))
                    .num_columns(if has_step_icons { 4 } else { 3 })
                    .striped(true)
                    .spacing([16.0, 4.0])
                    .show(ui, |ui| {
                        if has_step_icons {
                            ui.strong("Icon");
                        }
                        ui.strong("Index");
                        ui.strong("Name");
                        ui.strong("Progress total");
                        ui.end_row();
                        for (step_index, step) in definition.steps.iter().enumerate() {
                            if has_step_icons {
                                if let Some(container) = step.icon_container
                                    && let Some(icon) = catalog.icon_texture_from_container(
                                        ui.ctx(),
                                        0x2_0000_0000
                                            | (u64::from(definition.definition_index) << 16)
                                            | u64::try_from(step_index).unwrap_or_default(),
                                        container,
                                    )
                                {
                                    ui.add(
                                        egui::Image::new(&icon)
                                            .fit_to_exact_size(egui::vec2(24.0, 24.0))
                                            .maintain_aspect_ratio(true),
                                    );
                                } else {
                                    ui.label(egui::RichText::new("-").weak());
                                }
                            }
                            ui.monospace(step_index.to_string());
                            if step.name.trim().is_empty() {
                                ui.label(egui::RichText::new("-").weak());
                            } else {
                                ui.label(&step.name);
                            }
                            ui.monospace(step.progress_total.to_string());
                            ui.end_row();
                        }
                    });
            });
    }

    if !definition.reward_items.is_empty() {
        egui::CollapsingHeader::new(format!("Reward items ({})", definition.reward_items.len()))
            .id_salt(("hash_progression_reward_items", index))
            .default_open(false)
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt(("hash_progression_reward_rows_scroll", index))
                    .max_height(320.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        egui::Grid::new(("hash_progression_reward_rows", index))
                            .num_columns(4)
                            .striped(true)
                            .spacing([16.0, 4.0])
                            .show(ui, |ui| {
                                ui.strong("Level");
                                ui.strong("Item");
                                ui.strong("Hash");
                                ui.strong("Quantity");
                                ui.end_row();
                                for reward in &definition.reward_items {
                                    ui.monospace(reward.rewarded_at_progression_level.to_string());
                                    if let Some(name) = catalog.package_item_name(reward.item_hash)
                                    {
                                        draw_named_catalog_hash_link(
                                            ui,
                                            catalog,
                                            reward.item_hash,
                                            name,
                                        );
                                    } else {
                                        ui.label(egui::RichText::new("-").weak());
                                    }
                                    draw_catalog_hash_link(
                                        ui,
                                        catalog,
                                        reward.item_hash,
                                        format_hash_hex(reward.item_hash),
                                    );
                                    ui.monospace(reward.quantity.to_string());
                                    ui.end_row();
                                }
                            });
                    });
            });
    }
}

fn draw_hash_progression_identity_summary(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    index: usize,
    definition: &ProgressionDefinition,
) {
    metadata_subsection(ui, "Definition", |ui| {
        ui.horizontal_top(|ui| {
            if let Some(container) = definition.icon_container
                && let Some(icon) = catalog.icon_texture_from_container(
                    ui.ctx(),
                    0x1_0000_0000 | definition.hash,
                    container,
                )
            {
                ui.add(
                    egui::Image::new(&icon)
                        .fit_to_exact_size(egui::vec2(80.0, 80.0))
                        .maintain_aspect_ratio(true),
                );
                ui.add_space(8.0);
            }
            ui.vertical(|ui| {
                let name = if definition.name.trim().is_empty() {
                    egui::RichText::new("<not present>").weak().italics()
                } else {
                    egui::RichText::new(&definition.name).strong().size(16.0)
                };
                ui.label(name);
                egui::Grid::new(("hash_progression_definition", index))
                    .num_columns(2)
                    .spacing([16.0, 4.0])
                    .show(ui, |ui| {
                        hash_detail_field(
                            ui,
                            "Definition index",
                            definition.definition_index.to_string(),
                            true,
                        );
                    });
            });
        });
        ui.add_space(6.0);
        draw_hash_wrapped_detail(
            ui,
            "Description",
            if definition.description.trim().is_empty() {
                "<not present>"
            } else {
                &definition.description
            },
        );
        ui.add_space(4.0);
        draw_hash_wrapped_detail(
            ui,
            "Source",
            if definition.source.trim().is_empty() {
                "<not present>"
            } else {
                &definition.source
            },
        );
        ui.add_space(4.0);
        draw_hash_wrapped_detail(
            ui,
            "Display units name",
            if definition.display_units_name.trim().is_empty() {
                "<not present>"
            } else {
                &definition.display_units_name
            },
        );
    });
}

fn draw_hash_progression_persistence_summary(
    ui: &mut egui::Ui,
    document: Option<&Value>,
    index: usize,
    definition: &ProgressionDefinition,
) {
    let saved_lanes = document.and_then(|document| {
        saved_progression_lanes(
            document,
            definition.scope,
            usize::from(definition.definition_index),
        )
    });
    let target = progression_target(definition);
    metadata_subsection(ui, "Persistence", |ui| {
        egui::Grid::new(("hash_progression_persistence", index))
            .num_columns(2)
            .spacing([16.0, 4.0])
            .show(ui, |ui| {
                hash_detail_field(
                    ui,
                    "Scope",
                    progression_scope_label(definition.scope),
                    false,
                );
                hash_detail_field(
                    ui,
                    "Slot",
                    definition
                        .scope_slot
                        .map_or_else(|| "<unreplicated>".into(), |slot| slot.to_string()),
                    true,
                );
                hash_detail_field(
                    ui,
                    "Repeat last step",
                    yes_no(definition.repeat_last_step),
                    false,
                );
                hash_detail_field(ui, "Lane 0 meaning", "Progress", false);
                hash_detail_field(ui, "Lane 1 meaning", "<not decoded>", false);
                hash_detail_field(ui, "Lane 2 meaning", "<not decoded>", false);
            });
    });
    ui.add_space(8.0);
    metadata_subsection(ui, "Referenced save state", |ui| {
        egui::Grid::new(("hash_progression_save_state", index))
            .num_columns(2)
            .spacing([16.0, 4.0])
            .show(ui, |ui| {
                hash_detail_field(
                    ui,
                    "State",
                    if saved_lanes.is_some() {
                        "Present"
                    } else {
                        "Missing"
                    },
                    false,
                );
                match saved_lanes {
                    Some(lanes) => {
                        hash_detail_field(
                            ui,
                            "Progress",
                            target.map_or_else(
                                || lanes[0].to_string(),
                                |target| format!("{} / {target}", lanes[0]),
                            ),
                            true,
                        );
                        hash_detail_field(ui, "Lane 1", lanes[1].to_string(), true);
                        hash_detail_field(ui, "Lane 2", lanes[2].to_string(), true);
                    }
                    None => {
                        hash_detail_field(ui, "Progress", "<not present>", true);
                        hash_detail_field(ui, "Lane 1", "<not present>", true);
                        hash_detail_field(ui, "Lane 2", "<not present>", true);
                    }
                }
                hash_detail_field(
                    ui,
                    "Target",
                    target.map_or_else(|| "<not present>".into(), |target| target.to_string()),
                    true,
                );
            });
    });
}

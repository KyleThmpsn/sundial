use super::*;

pub(super) fn draw_hash_unlock_matches(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    matches: &CatalogHashMatches<'_>,
    snapshot: Option<&CollectionStateSnapshot>,
    progression_editable: bool,
    action: &mut HashInspectorAction,
) {
    let mut editor = UnlockEditor {
        snapshot,
        progression_editable,
        action,
    };
    draw_hash_unlock_kind(
        ui,
        catalog,
        "Unlock Flags",
        &matches.flag_definitions,
        false,
        &mut editor,
    );
    draw_hash_unlock_kind(
        ui,
        catalog,
        "Unlock Values",
        &matches.value_definitions,
        true,
        &mut editor,
    );
}

struct UnlockEditor<'a> {
    snapshot: Option<&'a CollectionStateSnapshot>,
    progression_editable: bool,
    action: &'a mut HashInspectorAction,
}

fn draw_hash_unlock_kind(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    title: &str,
    matches: &[(usize, &UnlockDefinition)],
    value_definition: bool,
    editor: &mut UnlockEditor<'_>,
) {
    if matches.is_empty() {
        return;
    }
    ui.add_space(8.0);
    if let [(index, definition)] = matches {
        draw_hash_unlock_definition(
            ui,
            catalog,
            title,
            *index,
            definition,
            value_definition,
            editor,
        );
        return;
    }
    hash_metadata_section(
        ui,
        &format!("{title} ({})", matches.len()),
        matches.len() <= 3,
        |ui| {
            for (index, definition) in matches {
                let name = definition
                    .name
                    .as_deref()
                    .filter(|name| !name.trim().is_empty())
                    .or_else(|| catalog.display_name(definition.hash));
                let heading = name.map_or_else(
                    || format!("Definition #{index}"),
                    |name| format!("{name} · Definition #{index}"),
                );
                metadata_subsection(ui, &heading, |ui| {
                    draw_hash_unlock_definition(
                        ui,
                        catalog,
                        title,
                        *index,
                        definition,
                        value_definition,
                        editor,
                    );
                });
            }
        },
    );
}

fn draw_hash_unlock_definition(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    definition_kind: &str,
    index: usize,
    definition: &UnlockDefinition,
    value_definition: bool,
    editor: &mut UnlockEditor<'_>,
) {
    egui::Grid::new(("hash_unlock_definition", definition_kind, index))
        .num_columns(2)
        .spacing([16.0, 4.0])
        .show(ui, |ui| {
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
            hash_detail_field(ui, "Code", format!("0x{:04X}", definition.code), true);
            hash_detail_field(ui, "Bank", definition.bank().to_string(), true);
            hash_detail_field(
                ui,
                "Compact Storage Slot",
                definition
                    .compact_slot
                    .map_or_else(|| "<none>".into(), |slot| slot.to_string()),
                true,
            );
            hash_detail_field(
                ui,
                "Known References",
                definition.tested_by.len().to_string(),
                true,
            );
            draw_unlock_state(ui, index, definition, editor.snapshot, value_definition);
            if let Some(snapshot) = editor.snapshot {
                if snapshot.is_native() && !value_definition {
                    hash_detail_field(
                        ui,
                        "Native Bank State",
                        snapshot
                            .native_flag(definition)
                            .map_or("Unavailable", |set| if set { "Set (2)" } else { "Not Set" }),
                        true,
                    );
                }
                let evaluated = if value_definition {
                    snapshot
                        .evaluated_value(index, catalog)
                        .map(|value| value.to_string())
                } else {
                    snapshot
                        .evaluated_flag(index, catalog)
                        .map(|value| value.to_string())
                };
                hash_detail_field(
                    ui,
                    if snapshot.is_native()
                        && (crate::app::progression::seasonal::is_derived_value(index)
                            && value_definition
                            || !value_definition
                                && catalog
                                    .seasonal()
                                    .is_some_and(|season| season.mod_for_flag(index).is_some()))
                    {
                        "After Sunrise Refresh"
                    } else {
                        "Evaluated State"
                    },
                    evaluated.unwrap_or_else(|| "Unresolved".into()),
                    true,
                );
            }
            if editor.progression_editable {
                draw_unlock_state_editor(
                    ui,
                    catalog,
                    index,
                    definition,
                    editor.snapshot,
                    value_definition,
                    editor.action,
                );
            }
        });
    if editor
        .snapshot
        .is_some_and(CollectionStateSnapshot::is_native)
    {
        if value_definition && crate::app::progression::seasonal::is_derived_value(index) {
            ui.label("Sunrise rebuilds this value from seasonal XP and character artifact ownership. Use Seasonal to change its inputs.");
        } else if !value_definition
            && let Some(entry) = catalog
                .seasonal()
                .and_then(|season| season.mod_for_flag(index))
        {
            ui.label(format!(
                "Artifact Mod · Column {} · Sale Row {} · Character Flag Slot {}",
                entry.column() + 1,
                entry.sale_index,
                entry.character_slot
            ));
            draw_catalog_hash_link(ui, catalog, entry.item_hash, "Artifact Mod");
            draw_catalog_hash_link(ui, catalog, entry.collectible_hash, "Collectible");
            ui.label("Ownership is loaded automatically, including saved artifact overrides. Edits preserve the other characters' effective ownership and update the selected character's points.");
        }
    }
    draw_hash_unlock_readers(ui, catalog, definition_kind, index, definition);
}

fn draw_unlock_state_editor(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    index: usize,
    definition: &UnlockDefinition,
    snapshot: Option<&CollectionStateSnapshot>,
    value_definition: bool,
    action: &mut HashInspectorAction,
) {
    let Some(snapshot) = snapshot else {
        return;
    };
    ui.label(metadata_label_text(ui, "Edit Availability"));
    if snapshot.is_native()
        && value_definition
        && crate::app::progression::seasonal::is_derived_value(index)
    {
        ui.weak("Use Seasonal XP or Artifact Mods");
    } else if snapshot.is_native()
        && !value_definition
        && let Some(season) = catalog.seasonal()
        && let Some(entry) = season.mod_for_flag(index)
    {
        let owned = snapshot.artifact_mask(season, true) & entry.bit() != 0;
        let available = snapshot.seasonal_experience(season).and_then(|experience| {
            season.unlock(
                snapshot.artifact_mask(season, true),
                entry.sale_index,
                experience.points_earned,
            )
        });
        let enabled = owned || available.is_ok();
        let response = ui.add_enabled(
            enabled,
            egui::Button::new(if owned { "Remove Mod" } else { "Unlock Mod" }).small(),
        );
        if response.clicked() {
            action.progression_edit = Some(InspectorProgressionEdit::Flag {
                definition_index: index,
                set: !owned,
            });
        }
        if !owned && let Err(error) = available {
            response.on_hover_text(error);
        }
    } else if !unlock_state_editable(definition, value_definition) {
        ui.label(egui::RichText::new("Read Only").weak())
            .on_hover_text("This definition uses a storage bank Sundial does not write");
    } else if value_definition {
        draw_unlock_value_editor(ui, index, definition, snapshot, action);
    } else {
        draw_unlock_flag_editor(ui, index, definition, snapshot, action);
    }
    ui.end_row();
}

fn unlock_state_editable(definition: &UnlockDefinition, value_definition: bool) -> bool {
    definition.compact_slot.is_none()
        || if value_definition {
            matches!(definition.bank(), 1 | 2)
        } else {
            matches!(definition.bank(), 1 | 2 | 3 | 6)
        }
}

fn draw_unlock_flag_editor(
    ui: &mut egui::Ui,
    index: usize,
    definition: &UnlockDefinition,
    snapshot: &CollectionStateSnapshot,
    action: &mut HashInspectorAction,
) {
    let current = snapshot.flag_value(index, definition);
    ui.horizontal(|ui| {
        if ui
            .add_enabled(current != Some(true), egui::Button::new("Set").small())
            .clicked()
        {
            action.progression_edit = Some(InspectorProgressionEdit::Flag {
                definition_index: index,
                set: true,
            });
        }
        if ui
            .add_enabled(current != Some(false), egui::Button::new("Unset").small())
            .clicked()
        {
            action.progression_edit = Some(InspectorProgressionEdit::Flag {
                definition_index: index,
                set: false,
            });
        }
    });
}

fn draw_unlock_value_editor(
    ui: &mut egui::Ui,
    index: usize,
    definition: &UnlockDefinition,
    snapshot: &CollectionStateSnapshot,
    action: &mut HashInspectorAction,
) {
    let editor_id = egui::Id::new(("hash_unlock_value_editor", index));
    let current = snapshot.value(index, definition).unwrap_or_default();
    let mut value = ui
        .data_mut(|data| data.get_temp::<i32>(editor_id))
        .unwrap_or(current);
    ui.horizontal(|ui| {
        ui.add(
            egui::DragValue::new(&mut value)
                .speed(1.0)
                .range(i32::MIN..=i32::MAX),
        );
        if ui
            .add_enabled(value != current, egui::Button::new("Apply").small())
            .clicked()
        {
            action.progression_edit = Some(InspectorProgressionEdit::Value {
                definition_index: index,
                value,
            });
        }
    });
    ui.data_mut(|data| data.insert_temp(editor_id, value));
}

fn draw_unlock_state(
    ui: &mut egui::Ui,
    index: usize,
    definition: &UnlockDefinition,
    snapshot: Option<&CollectionStateSnapshot>,
    value_definition: bool,
) {
    let Some(snapshot) = snapshot else {
        return;
    };
    let state = if value_definition {
        snapshot.value_text(index, definition)
    } else {
        snapshot.flag_text(index, definition)
    };
    let storage = definition.compact_slot.map_or_else(
        || "investment override".to_owned(),
        |slot| format!("bank {} · compact slot {slot}", definition.bank()),
    );
    hash_state_field(
        ui,
        state,
        format!(
            "Saved account state · {storage}\nActivity, progression, and other runtime writers can change the in-game result."
        ),
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
    egui::CollapsingHeader::new(format!("Known References ({})", definition.tested_by.len()))
        .id_salt(("hash_unlock_readers", definition_kind, definition_index))
        .default_open(definition.tested_by.len() <= HASH_RELATIONSHIP_AUTO_EXPAND_LIMIT)
        .show(ui, |ui| {
            ui.label("Package expressions, direct references, and runtime outputs found by Sundial. Each output is identified separately.");
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
                            "{}. {name} · {}",
                            context_index + 1,
                            progression_context_kind_label(context.kind)
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
                        for reference in &context.direct_references {
                            ui.label(reference);
                        }
                        egui::Grid::new((
                            "hash_unlock_reader_fields",
                            definition_kind,
                            definition_index,
                            context_index,
                        ))
                        .num_columns(2)
                        .spacing([16.0, 4.0])
                        .show(ui, |ui| {
                            match context.kind {
                                crate::catalog::ProgressionContextKind::PackageExpression => {
                                    hash_detail_field(ui, "Package Tag", format!("0x{:08X}", context.hash >> 32), true);
                                    hash_detail_field(ui, "Expression Offset", format!("0x{:X}", context.hash as u32), true);
                                }
                                crate::catalog::ProgressionContextKind::ExpressionMapping => hash_detail_field(ui, "Mapping Index", context.hash.to_string(), true),
                                _ => catalog_hash_hex_and_decimal_field(ui, catalog, "Definition Hash", context.hash),
                            }
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
                                "Condition Programs",
                                context.condition_programs.len().to_string(),
                                true,
                            );
                        });
                        draw_metadata_paths(ui, &context.paths);
                    });
            }
        });
}

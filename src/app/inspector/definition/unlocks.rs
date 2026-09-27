use super::item_details::{Cell, draw_table, item_name};
use super::progression::{paths_row, progression_cell, property_link, reference_role};
use super::*;
use crate::app::inspector::{
    definition_name, look, objective_target_text, resolved_objective_table_text,
};

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
        matches.inspected_hash,
        "Flag",
        &matches.flag_definitions,
        false,
        &mut editor,
    );
    draw_hash_unlock_kind(
        ui,
        catalog,
        matches.inspected_hash,
        "Value",
        &matches.value_definitions,
        true,
        &mut editor,
    );
}

/// An unlock definition's name, or its kind and index.
pub(super) fn unlock_label(
    catalog: &Catalog,
    kind: &str,
    index: usize,
    definition: &UnlockDefinition,
) -> String {
    definition_name(definition)
        .map(str::trim)
        .or_else(|| catalog.display_name(definition.hash))
        .map_or_else(|| format!("{kind} #{index}"), str::to_owned)
}

struct UnlockEditor<'a> {
    snapshot: Option<&'a CollectionStateSnapshot>,
    progression_editable: bool,
    action: &'a mut HashInspectorAction,
}

fn draw_hash_unlock_kind(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    current: u64,
    kind: &'static str,
    matches: &[(usize, &UnlockDefinition)],
    value_definition: bool,
    editor: &mut UnlockEditor<'_>,
) {
    if let [(index, definition)] = matches {
        draw_hash_unlock_definition(
            ui,
            catalog,
            current,
            kind,
            *index,
            definition,
            value_definition,
            editor,
        );
        return;
    }
    for (index, definition) in matches {
        let heading = definition_name(definition)
            .or_else(|| catalog.display_name(definition.hash))
            .map_or_else(
                || format!("{kind} #{index}"),
                |name| format!("{} · {kind} #{index}", name.trim()),
            );
        look::section(
            ui,
            ("hash_unlock_definition", kind, *index),
            &heading,
            None,
            matches.len() <= 3,
            |ui| {
                draw_hash_unlock_definition(
                    ui,
                    catalog,
                    current,
                    kind,
                    *index,
                    definition,
                    value_definition,
                    editor,
                );
            },
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_hash_unlock_definition(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    current: u64,
    kind: &'static str,
    index: usize,
    definition: &UnlockDefinition,
    value_definition: bool,
    editor: &mut UnlockEditor<'_>,
) {
    let seasonal_mod = catalog
        .seasonal()
        .and_then(|season| season.mod_for_flag(index))
        .filter(|_| !value_definition);
    look::properties(ui, ("hash_unlock_definition", kind, index), |p| {
        p.text(
            "Description",
            definition.description.as_deref().unwrap_or_default().trim(),
        );
        p.mono("Definition Index", index.to_string());
        p.mono("Code", format!("0x{:04X}", definition.code));
        p.mono("Bank", definition.bank().to_string());
        if let Some(slot) = definition.compact_slot {
            p.mono("Compact Slot", slot.to_string());
        }
        if let Some(snapshot) = editor.snapshot {
            let state = if value_definition {
                snapshot.value_text(index, definition)
            } else {
                snapshot.flag_text(index, definition)
            };
            let storage = definition.compact_slot.map_or_else(
                || "investment override".to_owned(),
                |slot| format!("bank {} · compact slot {slot}", definition.bank()),
            );
            p.custom("Current State", |ui| {
                ui.add(egui::Label::new(state).wrap())
                    .on_hover_text(format!("Saved account state · {storage}"));
            });
            if snapshot.is_native() && !value_definition {
                p.mono(
                    "Native Bank State",
                    snapshot
                        .native_flag(definition)
                        .map_or("Unavailable", |set| if set { "Set (2)" } else { "Not Set" }),
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
            p.mono(
                if snapshot.seasonal_authoring()
                    && (crate::app::progression::seasonal::is_derived_value(index)
                        && value_definition
                        || seasonal_mod.is_some())
                {
                    "After Sunrise Refresh"
                } else {
                    "Evaluated State"
                },
                evaluated.unwrap_or_else(|| "Unresolved".into()),
            );
            if editor.progression_editable {
                p.custom("Edit", |ui| {
                    draw_unlock_state_editor(
                        ui,
                        catalog,
                        index,
                        definition,
                        snapshot,
                        value_definition,
                        editor.action,
                    );
                });
            }
        }
    });
    if let Some(snapshot) = editor.snapshot
        && snapshot.is_dawn()
        && ((value_definition && crate::app::progression::seasonal::is_derived_value(index))
            || seasonal_mod.is_some())
    {
        ui.add_space(4.0);
        look::empty_state(ui, DAWN_SEASONAL_NOTE);
    }
    if editor
        .snapshot
        .is_some_and(CollectionStateSnapshot::seasonal_authoring)
        && let Some(entry) = seasonal_mod
    {
        look::subheading(ui, "Artifact Mod");
        look::properties(ui, ("hash_unlock_artifact_mod", index), |p| {
            p.mono("Column", (entry.column() + 1).to_string());
            p.mono("Sale Row", entry.sale_index.to_string());
            p.mono("Character Flag Slot", entry.character_slot.to_string());
            property_link(
                p,
                "Mod Item",
                catalog,
                entry.item_hash,
                current,
                item_name(catalog, entry.item_hash),
            );
            property_link(
                p,
                "Collectible",
                catalog,
                entry.collectible_hash,
                current,
                catalog
                    .display_name(entry.collectible_hash)
                    .unwrap_or_else(|| item_name(catalog, entry.item_hash)),
            );
        });
    }
    if value_definition {
        draw_unlock_value_objectives(ui, catalog, current, index);
    }
    draw_unlock_writers(ui, catalog, current, kind, index, definition);
    draw_hash_unlock_readers(ui, catalog, current, kind, index, definition);
}

/// Shown on Dawn for a flag or value Sunrise would manage through Seasonal.
pub(super) const DAWN_SEASONAL_NOTE: &str = "Dawn: seasonal counters are not updated.";

fn draw_unlock_state_editor(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    index: usize,
    definition: &UnlockDefinition,
    snapshot: &CollectionStateSnapshot,
    value_definition: bool,
    action: &mut HashInspectorAction,
) {
    if snapshot.seasonal_authoring()
        && value_definition
        && crate::app::progression::seasonal::is_derived_value(index)
    {
        ui.weak("Use Seasonal XP or Artifact Mods");
    } else if snapshot.seasonal_authoring()
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
            response.on_disabled_hover_text(error);
        }
    } else if !unlock_state_editable(definition, value_definition) {
        ui.weak("Read Only");
    } else if value_definition {
        draw_unlock_value_editor(ui, index, definition, snapshot, action);
    } else {
        draw_unlock_flag_editor(ui, index, definition, snapshot, action);
    }
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
    let editor_id = egui::Id::new(("hash_unlock_value_editor", definition.hash, index));
    let current = snapshot.value(index, definition).unwrap_or_default();
    // The draft keeps the value it started from and is dropped once that value changes.
    let mut value = ui
        .data(|data| data.get_temp::<(i32, i32)>(editor_id))
        .filter(|(start, _)| *start == current)
        .map_or(current, |(_, draft)| draft);
    let mut applied = false;
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
            applied = true;
        }
    });
    if applied || value == current {
        ui.data_mut(|data| data.remove::<(i32, i32)>(editor_id));
    } else {
        ui.data_mut(|data| data.insert_temp(editor_id, (current, value)));
    }
}

/// Objectives whose progress this unlock value holds.
fn draw_unlock_value_objectives(ui: &mut egui::Ui, catalog: &Catalog, current: u64, index: usize) {
    let objectives = catalog.objectives_for_unlock_value(index);
    if objectives.is_empty() {
        return;
    }
    let rows = objectives
        .iter()
        .map(|objective| {
            vec![
                Cell::link_unless(
                    objective.hash,
                    current,
                    resolved_objective_table_text(catalog, objective, None),
                ),
                Cell::mono(objective_target_text(objective)),
            ]
        })
        .collect::<Vec<_>>();
    look::section(
        ui,
        ("hash_unlock_value_objectives", index),
        "Objectives",
        Some(rows.len()),
        true,
        |ui| {
            draw_table(
                ui,
                catalog,
                ("hash_unlock_value_objectives", index),
                &["Objective", "Target"],
                &rows,
            );
        },
    );
}

/// What writes this flag or value at runtime.
fn draw_unlock_writers(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    current: u64,
    kind: &'static str,
    index: usize,
    definition: &UnlockDefinition,
) {
    if definition.runtime_writers.is_empty() {
        return;
    }
    let rows = definition
        .runtime_writers
        .iter()
        .map(|writer| match writer {
            UnlockWriter::ProgressionStep {
                definition_index,
                step_index,
            } => vec![
                progression_cell(catalog, *definition_index, current),
                Cell::text("Progression Rank"),
                Cell::text(format!("Rank {}", u32::from(*step_index) + 1)),
            ],
            UnlockWriter::ProgressionLevel { definition_index } => vec![
                progression_cell(catalog, *definition_index, current),
                Cell::text("Progression Level"),
                Cell::muted(""),
            ],
            UnlockWriter::ValueCounter { programs } => vec![
                Cell::text("Value Counter"),
                Cell::text("Counter"),
                Cell::text(match programs.len() {
                    1 => "1 condition program".to_owned(),
                    count => format!("{count} condition programs"),
                }),
            ],
            UnlockWriter::Context { source } => vec![
                Cell::text(source.as_str()),
                Cell::text("Context"),
                Cell::muted(""),
            ],
        })
        .collect::<Vec<_>>();
    look::section(
        ui,
        ("hash_unlock_writers", kind, index),
        "Written By",
        Some(rows.len()),
        rows.len() <= HASH_RELATIONSHIP_AUTO_EXPAND_LIMIT,
        |ui| {
            draw_table(
                ui,
                catalog,
                ("hash_unlock_writers", kind, index),
                &["Source", "Kind", "Detail"],
                &rows,
            );
            for (position, writer) in definition.runtime_writers.iter().enumerate() {
                if let UnlockWriter::ValueCounter { programs } = writer {
                    draw_hash_condition_programs(
                        ui,
                        egui::Id::new(("hash_unlock_writer", kind, index, position)),
                        programs,
                        catalog,
                    );
                }
            }
        },
    );
}

/// A known reference's name: its own, the catalog's, or its type.
fn reference_name(catalog: &Catalog, context: &ProgressionContextDef) -> String {
    (!context.name.trim().is_empty())
        .then_some(context.name.trim())
        .or_else(|| catalog.display_name(context.hash))
        .or_else(|| (!context.type_name.trim().is_empty()).then_some(context.type_name.trim()))
        .map_or_else(
            || {
                format!(
                    "{} · 0x{:08X}",
                    progression_context_kind_label(context.kind),
                    context.hash
                )
            },
            str::to_owned,
        )
}

/// Whether a reference's hash names a definition rather than a package position.
const fn reference_is_definition(context: &ProgressionContextDef) -> bool {
    !matches!(
        context.kind,
        crate::catalog::ProgressionContextKind::PackageExpression
            | crate::catalog::ProgressionContextKind::ExpressionMapping
    )
}

fn draw_hash_unlock_readers(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    current: u64,
    kind: &'static str,
    index: usize,
    definition: &UnlockDefinition,
) {
    if definition.tested_by.is_empty() {
        return;
    }
    let rows = definition
        .tested_by
        .iter()
        .map(|context| {
            let name = reference_name(catalog, context);
            vec![
                if reference_is_definition(context) {
                    Cell::link_unless(context.hash, current, name)
                } else {
                    Cell::Text(name)
                },
                Cell::text(progression_context_kind_label(context.kind)),
                Cell::text(reference_role(context)),
                Cell::mono(context.condition_programs.len()),
            ]
        })
        .collect::<Vec<_>>();
    look::section(
        ui,
        ("hash_unlock_readers", kind, index),
        "Referenced By",
        Some(rows.len()),
        true,
        |ui| {
            draw_table(
                ui,
                catalog,
                ("hash_unlock_readers", kind, index),
                &["Reference", "Kind", "Role", "Programs"],
                &rows,
            );
        },
    );
    look::section(
        ui,
        ("hash_unlock_reader_details", kind, index),
        "Reference Details",
        Some(definition.tested_by.len()),
        false,
        |ui| {
            for (context_index, context) in definition.tested_by.iter().enumerate() {
                look::subheading(ui, &reference_name(catalog, context));
                look::properties(
                    ui,
                    ("hash_unlock_reader_fields", kind, index, context_index),
                    |p| {
                        match context.kind {
                            crate::catalog::ProgressionContextKind::PackageExpression => {
                                p.mono("Package Tag", format!("0x{:08X}", context.hash >> 32));
                                p.mono("Expression Offset", format!("0x{:X}", context.hash as u32));
                            }
                            crate::catalog::ProgressionContextKind::ExpressionMapping => {
                                p.mono("Mapping Index", context.hash.to_string());
                            }
                            _ => p.hash("Definition Hash", catalog, context.hash),
                        }
                        p.text("Kind", progression_context_kind_label(context.kind));
                        p.text("Type", context.type_name.trim());
                        p.text("Description", context.description.trim());
                        p.text("Role", reference_role(context));
                        paths_row(p, "Paths", &context.paths);
                    },
                );
                draw_hash_condition_programs(
                    ui,
                    egui::Id::new(("hash_unlock_reader_detail", kind, index, context_index)),
                    &context.condition_programs,
                    catalog,
                );
            }
        },
    );
}

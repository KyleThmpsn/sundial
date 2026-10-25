//! Objective definitions, their owners, references and traits.
use super::*;

/// The four objective behaviour flags that are set, as short labels.
fn objective_behavior(objective: &ObjectiveDef) -> Vec<&'static str> {
    [
        (objective.allow_overcompletion, "Over-Completion"),
        (objective.allow_negative_value, "Negative Values"),
        (
            objective.allow_value_change_when_completed,
            "Changes after Completion",
        ),
        (objective.is_counting_downward, "Counts Downward"),
    ]
    .into_iter()
    .filter_map(|(set, label)| set.then_some(label))
    .collect()
}

fn objective_heading(catalog: &Catalog, index: usize, objective: &ObjectiveDef) -> String {
    if objective.name.trim().is_empty() {
        catalog
            .display_name(objective.hash)
            .map_or_else(|| format!("Objective #{index}"), str::to_owned)
    } else {
        objective.name.trim().to_owned()
    }
}

pub(super) fn draw_objective_matches(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    snapshot: Option<&CollectionStateSnapshot>,
    matches: &CatalogHashMatches<'_>,
) {
    let current = matches.inspected_hash;
    match matches.objectives.as_slice() {
        [] => {}
        [(index, objective)] => draw_objective(ui, catalog, snapshot, current, *index, objective),
        objectives => {
            for (index, objective) in objectives {
                look::section(
                    ui,
                    ("hash_objective", *index),
                    &objective_heading(catalog, *index, objective),
                    None,
                    objectives.len() <= 3,
                    |ui| draw_objective(ui, catalog, snapshot, current, *index, objective),
                );
            }
        }
    }
}

fn draw_objective(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    snapshot: Option<&CollectionStateSnapshot>,
    current: u64,
    index: usize,
    objective: &ObjectiveDef,
) {
    let value_index = objective
        .related_unlock_value_definition_index
        .map(usize::from);
    look::properties(ui, ("hash_objective", index), |p| {
        let display = objective.display_description.trim();
        let progress = objective.progress_description.trim();
        p.text(
            "Description",
            if display.is_empty() {
                objective.description.trim()
            } else {
                display
            },
        );
        if !progress.eq_ignore_ascii_case(display) {
            p.text("Progress Description", progress);
        }
        p.mono("Completion Value", objective.completion_value.to_string());
        if let (Some(snapshot), Some(value_index)) = (snapshot, value_index)
            && let Some(value) = snapshot.evaluated_value(value_index, catalog)
        {
            p.mono(
                "Current Progress",
                if objective_complete(objective, value) {
                    format!("{value} / {} · Complete", objective.completion_value)
                } else {
                    format!("{value} / {}", objective.completion_value)
                },
            );
        }
        let behavior = objective_behavior(objective);
        if !behavior.is_empty() {
            p.text("Behavior", behavior.join(" · "));
        }
        if let Some(value_index) = value_index
            && let Some(value) = catalog.unlock_value_definition(value_index)
        {
            property_link(
                p,
                "Unlock Value",
                catalog,
                value.hash,
                current,
                unlock_label(catalog, "Value", value_index, value),
            );
        }
        p.mono("Objective Index", index.to_string());
    });
    draw_objective_owner_table(ui, catalog, current, objective);
    draw_objective_intrinsic_perks(ui, catalog, current, objective);
    draw_objective_references(ui, catalog, current, objective);
    draw_hash_condition_programs(
        ui,
        egui::Id::new(("hash_objective_conditions", index, objective.hash)),
        &objective.condition_programs,
        catalog,
    );
}

/// An objective owner's name.
fn owner_name(catalog: &Catalog, owner: &ObjectiveOwnerDef) -> String {
    let name = owner.name.trim();
    if name.is_empty() {
        catalog
            .display_name(owner.hash)
            .unwrap_or(UNNAMED)
            .to_owned()
    } else {
        name.to_owned()
    }
}

fn draw_objective_owner_table(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    current: u64,
    objective: &ObjectiveDef,
) {
    if objective.owners.is_empty() {
        return;
    }
    let show_traits = objective
        .owners
        .iter()
        .any(|owner| !owner.traits.is_empty());
    let rows = objective
        .owners
        .iter()
        .map(|owner| {
            let kind = objective_owner_kind_label(owner.kind);
            let type_name = progression_type_label(&owner.type_name);
            let mut row = vec![
                Cell::link_unless(owner.hash, current, owner_name(catalog, owner)),
                Cell::text(kind),
                Cell::muted(if type_name.eq_ignore_ascii_case(kind) {
                    ""
                } else {
                    type_name
                }),
            ];
            if show_traits {
                row.push(Cell::Links(
                    owner
                        .traits
                        .iter()
                        .map(|trait_definition| {
                            let name = trait_definition.name.trim();
                            (
                                trait_definition.hash,
                                if name.is_empty() { UNNAMED } else { name }.to_owned(),
                            )
                        })
                        .collect(),
                ));
            }
            row
        })
        .collect::<Vec<_>>();
    let headings: &[&str] = if show_traits {
        &["Owner", "Kind", "Type", "Traits"]
    } else {
        &["Owner", "Kind", "Type"]
    };
    look::section(
        ui,
        ("hash_objective_owners", objective.hash),
        "Owners",
        Some(rows.len()),
        rows.len() <= HASH_RELATIONSHIP_AUTO_EXPAND_LIMIT,
        |ui| {
            draw_table(
                ui,
                catalog,
                ("hash_objective_owners", objective.hash),
                headings,
                &rows,
            );
        },
    );
}

fn draw_objective_intrinsic_perks(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    current: u64,
    objective: &ObjectiveDef,
) {
    if objective.intrinsic_perk_flag_definition_indices.is_empty() {
        return;
    }
    let rows = objective
        .intrinsic_perk_flag_definition_indices
        .iter()
        .map(|&raw_index| {
            let index = usize::from(raw_index);
            vec![
                catalog.unlock_flag_definition(index).map_or_else(
                    || Cell::muted(format!("Flag #{index}")),
                    |definition| {
                        Cell::link_unless(
                            definition.hash,
                            current,
                            unlock_label(catalog, "Flag", index, definition),
                        )
                    },
                ),
                Cell::mono(index),
            ]
        })
        .collect::<Vec<_>>();
    look::section(
        ui,
        ("hash_objective_intrinsic_perks", objective.hash),
        "Intrinsic Perk Flags",
        Some(rows.len()),
        true,
        |ui| {
            draw_table(
                ui,
                catalog,
                ("hash_objective_intrinsic_perks", objective.hash),
                &["Flag", "Index"],
                &rows,
            );
        },
    );
}

fn draw_objective_references(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    current: u64,
    objective: &ObjectiveDef,
) {
    if objective.referenced_objective_indices.is_empty() {
        return;
    }
    let rows = objective
        .referenced_objective_indices
        .iter()
        .map(|&raw_index| {
            let index = usize::from(raw_index);
            match catalog.objective_definition(index) {
                Some(target) => vec![
                    Cell::link_unless(
                        target.hash,
                        current,
                        resolved_objective_table_text(catalog, target, None),
                    ),
                    Cell::mono(target.completion_value),
                ],
                None => vec![Cell::muted(format!("Objective #{index}")), Cell::muted("")],
            }
        })
        .collect::<Vec<_>>();
    look::section(
        ui,
        ("hash_objective_references", objective.hash),
        "Referenced Objectives",
        Some(rows.len()),
        true,
        |ui| {
            draw_table(
                ui,
                catalog,
                ("hash_objective_references", objective.hash),
                &["Objective", "Completion Value"],
                &rows,
            );
        },
    );
}

/// Objectives the inspected hash owns, one row each. A record's own objectives are listed with
/// the record and left out here.
pub(super) fn draw_objective_owners(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    current: u64,
    matches: &CatalogHashMatches<'_>,
) {
    let listed = matches
        .record_matches
        .iter()
        .flat_map(|(_, record)| record.objectives.iter().copied())
        .collect::<std::collections::HashSet<_>>();
    let rows = matches
        .owner_matches
        .iter()
        .filter(|(objective_index, _, owner)| {
            owner.kind != ObjectiveOwnerKind::Record || !listed.contains(objective_index)
        })
        .map(|(_, objective, owner)| {
            vec![
                Cell::link_unless(owner.hash, current, owner_name(catalog, owner)),
                Cell::text(objective_owner_kind_label(owner.kind)),
                Cell::link_unless(
                    objective.hash,
                    current,
                    resolved_objective_table_text(catalog, objective, None),
                ),
                Cell::mono(objective.completion_value),
            ]
        })
        .collect::<Vec<_>>();
    if rows.is_empty() {
        return;
    }
    look::section(
        ui,
        ("hash_owned_objectives", current),
        "Owned Objectives",
        Some(rows.len()),
        rows.len() <= HASH_RELATIONSHIP_AUTO_EXPAND_LIMIT,
        |ui| {
            draw_table(
                ui,
                catalog,
                ("hash_owned_objectives", current),
                &["Owner", "Kind", "Objective", "Completion Value"],
                &rows,
            );
        },
    );
}

/// Objective owners that carry the inspected hash as a trait.
pub(super) fn draw_objective_traits(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    current: u64,
    matches: &CatalogHashMatches<'_>,
) {
    let trait_matches = &matches.trait_matches;
    if trait_matches.is_empty() {
        return;
    }
    let rows = trait_matches
        .iter()
        .map(|(_, objective, owner, _)| {
            vec![
                Cell::link_unless(owner.hash, current, owner_name(catalog, owner)),
                Cell::text(objective_owner_kind_label(owner.kind)),
                Cell::link_unless(
                    objective.hash,
                    current,
                    resolved_objective_table_text(catalog, objective, None),
                ),
            ]
        })
        .collect::<Vec<_>>();
    look::section(
        ui,
        ("hash_objective_traits", current),
        "Objective Owners",
        Some(rows.len()),
        rows.len() <= HASH_RELATIONSHIP_AUTO_EXPAND_LIMIT,
        |ui| {
            draw_table(
                ui,
                catalog,
                ("hash_objective_traits", current),
                &["Owner", "Kind", "Objective"],
                &rows,
            );
        },
    );
}

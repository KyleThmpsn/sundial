//! Record definitions, their status and references.
use super::*;

/// A record's name, or its index.
pub(crate) fn record_name(catalog: &Catalog, record: &RecordDefinition) -> String {
    if !record.name.trim().is_empty() {
        return record.name.trim().to_owned();
    }
    catalog
        .display_name(record.hash)
        .map_or_else(|| format!("Record #{}", record.index), str::to_owned)
}

/// A record's progress read from saved state.
pub(crate) struct RecordStatus {
    /// Each objective's current value, in the record's objective order.
    pub(crate) values: Vec<Option<i32>>,
    pub(crate) label: &'static str,
}

pub(super) fn objective_complete(objective: &ObjectiveDef, value: i32) -> bool {
    if objective.is_counting_downward {
        value <= objective.completion_value
    } else {
        value >= objective.completion_value
    }
}

/// The status the Triumphs page shows for a record.
pub(crate) fn record_status(
    record: &RecordDefinition,
    catalog: &Catalog,
    snapshot: &CollectionStateSnapshot,
) -> RecordStatus {
    let (values, label) = crate::app::progression::record_progress(record, catalog, snapshot);
    RecordStatus { values, label }
}

/// Records (triumphs) that carry this hash. The Triumphs page renders the same definitions. This
/// makes them reachable by hash.
pub(super) fn draw_record_matches(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    snapshot: Option<&CollectionStateSnapshot>,
    inspected_hash: u64,
    matches: &CatalogHashMatches<'_>,
) {
    match matches.record_matches.as_slice() {
        [] => {}
        [(index, record)] => draw_record(ui, catalog, snapshot, inspected_hash, *index, record),
        records => {
            for (index, record) in records {
                look::section(
                    ui,
                    ("hash_record", *index),
                    &record_name(catalog, record),
                    None,
                    records.len() <= 3,
                    |ui| draw_record(ui, catalog, snapshot, inspected_hash, *index, record),
                );
            }
        }
    }
}

fn draw_record(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    snapshot: Option<&CollectionStateSnapshot>,
    current: u64,
    index: usize,
    record: &RecordDefinition,
) {
    let status = snapshot.map(|snapshot| record_status(record, catalog, snapshot));
    look::properties(ui, ("hash_record", index), |p| {
        if let Some(status) = &status {
            p.text("Status", status.label);
        }
        if let Some(runtime) = record.runtime.as_ref().filter(|runtime| runtime.score > 0) {
            p.mono("Score", runtime.score.to_string());
        }
        if let Some(flag_index) = record.completion_flag.map(usize::from)
            && let Some(flag) = catalog.unlock_flag_definition(flag_index)
        {
            property_link(
                p,
                "Completion Flag",
                catalog,
                flag.hash,
                current,
                unlock_label(catalog, "Flag", flag_index, flag),
            );
        }
        if let Some(value_index) = record.redeemed_intervals.map(usize::from)
            && let Some(value) = catalog.unlock_value_definition(value_index)
        {
            property_link(
                p,
                "Redeemed Intervals",
                catalog,
                value.hash,
                current,
                unlock_label(catalog, "Value", value_index, value),
            );
        }
        p.mono("Record Index", index.to_string());
        parent_nodes_row(p, catalog, &record.parent_nodes);
        paths_row(p, "Paths", &record.paths);
    });

    if !record.objectives.is_empty() {
        let rows = record
            .objectives
            .iter()
            .enumerate()
            .map(|(position, objective_index)| {
                let mut row = match catalog.objective_definition(*objective_index) {
                    Some(objective) => vec![
                        Cell::link_unless(
                            objective.hash,
                            current,
                            resolved_objective_table_text(catalog, objective, None),
                        ),
                        Cell::mono(objective.completion_value),
                    ],
                    None => vec![
                        Cell::muted(format!("Objective #{objective_index}")),
                        Cell::muted(""),
                    ],
                };
                if let Some(status) = &status {
                    row.push(status.values.get(position).copied().flatten().map_or_else(
                        || Cell::muted("Unresolved"),
                        |value| Cell::Mono(value.to_string()),
                    ));
                }
                row
            })
            .collect::<Vec<_>>();
        let headings: &[&str] = if status.is_some() {
            &["Objective", "Completion Value", "Progress"]
        } else {
            &["Objective", "Completion Value"]
        };
        look::section(
            ui,
            ("hash_record_objectives", index),
            "Objectives",
            Some(rows.len()),
            true,
            |ui| {
                draw_table(
                    ui,
                    catalog,
                    ("hash_record_objectives", index),
                    headings,
                    &rows,
                );
            },
        );
    }

    let Some(runtime) = &record.runtime else {
        return;
    };
    let item_cell = |item_index: usize| {
        catalog.item_hash_for_index(item_index).map_or_else(
            || Cell::muted(format!("Item #{item_index}")),
            |hash| Cell::link_unless(hash, current, item_name(catalog, hash)),
        )
    };
    if !runtime.interval_scores.is_empty() {
        let rows = runtime
            .interval_scores
            .iter()
            .enumerate()
            .map(|(interval, score)| {
                vec![
                    Cell::mono(interval + 1),
                    Cell::mono(score),
                    runtime
                        .interval_items
                        .get(interval)
                        .copied()
                        .flatten()
                        .map_or_else(|| Cell::muted(""), item_cell),
                ]
            })
            .collect::<Vec<_>>();
        look::section(
            ui,
            ("hash_record_intervals", index),
            "Intervals",
            Some(rows.len()),
            rows.len() <= HASH_RELATIONSHIP_AUTO_EXPAND_LIMIT,
            |ui| {
                draw_table(
                    ui,
                    catalog,
                    ("hash_record_intervals", index),
                    &["Interval", "Score", "Reward"],
                    &rows,
                );
            },
        );
    }
    if !runtime.rewards.is_empty() {
        let rows = runtime
            .rewards
            .iter()
            .map(|(item_index, quantity)| vec![item_cell(*item_index), Cell::mono(quantity)])
            .collect::<Vec<_>>();
        look::section(
            ui,
            ("hash_record_rewards", index),
            "Rewards",
            Some(rows.len()),
            true,
            |ui| {
                draw_table(
                    ui,
                    catalog,
                    ("hash_record_rewards", index),
                    &["Item", "Quantity"],
                    &rows,
                );
            },
        );
    }
}

/// Records that reach the inspected hash through an objective or their completion flag.
pub(super) fn draw_record_references(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    snapshot: Option<&CollectionStateSnapshot>,
    current: u64,
    matches: &CatalogHashMatches<'_>,
) {
    let references = &matches.record_references;
    if references.is_empty() {
        return;
    }
    let rows = references
        .iter()
        .map(|(_, record, kind)| {
            let mut row = vec![
                Cell::link_unless(record.hash, current, record_name(catalog, record)),
                Cell::text(*kind),
            ];
            if let Some(snapshot) = snapshot {
                row.push(Cell::text(record_status(record, catalog, snapshot).label));
            }
            row
        })
        .collect::<Vec<_>>();
    let headings: &[&str] = if snapshot.is_some() {
        &["Record", "Via", "Status"]
    } else {
        &["Record", "Via"]
    };
    look::section(
        ui,
        ("hash_record_references", current),
        "Used by Records",
        Some(rows.len()),
        rows.len() <= HASH_RELATIONSHIP_AUTO_EXPAND_LIMIT,
        |ui| {
            draw_table(
                ui,
                catalog,
                ("hash_record_references", current),
                headings,
                &rows,
            )
        },
    );
}

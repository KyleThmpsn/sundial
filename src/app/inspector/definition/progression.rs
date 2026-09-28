use super::item_details::{Cell, draw_table, item_name};
use super::unlocks::unlock_label;
use super::*;
use crate::app::inspector::{look, progression_type_label, resolved_objective_table_text};

pub(super) fn draw_hash_progression_matches(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    document: Option<&Value>,
    collection_state: Option<&CollectionStateSnapshot>,
    inspected_hash: u64,
    matches: &CatalogHashMatches<'_>,
) {
    draw_progression_definitions(ui, catalog, document, collection_state, matches);
    draw_objective_matches(ui, catalog, collection_state, matches);
    draw_record_matches(ui, catalog, collection_state, inspected_hash, matches);
    draw_seasonal_matches(ui, catalog, collection_state, matches);
    draw_mission_matches(ui, catalog, document, matches);
    draw_hash_references(ui, catalog, collection_state, inspected_hash, matches);
}

/// Everything that points at the inspected hash, or that it points at, after its own content.
fn draw_hash_references(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    snapshot: Option<&CollectionStateSnapshot>,
    inspected_hash: u64,
    matches: &CatalogHashMatches<'_>,
) {
    draw_record_references(ui, catalog, snapshot, inspected_hash, matches);
    draw_objective_owners(ui, catalog, inspected_hash, matches);
    draw_objective_traits(ui, catalog, inspected_hash, matches);
    draw_reward_references(ui, catalog, inspected_hash, matches);
    draw_faction_references(ui, catalog, inspected_hash, matches);
    draw_progression_readers(ui, catalog, inspected_hash, matches);
}

/// A property row that opens another definition, or plain text when it names the page shown.
pub(super) fn property_link(
    p: &mut look::Properties<'_>,
    label: &str,
    catalog: &Catalog,
    hash: u64,
    current: u64,
    name: impl Into<String>,
) {
    if hash == current {
        p.text(label, name);
    } else {
        p.link(label, catalog, hash, name);
    }
}

/// A presentation node's name.
pub(super) fn node_name(catalog: &Catalog, hash: u64) -> String {
    catalog
        .presentation_node(hash)
        .map(|node| node.name.trim())
        .filter(|name| !name.is_empty())
        .or_else(|| catalog.display_name(hash))
        .unwrap_or(UNNAMED)
        .to_owned()
}

/// A "Parent Nodes" row of presentation node links.
pub(super) fn parent_nodes_row(p: &mut look::Properties<'_>, catalog: &Catalog, parents: &[u64]) {
    if parents.is_empty() {
        return;
    }
    p.custom("Parent Nodes", |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            for parent in parents {
                draw_named_catalog_hash_link(ui, catalog, *parent, node_name(catalog, *parent));
            }
        });
    });
}

/// Paths as root-first "A › B › C" text. Package paths are stored leaf first.
pub(super) fn path_texts(paths: &[Vec<String>]) -> Vec<String> {
    paths
        .iter()
        .map(|path| {
            path.iter()
                .rev()
                .map(|part| part.trim())
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join(" › ")
        })
        .filter(|path| !path.is_empty())
        .collect()
}

/// A "Paths" row of muted path lines.
pub(super) fn paths_row(p: &mut look::Properties<'_>, label: &str, paths: &[Vec<String>]) {
    let paths = path_texts(paths);
    if paths.is_empty() {
        return;
    }
    p.custom(label, |ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            for path in paths {
                ui.label(egui::RichText::new(path).color(look::muted(ui)));
            }
        });
    });
}

/// Artifact mods and Season Pass rewards inspected by their item or collectible hash. The
/// Seasonal page renders the same definitions for Sunrise. Here they resolve by hash on either
/// runtime, with ownership read from the loaded view.
fn draw_seasonal_matches(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    snapshot: Option<&CollectionStateSnapshot>,
    matches: &CatalogHashMatches<'_>,
) {
    let Some(season) = catalog.seasonal() else {
        return;
    };
    let current = matches.inspected_hash;
    if !matches.artifact_mods.is_empty() {
        look::section(
            ui,
            ("hash_artifact_mods", current),
            "Artifact Mods",
            Some(matches.artifact_mods.len()),
            true,
            |ui| {
                if snapshot.is_some_and(CollectionStateSnapshot::is_dawn) {
                    look::empty_state(ui, crate::app::progression::seasonal::DAWN_UNAVAILABLE);
                }
                for entry in &matches.artifact_mods {
                    look::subheading(
                        ui,
                        &format!(
                            "Column {} · Sale Row {}",
                            entry.column() + 1,
                            entry.sale_index
                        ),
                    );
                    look::properties(ui, ("hash_artifact_mod", entry.sale_index), |p| {
                        p.mono("Points Required", entry.points_required().to_string());
                        p.mono("Character Flag Slot", entry.character_slot.to_string());
                        if let Some(snapshot) = snapshot {
                            let owned = snapshot.artifact_mask(season, true) & entry.bit() != 0;
                            p.text("Owned by Selected Character", yes_no(owned));
                        }
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
                        let flag_index = usize::from(entry.flag_definition);
                        if let Some(flag) = catalog.unlock_flag_definition(flag_index) {
                            property_link(
                                p,
                                "Unlock Flag",
                                catalog,
                                flag.hash,
                                current,
                                unlock_label(catalog, "Flag", flag_index, flag),
                            );
                        }
                    });
                }
            },
        );
    }
    if let Some(grant) = matches.season_pass_reward {
        look::section(
            ui,
            ("hash_season_pass_reward", current),
            "Season Pass Reward",
            None,
            true,
            |ui| {
                look::properties(ui, ("hash_season_pass_reward", current), |p| {
                    p.text("Grant", grant.label());
                });
                if let crate::investment::seasonal::RewardGrant::ClassPackage(items) = grant {
                    let rows = items
                        .iter()
                        .map(|item| {
                            vec![
                                Cell::link_unless(*item, current, item_name(catalog, *item)),
                                Cell::muted(
                                    catalog.package_item_type_name(*item).unwrap_or_default(),
                                ),
                            ]
                        })
                        .collect::<Vec<_>>();
                    draw_table(
                        ui,
                        catalog,
                        ("hash_season_pass_reward_items", current),
                        &["Package Item", "Type"],
                        &rows,
                    );
                }
            },
        );
    }
}

/// A mission Dawn tracks by the FNV-1a hash of its scenario package name, with the saved state
/// the loaded Dawn account holds for it.
fn draw_mission_matches(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    document: Option<&Value>,
    matches: &CatalogHashMatches<'_>,
) {
    let Some(scenario) = matches.mission_scenario else {
        return;
    };
    look::section(
        ui,
        ("hash_dawn_mission", scenario),
        "Dawn Mission",
        None,
        true,
        |ui| {
            look::properties(ui, ("hash_dawn_mission", scenario), |p| {
                p.mono("Scenario Package", scenario);
            });
            let Some(document) = document else {
                look::empty_state(ui, "No Account Loaded");
                return;
            };
            if document.get("_dawn_activity").is_none() {
                look::empty_state(ui, "Dawn Accounts Only");
                return;
            }
            let rows = document["_dawn_activity"]["missions"]
                .as_array()
                .map(|rows| {
                    rows.iter()
                        .filter(|row| {
                            row["hash"]
                                .as_u64()
                                .and_then(|hash| u32::try_from(hash).ok())
                                == u32::try_from(matches.inspected_hash).ok()
                        })
                        .map(|row| {
                            vec![
                                Cell::text(row["scope"].as_str().unwrap_or("?")),
                                Cell::text(yes_no(row["completed"].as_bool().unwrap_or(false))),
                                Cell::mono(&row["progress"]),
                                Cell::mono(&row["activity"]),
                                Cell::Mono(format!(
                                    "{:08X}",
                                    row["checkpoint"].as_u64().unwrap_or(0)
                                )),
                            ]
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if rows.is_empty() {
                look::empty_state(ui, "Not Started");
            } else {
                draw_table(
                    ui,
                    catalog,
                    ("hash_dawn_mission_rows", scenario),
                    &[
                        "Scope",
                        "Completed",
                        "Progress",
                        "Activity Index",
                        "Checkpoint",
                    ],
                    &rows,
                );
            }
        },
    );
}

/// The Dawn vendor a progression definition backs, with the reputation the loaded account holds.
fn draw_dawn_vendor(
    ui: &mut egui::Ui,
    document: Option<&Value>,
    definition: &ProgressionDefinition,
) {
    let Some((vendor, personal, per_package)) =
        crate::app::dawn_state::vendors::vendor_for_progression(definition.definition_index)
    else {
        return;
    };
    // A Sunrise account keeps this definition as an ordinary faction progression. The vendor
    // join is Dawn's, so it is shown for Dawn views and for package browsing without an account.
    if document.is_some_and(|document| {
        !crate::persistence::progression::is_dawn_progression_view(document)
    }) {
        return;
    }
    look::subheading(ui, "Dawn Vendor");
    look::properties(ui, ("hash_dawn_vendor", vendor), |p| {
        p.mono("Vendor Index", vendor.to_string());
        p.text(
            "Progress Scope",
            if personal { "Character" } else { "Account" },
        );
        p.mono("Points per Package", per_package.to_string());
        let rows = document
            .and_then(|document| document["_dawn_activity"]["vendors"].as_array())
            .map(|rows| {
                rows.iter()
                    .filter(|row| row["vendor"].as_u64() == Some(u64::from(vendor)))
                    .collect::<Vec<_>>()
            });
        match rows {
            None => p.text("Reputation", "No Account Loaded"),
            Some(rows) if rows.is_empty() => p.text("Reputation", "Not Started"),
            Some(rows) => {
                for row in rows {
                    let points = row["points"].as_i64().unwrap_or(0);
                    let rewards = row["rewards"].as_i64().unwrap_or(0);
                    let available = (points / i64::from(per_package) - rewards).max(0);
                    p.text(
                        &format!("Reputation ({})", row["scope"].as_str().unwrap_or("?")),
                        format!(
                            "{points} points · {rewards} packages claimed · {available} available"
                        ),
                    );
                }
            }
        }
    });
}

/// A record's name, or its index.
pub(super) fn record_name(catalog: &Catalog, record: &RecordDefinition) -> String {
    if !record.name.trim().is_empty() {
        return record.name.trim().to_owned();
    }
    catalog
        .display_name(record.hash)
        .map_or_else(|| format!("Record #{}", record.index), str::to_owned)
}

/// A record's progress read from saved state.
pub(super) struct RecordStatus {
    /// Each objective's current value, in the record's objective order.
    pub(super) values: Vec<Option<i32>>,
    pub(super) label: &'static str,
}

fn objective_complete(objective: &ObjectiveDef, value: i32) -> bool {
    if objective.is_counting_downward {
        value <= objective.completion_value
    } else {
        value >= objective.completion_value
    }
}

/// The status the Triumphs page shows for a record.
pub(super) fn record_status(
    record: &RecordDefinition,
    catalog: &Catalog,
    snapshot: &CollectionStateSnapshot,
) -> RecordStatus {
    let (values, label) = crate::app::progression::record_progress(record, catalog, snapshot);
    RecordStatus { values, label }
}

/// Records (triumphs) that carry this hash. The Triumphs page renders the same definitions. This
/// makes them reachable by hash.
fn draw_record_matches(
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
fn draw_record_references(
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

fn draw_progression_definitions(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    document: Option<&Value>,
    collection_state: Option<&CollectionStateSnapshot>,
    matches: &CatalogHashMatches<'_>,
) {
    match matches.progression_definitions.as_slice() {
        [] => {}
        [(index, definition)] => draw_hash_progression_definition(
            ui,
            catalog,
            document,
            collection_state,
            *index,
            definition,
        ),
        definitions => {
            for (index, definition) in definitions {
                look::section(
                    ui,
                    ("hash_progression_definition", *index),
                    &progression_name(*index, definition),
                    None,
                    false,
                    |ui| {
                        draw_hash_progression_definition(
                            ui,
                            catalog,
                            document,
                            collection_state,
                            *index,
                            definition,
                        );
                    },
                );
            }
        }
    }
}

/// A progression's name, or its index.
fn progression_name(index: usize, definition: &ProgressionDefinition) -> String {
    progression_display_name(definition).unwrap_or_else(|| format!("Progression #{index}"))
}

/// A progression definition by its native definition index.
fn progression_by_definition_index(
    catalog: &Catalog,
    definition_index: u16,
) -> Option<(usize, &ProgressionDefinition)> {
    catalog
        .progression_definitions()
        .iter()
        .enumerate()
        .find(|(_, definition)| definition.definition_index == definition_index)
}

/// A progression named by its native definition index, as a table cell.
pub(super) fn progression_cell(catalog: &Catalog, definition_index: u16, current: u64) -> Cell {
    progression_by_definition_index(catalog, definition_index).map_or_else(
        || Cell::text(format!("Progression #{definition_index}")),
        |(index, definition)| {
            Cell::link_unless(
                definition.hash,
                current,
                progression_name(index, definition),
            )
        },
    )
}

fn draw_reward_references(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    current: u64,
    matches: &CatalogHashMatches<'_>,
) {
    let reward_matches = &matches.progression_reward_matches;
    if reward_matches.is_empty() {
        return;
    }
    let rows = reward_matches
        .iter()
        .map(|(definition_index, definition, reward_index)| {
            let reward = &definition.reward_items[*reward_index];
            vec![
                Cell::link_unless(
                    definition.hash,
                    current,
                    progression_name(*definition_index, definition),
                ),
                Cell::mono(reward.rewarded_at_progression_level),
                Cell::mono(reward.quantity),
                Cell::mono(reward_index),
            ]
        })
        .collect::<Vec<_>>();
    look::section(
        ui,
        ("hash_progression_reward_references", current),
        "Progression Rewards",
        Some(rows.len()),
        rows.len() <= HASH_RELATIONSHIP_AUTO_EXPAND_LIMIT,
        |ui| {
            draw_table(
                ui,
                catalog,
                ("hash_progression_reward_references", current),
                &["Progression", "Level", "Quantity", "Reward Index"],
                &rows,
            );
        },
    );
}

fn draw_faction_references(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    current: u64,
    matches: &CatalogHashMatches<'_>,
) {
    let faction_matches = &matches.progression_faction_matches;
    if faction_matches.is_empty() {
        return;
    }
    let rows = faction_matches
        .iter()
        .map(|(definition_index, definition, _, faction)| {
            vec![
                Cell::link_unless(
                    definition.hash,
                    current,
                    progression_name(*definition_index, definition),
                ),
                Cell::text(faction.name.trim()),
            ]
        })
        .collect::<Vec<_>>();
    look::section(
        ui,
        ("hash_progression_faction_references", current),
        "Faction Progressions",
        Some(rows.len()),
        true,
        |ui| {
            draw_table(
                ui,
                catalog,
                ("hash_progression_faction_references", current),
                &["Progression", "Faction"],
                &rows,
            );
        },
    );
}

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

fn draw_objective_matches(
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
fn draw_objective_owners(
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
fn draw_objective_traits(
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

/// Unlock flags and values whose known references include the inspected hash.
fn draw_progression_readers(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    current: u64,
    matches: &CatalogHashMatches<'_>,
) {
    let readers = &matches.context_matches;
    if readers.is_empty() {
        return;
    }
    let rows = readers
        .iter()
        .map(|(kind, definition_index, context)| {
            let definition = match *kind {
                "Flag" => catalog.unlock_flag_definition(*definition_index),
                "Value" => catalog.unlock_value_definition(*definition_index),
                _ => None,
            };
            vec![
                definition.map_or_else(
                    || Cell::text(format!("{kind} #{definition_index}")),
                    |definition| {
                        Cell::link_unless(
                            definition.hash,
                            current,
                            unlock_label(catalog, kind, *definition_index, definition),
                        )
                    },
                ),
                Cell::text(format!("Unlock {kind}")),
                Cell::text(reference_role(context)),
                Cell::mono(context.condition_programs.len()),
            ]
        })
        .collect::<Vec<_>>();
    look::section(
        ui,
        ("hash_referenced_unlocks", current),
        "Referenced Unlocks",
        Some(rows.len()),
        rows.len() <= HASH_RELATIONSHIP_AUTO_EXPAND_LIMIT,
        |ui| {
            draw_table(
                ui,
                catalog,
                ("hash_referenced_unlocks", current),
                &["Unlock", "Kind", "Role", "Programs"],
                &rows,
            );
            draw_reference_details(ui, catalog, current, readers);
        },
    );
}

/// How a reference reaches its unlock definition: its direct reference fields, or a condition.
pub(super) fn reference_role(context: &ProgressionContextDef) -> String {
    if !context.direct_references.is_empty() {
        context.direct_references.join(" · ")
    } else if !context.condition_programs.is_empty() {
        "Condition".into()
    } else {
        String::new()
    }
}

/// Descriptions, paths and programs of the references, each distinct one once.
fn draw_reference_details(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    current: u64,
    readers: &[(&'static str, usize, &ProgressionContextDef)],
) {
    let mut distinct: Vec<&ProgressionContextDef> = Vec::new();
    for (_, _, context) in readers {
        let has_details = !context.description.trim().is_empty()
            || !context.paths.is_empty()
            || !context.condition_programs.is_empty();
        if has_details
            && !distinct.iter().any(|seen| {
                seen.kind == context.kind
                    && seen.type_name == context.type_name
                    && seen.description == context.description
                    && seen.paths == context.paths
                    && seen.condition_programs == context.condition_programs
            })
        {
            distinct.push(*context);
        }
    }
    if distinct.is_empty() {
        return;
    }
    look::section(
        ui,
        ("hash_reference_details", current),
        "Reference Details",
        None,
        false,
        |ui| {
            for (position, context) in distinct.into_iter().enumerate() {
                look::properties(ui, ("hash_reference_detail", current, position), |p| {
                    p.text("Type", progression_reader_type(context));
                    p.text("Description", context.description.trim());
                    paths_row(p, "Paths", &context.paths);
                });
                draw_hash_condition_programs(
                    ui,
                    egui::Id::new(("hash_reference_detail", current, position)),
                    &context.condition_programs,
                    catalog,
                );
            }
        },
    );
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
    collection_state: Option<&CollectionStateSnapshot>,
    index: usize,
    definition: &ProgressionDefinition,
) {
    let rank = collection_state.and_then(|snapshot| snapshot.progression_rank(definition));
    if hash_inspector_uses_wide_summary(ui.available_width()) {
        ui.columns(2, |columns| {
            draw_hash_progression_identity_summary(&mut columns[0], catalog, index, definition);
            draw_hash_progression_persistence_summary(
                &mut columns[1],
                document,
                rank,
                index,
                definition,
            );
        });
    } else {
        draw_hash_progression_identity_summary(ui, catalog, index, definition);
        draw_hash_progression_persistence_summary(ui, document, rank, index, definition);
    }
    draw_dawn_vendor(ui, document, definition);

    let seasonal = crate::app::progression::seasonal::is_xp_progression(index);
    if seasonal && document.is_some_and(crate::persistence::progression::is_dawn_progression_view) {
        ui.add_space(6.0);
        look::empty_state(ui, crate::app::progression::seasonal::DAWN_UNAVAILABLE);
    }
    if seasonal
        && document.is_some_and(crate::persistence::progression::supports_seasonal_authoring)
        && let Some(snapshot) = collection_state
        && let Some(season) = catalog.seasonal()
        && let Ok(experience) = snapshot.seasonal_experience(season)
    {
        look::subheading(ui, "After Sunrise Refresh");
        look::properties(ui, ("hash_progression_seasonal", index), |p| {
            p.mono("Rank", experience.rank.to_string());
            p.mono("Artifact Power", format!("+{}", experience.power_bonus));
            p.mono("Artifact Points", experience.points_earned.to_string());
            if let Some((_, total)) = experience
                .lanes()
                .into_iter()
                .find(|(slot, _)| *slot == index)
            {
                p.mono("Lane 0", total.to_string());
            }
        });
    }

    if !definition.factions.is_empty() {
        let rows = definition
            .factions
            .iter()
            .enumerate()
            .map(|(faction_index, faction)| {
                let name = if faction.name.trim().is_empty() {
                    format!("Faction #{faction_index}")
                } else {
                    faction.name.trim().to_owned()
                };
                vec![
                    Cell::link_unless(faction.hash, definition.hash, name),
                    Cell::muted(faction.description.trim()),
                ]
            })
            .collect::<Vec<_>>();
        look::section(
            ui,
            ("hash_progression_factions", index),
            "Factions",
            Some(rows.len()),
            true,
            |ui| {
                draw_table(
                    ui,
                    catalog,
                    ("hash_progression_factions", index),
                    &["Faction", "Description"],
                    &rows,
                );
            },
        );
    }

    if !definition.steps.is_empty() {
        look::section(
            ui,
            ("hash_progression_steps", index),
            "Steps",
            Some(definition.steps.len()),
            definition.steps.len() <= 20,
            |ui| draw_progression_steps(ui, catalog, rank, index, definition),
        );
    }

    if !definition.reward_items.is_empty() {
        look::section(
            ui,
            ("hash_progression_reward_items", index),
            "Reward Items",
            Some(definition.reward_items.len()),
            false,
            |ui| {
                crate::app::progression::seasonal::rewards::draw(ui, catalog, document, definition);
            },
        );
    }
}

fn draw_progression_steps(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    rank: Option<i32>,
    index: usize,
    definition: &ProgressionDefinition,
) {
    let has_step_icons = definition
        .steps
        .iter()
        .any(|step| step.icon_container.is_some());
    let has_flags = definition
        .steps
        .iter()
        .any(|step| step.unlock_flag.is_some());
    let columns =
        3 + usize::from(has_step_icons) + usize::from(has_flags) + usize::from(rank.is_some());
    egui::Grid::new(("hash_progression_step_rows", index))
        .num_columns(columns)
        .striped(true)
        .spacing([16.0, 4.0])
        .show(ui, |ui| {
            if has_step_icons {
                ui.strong("Icon");
            }
            ui.strong("Rank");
            ui.strong("Name");
            ui.strong("Cost");
            if has_flags {
                ui.strong("Unlock Flag");
            }
            if rank.is_some() {
                ui.strong("State");
            }
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
                        ui.label("");
                    }
                }
                ui.monospace((step_index + 1).to_string());
                ui.label(step.name.trim());
                ui.monospace(step.cost.to_string());
                if has_flags {
                    if let Some(flag_index) = step.unlock_flag.map(usize::from)
                        && let Some(flag) = catalog.unlock_flag_definition(flag_index)
                    {
                        draw_named_catalog_hash_link(
                            ui,
                            catalog,
                            flag.hash,
                            unlock_label(catalog, "Flag", flag_index, flag),
                        );
                    } else {
                        ui.label("");
                    }
                }
                if let Some(rank) = rank {
                    let reached = i32::try_from(step_index).is_ok_and(|step| step < rank);
                    ui.label(if reached { "Reached" } else { "" });
                }
                ui.end_row();
            }
        });
}

fn draw_hash_progression_identity_summary(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    index: usize,
    definition: &ProgressionDefinition,
) {
    look::subheading(ui, "Definition");
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
                    .fit_to_exact_size(egui::vec2(56.0, 56.0))
                    .maintain_aspect_ratio(true),
            );
            ui.add_space(8.0);
        }
        ui.vertical(|ui| {
            look::properties(ui, ("hash_progression_definition", index), |p| {
                p.text("Description", definition.description.trim());
                p.text("Source", definition.source.trim());
                p.text("Display Units", definition.display_units_name.trim());
                p.mono("Definition Index", definition.definition_index.to_string());
                if let Some(target) = progression_target(definition) {
                    p.mono("Target", target.to_string());
                }
                if let Some(value_index) = definition.level_value.map(usize::from)
                    && let Some(value) = catalog.unlock_value_definition(value_index)
                {
                    p.link(
                        "Level Value",
                        catalog,
                        value.hash,
                        unlock_label(catalog, "Value", value_index, value),
                    );
                }
            });
        });
    });
}

fn draw_hash_progression_persistence_summary(
    ui: &mut egui::Ui,
    document: Option<&Value>,
    rank: Option<i32>,
    index: usize,
    definition: &ProgressionDefinition,
) {
    look::subheading(ui, "Persistence");
    look::properties(ui, ("hash_progression_persistence", index), |p| {
        p.text("Scope", progression_scope_label(definition.scope));
        if let Some(slot) = definition.scope_slot {
            p.mono("Slot", slot.to_string());
        }
        p.text("Repeat Last Step", yes_no(definition.repeat_last_step));
    });
    let Some(document) = document else {
        return;
    };
    let saved_lanes = saved_progression_lanes(
        document,
        definition.scope,
        usize::from(definition.definition_index),
    );
    let target = progression_target(definition);
    look::subheading(ui, "Saved State");
    look::properties(ui, ("hash_progression_save_state", index), |p| {
        if let Some(rank) = rank {
            p.mono(
                "Current Rank",
                if definition.repeat_last_step || definition.steps.is_empty() {
                    rank.to_string()
                } else {
                    format!("{rank} of {}", definition.steps.len())
                },
            );
        }
        match saved_lanes {
            Some(lanes) => {
                p.mono(
                    "Progress",
                    target.map_or_else(
                        || lanes[0].to_string(),
                        |target| format!("{} / {target}", lanes[0]),
                    ),
                );
                p.mono("Lane 1", lanes[1].to_string());
                p.mono("Lane 2", lanes[2].to_string());
            }
            None => p.text("State", "Not Saved"),
        }
    });
}

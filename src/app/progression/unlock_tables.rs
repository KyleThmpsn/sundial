use super::*;
use super::{
    hierarchy::*, mutations::*, override_tables::sortable_table_header, state::*, table_ui::*,
};

#[derive(Clone, Copy)]
pub(super) struct FlagTableConfig {
    pub(super) id: &'static str,
    pub(super) bank: u8,
    pub(super) capacity: usize,
}

pub(super) struct TableDrawContext<'a> {
    pub(super) catalog: &'a Catalog,
    pub(super) query: &'a str,
    pub(super) state: &'a mut UiState,
    pub(super) document: &'a mut Value,
}

pub(super) fn draw_flag_runs(
    ui: &mut egui::Ui,
    config: FlagTableConfig,
    rows: &[FlagRun],
    catalog: &Catalog,
    query: &str,
    state: &mut UiState,
    document: &mut Value,
) -> bool {
    let slots = expanded_flag_slots(rows, config.capacity);
    draw_flag_slots(
        ui,
        config,
        &slots,
        Some(rows.len()),
        TableDrawContext {
            catalog,
            query,
            state,
            document,
        },
    )
}

pub(super) fn draw_flag_indices(
    ui: &mut egui::Ui,
    config: FlagTableConfig,
    rows: &[FlagIndex],
    catalog: &Catalog,
    query: &str,
    state: &mut UiState,
    document: &mut Value,
) -> bool {
    let mut present = vec![false; config.capacity];
    for row in rows {
        present[row.index] = true;
    }
    let slots = present
        .into_iter()
        .enumerate()
        .filter_map(|(slot, present)| present.then_some(slot))
        .collect::<Vec<_>>();
    draw_flag_slots(
        ui,
        config,
        &slots,
        None,
        TableDrawContext {
            catalog,
            query,
            state,
            document,
        },
    )
}

pub(super) fn draw_flag_slots(
    ui: &mut egui::Ui,
    config: FlagTableConfig,
    slots: &[usize],
    encoded_range_count: Option<usize>,
    context: TableDrawContext<'_>,
) -> bool {
    let TableDrawContext {
        catalog,
        query,
        state,
        document,
    } = context;
    let mut changed = false;
    let query = query.trim().to_lowercase();
    let mut filtered = slots
        .iter()
        .copied()
        .filter(|slot| flag_slot_matches(&query, *slot, config.bank, catalog))
        .collect::<Vec<_>>();
    let mapped = slots
        .iter()
        .filter(|slot| catalog.unlock_flag_for_state(config.bank, **slot).is_some())
        .count();
    let mut summary = vec![format!("{} flags", slots.len())];
    if mapped < slots.len() {
        summary.push(format!("{} unmapped", slots.len() - mapped));
    }
    let summary = ui
        .label(summary.join(" · "))
        .on_hover_text("Definition match: bank + compact slot");
    if let Some(count) = encoded_range_count {
        summary.on_hover_text(format!("Settings field: {count} flag runs"));
    }

    let index_width = 96.0;
    let hash_width = 104.0;
    let state_width = 64.0;
    let tested_by_width = (ui.available_width()
        - index_width
        - hash_width
        - state_width
        - TABLE_ACTION_WIDTH
        - TABLE_COLUMN_GAP * 4.0)
        .max(150.0);
    ui.add_space(4.0);
    let sort = sortable_table_header(
        ui,
        config.id,
        &[
            (index_width, "Index"),
            (hash_width, "Hash"),
            (tested_by_width, "Readers"),
            (state_width, "Slot"),
            (TABLE_ACTION_WIDTH, ""),
        ],
        TableSort::ascending(3),
        state,
    );
    ui.separator();
    if filtered.is_empty() {
        ui.label(egui::RichText::new("No matching rows").weak());
        return false;
    }

    match sort.column {
        2 => sort_by_optional_cached_key(&mut filtered, sort.descending, |slot| {
            catalog
                .unlock_flag_for_state(config.bank, *slot)
                .and_then(|(_, definition)| definition_context_sort_key(definition))
        }),
        _ => filtered
            .sort_by(|left, right| compare_flag_slots(*left, *right, config.bank, catalog, sort)),
    }
    let display_lines = definition_context_display_lines(
        filtered.len(),
        |row_index| catalog.unlock_flag_for_state(config.bank, filtered[row_index]),
        sort.column == 2 && sort.descending,
    );

    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = TABLE_ROW_GAP;
        egui::ScrollArea::vertical()
            .id_salt(("progression_table", config.id))
            .auto_shrink([false, false])
            .max_height(ui.available_height().max(TABLE_ROW_STRIDE * 3.0))
            .show_rows(ui, TABLE_CELL_HEIGHT, display_lines.len(), |ui, range| {
                egui::Grid::new((config.id, "rows"))
                    .num_columns(5)
                    .striped(true)
                    .spacing([TABLE_COLUMN_GAP, TABLE_ROW_GAP])
                    .show(ui, |ui| {
                        for line_index in range {
                            let line = &display_lines[line_index];
                            let slot = filtered[line.row_index];
                            if line.primary {
                                if let (Some(definition_index), Some(definition)) =
                                    (line.definition_index, line.definition)
                                {
                                    draw_definition_index_cell(
                                        ui,
                                        index_width,
                                        definition_index,
                                        Some(definition),
                                        MetadataSelection::FlagDefinition(definition_index),
                                        state,
                                    );
                                    draw_definition_hash_hex_cell(ui, hash_width, Some(definition));
                                } else {
                                    table_cell(ui, index_width, egui::RichText::new("-").weak())
                                        .on_hover_text("No package definition");
                                    table_cell(ui, hash_width, egui::RichText::new("-").weak());
                                }
                            } else {
                                table_cell(ui, index_width, "");
                                table_cell(ui, hash_width, "");
                            }
                            draw_context_cell(ui, tested_by_width, line.context.as_ref());
                            if line.primary {
                                table_cell(
                                    ui,
                                    state_width,
                                    egui::RichText::new(slot.to_string()).monospace(),
                                );
                                if draw_remove_cell(ui, TABLE_ACTION_WIDTH, "Remove state entry")
                                    .clicked()
                                    && set_unlock_flag(document, config.id, slot, false)
                                {
                                    changed = true;
                                }
                            } else {
                                table_cell(ui, state_width, "");
                                table_cell(ui, TABLE_ACTION_WIDTH, "");
                            }
                            ui.end_row();
                        }
                    });
            });
    });
    changed
}

pub(super) fn draw_objective_values(
    ui: &mut egui::Ui,
    id: &'static str,
    rows: &[IndexedValue],
    bank: u8,
    context: TableDrawContext<'_>,
) -> bool {
    let TableDrawContext {
        catalog,
        query,
        state,
        document,
    } = context;
    let mut changed = false;
    let query = query.trim().to_lowercase();
    let filtered = rows
        .iter()
        .filter(|row| objective_hierarchy_row_matches(&query, row, bank, catalog))
        .collect::<Vec<_>>();
    let objectives = rows
        .iter()
        .filter_map(|row| catalog.unlock_value_for_state(bank, row.index))
        .filter(|(definition_index, _)| {
            catalog
                .objective_for_unlock_value(*definition_index)
                .is_some()
        })
        .count();
    let mut summary = vec![format!("{} objective values", rows.len())];
    if objectives < rows.len() {
        summary.push(format!(
            "{} without a resolved objective",
            rows.len() - objectives
        ));
    }
    ui.label(summary.join(" · "))
        .on_hover_text("Definition: bank + compact slot\nHierarchy: package owner paths");
    let state_width = 72.0;
    let value_width = 76.0;
    let index_width = 118.0;
    let hash_width = 104.0;
    let objective_width = (ui.available_width()
        - index_width
        - hash_width
        - state_width
        - value_width
        - TABLE_ACTION_WIDTH
        - TABLE_COLUMN_GAP * 5.0)
        .max(150.0);
    ui.add_space(4.0);
    let sort = sortable_table_header(
        ui,
        id,
        &[
            (objective_width, "Objective"),
            (index_width, "Index"),
            (hash_width, "Hash"),
            (value_width, "Value"),
            (state_width, "Objective index"),
            (TABLE_ACTION_WIDTH, ""),
        ],
        TableSort::ascending(0),
        state,
    );
    ui.separator();
    if filtered.is_empty() {
        ui.label(egui::RichText::new("No matching rows").weak());
        return false;
    }

    let mut hierarchy = build_objective_hierarchy(&filtered, bank, catalog);
    sort_objective_hierarchy(&mut hierarchy, sort);
    let auto_expand = !query.is_empty();
    let display_lines = objective_matrix_lines(&hierarchy, id, state, auto_expand);

    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = TABLE_ROW_GAP;
        egui::ScrollArea::vertical()
            .id_salt(("progression_table", id))
            .auto_shrink([false, false])
            .max_height(ui.available_height().max(TABLE_ROW_STRIDE * 3.0))
            .show_rows(ui, TABLE_CELL_HEIGHT, display_lines.len(), |ui, range| {
                egui::Grid::new((id, "rows"))
                    .num_columns(6)
                    .striped(true)
                    .spacing([TABLE_COLUMN_GAP, TABLE_ROW_GAP])
                    .show(ui, |ui| {
                        for line_index in range {
                            match display_lines[line_index] {
                                ObjectiveMatrixLine::Branch {
                                    branch,
                                    depth,
                                    expanded,
                                } => {
                                    let response = draw_hierarchy_branch_cell(
                                        ui,
                                        objective_width,
                                        depth,
                                        &branch.label,
                                        expanded,
                                        !auto_expand,
                                    );
                                    let response = response.on_hover_text(branch.path.join(" > "));
                                    if !auto_expand && response.clicked() {
                                        state.objective_expansion.insert(
                                            ObjectiveBranchKey {
                                                table: id,
                                                path: branch.path.clone(),
                                            },
                                            !expanded,
                                        );
                                    }
                                    table_cell(ui, index_width, "");
                                    table_cell(ui, hash_width, "");
                                    table_cell(ui, value_width, "");
                                    table_cell(ui, state_width, "");
                                    table_cell(ui, TABLE_ACTION_WIDTH, "");
                                }
                                ObjectiveMatrixLine::Leaf { leaf, depth } => {
                                    if let Some(objective) = leaf.objective {
                                        let response = draw_hierarchy_leaf_cell(
                                            ui,
                                            objective_width,
                                            depth,
                                            resolved_objective_table_text(
                                                catalog,
                                                objective,
                                                leaf.definition,
                                            ),
                                        );
                                        if let Some(definition_index) = leaf.definition_index {
                                            if metadata_click(response, "Open objective definition")
                                                .on_hover_text(objective_details_tooltip(objective))
                                                .clicked()
                                            {
                                                state.metadata_inspector.open(
                                                    MetadataSelection::ValueDefinition(
                                                        definition_index,
                                                    ),
                                                );
                                            }
                                        } else {
                                            response.on_hover_text(objective_details_tooltip(
                                                objective,
                                            ));
                                        }
                                    } else {
                                        let response = draw_hierarchy_leaf_cell(
                                            ui,
                                            objective_width,
                                            depth,
                                            egui::RichText::new("-").weak(),
                                        );
                                        let response =
                                            response.on_hover_text(if leaf.definition.is_some() {
                                                "No same-hash objective"
                                            } else {
                                                "No package definition"
                                            });
                                        if let Some(definition_index) = leaf.definition_index
                                            && metadata_click(response, "Open objective definition")
                                                .clicked()
                                        {
                                            state.metadata_inspector.open(
                                                MetadataSelection::ValueDefinition(
                                                    definition_index,
                                                ),
                                            );
                                        }
                                    }
                                    let objective_index_text = leaf.objective_index.map_or_else(
                                        || egui::RichText::new("-").weak(),
                                        |objective_index| {
                                            egui::RichText::new(format!("#{objective_index}"))
                                                .monospace()
                                        },
                                    );
                                    let objective_index_response =
                                        if leaf.definition_index.is_some()
                                            && leaf.objective_index.is_some()
                                        {
                                            table_link(ui, index_width, objective_index_text)
                                                .on_hover_text("Open objective definition")
                                        } else {
                                            table_cell(ui, index_width, objective_index_text)
                                        };
                                    if let Some(definition_index) = leaf.definition_index
                                        && objective_index_response.clicked()
                                    {
                                        state.metadata_inspector.open(
                                            MetadataSelection::ValueDefinition(definition_index),
                                        );
                                    }
                                    draw_hash_hex_cell(
                                        ui,
                                        hash_width,
                                        leaf.objective.map(|objective| objective.hash),
                                    );
                                    let row = leaf.row;
                                    let mut value = row.value;
                                    let reserved = id == "character_object_objective_values"
                                        && RESERVED_CHARACTER_OBJECTIVE_VALUES
                                            .iter()
                                            .any(|(index, _)| *index == row.index);
                                    if reserved {
                                        table_cell(
                                            ui,
                                            value_width,
                                            egui::RichText::new(value.to_string()).monospace(),
                                        )
                                        .on_hover_text("Reserved runtime value");
                                    } else if table_drag_value(ui, value_width, &mut value)
                                        .changed()
                                        && set_unlock_value(document, id, row.index, value)
                                    {
                                        changed = true;
                                    }
                                    table_cell(
                                        ui,
                                        state_width,
                                        egui::RichText::new(row.index.to_string()).monospace(),
                                    );
                                    if reserved {
                                        table_cell(ui, TABLE_ACTION_WIDTH, "");
                                    } else if draw_remove_cell(
                                        ui,
                                        TABLE_ACTION_WIDTH,
                                        "Remove objective value",
                                    )
                                    .clicked()
                                        && remove_unlock_value(document, id, row.index)
                                    {
                                        changed = true;
                                    }
                                }
                            }
                            ui.end_row();
                        }
                    });
            });
    });
    changed
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_progression_values(
    ui: &mut egui::Ui,
    id: &'static str,
    rows: &[ProgressionValue],
    scope: ProgressionScope,
    catalog: &Catalog,
    query: &str,
    state: &mut UiState,
    document: &mut Value,
) -> bool {
    let query = query.trim().to_lowercase();
    let mut filtered = progression_display_rows(rows, catalog.progression_definitions(), scope)
        .into_iter()
        .filter(|row| {
            let definition = catalog.progression_definition(row.definition_index);
            query.is_empty()
                || row.definition_index.to_string().contains(&query)
                || definition
                    .is_some_and(|definition| progression_definition_matches(&query, definition))
                || row
                    .lanes
                    .into_iter()
                    .flatten()
                    .any(|lane| lane.to_string().contains(&query))
                || definition
                    .and_then(|definition| definition.scope_slot)
                    .is_some_and(|slot| slot.to_string().contains(&query))
        })
        .collect::<Vec<_>>();
    let definition_width = 72.0;
    let hash_width = 118.0;
    let name_width = 180.0;
    let slot_width = 72.0;
    let lane_width = 82.0;
    let target_width = 92.0;
    let sort = sortable_table_header(
        ui,
        id,
        &[
            (definition_width, "Index"),
            (hash_width, "Hash"),
            (name_width, "Name"),
            (slot_width, "Slot"),
            (lane_width, "Progress"),
            (target_width, "Target"),
            (lane_width, "Lane 1"),
            (lane_width, "Lane 2"),
            (TABLE_ACTION_WIDTH, ""),
        ],
        TableSort::ascending(0),
        state,
    );
    filtered.sort_by(|left, right| {
        let left_definition = catalog.progression_definition(left.definition_index);
        let right_definition = catalog.progression_definition(right.definition_index);
        let order = match sort.column {
            0 => left.definition_index.cmp(&right.definition_index),
            1 => {
                return compare_optional(
                    left_definition.map(|definition| definition.hash),
                    right_definition.map(|definition| definition.hash),
                    sort.descending,
                );
            }
            2 => {
                return compare_optional(
                    left_definition
                        .and_then(progression_display_name)
                        .map(|name| name.to_lowercase()),
                    right_definition
                        .and_then(progression_display_name)
                        .map(|name| name.to_lowercase()),
                    sort.descending,
                );
            }
            3 => {
                return compare_optional(
                    left_definition.and_then(|definition| definition.scope_slot),
                    right_definition.and_then(|definition| definition.scope_slot),
                    sort.descending,
                );
            }
            4 => {
                return compare_optional(
                    left.lanes.map(|lanes| lanes[0]),
                    right.lanes.map(|lanes| lanes[0]),
                    sort.descending,
                );
            }
            5 => {
                return compare_optional(
                    left_definition.and_then(progression_target),
                    right_definition.and_then(progression_target),
                    sort.descending,
                );
            }
            6..=7 => {
                return compare_optional(
                    left.lanes.map(|lanes| lanes[sort.column - 5]),
                    right.lanes.map(|lanes| lanes[sort.column - 5]),
                    sort.descending,
                );
            }
            _ => Ordering::Equal,
        };
        if sort.descending {
            order.reverse()
        } else {
            order
        }
    });
    ui.separator();
    if filtered.is_empty() {
        ui.label(egui::RichText::new("No matching rows").weak());
        return false;
    }
    let mut changed = false;
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = TABLE_ROW_GAP;
        egui::ScrollArea::vertical()
            .id_salt(("progression_table", id))
            .auto_shrink([false, false])
            .show_rows(ui, TABLE_CELL_HEIGHT, filtered.len(), |ui, range| {
                egui::Grid::new((id, "rows"))
                    .num_columns(9)
                    .striped(true)
                    .spacing([TABLE_COLUMN_GAP, TABLE_ROW_GAP])
                    .show(ui, |ui| {
                        for row_index in range {
                            let row = filtered[row_index];
                            let definition = catalog.progression_definition(row.definition_index);
                            let index = egui::RichText::new(format!("#{}", row.definition_index))
                                .monospace();
                            let index = if state.progression_changed(id, row.definition_index) {
                                index.color(ui.visuals().warn_fg_color)
                            } else {
                                index
                            };
                            table_cell(ui, definition_width, index);
                            draw_hash_hex_cell(
                                ui,
                                hash_width,
                                definition.map(|definition| definition.hash),
                            );
                            table_cell(
                                ui,
                                name_width,
                                definition.map_or_else(
                                    || egui::RichText::new("-").weak(),
                                    |definition| {
                                        progression_display_name(definition).map_or_else(
                                            || egui::RichText::new("-").weak(),
                                            egui::RichText::new,
                                        )
                                    },
                                ),
                            );
                            let scope_slot = definition
                                .filter(|definition| definition.scope == scope)
                                .and_then(|definition| definition.scope_slot);
                            table_cell(
                                ui,
                                slot_width,
                                egui::RichText::new(
                                    scope_slot.map_or_else(|| "-".into(), |slot| slot.to_string()),
                                )
                                .monospace(),
                            );
                            let target = definition.and_then(progression_target);
                            if let Some(previous_lanes) = row.lanes {
                                let mut lanes = previous_lanes;
                                let mut row_changed =
                                    table_drag_value(ui, lane_width, &mut lanes[0]).changed();
                                table_cell(
                                    ui,
                                    target_width,
                                    target.map_or_else(
                                        || egui::RichText::new("-").weak(),
                                        |target| {
                                            egui::RichText::new(target.to_string()).monospace()
                                        },
                                    ),
                                );
                                for lane in &mut lanes[1..] {
                                    if state.edit_progression_lanes {
                                        row_changed |=
                                            table_drag_value(ui, lane_width, lane).changed();
                                    } else {
                                        table_cell(
                                            ui,
                                            lane_width,
                                            egui::RichText::new(lane.to_string())
                                                .monospace()
                                                .weak(),
                                        );
                                    }
                                }
                                if row_changed
                                    && set_progression_value(
                                        document,
                                        id,
                                        row.definition_index,
                                        lanes,
                                    )
                                {
                                    state.record_progression_change(
                                        id,
                                        row.definition_index,
                                        Some(previous_lanes),
                                        Some(lanes),
                                    );
                                    changed = true;
                                }
                                if draw_remove_cell(ui, TABLE_ACTION_WIDTH, "Remove progression")
                                    .clicked()
                                    && remove_progression_value(document, id, row.definition_index)
                                {
                                    state.record_progression_change(
                                        id,
                                        row.definition_index,
                                        Some(previous_lanes),
                                        None,
                                    );
                                    changed = true;
                                }
                            } else {
                                let add = missing_progression_cell(ui, lane_width);
                                table_cell(
                                    ui,
                                    target_width,
                                    target.map_or_else(
                                        || egui::RichText::new("-").weak(),
                                        |target| {
                                            egui::RichText::new(target.to_string()).monospace()
                                        },
                                    ),
                                );
                                table_cell(ui, lane_width, egui::RichText::new("-").weak());
                                table_cell(ui, lane_width, egui::RichText::new("-").weak());
                                table_cell(ui, TABLE_ACTION_WIDTH, "");
                                if add
                                    && set_progression_value(
                                        document,
                                        id,
                                        row.definition_index,
                                        [0; 3],
                                    )
                                {
                                    state.record_progression_change(
                                        id,
                                        row.definition_index,
                                        None,
                                        Some([0; 3]),
                                    );
                                    changed = true;
                                }
                            }
                            ui.end_row();
                        }
                    });
            });
    });
    changed
}

pub(in crate::app) fn progression_target(definition: &ProgressionDefinition) -> Option<i32> {
    definition
        .steps
        .iter()
        .map(|step| step.progress_total)
        .max()
}

pub(in crate::app) fn saved_progression_lanes(
    document: &Value,
    scope: ProgressionScope,
    definition_index: usize,
) -> Option<[i32; 3]> {
    let key = match scope {
        ProgressionScope::Account => "account_progressions",
        ProgressionScope::Character => "character_progressions",
        ProgressionScope::Unreplicated => return None,
    };
    let rows = document
        .pointer(&format!("/state/unlocks/{key}"))?
        .as_array()?;
    let mut saved = None::<[i32; 3]>;
    for row in rows {
        let Some(values) = row.as_array() else {
            continue;
        };
        let [index, lane_0, lane_1, lane_2] = values.as_slice() else {
            continue;
        };
        if index.as_u64().and_then(|index| usize::try_from(index).ok()) != Some(definition_index) {
            continue;
        }
        let lanes = [lane_0, lane_1, lane_2]
            .map(|lane| lane.as_i64().and_then(|lane| i32::try_from(lane).ok()));
        let [Some(lane_0), Some(lane_1), Some(lane_2)] = lanes else {
            continue;
        };
        let lanes = [lane_0, lane_1, lane_2];
        if let Some(current) = saved.as_mut() {
            for lane in 0..3 {
                current[lane] = current[lane].max(lanes[lane]);
            }
        } else {
            saved = Some(lanes);
        }
    }
    saved
}

pub(super) fn missing_progression_cell(ui: &mut egui::Ui, width: f32) -> bool {
    ui.allocate_ui_with_layout(
        egui::vec2(width, TABLE_CELL_HEIGHT),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.label(egui::RichText::new("Missing").weak());
            ui.small_button("Add").clicked()
        },
    )
    .inner
}

pub(super) fn draw_unreplicated_progressions(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    query: &str,
    state: &mut UiState,
) {
    let query = query.trim().to_lowercase();
    let mut definitions = catalog
        .progression_definitions()
        .iter()
        .filter(|definition| {
            definition.scope == ProgressionScope::Unreplicated
                && (query.is_empty() || progression_definition_matches(&query, definition))
        })
        .collect::<Vec<_>>();
    let index_width = 72.0;
    let hash_width = 118.0;
    let name_width =
        (ui.available_width() - index_width - hash_width - TABLE_COLUMN_GAP * 2.0).max(180.0);
    let sort = sortable_table_header(
        ui,
        "unreplicated_progressions",
        &[
            (index_width, "Index"),
            (hash_width, "Hash"),
            (name_width, "Name"),
        ],
        TableSort::ascending(0),
        state,
    );
    definitions.sort_by(|left, right| {
        let order = match sort.column {
            0 => left.definition_index.cmp(&right.definition_index),
            1 => left.hash.cmp(&right.hash),
            2 => {
                return compare_optional(
                    progression_display_name(left).map(|name| name.to_lowercase()),
                    progression_display_name(right).map(|name| name.to_lowercase()),
                    sort.descending,
                );
            }
            _ => Ordering::Equal,
        };
        compare_ordering(order, sort.descending)
    });
    ui.separator();
    if definitions.is_empty() {
        ui.label(egui::RichText::new("No matching rows").weak());
        return;
    }
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = TABLE_ROW_GAP;
        egui::ScrollArea::vertical()
            .id_salt("unreplicated_progression_table")
            .auto_shrink([false, false])
            .show_rows(ui, TABLE_CELL_HEIGHT, definitions.len(), |ui, range| {
                egui::Grid::new("unreplicated_progression_rows")
                    .num_columns(3)
                    .striped(true)
                    .spacing([TABLE_COLUMN_GAP, TABLE_ROW_GAP])
                    .show(ui, |ui| {
                        for row in range {
                            let definition = definitions[row];
                            table_cell(
                                ui,
                                index_width,
                                egui::RichText::new(format!("#{}", definition.definition_index))
                                    .monospace(),
                            );
                            draw_hash_hex_cell(ui, hash_width, Some(definition.hash));
                            table_cell(
                                ui,
                                name_width,
                                progression_display_name(definition).map_or_else(
                                    || egui::RichText::new("-").weak(),
                                    egui::RichText::new,
                                ),
                            );
                            ui.end_row();
                        }
                    });
            });
    });
}

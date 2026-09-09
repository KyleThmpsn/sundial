use super::*;
use super::{hierarchy::*, mutations::*, state::*, table_ui::*};

pub(super) fn draw_flag_overrides(
    ui: &mut egui::Ui,
    rows: &[FlagOverride],
    catalog: &Catalog,
    query: &str,
    state: &mut UiState,
    document: &mut Value,
) -> bool {
    let mut changed = false;
    let query = query.trim().to_lowercase();
    let mut filtered = rows
        .iter()
        .filter(|row| {
            let definition = catalog.unlock_flag_definition(row.definition_index);
            family5_flag_matches(&query, row, catalog)
                && override_filter_matches(state.override_filter, definition)
        })
        .collect::<Vec<_>>();
    let mapped = rows
        .iter()
        .filter(|row| {
            catalog
                .unlock_flag_definition(row.definition_index)
                .is_some()
        })
        .count();
    let mut summary = vec![format!("{} flag overrides", rows.len())];
    if mapped < rows.len() {
        summary.push(format!("{} unmapped", rows.len() - mapped));
    }
    let unresolved_readers = rows
        .iter()
        .filter_map(|row| catalog.unlock_flag_definition(row.definition_index))
        .filter(|definition| definition.tested_by.is_empty())
        .count();
    let partially_decoded = rows
        .iter()
        .filter_map(|row| catalog.unlock_flag_definition(row.definition_index))
        .filter(|definition| definition_has_undecoded_opcodes(definition))
        .count();
    draw_override_coverage_summary(
        ui,
        &summary.join(" · "),
        unresolved_readers,
        partially_decoded,
        "Each row stores a package definition index and the logical state used by account-wide progression checks.",
    );

    let available_width = ui.available_width();
    let show_hash = available_width >= RESPONSIVE_HASH_COLUMN_BREAKPOINT;
    let index_width = 96.0;
    let hash_width = if show_hash { 104.0 } else { 0.0 };
    let value_width = 124.0;
    let action_width = TABLE_ACTION_WIDTH;
    let column_gaps = if show_hash { 4.0 } else { 3.0 };
    let meaning_width = (available_width
        - index_width
        - hash_width
        - value_width
        - action_width
        - TABLE_COLUMN_GAP * column_gaps)
        .max(96.0);
    ui.add_space(4.0);
    let mut columns = vec![(index_width, "Index")];
    if show_hash {
        columns.push((hash_width, "Hash"));
    }
    columns.extend([
        (value_width, "Logical flag value"),
        (meaning_width, "References"),
        (action_width, ""),
    ]);
    let sort = sortable_table_header(
        ui,
        if show_hash {
            "family5_flag_overrides"
        } else {
            "family5_flag_overrides_compact"
        },
        &columns,
        TableSort::ascending(0),
        state,
    );
    let sort = override_table_sort(sort, show_hash);
    ui.separator();
    if filtered.is_empty() {
        ui.label(egui::RichText::new("No matching rows").weak());
        return false;
    }
    filtered.sort_by(|left, right| compare_flag_overrides(left, right, catalog, sort));
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = TABLE_ROW_GAP;
        egui::ScrollArea::vertical()
            .id_salt(("progression_table", "family5_flag_overrides"))
            .auto_shrink([false, false])
            .max_height(ui.available_height().max(TABLE_ROW_STRIDE * 3.0))
            .show_rows(ui, TABLE_CELL_HEIGHT, filtered.len(), |ui, range| {
                egui::Grid::new("family5_flag_override_rows")
                    .num_columns(columns.len())
                    .striped(true)
                    .spacing([TABLE_COLUMN_GAP, TABLE_ROW_GAP])
                    .show(ui, |ui| {
                        for row_index in range {
                            let row = filtered[row_index];
                            let definition = catalog.unlock_flag_definition(row.definition_index);
                            draw_definition_index_cell(
                                ui,
                                index_width,
                                row.definition_index,
                                definition,
                                MetadataSelection::FlagOverride(row.definition_index, row.value),
                                state,
                            );
                            if show_hash {
                                draw_definition_hash_hex_cell(ui, hash_width, definition);
                            }
                            let mut value = row.value;
                            let prior_value = value;
                            ui.allocate_ui_with_layout(
                                egui::vec2(value_width, TABLE_CELL_HEIGHT),
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    ui.add_enabled_ui(!state.read_only, |ui| {
                                        egui::ComboBox::from_id_salt((
                                            "family5_flag_override_value",
                                            row.definition_index,
                                        ))
                                        .selected_text(flag_override_state_label(value))
                                        .width(value_width - 12.0)
                                        .show_ui(ui, |ui| {
                                            for candidate in 0..=FAMILY5_FLAG_VALUE_MAXIMUM {
                                                ui.selectable_value(
                                                    &mut value,
                                                    candidate,
                                                    flag_override_state_label(candidate),
                                                );
                                            }
                                        })
                                        .response
                                        .on_hover_text(flag_override_state_help())
                                    });
                                },
                            );
                            if !state.read_only
                                && value != prior_value
                                && set_investment_override(
                                    document,
                                    InvestmentTable::FlagOverrides,
                                    row.definition_index,
                                    i32::from(value),
                                )
                            {
                                state.last_investment_change = Some(InvestmentUndo::Flag {
                                    definition_index: row.definition_index,
                                    previous: Some(prior_value),
                                });
                                changed = true;
                            }
                            draw_override_meaning(
                                ui,
                                meaning_width,
                                definition,
                                MetadataSelection::FlagOverride(row.definition_index, row.value),
                                state,
                            );
                            if draw_remove_cell(
                                ui,
                                action_width,
                                "Remove flag override",
                                !state.read_only,
                            )
                            .clicked()
                                && remove_investment_override(
                                    document,
                                    InvestmentTable::FlagOverrides,
                                    row.definition_index,
                                )
                            {
                                state.last_investment_change = Some(InvestmentUndo::Flag {
                                    definition_index: row.definition_index,
                                    previous: Some(row.value),
                                });
                                changed = true;
                            }
                            ui.end_row();
                        }
                    });
            });
    });
    changed
}

pub(super) fn draw_value_overrides(
    ui: &mut egui::Ui,
    rows: &[ValueOverride],
    catalog: &Catalog,
    query: &str,
    state: &mut UiState,
    document: &mut Value,
) -> bool {
    let mut changed = false;
    let query = query.trim().to_lowercase();
    let mut filtered = rows
        .iter()
        .filter(|row| {
            let definition = catalog.unlock_value_definition(row.definition_index);
            family5_value_matches(&query, row, catalog)
                && override_filter_matches(state.override_filter, definition)
        })
        .collect::<Vec<_>>();
    let mapped = rows
        .iter()
        .filter(|row| {
            catalog
                .unlock_value_definition(row.definition_index)
                .is_some()
        })
        .count();
    let mut summary = vec![format!("{} value overrides", rows.len())];
    if mapped < rows.len() {
        summary.push(format!("{} unmapped", rows.len() - mapped));
    }
    let unresolved_readers = rows
        .iter()
        .filter_map(|row| catalog.unlock_value_definition(row.definition_index))
        .filter(|definition| definition.tested_by.is_empty())
        .count();
    let partially_decoded = rows
        .iter()
        .filter_map(|row| catalog.unlock_value_definition(row.definition_index))
        .filter(|definition| definition_has_undecoded_opcodes(definition))
        .count();
    draw_override_coverage_summary(
        ui,
        &summary.join(" · "),
        unresolved_readers,
        partially_decoded,
        "Each row stores a package definition index and the signed number used by account-wide progression checks.",
    );

    let available_width = ui.available_width();
    let show_hash = available_width >= RESPONSIVE_HASH_COLUMN_BREAKPOINT;
    let index_width = 96.0;
    let hash_width = if show_hash { 104.0 } else { 0.0 };
    let value_width = 110.0;
    let action_width = TABLE_ACTION_WIDTH;
    let column_gaps = if show_hash { 4.0 } else { 3.0 };
    let meaning_width = (available_width
        - index_width
        - hash_width
        - value_width
        - action_width
        - TABLE_COLUMN_GAP * column_gaps)
        .max(96.0);
    ui.add_space(4.0);
    let mut columns = vec![(index_width, "Index")];
    if show_hash {
        columns.push((hash_width, "Hash"));
    }
    columns.extend([
        (value_width, "Value"),
        (meaning_width, "References"),
        (action_width, ""),
    ]);
    let sort = sortable_table_header(
        ui,
        if show_hash {
            "family5_value_overrides"
        } else {
            "family5_value_overrides_compact"
        },
        &columns,
        TableSort::ascending(0),
        state,
    );
    let sort = override_table_sort(sort, show_hash);
    ui.separator();
    if filtered.is_empty() {
        ui.label(egui::RichText::new("No matching rows").weak());
        return false;
    }
    filtered.sort_by(|left, right| compare_value_overrides(left, right, catalog, sort));
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = TABLE_ROW_GAP;
        egui::ScrollArea::vertical()
            .id_salt(("progression_table", "family5_value_overrides"))
            .auto_shrink([false, false])
            .max_height(ui.available_height().max(TABLE_ROW_STRIDE * 3.0))
            .show_rows(ui, TABLE_CELL_HEIGHT, filtered.len(), |ui, range| {
                egui::Grid::new("family5_value_override_rows")
                    .num_columns(columns.len())
                    .striped(true)
                    .spacing([TABLE_COLUMN_GAP, TABLE_ROW_GAP])
                    .show(ui, |ui| {
                        for row_index in range {
                            let row = filtered[row_index];
                            let definition = catalog.unlock_value_definition(row.definition_index);
                            let selection =
                                MetadataSelection::ValueOverride(row.definition_index, row.value);
                            draw_definition_index_cell(
                                ui,
                                index_width,
                                row.definition_index,
                                definition,
                                selection,
                                state,
                            );
                            if show_hash {
                                draw_definition_hash_hex_cell(ui, hash_width, definition);
                            }
                            let mut value = row.value;
                            let prior_value = value;
                            if table_drag_value(ui, value_width, &mut value, !state.read_only)
                                .changed()
                                && set_investment_override(
                                    document,
                                    InvestmentTable::ValueOverrides,
                                    row.definition_index,
                                    value,
                                )
                            {
                                state.last_investment_change = Some(InvestmentUndo::Value {
                                    definition_index: row.definition_index,
                                    previous: Some(prior_value),
                                });
                                changed = true;
                            }
                            draw_override_meaning(ui, meaning_width, definition, selection, state);
                            if draw_remove_cell(
                                ui,
                                action_width,
                                "Remove value override",
                                !state.read_only,
                            )
                            .clicked()
                                && remove_investment_override(
                                    document,
                                    InvestmentTable::ValueOverrides,
                                    row.definition_index,
                                )
                            {
                                state.last_investment_change = Some(InvestmentUndo::Value {
                                    definition_index: row.definition_index,
                                    previous: Some(row.value),
                                });
                                changed = true;
                            }
                            ui.end_row();
                        }
                    });
            });
    });
    changed
}

pub(super) fn draw_override_meaning(
    ui: &mut egui::Ui,
    width: f32,
    definition: Option<&UnlockDefinition>,
    selection: MetadataSelection,
    state: &mut UiState,
) {
    let Some(definition) = definition else {
        table_cell(
            ui,
            width,
            egui::RichText::new("Not in package table").weak(),
        );
        return;
    };
    let meaning = override_meaning(definition);
    let text = if override_meaning_contexts(definition).is_empty()
        && definition_name(definition).is_none()
    {
        egui::RichText::new(meaning).weak().underline()
    } else {
        egui::RichText::new(meaning).underline()
    };
    let response = table_link(ui, width, text).on_hover_text(format!(
        "{} reference{}",
        definition.tested_by.len(),
        if definition.tested_by.len() == 1 {
            ""
        } else {
            "s"
        }
    ));
    if response.clicked() {
        state.metadata_inspector.open(selection);
    }
}

pub(super) fn draw_override_coverage_summary(
    ui: &mut egui::Ui,
    summary: &str,
    unresolved_readers: usize,
    partially_decoded: usize,
    tooltip: &str,
) {
    ui.horizontal_wrapped(|ui| {
        ui.label(summary).on_hover_text(tooltip);
        if unresolved_readers > 0 {
            ui.label(
                egui::RichText::new(format!("· {unresolved_readers} with no known references"))
                    .weak(),
            )
            .on_hover_text("No package reference was found for this definition. This does not prove it is unused.");
        }
        if partially_decoded > 0 {
            ui.label(
                egui::RichText::new(format!("· {partially_decoded} partially decoded")).weak(),
            )
            .on_hover_text("One or more condition programs contain undecoded opcodes.");
        }
    });
}

pub(super) fn sortable_table_header(
    ui: &mut egui::Ui,
    id: &'static str,
    columns: &[(f32, &str)],
    default: TableSort,
    state: &mut UiState,
) -> TableSort {
    let mut sort = state.table_sorts.get(id).copied().unwrap_or(default);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = TABLE_COLUMN_GAP;
        for (column, (width, label)) in columns.iter().enumerate() {
            if label.is_empty() {
                ui.allocate_space(egui::vec2(*width, TABLE_CELL_HEIGHT));
                continue;
            }
            let marker = if sort.column == column {
                if sort.descending {
                    Some(Glyph::ChevronDown)
                } else {
                    Some(Glyph::ChevronUp)
                }
            } else {
                None
            };
            let response = sortable_header_cell(ui, *width, label, marker).on_hover_text("Sort");
            if response.clicked() {
                if sort.column == column {
                    sort.descending = !sort.descending;
                } else {
                    sort = TableSort::ascending(column);
                }
            }
        }
    });
    state.table_sorts.insert(id, sort);
    sort
}

pub(super) const fn override_table_sort(sort: TableSort, show_hash: bool) -> TableSort {
    if show_hash {
        return sort;
    }
    TableSort {
        column: match sort.column {
            0 => 0,
            1 => 2,
            2 => 3,
            _ => usize::MAX,
        },
        descending: sort.descending,
    }
}

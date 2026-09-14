use super::*;

fn value_cell(ui: &mut egui::Ui, row: &Row, enabled: bool) -> Option<i32> {
    if !enabled {
        table_cell(ui, 90.0, row.value.to_string())
            .on_hover_text(row.blocked.unwrap_or("Editing is disabled"));
        return None;
    }
    if row.kind == Kind::FlagOverride {
        let mut value = row.value as i32;
        ui.allocate_ui_with_layout(
            egui::vec2(90.0, TABLE_CELL_HEIGHT),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_min_size(egui::vec2(90.0, TABLE_CELL_HEIGHT));
                egui::ComboBox::from_id_salt(("stored_flag", row.key))
                    .width(78.0)
                    .selected_text(match value {
                        0 => "Clear",
                        1 => "Logical 1",
                        _ => "Set",
                    })
                    .show_ui(ui, |ui| {
                        for (value_to_set, label) in [(0, "Clear"), (1, "Logical 1"), (2, "Set")] {
                            ui.selectable_value(&mut value, value_to_set, label);
                        }
                    })
                    .response
                    .on_hover_text(flag_override_state_help());
            },
        );
        return (i64::from(value) != row.value).then_some(value);
    }
    if row.kind == Kind::Unlock {
        let mut set = row.value == 2;
        let changed = ui
            .allocate_ui_with_layout(
                egui::vec2(90.0, TABLE_CELL_HEIGHT),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_min_size(egui::vec2(90.0, TABLE_CELL_HEIGHT));
                    ui.checkbox(&mut set, if row.value == 2 { "Set" } else { "Clear" })
                        .changed()
                },
            )
            .inner;
        return changed.then_some(if set { 2 } else { 0 });
    }
    let mut value = row.value as i32;
    table_drag_value(ui, 90.0, &mut value, true)
        .changed()
        .then_some(value)
}

fn navigation(ui: &mut egui::Ui, cache: &mut State, state: &mut UiState, mode: Mode) {
    ui.add(
        egui::TextEdit::singleline(&mut state.query)
            .hint_text("Search Saved Values…")
            .desired_width(230.0),
    );
    if mode == Mode::All {
        egui::ComboBox::from_id_salt("stored_kind")
            .selected_text(cache.kind.map_or("All Types", Kind::label))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut cache.kind, None, "All Types");
                for kind in Kind::ALL {
                    ui.selectable_value(&mut cache.kind, Some(kind), kind.label());
                }
            });
        egui::ComboBox::from_id_salt("stored_scope")
            .selected_text(cache.scope.unwrap_or("All Scopes"))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut cache.scope, None, "All Scopes");
                for scope in [
                    "Account",
                    "Profile",
                    "Character",
                    "Character Object",
                    "Unknown",
                ] {
                    ui.selectable_value(&mut cache.scope, Some(scope), scope);
                }
            });
        ui.add_enabled(
            !state.read_only,
            egui::Checkbox::new(&mut cache.edit_extra, "Edit Extra Rank Values"),
        )
        .on_hover_text("The meanings of rank values 1 and 2 are not decoded");
    }
}

fn row_details(ui: &mut egui::Ui, rows: &[Row], cache: &mut State) {
    if let Some(row) = cache
        .selected
        .and_then(|key| rows.iter().find(|row| row.key == key))
    {
        ui.scope(|ui| {
            ui.horizontal(|ui| {
                ui.strong(&row.name);
                if ui.small_button("Close Details").clicked() {
                    cache.selected = None;
                }
            });
            ui.label(format!(
                "{} · {} · {}",
                row.scope,
                row.kind.label(),
                row.location
            ));
            ui.monospace(format!(
                "Bank {} · Slot {} · Lane {} · Value {}",
                row.key.bank, row.key.slot, row.key.lane, row.value
            ));
            if let Some(field) = field(row.key) {
                ui.monospace(field);
            }
            if let Some(reason) = row.blocked {
                ui.label(reason);
            }
        });
    }
}

fn table(
    ui: &mut egui::Ui,
    document: &mut Value,
    rows: &[Row],
    filtered: &[usize],
    cache: &mut State,
    state: &mut UiState,
    layout: (f32, bool),
) -> bool {
    let (name_width, compact) = layout;
    let mut changed = false;
    ui.spacing_mut().item_spacing.y = TABLE_ROW_GAP;
    egui::ScrollArea::vertical()
        .id_salt("stored_rows")
        .auto_shrink([false, false])
        .show_rows(ui, TABLE_CELL_HEIGHT, filtered.len(), |ui, range| {
            egui::Grid::new("stored_grid")
                .num_columns(if compact { 5 } else { 6 })
                .striped(true)
                .spacing([TABLE_COLUMN_GAP, TABLE_ROW_GAP])
                .show(ui, |ui| {
                    for offset in range {
                        let row = &rows[filtered[offset]];

                        let open = table_link(ui, name_width, destiny_text(ui, &row.name))
                            .on_hover_text(format!(
                                "{}\n{}\n{}",
                                row.name,
                                row.location,
                                field(row.key).unwrap_or("Unknown Saved Bank")
                            ))
                            .clicked();
                        table_cell(ui, 120.0, row.kind.label());
                        table_cell(ui, 88.0, row.scope);
                        let enabled = !state.read_only
                            && row.blocked.is_none()
                            && (row.kind != Kind::RankData || cache.edit_extra);
                        if let Some(value) =
                            ui.push_id(row.key, |ui| value_cell(ui, row, enabled)).inner
                        {
                            if edits::apply(document, row, Some(value), state) {
                                changed = true;
                            } else {
                                cache.feedback = Some(format!("{} could not be changed", row.name));
                            }
                        }
                        if !compact {
                            table_cell(ui, 118.0, &row.location)
                                .on_hover_text(field(row.key).unwrap_or("Unknown Saved Bank"));
                        }
                        if open {
                            if let Some((index, value)) = row.definition {
                                state
                                    .metadata_inspector
                                    .open(match (row.key.family, value) {
                                        (true, true) => MetadataSelection::ValueOverride(
                                            index,
                                            row.value as i32,
                                        ),
                                        (true, false) => {
                                            MetadataSelection::FlagOverride(index, row.value as u8)
                                        }
                                        (false, true) => MetadataSelection::ValueDefinition(index),
                                        (false, false) => MetadataSelection::FlagDefinition(index),
                                    });
                            } else if let Some(hash) = row.hash {
                                crate::app::inspector::request_definition(ui.ctx(), hash);
                            } else {
                                cache.selected = Some(row.key);
                            }
                        }
                        if draw_remove_cell(
                            ui,
                            24.0,
                            if matches!(row.kind, Kind::RankProgress | Kind::RankData) {
                                "Reset This Value"
                            } else {
                                "Remove Saved Entry"
                            },
                            enabled,
                        )
                        .clicked()
                        {
                            changed |= edits::apply(document, row, None, state);
                        }
                        ui.end_row();
                    }
                });
        });
    changed
}

pub(in crate::app::progression) fn draw(
    ui: &mut egui::Ui,
    document: &mut Value,
    catalog: &Catalog,
    state: &mut UiState,
    mode: Mode,
) -> bool {
    let mut cache = std::mem::take(&mut state.storage);
    match prepare(ui, document, catalog, &mut cache, mode) {
        Ok(true) => {}
        Ok(false) => {
            state.storage = cache;
            return false;
        }
        Err(error) => {
            ui.colored_label(ui.visuals().error_fg_color, error);
            state.storage = cache;
            return false;
        }
    }
    let rows = cache.rows.take().unwrap_or_default();
    progression_toolbar(ui, |ui| {
        navigation(ui, &mut cache, state, mode);
        let sort = state
            .table_sorts
            .get("saved_values")
            .copied()
            .unwrap_or(TableSort::ascending(0));
        let filter = Filter {
            query: state.query.trim().to_lowercase(),
            kind: if mode == Mode::All { cache.kind } else { None },
            scope: if mode == Mode::All { cache.scope } else { None },
            mode,
            sort,
            coverage: state.override_filter,
        };
        if cache
            .filtered
            .as_ref()
            .is_none_or(|(previous, _)| previous != &filter)
        {
            let filtered = filtered_rows(&rows, &filter, catalog);
            cache.filtered = Some((filter, filtered));
        }
        ui.weak(format!(
            "{} / {} saved values",
            cache
                .filtered
                .as_ref()
                .expect("filtered saved values")
                .1
                .len(),
            rows.len()
        ));
    });
    row_details(ui, &rows, &mut cache);
    if let Some(feedback) = &cache.feedback {
        ui.colored_label(ui.visuals().error_fg_color, feedback);
    }
    let compact = ui.available_width() < 800.0;
    let other_width = if compact {
        322.0 + TABLE_COLUMN_GAP * 4.0
    } else {
        440.0 + TABLE_COLUMN_GAP * 5.0
    };
    let name_width = (ui.available_width() - other_width).max(150.0);
    let mut changed = false;
    egui::ScrollArea::horizontal()
        .id_salt("stored_columns")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.set_min_width(name_width + other_width);
            let mut columns = vec![
                (name_width, "Entry"),
                (120.0, "Type"),
                (88.0, "Scope"),
                (90.0, "Value"),
            ];
            if !compact {
                columns.push((118.0, "Stored Field"));
            }
            columns.push((24.0, ""));
            super::super::table_ui::sortable_table_header(
                ui,
                "saved_values",
                &columns,
                TableSort::ascending(0),
                state,
            );
            let (filter, filtered) = cache.filtered.take().expect("filtered saved values");
            ui.separator();
            changed = table(
                ui,
                document,
                &rows,
                &filtered,
                &mut cache,
                state,
                (name_width, compact),
            );
            cache.filtered = Some((filter, filtered));
        });
    cache.rows = Some(rows);
    if changed {
        cache.invalidate();
    }
    state.storage = cache;
    changed
}

fn prepare(
    ui: &mut egui::Ui,
    document: &Value,
    catalog: &Catalog,
    cache: &mut State,
    mode: Mode,
) -> Result<bool, String> {
    if cache.mode != Some(mode) {
        cache.invalidate();
        cache.mode = Some(mode);
    }
    if cache.rows.is_some() {
        return Ok(true);
    }
    if cache.preparing.is_none() {
        cache.preparing = Some(Preparing::new(document, catalog, mode)?);
    }
    let preparing = cache.preparing.as_mut().expect("saved values preparation");
    if !preparing.step(catalog) {
        let (done, total) = preparing.progress();
        ui.add(
            egui::ProgressBar::new(done as f32 / total as f32)
                .text(format!("Preparing Saved Values {done} / {total}")),
        );
        ui.ctx().request_repaint();
        return Ok(false);
    }
    cache.rows = Some(
        cache
            .preparing
            .take()
            .expect("prepared saved values")
            .finish(),
    );
    Ok(true)
}

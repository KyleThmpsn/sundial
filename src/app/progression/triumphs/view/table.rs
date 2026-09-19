use super::*;

fn select_cell(ui: &mut egui::Ui, selected: &mut bool, partial: bool) -> bool {
    ui.allocate_ui_with_layout(
        egui::vec2(20.0, TABLE_CELL_HEIGHT),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_size(egui::vec2(20.0, TABLE_CELL_HEIGHT));
            ui.add(egui::Checkbox::without_text(selected).indeterminate(partial))
                .on_hover_text("Select Triumph")
                .changed()
        },
    )
    .inner
}

struct Table<'a> {
    rows: &'a [Row],
    root: &'a Branch,
    state: &'a mut State,
    catalog: &'a Catalog,
    name_width: f32,
    auto_expand: bool,
    enabled: bool,
    navigation_changed: bool,
    single: Option<(RecordDefinition, bool)>,
}

impl Table<'_> {
    fn branch(&mut self, ui: &mut egui::Ui, path: &[String], depth: usize) {
        let branch = self.root.at(path);
        let selected_count = branch
            .members
            .iter()
            .filter(|index| {
                self.state
                    .selected
                    .contains(&self.rows[**index].record.hash)
            })
            .count();
        let mut selected = selected_count == branch.members.len();
        let expanded = self.auto_expand || self.state.expansion.contains(path);
        let response = hierarchy_selection_cell(ui, self.name_width, depth, |ui, width| {
            if select_cell(
                ui,
                &mut selected,
                selected_count > 0 && selected_count < branch.members.len(),
            ) {
                for index in &branch.members {
                    if selected {
                        self.state.selected.insert(self.rows[*index].record.hash);
                    } else {
                        self.state.selected.remove(&self.rows[*index].record.hash);
                    }
                }
            }
            draw_hierarchy_branch_cell(
                ui,
                width,
                0,
                path.last().map_or("Triumphs", String::as_str),
                expanded,
                !self.auto_expand,
            )
        });
        if response.clicked() && !self.auto_expand {
            if expanded {
                self.state.expansion.remove(path);
            } else {
                self.state.expansion.insert(path.to_vec());
            }
            self.navigation_changed = true;
        }
        table_cell(
            ui,
            88.0,
            format!("{} / {}", branch.completed, branch.members.len()),
        )
        .on_hover_text("Completed Triumphs in this group");
        table_cell(ui, 132.0, "");
        table_cell(ui, 132.0, "");
    }

    fn record(&mut self, ui: &mut egui::Ui, row: usize, depth: usize) {
        let row = &self.rows[row];
        let mut selected = self.state.selected.contains(&row.record.hash);
        let expanded = self.state.expanded_records.contains(&row.record.hash);
        let response = hierarchy_selection_cell(ui, self.name_width, depth, |ui, width| {
            if select_cell(ui, &mut selected, false) {
                if selected {
                    self.state.selected.insert(row.record.hash);
                } else {
                    self.state.selected.remove(&row.record.hash);
                }
            }
            draw_hierarchy_branch_cell(
                ui,
                width,
                0,
                &row.name,
                expanded,
                !row.record.objectives.is_empty(),
            )
        });
        if response.clicked() {
            if expanded {
                self.state.expanded_records.remove(&row.record.hash);
            } else {
                self.state.expanded_records.insert(row.record.hash);
            }
            self.navigation_changed = true;
        }
        table_cell(
            ui,
            88.0,
            format!(
                "{} / {}",
                row.completed_objectives,
                row.record.objectives.len()
            ),
        );
        table_cell(ui, 132.0, row.status.label());
        ui.allocate_ui_with_layout(
            egui::vec2(132.0, TABLE_CELL_HEIGHT),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_min_size(egui::vec2(132.0, TABLE_CELL_HEIGHT));
                if ui
                    .add_enabled(
                        self.enabled,
                        egui::Button::new(if row.status == Status::Completed {
                            "Reset"
                        } else {
                            "Complete"
                        })
                        .small(),
                    )
                    .on_hover_ui(|ui| {
                        ui.label(if row.status == Status::Completed { "Resets progress and subtracts this Triumph's score. Previously queued or delivered rewards remain. Undo restores the whole completion edit." } else { "Updates progress, completion, and Triumph score. Item rewards go to Pending Rewards for the selected character." });
                        if let Some(runtime) = &row.record.runtime {
                            let score = if row.record.completion_flag.is_some() {u64::from(runtime.score)} else {runtime.interval_scores.iter().map(|score|u64::from(*score)).sum()};
                            ui.label(format!("{score} Triumph Points"));
                            let rewards = runtime.rewards.iter().copied().chain(runtime.interval_items.iter().flatten().map(|item|(*item,1)));
                            for (index, quantity) in rewards {
                                let name = self.catalog.item_hash_for_index(index).and_then(|hash|self.catalog.inventory_definition(hash)).map(|item|item.name);
                                ui.label(format!("{quantity} × {}",name.filter(|name| !name.trim().is_empty()).map_or_else(||format!("Reward Item #{index}"),str::to_owned)));
                            }
                        }
                    })
                    .clicked()
                {
                    self.single = Some((row.record.clone(), row.status != Status::Completed));
                }
                if ui.small_button("Details").clicked() {
                    crate::app::inspector::request_definition(ui.ctx(), row.record.hash);
                }
            },
        );
    }

    fn objective(&self, ui: &mut egui::Ui, row: usize, objective: usize, depth: usize) {
        let row = &self.rows[row];
        if let Some(definition) = self
            .catalog
            .objective_definition(row.record.objectives[objective])
        {
            draw_hierarchy_leaf_cell(
                ui,
                self.name_width,
                depth,
                objective_table_text(definition, None),
            );
            table_cell(
                ui,
                88.0,
                row.values[objective].map_or_else(
                    || format!("? / {}", definition.completion_value),
                    |value| format!("{value} / {}", definition.completion_value),
                ),
            );
            table_cell(
                ui,
                132.0,
                row.values[objective].map_or("Unresolved", |value| {
                    if objective_complete(definition, value) {
                        "Complete"
                    } else {
                        "Incomplete"
                    }
                }),
            );
            if table_link(ui, 132.0, "Details").clicked() {
                crate::app::inspector::request_definition(ui.ctx(), definition.hash);
            }
        } else {
            table_cell(ui, self.name_width, "Objective Unavailable");
            table_cell(ui, 88.0, "");
            table_cell(ui, 132.0, "Unresolved");
            table_cell(ui, 132.0, "");
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw(
    ui: &mut egui::Ui,
    rows: &[Row],
    root: &Branch,
    visible: &[Line],
    state: &mut State,
    catalog: &Catalog,
    name_width: f32,
    auto_expand: bool,
    enabled: bool,
) -> (bool, Option<(RecordDefinition, bool)>) {
    let mut table = Table {
        rows,
        root,
        state,
        catalog,
        name_width,
        auto_expand,
        enabled,
        navigation_changed: false,
        single: None,
    };
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = TABLE_ROW_GAP;
        egui::ScrollArea::vertical()
            .id_salt("triumphs_rows")
            .auto_shrink([false, false])
            .show_rows(ui, TABLE_CELL_HEIGHT, visible.len(), |ui, range| {
                egui::Grid::new("triumphs_grid")
                    .num_columns(4)
                    .striped(true)
                    .spacing([TABLE_COLUMN_GAP, TABLE_ROW_GAP])
                    .show(ui, |ui| {
                        for offset in range {
                            match &visible[offset] {
                                Line::Branch { path, depth } => table.branch(ui, path, *depth),
                                Line::Record { row, depth } => table.record(ui, *row, *depth),
                                Line::Objective {
                                    row,
                                    objective,
                                    depth,
                                } => table.objective(ui, *row, *objective, *depth),
                            }
                            ui.end_row();
                        }
                    });
            });
    });
    (table.navigation_changed, table.single)
}

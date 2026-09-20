use super::*;
mod table;

fn finish(job: edit::Job, document: &mut Value, catalog: &Catalog, state: &mut State) -> bool {
    let queued = job.queued_count();
    let direct = job.direct_count();
    match job.finish(document, catalog) {
        Ok(count) => {
            state.feedback = Some((
                false,
                if direct > 0 {
                    format!(
                        "{count} Triumphs updated · {queued} pending rewards · {direct} consumable grants"
                    )
                } else if queued > 0 {
                    format!("{count} Triumphs updated · {queued} pending rewards added")
                } else {
                    format!("{count} Triumphs updated")
                },
            ));
            state.invalidate();
            count > 0
        }
        Err(error) => {
            state.feedback = Some((true, error));
            false
        }
    }
}

fn jobs(
    ui: &mut egui::Ui,
    document: &mut Value,
    catalog: &Catalog,
    state: &mut State,
    read_only: bool,
) -> bool {
    if state.job.is_none() && state.ready.is_none() {
        return false;
    }
    let (changed, close) = crate::app::ui::edit_modal(ui, "triumph_edit", |ui| {
        draw_job(ui, document, catalog, state, read_only)
    });
    if close {
        state.job = None;
        state.ready = None;
    }
    changed
}

fn draw_issues(ui: &mut egui::Ui, issues: &[edit::Issue]) {
    for issue in issues {
        ui.label(destiny_text(ui, &issue.name).strong());
        ui.label(destiny_text(ui, &issue.reason).weak());
        ui.add_space(6.0);
    }
}

fn draw_job(
    ui: &mut egui::Ui,
    document: &mut Value,
    catalog: &Catalog,
    state: &mut State,
    read_only: bool,
) -> bool {
    if read_only {
        state.job = None;
        state.ready = None;
    }
    if let Some(mut job) = state.job.take() {
        let (done, total) = job.progress();
        let cancel = crate::app::ui::modal_progress(ui, "Updating Triumphs", done, total);
        if !cancel {
            if job.step(catalog) {
                if job.issues.is_empty()
                    && job.conflicts.is_empty()
                    && job.direct_count() == 0
                    && job
                        .review
                        .as_ref()
                        .is_some_and(|review| review.related.is_empty())
                {
                    return finish(job, document, catalog, state);
                }
                state.ready = Some(job);
            } else {
                state.job = Some(job);
                ui.ctx().request_repaint();
            }
        }
    }
    if let Some(job) = &state.ready {
        let skipped = job.issues.len() + job.conflicts.len();
        let changing = job.changing_count();
        let settled = job.supported_count().saturating_sub(changing);
        let mut counts = vec![(false, format!("{changing} to Change"))];
        if settled > 0 {
            counts.push((true, format!("{settled} Already Set")));
        }
        if skipped > 0 {
            counts.push((true, format!("{skipped} Skipped")));
        }
        if job.queued_count() > 0 {
            counts.push((true, format!("{} Rewards Queued", job.queued_count())));
        }
        crate::app::ui::review_header(ui, "Review Changes", &counts);
        crate::app::ui::review_body(ui, "triumph_review_body", |ui| {
            if !job.conflicts.is_empty() {
                ui.label("Conflicting Triumphs will be skipped. Their rewards are excluded.");
                egui::CollapsingHeader::new(format!("{} Conflicts", job.conflicts.len()))
                    .id_salt("triumph_review_conflicts")
                    .show(ui, |ui| draw_issues(ui, &job.conflicts));
            }
            job.draw_consumables(ui, catalog);
            if !job.issues.is_empty() {
                egui::CollapsingHeader::new(format!("{} Skipped Triumphs", job.issues.len()))
                    .id_salt("triumph_review_skipped")
                    .show(ui, |ui| draw_issues(ui, &job.issues));
            }
            if let Some(review) = &job.review {
                review.draw(ui);
            }
        });
        let apply_label = if !job.conflicts.is_empty() {
            "Apply & Skip Conflicts"
        } else if job.direct_count() > 0 {
            "Apply Anyway"
        } else {
            "Apply Changes"
        };
        let (apply, cancel) = crate::app::ui::review_actions(
            ui,
            apply_label,
            changing > 0,
            "Every supported Triumph already has this state.",
        );
        if apply {
            return finish(
                state.ready.take().expect("ready edit"),
                document,
                catalog,
                state,
            );
        }
        if cancel {
            state.ready = None;
        }
    }
    false
}

fn navigation(ui: &mut egui::Ui, state: &mut State) -> bool {
    ui.add(
        egui::TextEdit::singleline(&mut state.query)
            .hint_text("Search Triumphs…")
            .desired_width((ui.available_width() * 0.25).clamp(150.0, 230.0)),
    );
    egui::ComboBox::from_id_salt("triumph_status")
        .selected_text(state.filter.map_or("All States", Status::label))
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut state.filter, None, "All States");
            for status in Status::ALL {
                ui.selectable_value(&mut state.filter, Some(status), status.label());
            }
        });
    let expand_all = ui.button("Expand All").clicked();
    if ui.button("Collapse All").clicked() {
        state.expansion.clear();
        state.expanded_records.clear();
        state.visible = None;
    }
    expand_all
}

fn header(ui: &mut egui::Ui, state: &mut State, rows: &[Row], name_width: f32) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = TABLE_COLUMN_GAP;
        for (index, width, label) in [
            (0, name_width, "Triumph"),
            (1, 88.0, "Progress"),
            (2, 132.0, "State"),
        ] {
            let marker = (state.sort.column == index).then_some(if state.sort.descending {
                Glyph::ChevronDown
            } else {
                Glyph::ChevronUp
            });
            let response = if index == 0 {
                hierarchy_selection_cell(ui, width, 0, |ui, width| {
                    let filtered = &state.filtered.as_ref().expect("filtered Triumphs").1;
                    let count = filtered
                        .iter()
                        .filter(|index| state.selected.contains(&rows[**index].record.hash))
                        .count();
                    let mut all = !filtered.is_empty() && count == filtered.len();
                    let partial = count > 0 && !all;
                    let toggle = ui
                        .allocate_ui_with_layout(
                            egui::vec2(20.0, TABLE_CELL_HEIGHT),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.add_enabled(
                                    !filtered.is_empty(),
                                    egui::Checkbox::without_text(&mut all).indeterminate(partial),
                                )
                                .on_hover_text("Select or clear all filtered Triumphs")
                            },
                        )
                        .inner;
                    ui.ctx().accesskit_node_builder(toggle.id, |node| {
                        node.set_label("Select Filtered");
                    });
                    if toggle.changed() {
                        state.selected.clear();
                        if all {
                            state
                                .selected
                                .extend(filtered.iter().map(|index| rows[*index].record.hash));
                        }
                    }
                    sortable_header_cell(ui, width, label, marker)
                })
            } else {
                sortable_header_cell(ui, width, label, marker)
            };
            if response.clicked() {
                state.sort = super::super::state::TableSort {
                    column: index,
                    descending: state.sort.column == index && !state.sort.descending,
                };
                ui.ctx().request_repaint();
            }
        }
        table_cell(ui, 132.0, "Actions");
    });
}

fn selection(ui: &mut egui::Ui, state: &mut State, enabled: bool) -> Option<bool> {
    let mut desired = None;
    if state.selected.is_empty() {
        return None;
    }
    ui.label(format!("{} selected", state.selected.len()));
    if ui
        .add_enabled(enabled, egui::Button::new("Complete Selected"))
        .clicked()
    {
        desired = Some(true);
    }
    if ui
        .add_enabled(enabled, egui::Button::new("Reset Selected"))
        .clicked()
    {
        desired = Some(false);
    }
    desired
}

pub(in crate::app::progression) fn draw(
    ui: &mut egui::Ui,
    document: &mut Value,
    catalog: &Catalog,
    state: &mut State,
    read_only: bool,
) -> bool {
    if jobs(ui, document, catalog, state, read_only) {
        return true;
    }
    if catalog.records().is_none() {
        ui.label("Triumph records are unavailable. Rescan the catalog to load them.");
        return false;
    }
    if state.rows.is_none() {
        let Some(snapshot) = collection_state_snapshot(document) else {
            ui.label("Progression state unavailable");
            return false;
        };
        state.rows = Some(rows(catalog, &snapshot));
    }
    let rows = state.rows.take().unwrap_or_default();
    let name_width = (ui.available_width() - 352.0 - TABLE_COLUMN_GAP * 3.0).max(140.0);
    let enabled = !read_only && state.job.is_none() && state.ready.is_none();
    let (expand_all, desired) = progression_toolbar(ui, |ui| {
        let expand_all = navigation(ui, state);
        let filter = Filter {
            query: state.query.trim().to_lowercase(),
            status: state.filter,
            sort: state.sort,
        };
        if state
            .filtered
            .as_ref()
            .is_none_or(|(old, _)| old != &filter)
        {
            let filtered = filter_rows(&rows, &filter);
            let included = filtered
                .iter()
                .map(|index| rows[*index].record.hash)
                .collect::<HashSet<_>>();
            state.selected.retain(|hash| included.contains(hash));
            state.completed = filtered
                .iter()
                .filter(|index| rows[**index].status == Status::Completed)
                .count();
            state.tree = Some(tree(&rows, &filtered));
            state.filtered = Some((filter, filtered));
            state.visible = None;
        }
        ui.weak(format!(
            "{} / {}",
            state.completed,
            state.filtered.as_ref().expect("filtered Triumphs").1.len()
        ))
        .on_hover_text("Completed Triumphs in the current filter");
        let desired = selection(ui, state, enabled);
        (expand_all, desired)
    });
    if let Some((error, feedback)) = &state.feedback {
        ui.colored_label(
            if *error {
                ui.visuals().error_fg_color
            } else {
                ui.visuals().weak_text_color()
            },
            feedback,
        );
    }
    header(ui, state, &rows, name_width);
    ui.separator();
    let root = state.tree.take().unwrap_or_default();
    if !state.navigation_initialized {
        state
            .expansion
            .extend(root.children.keys().map(|name| vec![name.clone()]));
        state.navigation_initialized = true;
        state.visible = None;
    }
    if expand_all {
        for line in lines(&root, &rows, state, true) {
            if let Line::Branch { path, .. } = line {
                state.expansion.insert(path);
            }
        }
        state.visible = None;
    }
    let auto_expand = !state.query.trim().is_empty();
    let visible = state
        .visible
        .take()
        .unwrap_or_else(|| lines(&root, &rows, state, auto_expand));
    let (navigation_changed, single) = table::draw(
        ui,
        &rows,
        &root,
        &visible,
        state,
        catalog,
        name_width,
        auto_expand,
        enabled,
        document["_native_progression"]["runtime"] != "dawn",
    );
    if let Some(complete) = desired {
        state.job = Some(edit::Job::new(
            document,
            rows.iter()
                .filter(|row| state.selected.contains(&row.record.hash))
                .map(|row| row.record.clone())
                .collect(),
            complete,
        ));
    } else if let Some((record, complete)) = single {
        state.job = Some(edit::Job::new(document, vec![record], complete));
    }
    if state.job.is_some() || navigation_changed {
        ui.ctx().request_repaint();
    }
    if !navigation_changed {
        state.visible = Some(visible);
    }
    state.tree = Some(root);
    state.rows = Some(rows);
    false
}

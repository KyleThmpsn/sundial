use super::*;

#[derive(Debug)]
struct Row {
    saved: ProgressionDisplayRow,
    name: String,
    rank: Option<i32>,
    seasonal: bool,
}
#[derive(Debug)]
pub(super) struct State {
    scope: ProgressionScope,
    rows: Option<Vec<Row>>,
    filtered: Option<(String, TableSort, Vec<usize>)>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            scope: ProgressionScope::Account,
            rows: None,
            filtered: None,
        }
    }
}
impl State {
    pub fn invalidate(&mut self) {
        self.rows = None;
        self.filtered = None;
    }
}

fn rows(
    document: &Value,
    policy: &UnlockPolicy,
    catalog: &Catalog,
    scope: ProgressionScope,
) -> Vec<Row> {
    let authored = match scope {
        ProgressionScope::Account => policy.account_progressions.as_slice(),
        ProgressionScope::Character => policy.character_progressions.as_slice(),
        ProgressionScope::Unreplicated => &[],
    };
    let snapshot = collection_state_snapshot(document);
    super::super::hierarchy::progression_display_rows(
        authored,
        catalog.progression_definitions(),
        scope,
    )
    .into_iter()
    .map(|saved| {
        let definition = catalog
            .progression_definition(saved.definition_index)
            .filter(|definition| definition.scope == scope);
        let name = definition
            .and_then(super::super::progression_display_name)
            .unwrap_or_else(|| format!("Unnamed Rank #{}", saved.definition_index));
        let rank =
            definition.and_then(|definition| snapshot.as_ref()?.progression_rank(definition));
        let seasonal = scope == ProgressionScope::Account
            && crate::persistence::progression::supports_seasonal_authoring(document)
            && (38..=41).contains(&saved.definition_index);
        Row {
            saved,
            name,
            rank,
            seasonal,
        }
    })
    .collect()
}

pub(super) fn draw(
    ui: &mut egui::Ui,
    document: &mut Value,
    policy: &UnlockPolicy,
    catalog: &Catalog,
    state: &mut UiState,
    cache: &mut State,
) -> bool {
    let previous_scope = cache.scope;
    let mut undo = false;
    progression_toolbar(ui, |ui| {
        for (scope, label) in [
            (ProgressionScope::Account, "Account"),
            (ProgressionScope::Character, "Character"),
            (ProgressionScope::Unreplicated, "Other"),
        ] {
            ui.selectable_value(&mut cache.scope, scope, label);
        }
        super::super::page::draw_filter(ui, &mut state.query);
        if state.last_progression_change.is_some() {
            undo = ui
                .add_enabled(
                    !state.read_only && state.last_progression_change.is_some(),
                    egui::Button::new("Undo Rank Change"),
                )
                .clicked();
        }
        if previous_scope != cache.scope {
            cache.invalidate();
        }
        let sort = state
            .table_sorts
            .get("content_ranks")
            .copied()
            .unwrap_or(TableSort::ascending(0));
        let rows = cache
            .rows
            .get_or_insert_with(|| rows(document, policy, catalog, cache.scope));
        let query = state.query.trim().to_lowercase();
        if cache
            .filtered
            .as_ref()
            .is_none_or(|(old_query, old_sort, _)| old_query != &query || *old_sort != sort)
        {
            let mut filtered = rows
                .iter()
                .enumerate()
                .filter(|(_, row)| {
                    query.is_empty()
                        || row.name.to_lowercase().contains(&query)
                        || row.saved.definition_index.to_string().contains(&query)
                        || catalog
                            .progression_definition(row.saved.definition_index)
                            .is_some_and(|definition| {
                                super::super::hierarchy::progression_definition_matches(
                                    &query, definition,
                                )
                            })
                })
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            use super::super::sorting::by_key;
            match sort.column {
                1 => by_key(&mut filtered, sort.descending, |index| {
                    rows[*index].saved.lanes.map(|lanes| lanes[0])
                }),
                2 => by_key(&mut filtered, sort.descending, |index| rows[*index].rank),
                _ => by_key(&mut filtered, sort.descending, |index| {
                    Some(rows[*index].name.to_lowercase())
                }),
            }
            cache.filtered = Some((query, sort, filtered));
        }
        let filtered = &cache.filtered.as_ref().expect("filtered ranks").2;
        ui.weak(format!("{} / {} ranks", filtered.len(), rows.len()));
    });
    if undo {
        return super::super::mutations::undo_progression_change(document, state);
    }
    let id = if cache.scope == ProgressionScope::Account {
        "account_progressions"
    } else {
        "character_progressions"
    };
    let editable = !state.read_only && cache.scope != ProgressionScope::Unreplicated;
    let name_width = (ui.available_width() - 246.0 - TABLE_COLUMN_GAP * 3.0).max(160.0);
    sortable_table_header(
        ui,
        "content_ranks",
        &[
            (name_width, "Progression"),
            (100.0, "Progress"),
            (70.0, "Rank"),
            (76.0, "Details"),
        ],
        TableSort::ascending(0),
        state,
    );
    let rows = cache.rows.as_ref().expect("rank rows");
    let filtered = &cache.filtered.as_ref().expect("filtered ranks").2;
    ui.separator();
    let mut changed = false;
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = TABLE_ROW_GAP;
        egui::ScrollArea::vertical()
            .id_salt(("ranks", id, editable))
            .auto_shrink([false, false])
            .show_rows(ui, TABLE_CELL_HEIGHT, filtered.len(), |ui, range| {
                egui::Grid::new("rank_rows")
                    .num_columns(4)
                    .striped(true)
                    .spacing([TABLE_COLUMN_GAP, TABLE_ROW_GAP])
                    .show(ui, |ui| {
                        for offset in range {
                            let row = &rows[filtered[offset]];

                            let open =
                                table_link(ui, name_width, destiny_text(ui, &row.name)).clicked();
                            let previous = row.saved.lanes;
                            let mut lanes = previous.unwrap_or([0; 3]);
                            let edited = if row.seasonal {
                                table_cell(ui, 100.0, "Seasonal").on_hover_text("Managed by Seasonal where supported. These linked fields cannot be edited individually.");
                                false
                            } else if cache.scope == ProgressionScope::Unreplicated {
                                table_cell(ui, 100.0, "Not Saved");
                                false
                            } else {
                                ui.push_id(row.saved.definition_index, |ui| {
                                    table_drag_value(ui, 100.0, &mut lanes[0], editable)
                                })
                                .inner
                                .changed()
                            };
                            if edited
                                && super::super::mutations::set_progression_value(
                                    document,
                                    id,
                                    row.saved.definition_index,
                                    lanes,
                                )
                                .changed()
                            {
                                state.record_progression_change(
                                    id,
                                    row.saved.definition_index,
                                    previous,
                                    Some(lanes),
                                );
                                changed = true;
                            }
                            table_cell(
                                ui,
                                70.0,
                                row.rank
                                    .map_or_else(|| "Unknown".into(), |rank| rank.to_string()),
                            );
                            let details = table_link(ui, 76.0, "Details").clicked();
                            if (open || details)
                                && let Some(definition) =
                                    catalog.progression_definition(row.saved.definition_index)
                            {
                                crate::app::inspector::request_definition(
                                    ui.ctx(),
                                    definition.hash,
                                );
                            }
                            ui.end_row();
                        }
                    });
            });
    });
    changed
}

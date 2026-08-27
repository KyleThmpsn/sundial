use super::*;
use super::{add_dialogs::*, mutations::*, override_tables::*, state::*, unlock_tables::*};

pub(in crate::app) fn draw_content(
    ui: &mut egui::Ui,
    document: &mut Value,
    catalog: &Catalog,
    destiny_symbol_font_error: Option<&str>,
    state: &mut UiState,
    view: View,
) -> bool {
    if let Some(error) = catalog.progression_package_error() {
        ui.colored_label(ui.visuals().warn_fg_color, "Package scan incomplete")
            .on_hover_text(error);
        ui.add_space(4.0);
    }

    if let Some(error) = destiny_symbol_font_error {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            "Destiny symbol fonts unavailable",
        )
        .on_hover_text(error);
        ui.add_space(4.0);
    }

    let cached = state
        .cached_progression
        .take()
        .unwrap_or_else(|| parse(document));
    let policy = match cached {
        Ok(policy) => policy,
        Err(error) => {
            ui.colored_label(ui.visuals().error_fg_color, "Invalid progression settings");
            ui.label(&error);
            state.cached_progression = Some(Err(error));
            return false;
        }
    };

    let hash_inspector_open = state.hash_inspection.is_open();
    let inspector_full_width = draw_progression_metadata_workspace(
        ui,
        catalog,
        document,
        &mut state.metadata_inspector,
        hash_inspector_open,
    );

    if let Some(selection) = state.metadata_inspector.take_reveal_request() {
        reveal_metadata_selection(view, selection, catalog, state);
    }

    let mut changed = if inspector_full_width {
        false
    } else {
        match view {
            View::Unlocks => draw_unlocks(ui, document, &policy.unlocks, catalog, state),
            View::Investment => draw_investment(ui, document, &policy.investment, catalog, state),
        }
    };
    if let Some(hash) = take_hash_inspection_request(ui.ctx()) {
        let context = take_hash_inspection_context(ui.ctx(), hash);
        state.hash_inspection.open_with_context(hash, context);
    }
    changed |= draw_catalog_hash_window(
        ui.ctx(),
        catalog,
        Some(document),
        true,
        &mut state.hash_inspection,
        "progression",
    );
    if !changed {
        state.cached_progression = Some(Ok(policy));
    }
    changed
}

fn reveal_metadata_selection(
    view: View,
    selection: MetadataSelection,
    catalog: &Catalog,
    state: &mut UiState,
) {
    let index = selection.definition_index();
    state.query = catalog
        .unlock_value_definition(index)
        .filter(|_| selection.is_value())
        .or_else(|| {
            catalog
                .unlock_flag_definition(index)
                .filter(|_| !selection.is_value())
        })
        .map_or_else(
            || index.to_string(),
            |definition| format_hash_hex(definition.hash),
        );
    match view {
        View::Investment => {
            state.investment_table = if selection.is_value() {
                InvestmentTable::ValueOverrides
            } else {
                InvestmentTable::FlagOverrides
            };
        }
        View::Unlocks => {
            let definition = if selection.is_value() {
                catalog.unlock_value_definition(index)
            } else {
                catalog.unlock_flag_definition(index)
            };
            state.unlock_table = match (selection.is_value(), definition.map(|value| value.bank()))
            {
                (false, Some(ACCOUNT_FLAG_BANK)) => UnlockTable::AccountFlagRuns,
                (false, Some(PROFILE_FLAG_BANK)) => UnlockTable::ProfileFlagRuns,
                (false, Some(CHARACTER_OBJECT_FLAG_BANK)) => UnlockTable::CharacterObjectFlagRuns,
                (false, Some(CHARACTER_FLAG_BANK)) | (false, _) => UnlockTable::CharacterFlags,
                (true, Some(CHARACTER_OBJECTIVE_BANK)) => {
                    UnlockTable::CharacterObjectObjectiveValues
                }
                (true, _) => UnlockTable::ObjectiveValues,
            };
        }
    }
}

pub(super) fn draw_unlocks(
    ui: &mut egui::Ui,
    document: &mut Value,
    unlocks: &UnlockPolicy,
    catalog: &Catalog,
    state: &mut UiState,
) -> bool {
    let mut table_changed = false;
    let mut undo_progression_requested = false;
    progression_toolbar(ui, |ui| {
        ui.label(egui::RichText::new("Table").strong());
        let table_picker = egui::ComboBox::from_id_salt("progression_unlock_table")
            .selected_text(state.unlock_table.label())
            .width(220.0)
            .show_ui(ui, |ui| {
                for table in UnlockTable::ALL {
                    table_changed |= ui
                        .selectable_value(&mut state.unlock_table, table, table.label())
                        .changed();
                }
            });
        if let Some(field_name) = state.unlock_table.field_name() {
            table_picker
                .response
                .on_hover_text(format!("Settings field: {field_name}"));
        }
        ui.add_space(8.0);
        draw_filter(ui, &mut state.query);
        if state.unlock_table != UnlockTable::UnreplicatedProgressions
            && !state.unlock_table.is_progression()
            && ui.button("+ Add").clicked()
        {
            state.add_open = true;
            state.add_query.clear();
            state.add_value = 0;
            state.add_progression_lanes = [0; 3];
        }
        if state.unlock_table.is_progression() {
            ui.checkbox(&mut state.edit_progression_lanes, "Edit Lane 1–2")
                .on_hover_text("Lane 1 and Lane 2 meanings are not decoded from package data");
            if let Some(last_change) = state.last_progression_change
                && ui
                    .button("Undo progression change")
                    .on_hover_text(last_change.label())
                    .clicked()
            {
                undo_progression_requested = true;
            }
            if !state.progression_baselines.is_empty() {
                ui.label(
                    egui::RichText::new(format!("{} changed", state.progression_baselines.len()))
                        .color(ui.visuals().warn_fg_color),
                );
            }
        }
    });
    if table_changed {
        state.query.clear();
        state.add_open = false;
        state.edit_progression_lanes = false;
    }
    let query = state.query.clone();

    let mut changed = match state.unlock_table {
        UnlockTable::AccountFlagRuns => draw_flag_runs(
            ui,
            FlagTableConfig {
                id: "account_flag_runs",
                bank: ACCOUNT_FLAG_BANK,
                capacity: ACCOUNT_FLAG_CAPACITY,
            },
            &unlocks.account_flag_runs,
            catalog,
            &query,
            state,
            document,
        ),
        UnlockTable::ProfileFlagRuns => draw_flag_runs(
            ui,
            FlagTableConfig {
                id: "profile_flag_runs",
                bank: PROFILE_FLAG_BANK,
                capacity: PROFILE_FLAG_CAPACITY,
            },
            &unlocks.profile_flag_runs,
            catalog,
            &query,
            state,
            document,
        ),
        UnlockTable::CharacterFlags => draw_flag_indices(
            ui,
            FlagTableConfig {
                id: "character_flags",
                bank: CHARACTER_FLAG_BANK,
                capacity: CHARACTER_FLAG_CAPACITY,
            },
            &unlocks.character_flags,
            catalog,
            &query,
            state,
            document,
        ),
        UnlockTable::ObjectiveValues => draw_objective_values(
            ui,
            "objective_values",
            &unlocks.objective_values,
            ACCOUNT_OBJECTIVE_BANK,
            TableDrawContext {
                catalog,
                query: &query,
                state,
                document,
            },
        ),
        UnlockTable::CharacterObjectFlagRuns => draw_flag_runs(
            ui,
            FlagTableConfig {
                id: "character_object_flag_runs",
                bank: CHARACTER_OBJECT_FLAG_BANK,
                capacity: CHARACTER_OBJECT_FLAG_CAPACITY,
            },
            &unlocks.character_object_flag_runs,
            catalog,
            &query,
            state,
            document,
        ),
        UnlockTable::CharacterObjectObjectiveValues => draw_objective_values(
            ui,
            "character_object_objective_values",
            &unlocks.character_objective_values,
            CHARACTER_OBJECTIVE_BANK,
            TableDrawContext {
                catalog,
                query: &query,
                state,
                document,
            },
        ),
        UnlockTable::AccountProgressions => draw_progression_values(
            ui,
            "account_progressions",
            &unlocks.account_progressions,
            ProgressionScope::Account,
            catalog,
            &query,
            state,
            document,
        ),
        UnlockTable::CharacterProgressions => draw_progression_values(
            ui,
            "character_progressions",
            &unlocks.character_progressions,
            ProgressionScope::Character,
            catalog,
            &query,
            state,
            document,
        ),
        UnlockTable::UnreplicatedProgressions => {
            draw_unreplicated_progressions(ui, catalog, &query, state);
            false
        }
    };
    changed |= undo_progression_requested && undo_progression_change(document, state);
    changed |= draw_add_unlock_window(ui.ctx(), document, unlocks, catalog, state);
    changed
}

pub(super) fn draw_investment(
    ui: &mut egui::Ui,
    document: &mut Value,
    investment: &InvestmentPolicy,
    catalog: &Catalog,
    state: &mut UiState,
) -> bool {
    let mut table_changed = false;
    let mut undo_requested = false;
    let row_count = match state.investment_table {
        InvestmentTable::FlagOverrides => investment.flag_overrides.len(),
        InvestmentTable::ValueOverrides => investment.value_overrides.len(),
    };
    let can_add = row_count < FAMILY5_OVERRIDE_CAPACITY;
    progression_toolbar(ui, |ui| {
        ui.label(egui::RichText::new("Table").strong());
        let table_picker = egui::ComboBox::from_id_salt("progression_investment_table")
            .selected_text(state.investment_table.label())
            .width(220.0)
            .show_ui(ui, |ui| {
                for table in InvestmentTable::ALL {
                    table_changed |= ui
                        .selectable_value(&mut state.investment_table, table, table.label())
                        .changed();
                }
            });
        table_picker.response.on_hover_text(format!(
            "Settings field: {}",
            state.investment_table.field_name()
        ));
        let add = ui.add_enabled(can_add, egui::Button::new("Add override"));
        let add = if can_add {
            add
        } else {
            add.on_disabled_hover_text("100-row settings limit")
        };
        if add.clicked() {
            state.add_open = true;
            state.add_query.clear();
            state.add_value = 1;
        }
        ui.add_space(8.0);
        draw_filter(ui, &mut state.query);
        egui::ComboBox::from_id_salt("progression_override_coverage")
            .selected_text(state.override_filter.label())
            .width(170.0)
            .show_ui(ui, |ui| {
                for filter in OverrideFilter::ALL {
                    ui.selectable_value(&mut state.override_filter, filter, filter.label());
                }
            });
        if let Some(last_change) = state.last_investment_change {
            if ui
                .button("Undo last override change")
                .on_hover_text(last_change.label())
                .clicked()
            {
                undo_requested = true;
            }
        }
    });
    if table_changed {
        state.query.clear();
        state.add_open = false;
    }
    if !can_add {
        state.add_open = false;
    }
    ui.add_space(4.0);
    ui.label(state.investment_table.explanation());
    ui.add_space(4.0);
    let query = state.query.clone();

    let mut changed = undo_requested && undo_investment_change(document, state);
    changed |= match state.investment_table {
        InvestmentTable::FlagOverrides => draw_flag_overrides(
            ui,
            &investment.flag_overrides,
            catalog,
            &query,
            state,
            document,
        ),
        InvestmentTable::ValueOverrides => draw_value_overrides(
            ui,
            &investment.value_overrides,
            catalog,
            &query,
            state,
            document,
        ),
    };
    changed |= draw_add_investment_window(ui.ctx(), document, investment, catalog, state);
    changed
}

pub(super) fn draw_filter(ui: &mut egui::Ui, query: &mut String) {
    ui.add(
        egui::TextEdit::singleline(query)
            .hint_text("Filter rows…")
            .desired_width(300.0),
    );
}

use super::*;
use super::{add_dialogs::*, mutations::*, state::*};

pub(in crate::app) fn draw_content(
    ui: &mut egui::Ui,
    document: &mut Value,
    catalog: &Catalog,
    destiny_symbol_font_error: Option<&str>,
    state: &mut UiState,
    view: View,
) -> bool {
    if state.read_only {
        state.add_open = false;
        state.storage.edit_extra = false;
    }
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
            View::Unlocks => super::unlocks::draw(ui, document, &policy.unlocks, catalog, state),
            View::Triumphs => {
                super::triumphs::draw(ui, document, catalog, &mut state.triumphs, state.read_only)
            }
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
        !state.read_only,
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
        View::Triumphs => {}
        View::Investment => {
            state.investment_table = if selection.is_value() {
                InvestmentTable::ValueOverrides
            } else {
                InvestmentTable::FlagOverrides
            };
        }
        View::Unlocks => {
            state.unlock_browser.reveal(selection.is_value());
        }
    }
}

pub(super) fn draw_storage(
    ui: &mut egui::Ui,
    document: &mut Value,
    _unlocks: &UnlockPolicy,
    catalog: &Catalog,
    state: &mut UiState,
) -> bool {
    super::storage::draw(ui, document, catalog, state, super::storage::Mode::All)
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
    let hidden_count = super::native::hidden_count(document, state.investment_table);
    let can_add = !state.read_only && row_count + hidden_count < FAMILY5_OVERRIDE_CAPACITY;
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
        let add = ui.add_enabled(can_add, egui::Button::new("Add Override"));
        let add = if can_add {
            add
        } else {
            add.on_disabled_hover_text(if state.read_only {
                "Enable Progression Editing in Preferences to change state"
            } else {
                "100-row native limit, including preserved rows"
            })
        };
        if add.clicked() {
            state.add_open = true;
            state.add_query.clear();
            state.add_value = 1;
        }
        ui.add_space(8.0);
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
                .add_enabled(
                    !state.read_only,
                    egui::Button::new("Undo Last Override Change"),
                )
                .on_hover_text(last_change.label())
                .clicked()
            {
                undo_requested = true;
            }
        }
        let capacity = format!("{} / 100 Overrides", row_count + hidden_count);
        ui.weak(if hidden_count == 0 {
            capacity
        } else {
            format!("{capacity} · {hidden_count} Preserved Native Rows")
        });
    });
    if table_changed {
        state.query.clear();
        state.add_open = false;
    }
    if !can_add {
        state.add_open = false;
    }
    let mut changed = undo_requested && undo_investment_change(document, state);
    changed |= super::storage::draw(
        ui,
        document,
        catalog,
        state,
        super::storage::Mode::Overrides(state.investment_table),
    );
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

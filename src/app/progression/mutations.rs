//! Application undo policy over shared progression operations.
use super::state::*;
use super::*;
pub(in crate::app) use crate::persistence::progression::mutations::*;

pub(super) fn undo_investment_change(document: &mut Value, state: &mut UiState) -> bool {
    let Some(change) = state.last_investment_change.take() else {
        return false;
    };
    let changed = match change {
        InvestmentUndo::Flag {
            definition_index,
            previous: Some(value),
        } => set_investment_override(
            document,
            InvestmentTable::FlagOverrides,
            definition_index,
            i32::from(value),
        )
        .changed(),
        InvestmentUndo::Flag {
            definition_index,
            previous: None,
        } => remove_investment_override(document, InvestmentTable::FlagOverrides, definition_index)
            .changed(),
        InvestmentUndo::Value {
            definition_index,
            previous: Some(value),
        } => set_investment_override(
            document,
            InvestmentTable::ValueOverrides,
            definition_index,
            value,
        )
        .changed(),
        InvestmentUndo::Value {
            definition_index,
            previous: None,
        } => {
            remove_investment_override(document, InvestmentTable::ValueOverrides, definition_index)
                .changed()
        }
    };
    if !changed {
        state.last_investment_change = Some(change);
    }
    changed
}

pub(super) fn undo_progression_change(document: &mut Value, state: &mut UiState) -> bool {
    let Some(change) = state.last_progression_change else {
        return false;
    };
    let changed = match change.previous {
        Some(lanes) => {
            set_progression_value(document, change.table, change.definition_index, lanes).changed()
        }
        None => remove_progression_value(document, change.table, change.definition_index).changed(),
    };
    if changed {
        state.record_progression_change(
            change.table,
            change.definition_index,
            change.previous,
            change.previous,
        );
        state.last_progression_change = None;
    }
    changed
}

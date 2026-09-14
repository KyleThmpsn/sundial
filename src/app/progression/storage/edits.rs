use super::super::mutations::*;
use super::*;

pub(super) fn apply(
    document: &mut Value,
    row: &Row,
    requested: Option<i32>,
    state: &mut UiState,
) -> bool {
    if row.blocked.is_some() || state.read_only {
        return false;
    }
    let Some(field) = field(row.key) else {
        return false;
    };
    let slot = row.key.slot;
    if row.key.family {
        let table = if row.key.bank == 0 {
            InvestmentTable::FlagOverrides
        } else {
            InvestmentTable::ValueOverrides
        };
        let changed = if let Some(value) = requested {
            set_investment_override(document, table, slot, value)
        } else {
            remove_investment_override(document, table, slot)
        };
        if changed {
            state.last_investment_change = Some(if row.key.bank == 0 {
                InvestmentUndo::Flag {
                    definition_index: slot,
                    previous: u8::try_from(row.value).ok(),
                }
            } else {
                InvestmentUndo::Value {
                    definition_index: slot,
                    previous: i32::try_from(row.value).ok(),
                }
            });
        }
        return changed;
    }
    match row.kind {
        Kind::Unlock => set_unlock_flag(document, field, slot, requested == Some(2)),
        Kind::Counter => {
            if let Some(value) = requested {
                set_unlock_value(document, field, slot, value)
            } else {
                remove_unlock_value(document, field, slot)
            }
        }
        Kind::RankProgress | Kind::RankData => {
            let scope = if row.key.bank == 6 {
                ProgressionScope::Account
            } else {
                ProgressionScope::Character
            };
            let previous = saved_progression_lanes(document, scope, slot);
            let mut lanes = previous.unwrap_or([0; 3]);
            let Ok(lane) = usize::try_from(row.key.lane) else {
                return false;
            };
            let Some(value) = lanes.get_mut(lane) else {
                return false;
            };
            *value = requested.unwrap_or(0);
            let changed = set_progression_value(document, field, slot, lanes);
            if changed {
                state.record_progression_change(field, slot, previous, Some(lanes));
            }
            changed
        }
        _ => false,
    }
}

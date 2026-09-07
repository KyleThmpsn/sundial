use super::state::*;
use super::*;

pub(super) fn unlocks_object_mut(document: &mut Value) -> Option<&mut Map<String, Value>> {
    let root = document.as_object_mut()?;
    let state = root
        .entry("state")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()?;
    state
        .entry("unlocks")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
}

pub(super) fn write_unlock_array(document: &mut Value, key: &str, rows: Vec<Value>) -> bool {
    let Some(unlocks) = unlocks_object_mut(document) else {
        return false;
    };
    unlocks.insert(key.to_owned(), Value::Array(rows));
    true
}

pub(super) fn investment_object_mut(document: &mut Value) -> Option<&mut Map<String, Value>> {
    let root = document.as_object_mut()?;
    let state = root
        .entry("state")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()?;
    state
        .entry("investment")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
}

pub(super) fn write_investment_array(document: &mut Value, key: &str, rows: Vec<Value>) -> bool {
    let Some(investment) = investment_object_mut(document) else {
        return false;
    };
    investment.insert(key.to_owned(), Value::Array(rows));
    true
}

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
        ),
        InvestmentUndo::Flag {
            definition_index,
            previous: None,
        } => remove_investment_override(document, InvestmentTable::FlagOverrides, definition_index),
        InvestmentUndo::Value {
            definition_index,
            previous: Some(value),
        } => set_investment_override(
            document,
            InvestmentTable::ValueOverrides,
            definition_index,
            value,
        ),
        InvestmentUndo::Value {
            definition_index,
            previous: None,
        } => {
            remove_investment_override(document, InvestmentTable::ValueOverrides, definition_index)
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
            set_progression_value(document, change.table, change.definition_index, lanes)
        }
        None => remove_progression_value(document, change.table, change.definition_index),
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

pub(super) fn set_investment_override(
    document: &mut Value,
    table: InvestmentTable,
    definition_index: usize,
    value: i32,
) -> bool {
    let Ok(policy) = parse_investment(document.pointer("/state/investment")) else {
        return false;
    };
    match table {
        InvestmentTable::FlagOverrides => {
            if definition_index > FAMILY5_FLAG_SLOT_MAXIMUM
                || !(0..=i32::from(FAMILY5_FLAG_VALUE_MAXIMUM)).contains(&value)
            {
                return false;
            }
            let mut rows = policy.flag_overrides;
            if let Some(row) = rows
                .iter_mut()
                .rev()
                .find(|row| row.definition_index == definition_index)
            {
                if i32::from(row.value) == value {
                    return false;
                }
                row.value = value as u8;
            } else {
                if rows.len() >= FAMILY5_OVERRIDE_CAPACITY {
                    return false;
                }
                rows.push(FlagOverride {
                    definition_index,
                    value: value as u8,
                });
            }
            rows.sort_by_key(|row| row.definition_index);
            write_investment_array(
                document,
                "family5_flag_overrides",
                rows.into_iter()
                    .map(|row| serde_json::json!([row.definition_index, row.value]))
                    .collect(),
            )
        }
        InvestmentTable::ValueOverrides => {
            if definition_index > FAMILY5_VALUE_SLOT_MAXIMUM {
                return false;
            }
            let mut rows = policy.value_overrides;
            if let Some(row) = rows
                .iter_mut()
                .rev()
                .find(|row| row.definition_index == definition_index)
            {
                if row.value == value {
                    return false;
                }
                row.value = value;
            } else {
                if rows.len() >= FAMILY5_OVERRIDE_CAPACITY {
                    return false;
                }
                rows.push(ValueOverride {
                    definition_index,
                    value,
                });
            }
            rows.sort_by_key(|row| row.definition_index);
            write_investment_array(
                document,
                "family5_value_overrides",
                rows.into_iter()
                    .map(|row| serde_json::json!([row.definition_index, row.value]))
                    .collect(),
            )
        }
    }
}

pub(super) fn remove_investment_override(
    document: &mut Value,
    table: InvestmentTable,
    definition_index: usize,
) -> bool {
    let Ok(policy) = parse_investment(document.pointer("/state/investment")) else {
        return false;
    };
    match table {
        InvestmentTable::FlagOverrides => {
            let mut rows = policy.flag_overrides;
            let prior_len = rows.len();
            rows.retain(|row| row.definition_index != definition_index);
            if rows.len() == prior_len {
                return false;
            }
            write_investment_array(
                document,
                "family5_flag_overrides",
                rows.into_iter()
                    .map(|row| serde_json::json!([row.definition_index, row.value]))
                    .collect(),
            )
        }
        InvestmentTable::ValueOverrides => {
            let mut rows = policy.value_overrides;
            let prior_len = rows.len();
            rows.retain(|row| row.definition_index != definition_index);
            if rows.len() == prior_len {
                return false;
            }
            write_investment_array(
                document,
                "family5_value_overrides",
                rows.into_iter()
                    .map(|row| serde_json::json!([row.definition_index, row.value]))
                    .collect(),
            )
        }
    }
}

pub(super) fn flag_table_key(id: &str) -> Option<(&'static str, usize, bool)> {
    match id {
        "account_flag_runs" => Some(("account_flag_runs", ACCOUNT_FLAG_CAPACITY, true)),
        "profile_flag_runs" => Some(("profile_flag_runs", PROFILE_FLAG_CAPACITY, true)),
        "character_flags" => Some(("character_flags", CHARACTER_FLAG_CAPACITY, false)),
        "character_object_flag_runs" => {
            Some(("character_flag_runs", CHARACTER_OBJECT_FLAG_CAPACITY, true))
        }
        _ => None,
    }
}

pub(super) fn value_table_key(id: &str) -> Option<(&'static str, usize)> {
    match id {
        "objective_values" => Some(("objective_values", OBJECTIVE_VALUE_CAPACITY)),
        "character_object_objective_values" => Some((
            "character_objective_values",
            CHARACTER_OBJECT_VALUE_CAPACITY,
        )),
        _ => None,
    }
}

pub(super) fn progression_table_key(id: &str) -> Option<&'static str> {
    match id {
        "account_progressions" => Some("account_progressions"),
        "character_progressions" => Some("character_progressions"),
        _ => None,
    }
}

pub(super) fn set_progression_value(
    document: &mut Value,
    id: &str,
    definition_index: usize,
    lanes: [i32; 3],
) -> bool {
    let Some(key) = progression_table_key(id) else {
        return false;
    };
    if definition_index >= PROGRESSION_DEFINITION_CAPACITY {
        return false;
    }
    let Ok(current) = parse_unlocks(document.pointer("/state/unlocks")) else {
        return false;
    };
    let mut values = match id {
        "account_progressions" => current.account_progressions,
        "character_progressions" => current.character_progressions,
        _ => return false,
    };
    if let Some(row) = values
        .iter_mut()
        .rev()
        .find(|row| row.definition_index == definition_index)
    {
        if row.lanes == lanes {
            return false;
        }
        row.lanes = lanes;
    } else {
        values.push(ProgressionValue {
            definition_index,
            lanes,
        });
    }
    values.sort_by_key(|row| row.definition_index);
    write_unlock_array(
        document,
        key,
        values
            .into_iter()
            .map(|row| {
                serde_json::json!([
                    row.definition_index,
                    row.lanes[0],
                    row.lanes[1],
                    row.lanes[2]
                ])
            })
            .collect(),
    )
}

pub(super) fn remove_progression_value(
    document: &mut Value,
    id: &str,
    definition_index: usize,
) -> bool {
    let Some(key) = progression_table_key(id) else {
        return false;
    };
    let Ok(current) = parse_unlocks(document.pointer("/state/unlocks")) else {
        return false;
    };
    let mut values = match id {
        "account_progressions" => current.account_progressions,
        "character_progressions" => current.character_progressions,
        _ => return false,
    };
    let prior_len = values.len();
    values.retain(|row| row.definition_index != definition_index);
    if values.len() == prior_len {
        return false;
    }
    write_unlock_array(
        document,
        key,
        values
            .into_iter()
            .map(|row| {
                serde_json::json!([
                    row.definition_index,
                    row.lanes[0],
                    row.lanes[1],
                    row.lanes[2]
                ])
            })
            .collect(),
    )
}

pub(super) fn set_unlock_flag(document: &mut Value, id: &str, slot: usize, set: bool) -> bool {
    let Some((key, capacity, uses_runs)) = flag_table_key(id) else {
        return false;
    };
    if slot >= capacity {
        return false;
    }
    let Ok(current) = parse_unlocks(document.pointer("/state/unlocks")) else {
        return false;
    };
    let mut slots = match id {
        "account_flag_runs" => expanded_flag_slots(&current.account_flag_runs, capacity),
        "profile_flag_runs" => expanded_flag_slots(&current.profile_flag_runs, capacity),
        "character_flags" => current
            .character_flags
            .into_iter()
            .map(|row| row.index)
            .collect(),
        "character_object_flag_runs" => {
            expanded_flag_slots(&current.character_object_flag_runs, capacity)
        }
        _ => return false,
    };
    slots.sort_unstable();
    slots.dedup();
    match slots.binary_search(&slot) {
        Ok(index) if !set => {
            slots.remove(index);
        }
        Err(index) if set => slots.insert(index, slot),
        _ => return false,
    }
    let rows = if uses_runs {
        compress_flag_slots(&slots)
            .into_iter()
            .map(|run| serde_json::json!([run.start, run.length]))
            .collect()
    } else {
        slots.into_iter().map(Value::from).collect()
    };
    write_unlock_array(document, key, rows)
}

pub(in crate::app) fn set_collection_flag(
    document: &mut Value,
    definition_index: usize,
    definition: &UnlockDefinition,
    set: bool,
) -> bool {
    let Ok(investment) = parse_investment(document.pointer("/state/investment")) else {
        return false;
    };
    if investment
        .flag_overrides
        .iter()
        .any(|row| row.definition_index == definition_index)
    {
        return set_investment_override(
            document,
            InvestmentTable::FlagOverrides,
            definition_index,
            if set { 2 } else { 0 },
        );
    }
    let Some(slot) = definition.compact_slot.map(usize::from) else {
        return set_investment_override(
            document,
            InvestmentTable::FlagOverrides,
            definition_index,
            if set { 2 } else { 0 },
        );
    };
    let table = match definition.bank() {
        ACCOUNT_FLAG_BANK => "account_flag_runs",
        PROFILE_FLAG_BANK => "profile_flag_runs",
        CHARACTER_OBJECT_FLAG_BANK => "character_object_flag_runs",
        CHARACTER_FLAG_BANK => "character_flags",
        _ => return false,
    };
    set_unlock_flag(document, table, slot, set)
}

/// Remove both persisted lanes: an override can hide a still-set compact flag.
/// Badge completion is computed from these flags; stock objectives and XP stay untouched.
pub(in crate::app) fn remove_authored_collection_state(
    document: &mut Value,
    unlocks: &[crate::investment::AuthoredCollectionUnlock],
) -> Result<usize, String> {
    validate(document)?;
    let mut changed = 0;
    for unlock in unlocks {
        if unlock.bank != ACCOUNT_FLAG_BANK || usize::from(unlock.slot) >= ACCOUNT_FLAG_CAPACITY {
            return Err("Unsupported authored collection flag bank or slot".into());
        }
        let removed_override = remove_investment_override(
            document,
            InvestmentTable::FlagOverrides,
            usize::from(unlock.definition_index),
        );
        let removed_flag = set_unlock_flag(
            document,
            "account_flag_runs",
            usize::from(unlock.slot),
            false,
        );
        changed += usize::from(removed_override || removed_flag);
    }
    validate(document)?;
    Ok(changed)
}

pub(super) fn set_unlock_value(document: &mut Value, id: &str, slot: usize, value: i32) -> bool {
    let Some((key, capacity)) = value_table_key(id) else {
        return false;
    };
    if slot >= capacity {
        return false;
    }
    if id == "character_object_objective_values"
        && RESERVED_CHARACTER_OBJECTIVE_VALUES
            .iter()
            .any(|(reserved, expected)| *reserved == slot && *expected != value)
    {
        return false;
    }
    let Ok(current) = parse_unlocks(document.pointer("/state/unlocks")) else {
        return false;
    };
    let mut values = match id {
        "objective_values" => current.objective_values,
        "character_object_objective_values" => current.character_objective_values,
        _ => return false,
    };
    if let Some(row) = values.iter_mut().rev().find(|row| row.index == slot) {
        if row.value == value {
            return false;
        }
        row.value = value;
    } else {
        values.push(IndexedValue { index: slot, value });
    }
    values.sort_by_key(|row| row.index);
    write_unlock_array(
        document,
        key,
        values
            .into_iter()
            .map(|row| serde_json::json!([row.index, row.value]))
            .collect(),
    )
}

pub(in crate::app) fn set_collection_value(
    document: &mut Value,
    definition_index: usize,
    definition: &UnlockDefinition,
    value: i32,
) -> bool {
    let Ok(investment) = parse_investment(document.pointer("/state/investment")) else {
        return false;
    };
    if investment
        .value_overrides
        .iter()
        .any(|row| row.definition_index == definition_index)
    {
        return set_investment_override(
            document,
            InvestmentTable::ValueOverrides,
            definition_index,
            value,
        );
    }
    let Some(slot) = definition.compact_slot.map(usize::from) else {
        return set_investment_override(
            document,
            InvestmentTable::ValueOverrides,
            definition_index,
            value,
        );
    };
    let table = match definition.bank() {
        ACCOUNT_OBJECTIVE_BANK => "objective_values",
        CHARACTER_OBJECTIVE_BANK => "character_object_objective_values",
        _ => return false,
    };
    set_unlock_value(document, table, slot, value)
}

pub(super) fn remove_unlock_value(document: &mut Value, id: &str, slot: usize) -> bool {
    let Some((key, _)) = value_table_key(id) else {
        return false;
    };
    if id == "character_object_objective_values"
        && RESERVED_CHARACTER_OBJECTIVE_VALUES
            .iter()
            .any(|(reserved, _)| *reserved == slot)
    {
        return false;
    }
    let Ok(current) = parse_unlocks(document.pointer("/state/unlocks")) else {
        return false;
    };
    let mut values = match id {
        "objective_values" => current.objective_values,
        "character_object_objective_values" => current.character_objective_values,
        _ => return false,
    };
    let prior_len = values.len();
    values.retain(|row| row.index != slot);
    if values.len() == prior_len {
        return false;
    }
    write_unlock_array(
        document,
        key,
        values
            .into_iter()
            .map(|row| serde_json::json!([row.index, row.value]))
            .collect(),
    )
}

use super::*;

pub(crate) fn unlocks_object_mut(document: &mut Value) -> Option<&mut Map<String, Value>> {
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

pub(crate) fn write_unlock_array(document: &mut Value, key: &str, rows: Vec<Value>) -> Write {
    let Some(unlocks) = unlocks_object_mut(document) else {
        return Write::Refused;
    };
    unlocks.insert(key.to_owned(), Value::Array(rows));
    Write::Wrote
}

pub(crate) fn investment_object_mut(document: &mut Value) -> Option<&mut Map<String, Value>> {
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

pub(crate) fn write_investment_array(document: &mut Value, key: &str, rows: Vec<Value>) -> Write {
    let Some(investment) = investment_object_mut(document) else {
        return Write::Refused;
    };
    investment.insert(key.to_owned(), Value::Array(rows));
    Write::Wrote
}

pub(crate) fn set_investment_override(
    document: &mut Value,
    table: InvestmentTable,
    definition_index: usize,
    value: i32,
) -> Write {
    let Ok(policy) = parse_investment(document.pointer("/state/investment")) else {
        return Write::Refused;
    };
    let hidden_count = super::native::hidden_count(document, table);
    match table {
        InvestmentTable::FlagOverrides => {
            if definition_index > FAMILY5_FLAG_SLOT_MAXIMUM
                || !(0..=i32::from(FAMILY5_FLAG_VALUE_MAXIMUM)).contains(&value)
            {
                return Write::Refused;
            }
            let mut rows = policy.flag_overrides;
            if let Some(row) = rows
                .iter_mut()
                .rev()
                .find(|row| row.definition_index == definition_index)
            {
                if i32::from(row.value) == value {
                    return Write::Unchanged;
                }
                row.value = value as u8;
            } else {
                if rows.len() + hidden_count >= FAMILY5_OVERRIDE_CAPACITY {
                    return Write::Refused;
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
                return Write::Refused;
            }
            let mut rows = policy.value_overrides;
            if let Some(row) = rows
                .iter_mut()
                .rev()
                .find(|row| row.definition_index == definition_index)
            {
                if row.value == value {
                    return Write::Unchanged;
                }
                row.value = value;
            } else {
                if rows.len() + hidden_count >= FAMILY5_OVERRIDE_CAPACITY {
                    return Write::Refused;
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

pub(crate) fn remove_investment_override(
    document: &mut Value,
    table: InvestmentTable,
    definition_index: usize,
) -> Write {
    let Ok(policy) = parse_investment(document.pointer("/state/investment")) else {
        return Write::Refused;
    };
    match table {
        InvestmentTable::FlagOverrides => {
            let mut rows = policy.flag_overrides;
            let prior_len = rows.len();
            rows.retain(|row| row.definition_index != definition_index);
            if rows.len() == prior_len {
                return Write::Unchanged;
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
                return Write::Unchanged;
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

pub(crate) fn flag_table_key(id: &str) -> Option<(&'static str, usize, bool)> {
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

pub(crate) fn value_table_key(id: &str) -> Option<(&'static str, usize)> {
    match id {
        "objective_values" => Some(("objective_values", OBJECTIVE_VALUE_CAPACITY)),
        "character_object_objective_values" => Some((
            "character_objective_values",
            CHARACTER_OBJECT_VALUE_CAPACITY,
        )),
        _ => None,
    }
}

pub(crate) fn progression_table_key(id: &str) -> Option<&'static str> {
    match id {
        "account_progressions" => Some("account_progressions"),
        "character_progressions" => Some("character_progressions"),
        _ => None,
    }
}

pub(crate) fn set_progression_value(
    document: &mut Value,
    id: &str,
    definition_index: usize,
    lanes: [i32; 3],
) -> Write {
    let Some(key) = progression_table_key(id) else {
        return Write::Refused;
    };
    if definition_index >= PROGRESSION_DEFINITION_CAPACITY {
        return Write::Refused;
    }
    let Ok(current) = parse_document_unlocks(document) else {
        return Write::Refused;
    };
    let mut values = match id {
        "account_progressions" => current.account_progressions,
        "character_progressions" => current.character_progressions,
        _ => return Write::Refused,
    };
    if let Some(row) = values
        .iter_mut()
        .rev()
        .find(|row| row.definition_index == definition_index)
    {
        if row.lanes == lanes {
            return Write::Unchanged;
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

pub(crate) fn remove_progression_value(
    document: &mut Value,
    id: &str,
    definition_index: usize,
) -> Write {
    let Some(key) = progression_table_key(id) else {
        return Write::Refused;
    };
    let Ok(current) = parse_document_unlocks(document) else {
        return Write::Refused;
    };
    let mut values = match id {
        "account_progressions" => current.account_progressions,
        "character_progressions" => current.character_progressions,
        _ => return Write::Refused,
    };
    let prior_len = values.len();
    values.retain(|row| row.definition_index != definition_index);
    if values.len() == prior_len {
        return Write::Unchanged;
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

pub(crate) fn set_unlock_flag(document: &mut Value, id: &str, slot: usize, set: bool) -> Write {
    let Some((key, capacity, uses_runs)) = flag_table_key(id) else {
        return Write::Refused;
    };
    if slot >= capacity {
        return Write::Refused;
    }
    let Ok(current) = parse_document_unlocks(document) else {
        return Write::Refused;
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
        _ => return Write::Refused,
    };
    slots.sort_unstable();
    slots.dedup();
    match slots.binary_search(&slot) {
        Ok(index) if !set => {
            slots.remove(index);
        }
        Err(index) if set => slots.insert(index, slot),
        _ => return Write::Unchanged,
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

pub(crate) fn set_collection_flag(
    document: &mut Value,
    definition_index: usize,
    definition: &UnlockDefinition,
    set: bool,
) -> Write {
    let Ok(investment) = parse_investment(document.pointer("/state/investment")) else {
        return Write::Refused;
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
        _ => return Write::Refused,
    };
    set_unlock_flag(document, table, slot, set)
}

/// Remove both persisted lanes: an override can hide a still-set compact flag.
/// Badge completion is computed from these flags; stock objectives and XP stay untouched.
pub(crate) fn remove_authored_collection_state(
    document: &mut Value,
    unlocks: &[crate::account::AuthoredCollectionUnlock],
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
        changed += usize::from(removed_override.changed() || removed_flag.changed());
    }
    validate(document)?;
    Ok(changed)
}

pub(crate) fn set_unlock_value(document: &mut Value, id: &str, slot: usize, value: i32) -> Write {
    let Some((key, capacity)) = value_table_key(id) else {
        return Write::Refused;
    };
    if slot >= capacity {
        return Write::Refused;
    }
    if document.get("_native_progression").is_none()
        && id == "character_object_objective_values"
        && RESERVED_CHARACTER_OBJECTIVE_VALUES
            .iter()
            .any(|(reserved, expected)| *reserved == slot && *expected != value)
    {
        return Write::Refused;
    }
    let Ok(current) = parse_document_unlocks(document) else {
        return Write::Refused;
    };
    let mut values = match id {
        "objective_values" => current.objective_values,
        "character_object_objective_values" => current.character_objective_values,
        _ => return Write::Refused,
    };
    if let Some(row) = values.iter_mut().rev().find(|row| row.index == slot) {
        if row.value == value {
            return Write::Unchanged;
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

pub(crate) fn set_collection_value(
    document: &mut Value,
    definition_index: usize,
    definition: &UnlockDefinition,
    value: i32,
) -> Write {
    let Ok(investment) = parse_investment(document.pointer("/state/investment")) else {
        return Write::Refused;
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
        _ => return Write::Refused,
    };
    set_unlock_value(document, table, slot, value)
}

pub(crate) fn remove_unlock_value(document: &mut Value, id: &str, slot: usize) -> Write {
    let Some((key, _)) = value_table_key(id) else {
        return Write::Refused;
    };
    if document.get("_native_progression").is_none()
        && id == "character_object_objective_values"
        && RESERVED_CHARACTER_OBJECTIVE_VALUES
            .iter()
            .any(|(reserved, _)| *reserved == slot)
    {
        return Write::Refused;
    }
    let Ok(current) = parse_document_unlocks(document) else {
        return Write::Refused;
    };
    let mut values = match id {
        "objective_values" => current.objective_values,
        "character_object_objective_values" => current.character_objective_values,
        _ => return Write::Refused,
    };
    let prior_len = values.len();
    values.retain(|row| row.index != slot);
    if values.len() == prior_len {
        return Write::Unchanged;
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

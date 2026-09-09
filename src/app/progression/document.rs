use std::collections::{HashMap, HashSet};

use serde_json::{Map, Value};

use crate::catalog::UnlockDefinition;

pub(super) const ACCOUNT_FLAG_CAPACITY: usize =
    crate::package_authoring::SHADOWKEEP_ACCOUNT_FLAG_REGION_CAPACITY;
pub(super) const PROFILE_FLAG_CAPACITY: usize = 512;
pub(super) const CHARACTER_FLAG_CAPACITY: usize = 256;
pub(super) const OBJECTIVE_VALUE_CAPACITY: usize = 6_200;
pub(super) const CHARACTER_OBJECT_FLAG_CAPACITY: usize = 4_096;
pub(super) const CHARACTER_OBJECT_VALUE_CAPACITY: usize = 768;
pub(super) const RESERVED_CHARACTER_OBJECTIVE_VALUES: [(usize, i32); 2] = [(443, -1), (502, -1)];
pub(super) const PROGRESSION_DEFINITION_CAPACITY: usize = 256;
pub(super) const FAMILY5_OVERRIDE_CAPACITY: usize = 100;
pub(super) const FAMILY5_FLAG_SLOT_MAXIMUM: usize = 23_499;
pub(super) const FAMILY5_VALUE_SLOT_MAXIMUM: usize = 15_499;
pub(super) const FAMILY5_FLAG_VALUE_MAXIMUM: u8 = 2;
pub(super) const ACCOUNT_FLAG_BANK: u8 = crate::package_authoring::SHADOWKEEP_ACCOUNT_FLAG_BANK;
pub(super) const PROFILE_FLAG_BANK: u8 = 2;
pub(super) const CHARACTER_OBJECT_FLAG_BANK: u8 = 3;
pub(super) const CHARACTER_FLAG_BANK: u8 = 6;
pub(super) const ACCOUNT_OBJECTIVE_BANK: u8 = 1;
pub(super) const CHARACTER_OBJECTIVE_BANK: u8 = 2;
/// Definition kind 0 has no backing store and reads as the native zero value.
const UNBACKED_DEFAULT_KIND: u8 = 0;
/// Definition kind 5 is computed from inventory at runtime and cannot be reconstructed here.
const INVENTORY_COMPUTED_KIND: u8 = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct FlagRun {
    pub(super) start: usize,
    pub(super) length: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct FlagIndex {
    pub(super) index: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct IndexedValue {
    pub(super) index: usize,
    pub(super) value: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ProgressionValue {
    pub(super) definition_index: usize,
    pub(super) lanes: [i32; 3],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct FlagOverride {
    pub(super) definition_index: usize,
    pub(super) value: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ValueOverride {
    pub(super) definition_index: usize,
    pub(super) value: i32,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct UnlockPolicy {
    pub(super) account_flag_runs: Vec<FlagRun>,
    pub(super) profile_flag_runs: Vec<FlagRun>,
    pub(super) character_flags: Vec<FlagIndex>,
    pub(super) objective_values: Vec<IndexedValue>,
    pub(super) character_object_flag_runs: Vec<FlagRun>,
    pub(super) character_objective_values: Vec<IndexedValue>,
    pub(super) account_progressions: Vec<ProgressionValue>,
    pub(super) character_progressions: Vec<ProgressionValue>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct InvestmentPolicy {
    pub(super) flag_overrides: Vec<FlagOverride>,
    pub(super) value_overrides: Vec<ValueOverride>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Progression {
    pub(super) unlocks: UnlockPolicy,
    pub(super) investment: InvestmentPolicy,
}

pub(in crate::app) struct CollectionStateSnapshot {
    pub(super) flags: HashSet<(u8, usize)>,
    pub(super) values: HashMap<(u8, usize), i32>,
    pub(super) flag_overrides: HashMap<usize, u8>,
    pub(super) value_overrides: HashMap<usize, i32>,
    pub(super) account_progressions: Vec<ProgressionValue>,
    pub(super) character_progressions: Vec<ProgressionValue>,
}

impl CollectionStateSnapshot {
    pub(in crate::app) fn flag_value(
        &self,
        definition_index: usize,
        definition: &UnlockDefinition,
    ) -> Option<bool> {
        if let Some(value) = self.flag_overrides.get(&definition_index).copied() {
            // Family-5 is an override by definition, including for compact-backed definitions.
            return Some(value == 2);
        }
        let Some(slot) = definition.compact_slot.map(usize::from) else {
            return (definition.bank() == UNBACKED_DEFAULT_KIND).then_some(false);
        };
        matches!(
            definition.bank(),
            ACCOUNT_FLAG_BANK
                | PROFILE_FLAG_BANK
                | CHARACTER_OBJECT_FLAG_BANK
                | CHARACTER_FLAG_BANK
        )
        .then(|| self.flags.contains(&(definition.bank(), slot)))
    }

    pub(in crate::app) fn value(
        &self,
        definition_index: usize,
        definition: &UnlockDefinition,
    ) -> Option<i32> {
        if let Some(value) = self.value_overrides.get(&definition_index).copied() {
            return Some(value);
        }
        let Some(slot) = definition.compact_slot.map(usize::from) else {
            return (definition.bank() == UNBACKED_DEFAULT_KIND).then_some(0);
        };
        matches!(
            definition.bank(),
            ACCOUNT_OBJECTIVE_BANK | CHARACTER_OBJECTIVE_BANK
        )
        .then(|| {
            self.values
                .get(&(definition.bank(), slot))
                .copied()
                .unwrap_or_default()
        })
    }

    pub(in crate::app) fn flag_text(
        &self,
        definition_index: usize,
        definition: &UnlockDefinition,
    ) -> String {
        if let Some(value) = self.flag_overrides.get(&definition_index) {
            return format!("Override {value}");
        }
        let Some(slot) = definition.compact_slot.map(usize::from) else {
            return match definition.bank() {
                UNBACKED_DEFAULT_KIND => "Default false".into(),
                INVENTORY_COMPUTED_KIND => "Computed at runtime".into(),
                _ => "Unavailable".into(),
            };
        };
        if !matches!(
            definition.bank(),
            ACCOUNT_FLAG_BANK
                | PROFILE_FLAG_BANK
                | CHARACTER_OBJECT_FLAG_BANK
                | CHARACTER_FLAG_BANK
        ) {
            return "Unavailable".into();
        }
        if self.flags.contains(&(definition.bank(), slot)) {
            "Set".into()
        } else {
            "Unset".into()
        }
    }

    pub(in crate::app) fn value_text(
        &self,
        definition_index: usize,
        definition: &UnlockDefinition,
    ) -> String {
        if let Some(value) = self.value_overrides.get(&definition_index) {
            return format!("Override {value}");
        }
        let Some(slot) = definition.compact_slot.map(usize::from) else {
            return match definition.bank() {
                UNBACKED_DEFAULT_KIND => "Default 0".into(),
                _ => "Unavailable".into(),
            };
        };
        if !matches!(
            definition.bank(),
            ACCOUNT_OBJECTIVE_BANK | CHARACTER_OBJECTIVE_BANK
        ) {
            return "Unavailable".into();
        }
        self.values
            .get(&(definition.bank(), slot))
            .map_or_else(|| "Not listed".into(), i32::to_string)
    }
}

pub(in crate::app) fn collection_state_snapshot(
    document: &Value,
) -> Option<CollectionStateSnapshot> {
    let policy = parse(document).ok()?;
    let mut flags = HashSet::new();
    flags.extend(
        expanded_flag_slots(&policy.unlocks.account_flag_runs, ACCOUNT_FLAG_CAPACITY)
            .into_iter()
            .map(|slot| (ACCOUNT_FLAG_BANK, slot)),
    );
    flags.extend(
        expanded_flag_slots(&policy.unlocks.profile_flag_runs, PROFILE_FLAG_CAPACITY)
            .into_iter()
            .map(|slot| (PROFILE_FLAG_BANK, slot)),
    );
    flags.extend(
        expanded_flag_slots(
            &policy.unlocks.character_object_flag_runs,
            CHARACTER_OBJECT_FLAG_CAPACITY,
        )
        .into_iter()
        .map(|slot| (CHARACTER_OBJECT_FLAG_BANK, slot)),
    );
    flags.extend(
        policy
            .unlocks
            .character_flags
            .iter()
            .map(|row| (CHARACTER_FLAG_BANK, row.index)),
    );
    let values = policy
        .unlocks
        .objective_values
        .iter()
        .map(|row| ((ACCOUNT_OBJECTIVE_BANK, row.index), row.value))
        .chain(
            policy
                .unlocks
                .character_objective_values
                .iter()
                .map(|row| ((CHARACTER_OBJECTIVE_BANK, row.index), row.value)),
        )
        .collect();
    let mut snapshot = CollectionStateSnapshot {
        flags,
        values,
        account_progressions: policy.unlocks.account_progressions,
        character_progressions: policy.unlocks.character_progressions,
        flag_overrides: policy
            .investment
            .flag_overrides
            .iter()
            .map(|row| (row.definition_index, row.value))
            .collect(),
        value_overrides: policy
            .investment
            .value_overrides
            .iter()
            .map(|row| (row.definition_index, row.value))
            .collect(),
    };
    // The bounded editor omits raw native overrides. They still affect native readers.
    for row in document["_native_progression"]["family"]
        .as_array()
        .into_iter()
        .flatten()
    {
        let Some(index) = row[1]
            .as_u64()
            .and_then(|index| usize::try_from(index).ok())
        else {
            continue;
        };
        match row[0].as_u64() {
            Some(0) => {
                if let Some(value) = row[2].as_u64().and_then(|value| u8::try_from(value).ok())
                    && (index > FAMILY5_FLAG_SLOT_MAXIMUM || value > FAMILY5_FLAG_VALUE_MAXIMUM)
                {
                    snapshot.flag_overrides.entry(index).or_insert(value);
                }
            }
            Some(1) if index > FAMILY5_VALUE_SLOT_MAXIMUM => {
                if let Some(value) = row[2].as_i64().and_then(|value| i32::try_from(value).ok()) {
                    snapshot.value_overrides.entry(index).or_insert(value);
                }
            }
            _ => {}
        }
    }
    Some(snapshot)
}

pub(in crate::app) fn collection_flag_state_text(
    state: &CollectionStateSnapshot,
    definition_index: usize,
    definition: &UnlockDefinition,
) -> String {
    state.flag_text(definition_index, definition)
}

pub(in crate::app) fn collection_value_state_text(
    state: &CollectionStateSnapshot,
    definition_index: usize,
    definition: &UnlockDefinition,
) -> String {
    state.value_text(definition_index, definition)
}

pub(in crate::app) fn validate(document: &Value) -> Result<(), String> {
    parse(document).map(|_| ())
}

pub(super) fn parse(document: &Value) -> Result<Progression, String> {
    Ok(Progression {
        unlocks: parse_document_unlocks(document)?,
        investment: parse_investment(document.pointer("/state/investment"))?,
    })
}

pub(super) fn parse_unlocks(value: Option<&Value>) -> Result<UnlockPolicy, String> {
    parse_unlocks_with_policy(value, false)
}

pub(super) fn parse_document_unlocks(document: &Value) -> Result<UnlockPolicy, String> {
    if document.get("_native_progression").is_none() {
        return parse_unlocks(document.pointer("/state/unlocks"));
    }
    parse_unlocks_with_policy(document.pointer("/state/unlocks"), true)
}

fn parse_unlocks_with_policy(value: Option<&Value>, native: bool) -> Result<UnlockPolicy, String> {
    let Some(object) = optional_object(value, "state.unlocks")? else {
        return Ok(UnlockPolicy::default());
    };

    let character_objective_values = parse_indexed_values(
        object.get("character_objective_values"),
        "state.unlocks.character_objective_values",
        CHARACTER_OBJECT_VALUE_CAPACITY,
    )?;
    for row in &character_objective_values {
        if let Some((_, expected)) = RESERVED_CHARACTER_OBJECTIVE_VALUES
            .iter()
            .find(|(index, _)| *index == row.index)
            && !native
            && row.value != *expected
        {
            return Err(format!(
                "state.unlocks.character_objective_values slot {} must remain {expected}",
                row.index
            ));
        }
    }

    Ok(UnlockPolicy {
        account_flag_runs: parse_flag_runs(
            object.get("account_flag_runs"),
            "state.unlocks.account_flag_runs",
            ACCOUNT_FLAG_CAPACITY,
        )?,
        profile_flag_runs: parse_flag_runs(
            object.get("profile_flag_runs"),
            "state.unlocks.profile_flag_runs",
            PROFILE_FLAG_CAPACITY,
        )?,
        character_flags: parse_flag_indices(
            object.get("character_flags"),
            "state.unlocks.character_flags",
            CHARACTER_FLAG_CAPACITY,
        )?,
        objective_values: parse_indexed_values(
            object.get("objective_values"),
            "state.unlocks.objective_values",
            OBJECTIVE_VALUE_CAPACITY,
        )?,
        character_object_flag_runs: parse_flag_runs(
            object.get("character_flag_runs"),
            "state.unlocks.character_flag_runs",
            CHARACTER_OBJECT_FLAG_CAPACITY,
        )?,
        character_objective_values,
        account_progressions: parse_progression_values(
            object.get("account_progressions"),
            "state.unlocks.account_progressions",
        )?,
        character_progressions: parse_progression_values(
            object.get("character_progressions"),
            "state.unlocks.character_progressions",
        )?,
    })
}

pub(super) fn parse_investment(value: Option<&Value>) -> Result<InvestmentPolicy, String> {
    let Some(object) = optional_object(value, "state.investment")? else {
        return Ok(InvestmentPolicy::default());
    };

    Ok(InvestmentPolicy {
        flag_overrides: parse_flag_overrides(
            object.get("family5_flag_overrides"),
            "state.investment.family5_flag_overrides",
        )?,
        value_overrides: parse_value_overrides(
            object.get("family5_value_overrides"),
            "state.investment.family5_value_overrides",
        )?,
    })
}

fn optional_object<'a>(
    value: Option<&'a Value>,
    path: &str,
) -> Result<Option<&'a Map<String, Value>>, String> {
    value
        .map(|value| {
            value
                .as_object()
                .ok_or_else(|| format!("{path} must be an object"))
        })
        .transpose()
}

fn optional_array<'a>(value: Option<&'a Value>, path: &str) -> Result<&'a [Value], String> {
    value.map_or(Ok(&[]), |value| {
        value
            .as_array()
            .map(Vec::as_slice)
            .ok_or_else(|| format!("{path} must be an array"))
    })
}

fn pair<'a>(row: &'a Value, path: &str, row_index: usize) -> Result<[&'a Value; 2], String> {
    let values = row
        .as_array()
        .ok_or_else(|| format!("{path}[{row_index}] must be a two-value array"))?;
    let [first, second] = values.as_slice() else {
        return Err(format!(
            "{path}[{row_index}] must contain exactly two values"
        ));
    };
    Ok([first, second])
}

fn unsigned(value: &Value, path: &str) -> Result<usize, String> {
    let value = value
        .as_u64()
        .ok_or_else(|| format!("{path} must be an unsigned integer"))?;
    usize::try_from(value).map_err(|_| format!("{path} is too large"))
}

fn signed_32(value: &Value, path: &str) -> Result<i32, String> {
    let value = value
        .as_i64()
        .ok_or_else(|| format!("{path} must be a signed integer"))?;
    i32::try_from(value).map_err(|_| format!("{path} must fit a signed 32-bit integer"))
}

fn parse_flag_runs(
    value: Option<&Value>,
    path: &str,
    capacity: usize,
) -> Result<Vec<FlagRun>, String> {
    optional_array(value, path)?
        .iter()
        .enumerate()
        .map(|(row_index, row)| {
            let [start, length] = pair(row, path, row_index)?;
            let start = unsigned(start, &format!("{path}[{row_index}][0]"))?;
            let length = unsigned(length, &format!("{path}[{row_index}][1]"))?;
            if length == 0 {
                return Err(format!("{path}[{row_index}] must have a positive length"));
            }
            if start > capacity || length > capacity.saturating_sub(start) {
                return Err(format!(
                    "{path}[{row_index}] exceeds its {capacity}-slot bank"
                ));
            }
            Ok(FlagRun { start, length })
        })
        .collect()
}

fn parse_flag_indices(
    value: Option<&Value>,
    path: &str,
    capacity: usize,
) -> Result<Vec<FlagIndex>, String> {
    optional_array(value, path)?
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let value = unsigned(value, &format!("{path}[{index}]"))?;
            if value >= capacity {
                return Err(format!(
                    "{path}[{index}] must be below the {capacity}-slot bank capacity"
                ));
            }
            Ok(FlagIndex { index: value })
        })
        .collect()
}

fn parse_indexed_values(
    value: Option<&Value>,
    path: &str,
    capacity: usize,
) -> Result<Vec<IndexedValue>, String> {
    optional_array(value, path)?
        .iter()
        .enumerate()
        .map(|(row_index, row)| {
            let [index, value] = pair(row, path, row_index)?;
            let index = unsigned(index, &format!("{path}[{row_index}][0]"))?;
            if index >= capacity {
                return Err(format!(
                    "{path}[{row_index}][0] must be below the {capacity}-slot bank capacity"
                ));
            }
            let value = signed_32(value, &format!("{path}[{row_index}][1]"))?;
            Ok(IndexedValue { index, value })
        })
        .collect()
}

fn parse_progression_values(
    value: Option<&Value>,
    path: &str,
) -> Result<Vec<ProgressionValue>, String> {
    let mut authored = vec![None::<[i32; 3]>; PROGRESSION_DEFINITION_CAPACITY];
    for (row_index, row) in optional_array(value, path)?.iter().enumerate() {
        let values = row
            .as_array()
            .ok_or_else(|| format!("{path}[{row_index}] must be a four-value array"))?;
        let [definition_index, lane_0, lane_1, lane_2] = values.as_slice() else {
            return Err(format!(
                "{path}[{row_index}] must contain exactly four values"
            ));
        };
        let definition_index = unsigned(definition_index, &format!("{path}[{row_index}][0]"))?;
        if definition_index >= PROGRESSION_DEFINITION_CAPACITY {
            return Err(format!(
                "{path}[{row_index}][0] must be below the {PROGRESSION_DEFINITION_CAPACITY}-definition capacity"
            ));
        }
        let lanes = [
            signed_32(lane_0, &format!("{path}[{row_index}][1]"))?,
            signed_32(lane_1, &format!("{path}[{row_index}][2]"))?,
            signed_32(lane_2, &format!("{path}[{row_index}][3]"))?,
        ];
        if let Some(current) = authored[definition_index].as_mut() {
            for lane in 0..3 {
                current[lane] = current[lane].max(lanes[lane]);
            }
        } else {
            authored[definition_index] = Some(lanes);
        }
    }
    Ok(authored
        .into_iter()
        .enumerate()
        .filter_map(|(definition_index, lanes)| {
            lanes.map(|lanes| ProgressionValue {
                definition_index,
                lanes,
            })
        })
        .collect())
}

fn parse_flag_overrides(value: Option<&Value>, path: &str) -> Result<Vec<FlagOverride>, String> {
    let rows = optional_array(value, path)?;
    if rows.len() > FAMILY5_OVERRIDE_CAPACITY {
        return Err(format!(
            "{path} cannot contain more than {FAMILY5_OVERRIDE_CAPACITY} rows"
        ));
    }
    rows.iter()
        .enumerate()
        .map(|(row_index, row)| {
            let [slot, value] = pair(row, path, row_index)?;
            let slot = unsigned(slot, &format!("{path}[{row_index}][0]"))?;
            if slot > FAMILY5_FLAG_SLOT_MAXIMUM {
                return Err(format!(
                    "{path}[{row_index}][0] cannot exceed {FAMILY5_FLAG_SLOT_MAXIMUM}"
                ));
            }
            let value = unsigned(value, &format!("{path}[{row_index}][1]"))?;
            let value = u8::try_from(value)
                .ok()
                .filter(|value| *value <= FAMILY5_FLAG_VALUE_MAXIMUM)
                .ok_or_else(|| {
                    format!("{path}[{row_index}][1] cannot exceed {FAMILY5_FLAG_VALUE_MAXIMUM}")
                })?;
            Ok(FlagOverride {
                definition_index: slot,
                value,
            })
        })
        .collect()
}

fn parse_value_overrides(value: Option<&Value>, path: &str) -> Result<Vec<ValueOverride>, String> {
    let rows = optional_array(value, path)?;
    if rows.len() > FAMILY5_OVERRIDE_CAPACITY {
        return Err(format!(
            "{path} cannot contain more than {FAMILY5_OVERRIDE_CAPACITY} rows"
        ));
    }
    rows.iter()
        .enumerate()
        .map(|(row_index, row)| {
            let [slot, value] = pair(row, path, row_index)?;
            let slot = unsigned(slot, &format!("{path}[{row_index}][0]"))?;
            if slot > FAMILY5_VALUE_SLOT_MAXIMUM {
                return Err(format!(
                    "{path}[{row_index}][0] cannot exceed {FAMILY5_VALUE_SLOT_MAXIMUM}"
                ));
            }
            let value = signed_32(value, &format!("{path}[{row_index}][1]"))?;
            Ok(ValueOverride {
                definition_index: slot,
                value,
            })
        })
        .collect()
}

pub(super) fn expanded_flag_slots(rows: &[FlagRun], capacity: usize) -> Vec<usize> {
    let mut flags = vec![false; capacity];
    for row in rows {
        flags[row.start..row.start + row.length].fill(true);
    }
    flags
        .into_iter()
        .enumerate()
        .filter_map(|(slot, set)| set.then_some(slot))
        .collect()
}

pub(super) fn compress_flag_slots(slots: &[usize]) -> Vec<FlagRun> {
    let mut runs = Vec::new();
    let Some(&first) = slots.first() else {
        return runs;
    };
    let mut start = first;
    let mut previous = first;
    for &slot in &slots[1..] {
        if slot == previous.saturating_add(1) {
            previous = slot;
            continue;
        }
        runs.push(FlagRun {
            start,
            length: previous - start + 1,
        });
        start = slot;
        previous = slot;
    }
    runs.push(FlagRun {
        start,
        length: previous - start + 1,
    });
    runs
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn progression_parses_every_sunrise_table_shape() {
        let document = json!({
            "state": {
                "investment": {
                    "family5_flag_overrides": [[2003, 2]],
                    "family5_value_overrides": [[3510, -5]]
                },
                "unlocks": {
                    "account_flag_runs": [[26, 21], [40, 2]],
                    "profile_flag_runs": [[1, 1]],
                    "character_flags": [16, 17],
                    "objective_values": [[58, 5000]],
                    "character_flag_runs": [[61, 1]],
                    "character_objective_values": [[443, -1]],
                    "account_progressions": [[3, 1, 2, 3], [3, 4, 0, 5]],
                    "character_progressions": [[7, -1, 0, 9]],
                    "future_field": {"preserved": true}
                }
            }
        });

        let policy = parse(&document).unwrap();

        assert_eq!(policy.investment.flag_overrides.len(), 1);
        assert_eq!(policy.investment.value_overrides[0].value, -5);
        assert_eq!(policy.unlocks.account_flag_runs.len(), 2);
        assert_eq!(
            expanded_flag_slots(&policy.unlocks.account_flag_runs, 64).len(),
            21
        );
        assert_eq!(policy.unlocks.character_flags.len(), 2);
        assert_eq!(policy.unlocks.character_objective_values[0].value, -1);
        assert_eq!(
            policy.unlocks.account_progressions,
            [ProgressionValue {
                definition_index: 3,
                lanes: [4, 2, 5],
            }]
        );
        assert_eq!(
            policy.unlocks.character_progressions,
            [ProgressionValue {
                definition_index: 7,
                lanes: [-1, 0, 9],
            }]
        );
    }

    fn collection_state_fixture() -> CollectionStateSnapshot {
        let document = json!({
            "state": {
                "unlocks": {
                    "account_flag_runs": [[10, 1]],
                    "objective_values": [[20, 7]]
                },
                "investment": {
                    "family5_flag_overrides": [[30, 2], [31, 1], [60, 0]],
                    "family5_value_overrides": [[40, -1], [61, 99]]
                }
            }
        });
        collection_state_snapshot(&document).unwrap()
    }

    fn unlock_definition(hash: u64, code: u16, compact_slot: Option<u16>) -> UnlockDefinition {
        UnlockDefinition {
            hash,
            code,
            compact_slot,
            name: None,
            description: None,
            runtime_writers: Vec::new(),
            tested_by: Vec::new(),
        }
    }

    #[test]
    fn collection_state_snapshot_reads_compact_backing() {
        let snapshot = collection_state_fixture();
        let account_flag = unlock_definition(1, 1, Some(10));
        let absent_flag = unlock_definition(1, 1, Some(11));
        let account_value = unlock_definition(2, 1, Some(20));
        let absent_value = unlock_definition(2, 1, Some(21));

        assert_eq!(snapshot.flag_text(0, &account_flag), "Set");
        assert_eq!(snapshot.flag_text(0, &absent_flag), "Unset");
        assert_eq!(snapshot.value_text(0, &account_value), "7");
        assert_eq!(snapshot.value_text(0, &absent_value), "Not listed");
    }

    #[test]
    fn collection_state_snapshot_uses_native_unbacked_defaults() {
        let snapshot = collection_state_fixture();
        let unbanked = unlock_definition(3, 0, None);
        let computed = unlock_definition(3, INVENTORY_COMPUTED_KIND.into(), None);

        assert_eq!(snapshot.flag_value(50, &unbanked), Some(false));
        assert_eq!(snapshot.value(50, &unbanked), Some(0));
        assert_eq!(snapshot.flag_text(50, &unbanked), "Default false");
        assert_eq!(snapshot.value_text(50, &unbanked), "Default 0");
        assert_eq!(snapshot.flag_value(50, &computed), None);
        assert_eq!(snapshot.flag_text(50, &computed), "Computed at runtime");
    }

    #[test]
    fn collection_state_snapshot_applies_family5_overrides() {
        let snapshot = collection_state_fixture();
        let account_flag = unlock_definition(1, 1, Some(10));
        let account_value = unlock_definition(2, 1, Some(20));
        let unbanked = unlock_definition(3, 0, None);

        assert_eq!(snapshot.flag_text(30, &unbanked), "Override 2");
        assert_eq!(snapshot.value_text(40, &unbanked), "Override -1");
        assert_eq!(snapshot.flag_value(31, &unbanked), Some(false));
        assert_eq!(snapshot.flag_value(60, &account_flag), Some(false));
        assert_eq!(snapshot.flag_text(60, &account_flag), "Override 0");
        assert_eq!(snapshot.value(61, &account_value), Some(99));
        assert_eq!(snapshot.value_text(61, &account_value), "Override 99");
    }

    #[test]
    fn progression_accepts_missing_sections_and_unknown_fields() {
        let document = json!({
            "state": {
                "investment": {"future": [1, 2, 3]},
                "unlocks": {"future": [1, 2, 3]}
            }
        });

        assert_eq!(parse(&document).unwrap(), Progression::default());
        assert_eq!(parse(&json!({})).unwrap(), Progression::default());
    }

    #[test]
    fn progression_rejects_rows_sunrise_cannot_parse() {
        let invalid_run = json!({
            "state": {"unlocks": {"profile_flag_runs": [[511, 2]]}}
        });
        assert!(parse(&invalid_run).unwrap_err().contains("512-slot bank"));

        let zero_run = json!({
            "state": {"unlocks": {"account_flag_runs": [[1, 0]]}}
        });
        assert!(parse(&zero_run).unwrap_err().contains("positive length"));

        let invalid_override = json!({
            "state": {"investment": {"family5_flag_overrides": [[23500, 2]]}}
        });
        assert!(parse(&invalid_override).unwrap_err().contains("23499"));

        let invalid_value = json!({
            "state": {"unlocks": {"objective_values": [[1, 2147483648_i64]]}}
        });
        assert!(parse(&invalid_value).unwrap_err().contains("signed 32-bit"));
    }
}

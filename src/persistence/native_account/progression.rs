//! Storage-neutral sparse progression state shared by Dawn and Sunrise.
use serde_json::{Value, json};
use std::collections::BTreeMap;
mod edits;

pub(crate) type UnlockKey = (i32, i32, i32, i32);
const FLAG_SET: i32 = 2;
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Progression {
    pub(crate) unlocks: BTreeMap<UnlockKey, i32>,
    pub(crate) family: BTreeMap<(i32, i32), i32>,
    pub(crate) character_slots: Vec<i32>,
}
const FIELDS: [&str; 8] = [
    "account_flag_runs",
    "profile_flag_runs",
    "character_flags",
    "objective_values",
    "character_flag_runs",
    "character_objective_values",
    "account_progressions",
    "character_progressions",
];
fn scope(bank: i32, character: i32) -> i32 {
    if matches!(bank, 0 | 1 | 3 | 6) {
        -1
    } else {
        character
    }
}
impl Progression {
    pub(crate) fn account_flag_is_set(&self, definition_index: u16, slot: u16) -> bool {
        self.unlocks.get(&(-1, 0, i32::from(slot), 0)) == Some(&FLAG_SET)
            && self
                .family
                .get(&(0, i32::from(definition_index)))
                .is_none_or(|value| *value == FLAG_SET)
    }

    pub(crate) fn set_account_flag(
        &mut self,
        definition_index: u16,
        slot: u16,
    ) -> Result<bool, String> {
        if usize::from(slot) >= crate::account_contract::SHADOWKEEP_ACCOUNT_FLAG_REGION_CAPACITY {
            return Err("The account claim flag is outside the supported bank".to_owned());
        }
        let changed = !self.account_flag_is_set(definition_index, slot);
        self.unlocks.insert((-1, 0, i32::from(slot), 0), FLAG_SET);
        // An existing family-5 override must agree with the native claim checked by Sunrise.
        // Updating it does not consume another override row or change unrelated flags.
        if let Some(value) = self.family.get_mut(&(0, i32::from(definition_index))) {
            *value = FLAG_SET;
        }
        Ok(changed)
    }

    pub(crate) fn account_flags_changed_from(&self, before: &Self) -> bool {
        !self
            .unlocks
            .iter()
            .filter(|((owner, bank, _, _), _)| *owner == -1 && *bank == 0)
            .eq(before
                .unlocks
                .iter()
                .filter(|((owner, bank, _, _), _)| *owner == -1 && *bank == 0))
            || !self
                .family
                .iter()
                .filter(|((kind, _), _)| *kind == 0)
                .eq(before.family.iter().filter(|((kind, _), _)| *kind == 0))
    }

    pub(crate) fn view(&self, character_index: usize) -> Value {
        let character = self
            .character_slots
            .get(character_index)
            .copied()
            .unwrap_or(0);
        let mut view = json!({"version":16,"state":{"unlocks":{},"investment":{}}});
        for (bank, field) in FIELDS.iter().enumerate() {
            let bank = bank as i32;
            let mut slots: BTreeMap<i32, [i32; 3]> = BTreeMap::new();
            for (&(owner, b, slot, lane), &value) in &self.unlocks {
                if owner == scope(bank, character) && bank == b && (0..3).contains(&lane) {
                    slots.entry(slot).or_default()[lane as usize] = value;
                }
            }
            let values = match bank {
                0 | 1 | 4 => compress(
                    slots
                        .iter()
                        .filter_map(|(&slot, lanes)| (lanes[0] == FLAG_SET).then_some(slot))
                        .collect(),
                ),
                2 => slots
                    .iter()
                    .filter_map(|(&slot, lanes)| (lanes[0] == FLAG_SET).then_some(json!(slot)))
                    .collect(),
                3 | 5 => slots
                    .iter()
                    .filter_map(|(&slot, lanes)| (lanes[0] != 0).then_some(json!([slot, lanes[0]])))
                    .collect(),
                _ => slots
                    .iter()
                    .map(|(&slot, lanes)| json!([slot, lanes[0], lanes[1], lanes[2]]))
                    .collect(),
            };
            view["state"]["unlocks"][field] = Value::Array(values);
        }
        for (kind, field) in [
            (0, "family5_flag_overrides"),
            (1, "family5_value_overrides"),
        ] {
            view["state"]["investment"][field] = json!(
                self.family
                    .iter()
                    .filter_map(|(&(k, slot), &value)| (k == kind
                        && visible_family(k, slot, value))
                    .then_some(json!([slot, value])))
                    .collect::<Vec<_>>()
            );
        }
        // Read-only provenance for the editor. Never serialized into the native database.
        view["_native_progression"] = json!({
            "character_slot": character,
            "character_slots": self.character_slots,
            "unlocks": self.unlocks.iter().filter_map(|(&(owner, bank, slot, lane), &value)|
                (owner == scope(bank, character)).then_some([bank, slot, lane, value])
            ).collect::<Vec<_>>(),
            "family": self.family.iter().map(|(&(kind, slot), &value)| [kind, slot, value]).collect::<Vec<_>>(),
            "character_flags": self.unlocks.iter().filter_map(|(&(owner, bank, slot, lane), &value)|
                (bank == 4 && lane == 0).then_some([owner, slot, value])
            ).collect::<Vec<_>>(),
            "hidden_family_counts": ([0, 1].map(|kind| self.family.iter().filter(|entry| {
                let (&(k, slot), &value) = *entry;
                k == kind && !visible_family(k, slot, value)
            }).count())),
        });
        view
    }
    pub(crate) fn apply(&mut self, character_index: usize, after: &Value) -> Result<(), String> {
        let before = self.view(character_index);
        let character = self
            .character_slots
            .get(character_index)
            .copied()
            .unwrap_or(0);
        let mut candidate = self.clone();
        for (bank, field) in FIELDS.iter().enumerate() {
            let old = decode(bank, &before["state"]["unlocks"][field])?;
            let new = decode(bank, &after["state"]["unlocks"][field])?;
            for (&(slot, lane), &value) in &new {
                if old.get(&(slot, lane)) != Some(&value) {
                    candidate.unlocks.insert(
                        (scope(bank as i32, character), bank as i32, slot, lane),
                        value,
                    );
                }
            }
            for &(slot, lane) in old.keys() {
                if !new.contains_key(&(slot, lane)) {
                    candidate.unlocks.remove(&(
                        scope(bank as i32, character),
                        bank as i32,
                        slot,
                        lane,
                    ));
                }
            }
        }
        for (kind, field) in [
            (0, "family5_flag_overrides"),
            (1, "family5_value_overrides"),
        ] {
            let rows = after["state"]["investment"][field]
                .as_array()
                .ok_or_else(invalid)?;
            candidate
                .family
                .retain(|&(k, slot), value| k != kind || !visible_family(k, slot, *value));
            for row in rows {
                candidate
                    .family
                    .insert((kind, integer(&row[0])?), integer(&row[1])?);
            }
        }
        candidate.apply_artifact_seed(self, character, after)?;
        if (0..2).any(|kind| candidate.family.keys().filter(|&&(k, _)| k == kind).count() > 100) {
            return Err(
                "Each native override list supports at most 100 rows, including preserved rows."
                    .to_owned(),
            );
        }
        *self = candidate;
        Ok(())
    }
}
fn visible_family(kind: i32, slot: i32, value: i32) -> bool {
    match kind {
        0 => (0..=23_499).contains(&slot) && (0..=2).contains(&value),
        1 => (0..=15_499).contains(&slot),
        _ => false,
    }
}
fn compress(slots: Vec<i32>) -> Vec<Value> {
    let mut ranges: Vec<(i32, i32)> = Vec::new();
    for slot in slots {
        if let Some((start, length)) = ranges.last_mut()
            && *start + *length == slot
        {
            *length += 1;
        } else {
            ranges.push((slot, 1));
        }
    }
    ranges
        .into_iter()
        .map(|(start, length)| json!([start, length]))
        .collect()
}
fn decode(bank: usize, value: &Value) -> Result<BTreeMap<(i32, i32), i32>, String> {
    let rows = value.as_array().ok_or_else(invalid)?;
    let mut result = BTreeMap::new();
    for row in rows {
        if bank == 2 {
            result.insert((integer(row)?, 0), FLAG_SET);
            continue;
        }
        let slot = integer(&row[0])?;
        if matches!(bank, 0 | 1 | 4) {
            let length = integer(&row[1])?;
            if slot < 0 || length < 0 || slot.checked_add(length).is_none_or(|end| end > 65536) {
                return Err(invalid());
            }
            for i in slot..slot + length {
                result.insert((i, 0), FLAG_SET);
            }
        } else if matches!(bank, 3 | 5) {
            result.insert((slot, 0), integer(&row[1])?);
        } else {
            for lane in 0..3 {
                result.insert((slot, lane), integer(&row[(lane + 1) as usize])?);
            }
        }
    }
    Ok(result)
}
fn integer(value: &Value) -> Result<i32, String> {
    value
        .as_i64()
        .and_then(|v| i32::try_from(v).ok())
        .ok_or_else(invalid)
}
fn invalid() -> String {
    "Invalid progression edit".to_owned()
}

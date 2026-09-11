//! Validate and plan on a clone, then publish one change through the existing account path.
use serde_json::{Value, json};

use crate::{
    catalog::Catalog,
    investment::seasonal::{Definition, Experience},
};

use super::{
    super::{collection_state_snapshot, mutations, parse, state::InvestmentTable, validate},
    rules,
};

#[derive(Clone, Copy, Debug)]
pub(in crate::app) enum Edit {
    Experience(i32),
    Mod { sale_index: u16, owned: bool },
    Reset,
}

pub(in crate::app) fn apply(
    document: &mut Value,
    catalog: &Catalog,
    edit: Edit,
) -> Result<bool, String> {
    if document.get("_native_progression").is_none() {
        return Err("Seasonal authoring requires a current Sunrise SQLite account".into());
    }
    let definition = catalog
        .seasonal()
        .ok_or("The installed seasonal definitions are unavailable")?;
    let snapshot =
        collection_state_snapshot(document).ok_or("The account progression state is invalid")?;
    let overrides = snapshot.artifact_overrides(definition);
    let total = match edit {
        Edit::Experience(total) => total,
        _ => snapshot.seasonal_xp(),
    };
    let experience = definition.experience(total)?;
    let before_mask = snapshot.artifact_mask(definition, true);
    let mask = match edit {
        Edit::Mod {
            sale_index,
            owned: true,
        } => definition.unlock(before_mask, sale_index, experience.points_earned)?,
        Edit::Mod {
            sale_index,
            owned: false,
        } => {
            let entry = definition
                .mods
                .iter()
                .find(|entry| entry.sale_index == sale_index)
                .ok_or("The artifact mod is unavailable")?;
            before_mask & !entry.bit()
        }
        Edit::Reset => 0,
        _ => before_mask,
    };
    if matches!(edit, Edit::Experience(_)) {
        validate_character_budgets(document, definition, experience.points_earned)?;
    }
    let mut candidate = document.clone();
    reconcile_overrides(&mut candidate, definition, &overrides);
    publish(&mut candidate, definition, experience, mask)?;
    validate(&candidate)?;
    let changed = candidate != *document;
    if changed {
        *document = candidate;
    }
    Ok(changed)
}

fn reconcile_overrides(document: &mut Value, definition: &Definition, overrides: &[(usize, u8)]) {
    for &(index, _) in overrides {
        mutations::remove_investment_override(document, InvestmentTable::FlagOverrides, index);
    }
    // Match Sunrise's seed once, preserving the effective ownership of every character.
    // The selected character's requested final ownership is published below.
    if !overrides.is_empty() {
        document["_native_artifact_seed"] = json!({
            "flags": definition.mods.iter().map(|entry| [entry.flag_definition, entry.character_slot]).collect::<Vec<_>>(),
            "overrides": overrides,
        });
    }
}

pub(super) fn character_budgets(document: &Value, definition: &Definition) -> Vec<(usize, u32)> {
    let mut masks = std::collections::BTreeMap::<usize, u32>::new();
    let inherited = collection_state_snapshot(document).map_or(0, |snapshot| {
        snapshot
            .artifact_overrides(definition)
            .into_iter()
            .filter(|&(_, value)| value == 2)
            .filter_map(|(index, _)| definition.mod_for_flag(index))
            .fold(0, |mask, entry| mask | entry.bit())
    });
    for character in document["_native_progression"]["character_slots"]
        .as_array()
        .into_iter()
        .flatten()
    {
        if let Some(character) = character
            .as_u64()
            .and_then(|value| usize::try_from(value).ok())
        {
            masks.insert(character, inherited);
        }
    }
    for row in document["_native_progression"]["character_flags"]
        .as_array()
        .into_iter()
        .flatten()
    {
        if row[2].as_i64() != Some(2) {
            continue;
        }
        let Some(character) = row[0]
            .as_u64()
            .and_then(|value| usize::try_from(value).ok())
        else {
            continue;
        };
        if let Some(entry) = definition
            .mods
            .iter()
            .find(|entry| row[1].as_u64() == Some(u64::from(entry.character_slot)))
        {
            *masks.entry(character).or_default() |= entry.bit();
        }
    }
    masks
        .into_iter()
        .map(|(character, mask)| (character, mask.count_ones()))
        .collect()
}

pub(super) fn validate_character_budgets(
    document: &Value,
    definition: &Definition,
    earned: u16,
) -> Result<(), String> {
    for (character, used) in character_budgets(document, definition) {
        if used > u32::from(earned) {
            return Err(format!(
                "Character {} has {used} artifact mods, but this XP earns {earned} points. Reset or remove mods on that character, or increase XP",
                character + 1
            ));
        }
    }
    Ok(())
}

fn publish(
    document: &mut Value,
    definition: &Definition,
    experience: Experience,
    mask: u32,
) -> Result<(), String> {
    let before = parse(document)?;
    for (index, total) in experience.lanes() {
        let mut lanes = before
            .unlocks
            .account_progressions
            .iter()
            .find(|row| row.definition_index == index)
            .map_or([0; 3], |row| row.lanes);
        lanes[0] = total;
        mutations::set_progression_value(document, "account_progressions", index, lanes);
    }
    for entry in &definition.mods {
        mutations::set_unlock_flag(
            document,
            "character_object_flag_runs",
            usize::from(entry.character_slot),
            mask & entry.bit() != 0,
        );
    }
    let used = mask.count_ones();
    mutations::set_unlock_value(
        document,
        "character_object_objective_values",
        rules::USED_CHARACTER_SLOT,
        used as i32,
    );
    for (index, value) in experience.values(used) {
        mutations::set_investment_override(document, InvestmentTable::ValueOverrides, index, value);
    }
    // A bounded mutation can decline a write. Verify all outputs before exposing the clone.
    let after = parse(document)?;
    for (index, expected) in experience.values(used) {
        if !after
            .investment
            .value_overrides
            .iter()
            .any(|row| row.definition_index == index && row.value == expected)
        {
            return Err("Seasonal counters need room in the 100-row value override list. Remove unrelated overrides first".into());
        }
    }
    for (index, expected) in experience.lanes() {
        if !after
            .unlocks
            .account_progressions
            .iter()
            .any(|row| row.definition_index == index && row.lanes[0] == expected)
        {
            return Err("A seasonal progression could not be updated".into());
        }
    }
    let actual_used = after
        .unlocks
        .character_objective_values
        .iter()
        .find(|row| row.index == rules::USED_CHARACTER_SLOT)
        .map_or(0, |row| row.value);
    if actual_used != used as i32 {
        return Err("The artifact points-used counter could not be updated".into());
    }
    let after = collection_state_snapshot(document)
        .ok_or("The updated seasonal state could not be read")?;
    if after.artifact_mask(definition, false) != mask {
        return Err("Artifact ownership could not be updated".into());
    }
    Ok(())
}

use super::*;

pub(in crate::app) fn collect_class_armor_default_characters(
    document: &Value,
) -> HashMap<u64, usize> {
    let Some(characters) = document
        .pointer("/state/characters")
        .and_then(Value::as_array)
    else {
        return HashMap::new();
    };
    characters
        .iter()
        .enumerate()
        .filter_map(|(character_index, character)| {
            let class_type = character.get("class").and_then(Value::as_u64)?;
            character.get("equipment").and_then(Value::as_object)?;
            Some((class_type, character_index))
        })
        .fold(HashMap::new(), |mut defaults, (class_type, index)| {
            defaults.entry(class_type).or_insert(index);
            defaults
        })
}

#[cfg(test)]
pub(in crate::app) fn collect_class_armor_defaults(
    document: &Value,
) -> HashMap<u64, HashMap<String, Value>> {
    let mut defaults = HashMap::new();
    let Some(characters) = document
        .pointer("/state/characters")
        .and_then(Value::as_array)
    else {
        return defaults;
    };
    for character in characters {
        let Some(class_type) = character.get("class").and_then(Value::as_u64) else {
            continue;
        };
        let Some(equipment) = character.get("equipment").and_then(Value::as_object) else {
            continue;
        };
        let armor = ARMOR_SLOTS
            .iter()
            .filter_map(|slot| {
                equipment
                    .get(*slot)
                    .cloned()
                    .map(|item| ((*slot).into(), item))
            })
            .collect();
        defaults.entry(class_type).or_insert(armor);
    }
    defaults
}

#[cfg(test)]
pub(in crate::app) fn restore_class_armor(
    character: &mut serde_json::Map<String, Value>,
    defaults: &HashMap<String, Value>,
) -> bool {
    let Some(equipment) = character
        .get_mut("equipment")
        .and_then(Value::as_object_mut)
    else {
        return false;
    };
    let mut changed = false;
    for &slot in ARMOR_SLOTS {
        let Some(replacement) = defaults.get(slot) else {
            continue;
        };
        let Some(replacement) = replacement.as_object() else {
            continue;
        };
        let Some(existing) = equipment.get(slot).and_then(Value::as_object) else {
            continue;
        };
        let mut merged = existing.clone();
        for (key, value) in replacement {
            if key != "instance_soid" {
                merged.insert(key.clone(), value.clone());
            }
        }
        let merged = Value::Object(merged);
        if equipment.get(slot) != Some(&merged) {
            equipment.insert(slot.into(), merged);
            changed = true;
        }
    }
    changed
}

pub(in crate::app) const fn default_subclass_name(class_type: u64) -> &'static str {
    match class_type {
        0 => "Sunbreaker",
        1 => "Nightstalker",
        2 => "Dawnblade",
        _ => "",
    }
}

pub(in crate::app) fn selected_attunement_index(
    abilities: &catalog::AbilityOptions,
    super_ability: u64,
    melee: u64,
) -> usize {
    let paths = &abilities.attunements;
    paths
        .iter()
        .position(|path| {
            path.melee.entry == melee
                && path
                    .super_abilities
                    .iter()
                    .any(|choice| choice.entry == super_ability)
        })
        .or_else(|| {
            if super_ability == 10 {
                None
            } else {
                paths.iter().position(|path| {
                    path.super_abilities
                        .iter()
                        .chain(path.perks.iter())
                        .any(|choice| choice.entry == super_ability)
                })
            }
        })
        .or_else(|| paths.iter().position(|path| path.melee.entry == melee))
        .or_else(|| {
            paths.iter().position(|path| {
                path.super_abilities
                    .iter()
                    .any(|choice| choice.entry == super_ability)
            })
        })
        .unwrap_or(0)
}

pub(in crate::app) fn default_ability_values(
    class_type: u64,
    abilities: &catalog::AbilityOptions,
    settings_schema: Option<u64>,
) -> (u64, u64, u64, u64, u64) {
    let pick = |choices: &[AbilityChoice], preferred: u64| {
        choices
            .iter()
            .find(|choice| choice.entry == preferred)
            .or_else(|| choices.first())
            .map_or(preferred, |choice| choice.entry)
    };
    let movement = match class_type {
        0 if settings_schema.is_some_and(|version| version >= 3) => 6,
        0 | 2 => 5,
        1 => 6,
        _ => 4,
    };
    (
        pick(&abilities.movement, movement),
        pick(&abilities.grenade, 7),
        pick(&abilities.super_ability, 10),
        pick(&abilities.melee, 11),
        pick(&abilities.class_ability, 2),
    )
}

pub(in crate::app) const fn class_name(class_type: u64) -> &'static str {
    match class_type {
        0 => "Titan",
        1 => "Hunter",
        2 => "Warlock",
        _ => "Invalid class",
    }
}

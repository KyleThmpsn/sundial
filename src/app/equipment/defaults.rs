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

pub(in crate::app) fn subclass_display_name(item: &ItemDef, show_native_class: bool) -> String {
    if show_native_class && item.bucket_hash == 3_284_755_031 && item.class_type <= 2 {
        format!("{} ({})", item.name, class_name(item.class_type))
    } else {
        item.name.clone()
    }
}

pub(in crate::app) fn item_class_is_compatible(
    item: &ItemDef,
    character_class_type: u64,
    allow_cross_class_subclasses: bool,
) -> bool {
    item.class_type == 3
        || item.class_type == character_class_type
        || (allow_cross_class_subclasses && item.bucket_hash == 3_284_755_031)
}

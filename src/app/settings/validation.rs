//! Workspace structure, character limits, and supported ability-pair validation.
use crate::app::account_workspace as account;

use crate::app::{inventory, progression};
use crate::subclass::shadowkeep_subclass_rules;
use crate::{game_settings, hash::parse_unsigned_value};
use serde_json::Value;
use sundial_account::NO_DEFINITION_HASH;

pub(in crate::app) fn validate_document(document: &Value) -> Result<(), String> {
    game_settings::validate(document)?;
    progression::validate(document)?;
    validate_characters(document)?;
    inventory::validate_document_items(document).map_err(|error| error.to_string())
}

pub(in crate::app) fn validate_workspace_document(
    document: &account::WorkspaceDocument,
) -> Result<(), String> {
    if document.uses_json_account() {
        validate_document(document.json())
    } else {
        game_settings::validate_non_account(document.json())?;
        progression::validate(&document.progression_view(0))
    }
}

pub(in crate::app) fn validate_characters(document: &Value) -> Result<(), String> {
    use crate::account_contract::{
        CHARACTER_CAPACITY as MAX_CHARACTERS, MAX_ITEM_PLUGS as MAX_PLUGS,
    };
    let no_definition_hash = u64::from(NO_DEFINITION_HASH.get());
    let mode = inventory::schema_mode(document);

    let Some(characters_value) = document.pointer("/state/characters") else {
        return Ok(());
    };
    let characters = characters_value
        .as_array()
        .ok_or("state.characters must be an array")?;
    if characters.len() > MAX_CHARACTERS {
        return Err(format!(
            "state.characters cannot contain more than {MAX_CHARACTERS} characters"
        ));
    }
    for (character_index, character) in characters.iter().enumerate() {
        let number = character_index + 1;
        if mode.supports_v13() {
            crate::persistence::json_account::character_runtime::validate_character(character)
                .map_err(|error| format!("Character {number}: {error}"))?;
        }
        let character = character
            .as_object()
            .ok_or_else(|| format!("Character {number} must be an object"))?;
        character
            .get("soid")
            .and_then(parse_unsigned_value)
            .filter(|soid| *soid != 0)
            .ok_or_else(|| format!("Character {number} has an invalid SOID"))?;

        let optional_bounded = |key: &str, label: &str, maximum: u64| {
            let Some(value) = character.get(key) else {
                return Ok(());
            };
            value
                .as_u64()
                .filter(|value| *value <= maximum)
                .map(|_| ())
                .ok_or_else(|| format!("Character {number} has an invalid {label}"))
        };
        optional_bounded("class", "class", 2)?;
        optional_bounded("race", "race", 2)?;
        optional_bounded("gender", "gender", 1)?;
        optional_bounded("level", "level (expected 0 to 255)", u8::MAX.into())?;
        for (key, label) in [
            ("movement_ability", "movement ability"),
            ("grenade_ability", "grenade ability"),
            ("super_ability", "super ability"),
            ("melee_ability", "melee ability"),
            ("class_ability", "class ability"),
        ]
        .into_iter()
        .filter(|_| !mode.supports_v13())
        {
            optional_bounded(key, label, 63)?;
        }
        if game_settings::schema_version(document).is_none_or(|v| v < 16)
            && character
                .get("accepted")
                .is_some_and(|value| !value.is_boolean())
        {
            return Err(format!("Character {number} has an invalid accepted state"));
        }
        crate::persistence::json_account::character_runtime::validate_details(character)
            .map_err(|error| format!("Character {number}: {error}"))?;

        let Some(equipment_value) = character.get("equipment") else {
            continue;
        };
        let equipment = equipment_value
            .as_object()
            .ok_or_else(|| format!("Character {number} equipment must be an object"))?;

        if !mode.supports_v13()
            && let Some(issue) = character_ability_issue(character)
        {
            return Err(format!("Character {number} {issue}"));
        }
        for slot in equipment.keys() {
            if !mode.is_future()
                && !mode
                    .equipment_slots()
                    .iter()
                    .any(|(known, _, _)| known == slot)
            {
                return Err(format!(
                    "Character {number} has an unknown equipment slot: {slot}"
                ));
            }
        }
        for &(slot, label, _) in mode.equipment_slots() {
            let Some(equipped_value) = equipment.get(slot) else {
                continue;
            };
            if equipped_value.is_null() {
                continue;
            }
            let equipped = equipped_value
                .as_object()
                .ok_or_else(|| format!("Character {number} {label} must be an object or null"))?;
            equipped
                .get("definition_hash")
                .and_then(parse_unsigned_value)
                .filter(|hash| u32::try_from(*hash).is_ok() && *hash != no_definition_hash)
                .ok_or_else(|| {
                    format!("Character {number} {label} has an invalid definition hash")
                })?;
            equipped
                .get("instance_soid")
                .and_then(parse_unsigned_value)
                .filter(|soid| *soid != 0)
                .ok_or_else(|| {
                    format!("Character {number} {label} has an invalid instance SOID")
                })?;
            equipped
                .get("level")
                .and_then(Value::as_i64)
                .filter(|level| (0..=i64::from(i32::MAX)).contains(level))
                .ok_or_else(|| format!("Character {number} {label} has an invalid item level"))?;
            equipped
                .get("quantity")
                .and_then(Value::as_i64)
                .filter(|quantity| (1..=i64::from(i32::MAX)).contains(quantity))
                .ok_or_else(|| format!("Character {number} {label} has an invalid quantity"))?;

            match equipped.get("plugs") {
                Some(Value::Null) => {}
                Some(Value::Array(plugs)) => {
                    if plugs.len() > MAX_PLUGS {
                        return Err(format!(
                            "Character {number} {label} cannot contain more than {MAX_PLUGS} plugs"
                        ));
                    }
                    for plug in plugs {
                        if !plug.is_null()
                            && !parse_unsigned_value(plug).is_some_and(|hash| {
                                u32::try_from(hash).is_ok() && hash != no_definition_hash
                            })
                        {
                            return Err(format!(
                                "Character {number} {label} contains an invalid plug hash"
                            ));
                        }
                    }
                }
                _ => {
                    return Err(format!(
                        "Character {number} {label} plugs must be null or an array"
                    ));
                }
            }
            if let Some(flags) = equipped.get("flags") {
                if !mode.supports_equipment_flags() {
                    return Err(format!(
                        "Character {number} {label} flags require settings schema {} or newer",
                        inventory::EQUIPMENT_FLAGS_SCHEMA_VERSION
                    ));
                }
                if parse_unsigned_value(flags)
                    .is_none_or(|flags| flags > u64::from(mode.item_flag_mask()))
                {
                    return Err(format!(
                        "Character {number} {label} flags must be between 0 and {}",
                        mode.item_flag_mask()
                    ));
                }
            }
        }
    }
    Ok(())
}

pub(in crate::app) fn character_ability_issue(
    character: &serde_json::Map<String, Value>,
) -> Option<String> {
    let subclass_hash = character
        .get("equipment")?
        .as_object()?
        .get("subclass")?
        .as_object()?
        .get("definition_hash")
        .and_then(parse_unsigned_value)?;
    character_ability_issue_for_values(
        subclass_hash,
        character.get("movement_ability").and_then(Value::as_u64),
        character.get("grenade_ability").and_then(Value::as_u64),
        character.get("super_ability").and_then(Value::as_u64),
        character.get("melee_ability").and_then(Value::as_u64),
        character.get("class_ability").and_then(Value::as_u64),
    )
}

#[allow(clippy::too_many_arguments)]
pub(in crate::app) fn character_ability_issue_for_values(
    subclass_hash: u64,
    movement_ability: Option<u64>,
    grenade_ability: Option<u64>,
    super_ability: Option<u64>,
    melee_ability: Option<u64>,
    class_ability: Option<u64>,
) -> Option<String> {
    let (subclass_name, middle_super) = shadowkeep_subclass_rules(subclass_hash)?;

    for (value, range, label) in [
        (movement_ability, 4..=6, "movement ability"),
        (grenade_ability, 7..=9, "grenade ability"),
        (class_ability, 2..=3, "class ability"),
    ] {
        if let Some(value) = value
            && !range.contains(&value)
        {
            return Some(format!(
                "has an unsupported {label} entry {value} for {subclass_name}"
            ));
        }
    }

    let (Some(super_ability), Some(melee_ability)) = (super_ability, melee_ability) else {
        return None;
    };
    let supported = [(10, 11), (10, 15), (middle_super, 21)];
    (!supported.contains(&(super_ability, melee_ability))).then(|| {
        format!(
            "has an unsupported super and melee combination ({super_ability}/{melee_ability}) for {subclass_name}; expected 10/11, 10/15, or {middle_super}/21"
        )
    })
}

pub(in crate::app) fn repair_known_ability_pairs(
    document: &mut account::WorkspaceDocument,
) -> Result<usize, String> {
    if document.uses_json_account() && document.supports_v13_account() {
        return Ok(0);
    }
    let mut repairs = Vec::new();
    for character_index in 0..account::character_count(document) {
        let Some(subclass_hash) = account::equipped_item_snapshots(document, character_index)?
            .into_iter()
            .find(|item| item.slot == "subclass")
            .and_then(|item| item.definition_hash)
        else {
            continue;
        };
        let Some((_, middle_super)) = shadowkeep_subclass_rules(subclass_hash) else {
            continue;
        };
        let metadata = account::character_metadata(document, character_index)?;
        let super_ability = u64::from(metadata.abilities.super_ability);
        let melee_ability = u64::from(metadata.abilities.melee);
        let supported = [(10, 11), (10, 15), (middle_super, 21)];
        if supported.contains(&(super_ability, melee_ability)) {
            continue;
        }

        // The melee entry identifies the tree for every Shadowkeep subclass.
        // Prefer it when recovering a mismatched pair, then use a distinctive
        // middle-tree super as a fallback before returning to the top tree.
        let corrected = match melee_ability {
            11 => (10, 11),
            15 => (10, 15),
            21 => (middle_super, 21),
            _ if super_ability == 20 => (middle_super, 21),
            _ => (10, 11),
        };
        repairs.push((character_index, corrected));
    }

    let mut candidate = document.clone();
    for (character_index, (super_ability, melee)) in &repairs {
        account::apply_character_updates(
            &mut candidate,
            *character_index,
            vec![sundial_account::CharacterMetadataUpdate::SetSuperAndMelee {
                super_ability: u8::try_from(*super_ability)
                    .expect("known ability repair entries fit in u8"),
                melee: u8::try_from(*melee).expect("known ability repair entries fit in u8"),
            }],
        )?;
    }
    if !repairs.is_empty() {
        *document = candidate;
    }
    Ok(repairs.len())
}

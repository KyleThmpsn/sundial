//! Frozen test-only behavior of guided character-field JSON writes.

use serde_json::Value;
use sundial_account::CharacterMetadataUpdate;

pub(super) fn apply_updates(
    document: &mut Value,
    character_index: usize,
    updates: Vec<CharacterMetadataUpdate>,
) -> Result<bool, String> {
    let character = document
        .pointer_mut("/state/characters")
        .and_then(Value::as_array_mut)
        .and_then(|characters| characters.get_mut(character_index))
        .and_then(Value::as_object_mut)
        .ok_or_else(|| format!("Character {} must be an object", character_index + 1))?;
    let before = character.clone();
    for update in updates {
        match update {
            CharacterMetadataUpdate::SetAppearanceAndClass {
                race,
                gender,
                class_type,
            } => {
                character.insert("race".into(), Value::from(race));
                character.insert("gender".into(), Value::from(gender));
                character.insert("class".into(), Value::from(class_type));
            }
            CharacterMetadataUpdate::SetAbilities(abilities) => {
                for (field, value) in [
                    ("movement_ability", abilities.movement),
                    ("grenade_ability", abilities.grenade),
                    ("super_ability", abilities.super_ability),
                    ("melee_ability", abilities.melee),
                    ("class_ability", abilities.class_ability),
                ] {
                    character.insert(field.into(), Value::from(value));
                }
            }
            CharacterMetadataUpdate::SetSuperAndMelee {
                super_ability,
                melee,
            } => {
                character.insert("super_ability".into(), Value::from(super_ability));
                character.insert("melee_ability".into(), Value::from(melee));
            }
        }
    }
    Ok(*character != before)
}

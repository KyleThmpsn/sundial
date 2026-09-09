//! Application service for storage-neutral character-field commands.

use serde_json::Value;
use sundial_account::{CharacterCommand, CharacterMetadataUpdate};

use crate::persistence::json_account::JsonCharacterAdapter;

#[cfg(test)]
mod legacy;

pub(super) fn apply_updates(
    document: &mut Value,
    character_index: usize,
    updates: Vec<CharacterMetadataUpdate>,
) -> Result<bool, String> {
    if updates.is_empty() {
        return Ok(false);
    }
    let adapter = JsonCharacterAdapter::load_character_metadata(document, character_index)
        .map_err(|error| error.to_string())?;
    let character_id = adapter
        .character_id_at_index(character_index)
        .ok_or_else(|| format!("Character {} does not exist", character_index + 1))?;
    let mut commands = updates
        .into_iter()
        .map(|update| CharacterCommand::UpdateMetadata {
            character_id,
            update,
        });
    let first = commands
        .next()
        .expect("non-empty character metadata updates have a first command");
    let command = match commands.next() {
        None => first,
        Some(second) => CharacterCommand::Batch(
            std::iter::once(first)
                .chain(std::iter::once(second))
                .chain(commands)
                .collect(),
        ),
    };
    let (_, candidate, _) = adapter
        .apply(document, command)
        .map_err(|error| error.to_string())?;
    let changed = candidate != *document;
    if changed {
        *document = candidate;
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use sundial_account::{CharacterAbilities, CharacterMetadataUpdate};

    use super::{apply_updates, legacy};

    #[test]
    fn production_updates_match_the_frozen_json_behavior() {
        let cases = [
            CharacterMetadataUpdate::SetAppearanceAndClass {
                race: 2,
                gender: 1,
                class_type: 2,
            },
            CharacterMetadataUpdate::SetAbilities(CharacterAbilities {
                movement: 6,
                grenade: 9,
                super_ability: 20,
                melee: 21,
                class_ability: 3,
            }),
            CharacterMetadataUpdate::SetSuperAndMelee {
                super_ability: 10,
                melee: 15,
            },
        ];
        for update in cases {
            let mut production = json!({
                "version": 8,
                "state": {"characters": [{
                    "race": "malformed",
                    "future_character": {"keep": true}
                }]},
                "future_root": {"keep": true}
            });
            let mut expected = production.clone();

            legacy::apply_updates(&mut expected, 0, vec![update]).unwrap();
            apply_updates(&mut production, 0, vec![update]).unwrap();

            assert_eq!(production, expected);
        }
    }

    #[test]
    fn focused_updates_preserve_unknown_character_data() {
        let mut document = json!({
            "version": 8,
            "state": {"characters": [{
                "race": 1,
                "gender": 0,
                "class": 0,
                "movement_ability": 4,
                "grenade_ability": 7,
                "super_ability": 10,
                "melee_ability": 11,
                "class_ability": 2,
                "future_character": {"keep": true},
                "equipment": {"future_slot": {"keep": true}}
            }]},
            "future_root": {"keep": true}
        });

        assert!(
            apply_updates(
                &mut document,
                0,
                vec![CharacterMetadataUpdate::SetAbilities(CharacterAbilities {
                    movement: 6,
                    grenade: 8,
                    super_ability: 20,
                    melee: 21,
                    class_ability: 3,
                })]
            )
            .unwrap()
        );

        assert_eq!(
            document.pointer("/state/characters/0/future_character/keep"),
            Some(&Value::Bool(true))
        );
        assert_eq!(
            document.pointer("/state/characters/0/equipment/future_slot/keep"),
            Some(&Value::Bool(true))
        );
        assert_eq!(
            document.pointer("/future_root/keep"),
            Some(&Value::Bool(true))
        );
    }

    #[test]
    fn invalid_batches_are_atomic() {
        let mut document = json!({
            "version": 8,
            "state": {"characters": [{"race": 0, "gender": 0, "class": 0}]}
        });
        let before = document.clone();

        let error = apply_updates(
            &mut document,
            0,
            vec![
                CharacterMetadataUpdate::SetAppearanceAndClass {
                    race: 2,
                    gender: 1,
                    class_type: 2,
                },
                CharacterMetadataUpdate::SetAbilities(CharacterAbilities {
                    movement: 64,
                    grenade: 7,
                    super_ability: 10,
                    melee: 11,
                    class_ability: 2,
                }),
            ],
        )
        .unwrap_err();

        assert_eq!(error, "the selected character metadata is invalid");
        assert_eq!(document, before);
    }

    #[test]
    fn missing_fields_materialize_the_legacy_display_fallbacks() {
        let mut document = json!({"version": 8, "state": {"characters": [{}]}});

        apply_updates(
            &mut document,
            0,
            vec![
                CharacterMetadataUpdate::SetAppearanceAndClass {
                    race: 0,
                    gender: 0,
                    class_type: 0,
                },
                CharacterMetadataUpdate::SetAbilities(CharacterAbilities {
                    movement: 4,
                    grenade: 7,
                    super_ability: 10,
                    melee: 11,
                    class_ability: 2,
                }),
            ],
        )
        .unwrap();

        assert_eq!(
            document.pointer("/state/characters/0/race"),
            Some(&json!(0))
        );
        assert_eq!(
            document.pointer("/state/characters/0/class_ability"),
            Some(&json!(2))
        );
    }
}

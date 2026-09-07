use crate::app::account_workspace as account;

use crate::app::inventory::EQUIPMENT_FLAGS_SCHEMA_VERSION;
use crate::app::settings::character_ability_issue;
use crate::app::settings::repair_known_ability_pairs;
use crate::app::settings::validate_characters;
use crate::app::*;
use crate::hash::format_hash_hex;

#[test]
fn character_validation_accepts_sunrise_native_forms() {
    let document = serde_json::json!({
        "state": {
            "characters": [{
                "soid": 1,
                "level": 67,
                "equipment": {
                    "kinetic": {
                        "instance_soid": "0x0000000000000002",
                        "definition_hash": 42,
                        "level": 106,
                        "quantity": 1,
                        "plugs": null
                    },
                    "energy": {
                        "instance_soid": 3,
                        "definition_hash": "0x0000002B",
                        "level": 106,
                        "quantity": 1,
                        "plugs": [null, 44, "0x0000002D"]
                    },
                    "heavy": null
                }
            }]
        }
    });

    assert_eq!(validate_characters(&document), Ok(()));
}

#[test]
fn all_shadowkeep_ability_combinations_validate() {
    let subclasses = [
        (0xB055_4739_u64, 20),
        (0xB920_CE9A, 20),
        (0xC99B_33E9, 10),
        (0xD8B8_D1FC, 20),
        (0x4F91_DC97, 10),
        (0xC048_3D8B, 20),
        (0xCF88_FEA5, 20),
        (0x686A_154A, 20),
        (0xE7BC_88B0, 20),
    ];

    for (subclass_hash, middle_super) in subclasses {
        for movement in 4..=6 {
            for grenade in 7..=9 {
                for class_ability in 2..=3 {
                    for (super_ability, melee_ability) in [(10, 11), (10, 15), (middle_super, 21)] {
                        let document = character_with_abilities(
                            subclass_hash,
                            movement,
                            grenade,
                            super_ability,
                            melee_ability,
                            class_ability,
                        );
                        assert_eq!(validate_characters(&document), Ok(()));
                    }
                }
            }
        }
    }
}

#[test]
fn guard_subclasses_reject_entry_twenty_as_the_super() {
    for subclass_hash in [0x4F91_DC97, 0xC99B_33E9] {
        let document = character_with_abilities(subclass_hash, 6, 8, 20, 21, 3);
        assert!(validate_characters(&document).is_err());
        let warning = document
            .pointer("/state/characters/0")
            .and_then(Value::as_object)
            .and_then(character_ability_issue)
            .unwrap();
        assert!(warning.contains("unsupported super and melee combination (20/21)"));
        assert!(warning.contains("expected 10/11, 10/15, or 10/21"));
    }
}

#[test]
fn every_known_super_and_melee_pair_is_valid_after_save_repair() {
    let subclasses = [
        (0xB055_4739_u64, 20),
        (0xB920_CE9A, 20),
        (0xC99B_33E9, 10),
        (0xD8B8_D1FC, 20),
        (0x4F91_DC97, 10),
        (0xC048_3D8B, 20),
        (0xCF88_FEA5, 20),
        (0x686A_154A, 20),
        (0xE7BC_88B0, 20),
    ];

    for (subclass_hash, middle_super) in subclasses {
        let supported = [(10, 11), (10, 15), (middle_super, 21)];
        for super_ability in 0..=63 {
            for melee_ability in 0..=63 {
                let mut raw_document =
                    character_with_abilities(subclass_hash, 6, 8, super_ability, melee_ability, 3);
                raw_document["future_data"] = serde_json::json!({"keep": true});
                let mut document = account::WorkspaceDocument::json_only(raw_document);

                let repaired = repair_known_ability_pairs(&mut document).unwrap();
                let was_supported = supported.contains(&(super_ability, melee_ability));
                assert_eq!(repaired, usize::from(!was_supported));
                assert_eq!(validate_characters(&document), Ok(()));
                assert_eq!(
                    document.pointer("/future_data/keep"),
                    Some(&Value::Bool(true))
                );
            }
        }
    }
}

#[test]
fn unknown_subclasses_keep_loose_ability_validation() {
    let raw_document = character_with_abilities(0x1234_5678, 12, 13, 14, 15, 16);
    assert_eq!(validate_characters(&raw_document), Ok(()));
    let mut document = account::WorkspaceDocument::json_only(raw_document);
    let original = document.clone();
    assert_eq!(repair_known_ability_pairs(&mut document,).unwrap(), 0);
    assert_eq!(document, original);
}

fn character_with_abilities(
    subclass_hash: u64,
    movement: u64,
    grenade: u64,
    super_ability: u64,
    melee: u64,
    class_ability: u64,
) -> Value {
    serde_json::json!({
        "version": 8,
        "state": {
            "characters": [{
                "soid": "0x1",
                "movement_ability": movement,
                "grenade_ability": grenade,
                "super_ability": super_ability,
                "melee_ability": melee,
                "class_ability": class_ability,
                "equipment": {
                    "subclass": {
                        "instance_soid": "0x2",
            "definition_hash": format_hash_hex(subclass_hash),
                        "level": 0,
                        "quantity": 1,
                        "plugs": []
                    }
                }
            }]
        }
    })
}

#[test]
fn character_validation_keeps_sunrise_limits() {
    let mut document = serde_json::json!({
        "version": 6,
        "state": {
            "characters": [{
                "soid": "0x1",
                "level": 256,
                "equipment": {}
            }]
        }
    });
    assert!(validate_characters(&document).is_err());

    *document.pointer_mut("/state/characters/0/level").unwrap() = Value::from(255);
    document
        .pointer_mut("/state/characters/0/equipment")
        .unwrap()["kinetic"] = serde_json::json!({
        "instance_soid": "0x2",
        "definition_hash": "0x2A",
        "level": 106,
        "quantity": 1,
        "plugs": [null, null, null, null, null, null, null, null, null, null, null, null, null]
    });
    assert!(validate_characters(&document).is_err());

    document
        .pointer_mut("/state/characters/0/equipment/kinetic/plugs")
        .unwrap()
        .clone_from(&serde_json::json!([]));
    document
        .pointer_mut("/state/characters/0/equipment/kinetic")
        .unwrap()["flags"] = Value::String("0x3".into());
    assert_eq!(validate_characters(&document), Ok(()));
    document
        .pointer_mut("/state/characters/0/equipment/kinetic/flags")
        .unwrap()
        .clone_from(&Value::String("0x4".into()));
    assert!(validate_characters(&document).is_err());
}

#[test]
fn equipped_flags_follow_the_schema_four_introduction() {
    for version in 2..=6 {
        let document = serde_json::json!({
            "version": version,
            "state": {
                "characters": [{
                    "soid": "0x1",
                    "equipment": {
                        "kinetic": {
                            "instance_soid": "0x2",
                            "definition_hash": "0x2A",
                            "level": 106,
                            "quantity": 1,
                            "plugs": null,
                            "flags": 3
                        }
                    }
                }]
            }
        });
        let result = validate_characters(&document);
        if version < EQUIPMENT_FLAGS_SCHEMA_VERSION {
            assert!(
                result.is_err(),
                "schema {version} unexpectedly accepted flags"
            );
        } else {
            assert_eq!(result, Ok(()), "schema {version} rejected valid flags");
        }
    }
}

#[test]
fn future_schema_character_validation_ignores_unknown_equipment_slots() {
    let mut document = serde_json::json!({
        "version": crate::game_settings::MAX_SUPPORTED_SCHEMA,
        "state": {
            "characters": [{
                "soid": "0x1",
                "equipment": {
                    "future_slot": {
                        "opaque": {"keep": [1, 2, 3]}
                    }
                }
            }]
        }
    });

    assert!(validate_characters(&document).is_err());
    document["version"] = Value::from(crate::game_settings::MAX_SUPPORTED_SCHEMA + 1);
    let before = document.clone();
    assert_eq!(validate_characters(&document), Ok(()));
    assert_eq!(document, before);
}

#[test]
fn character_validation_checks_presence_gated_sunrise_scalars() {
    let valid = serde_json::json!({
        "state": {
            "characters": [{
                "soid": "0x1",
                "accepted": true,
                "preview_available": false,
                "appearance_value": -12.5,
                "last_orbited_destination": "0xFFFFFFFF",
                "content_bypass": true,
                "future_character_data": {"preserved": true}
            }]
        }
    });
    let before = valid.clone();
    assert_eq!(validate_characters(&valid), Ok(()));
    assert_eq!(valid, before);

    for (key, invalid) in [
        ("accepted", Value::from(1)),
        ("preview_available", Value::Null),
        ("content_bypass", Value::String("true".into())),
        ("appearance_value", Value::String("1.0".into())),
        ("appearance_value", Value::from(f64::from(f32::MAX) * 2.0)),
        (
            "last_orbited_destination",
            Value::from(u64::from(u32::MAX) + 1),
        ),
    ] {
        let mut candidate = valid.clone();
        candidate
            .pointer_mut("/state/characters/0")
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert(key.into(), invalid);
        assert!(
            validate_characters(&candidate).is_err(),
            "{key} unexpectedly validated"
        );
    }

    let optional_members_absent = serde_json::json!({
        "state": {"characters": [{"soid": 1, "future_character_data": true}]}
    });
    assert_eq!(validate_characters(&optional_members_absent), Ok(()));
}

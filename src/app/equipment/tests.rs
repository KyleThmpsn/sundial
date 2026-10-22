mod loadout_safety;

use serde_json::json;

use super::*;

#[test]
fn future_schema_equipment_edits_preserve_unknown_members() {
    let mut document = json!({
        "version": crate::game_settings::MAX_SUPPORTED_SCHEMA,
        "state": {
            "characters": [{
                "equipment": {
                    "kinetic": {
                        "instance_soid": "0x0000000000000001",
                        "definition_hash": "0x00000002",
                        "level": 75,
                        "quantity": 1,
                        "plugs": null,
                        "future_item_data": {"keep": [1, 2, 3]}
                    }
                }
            }]
        }
    });

    let supported = equipped_item_snapshots(&document, 0).unwrap();
    assert!(
        supported[0]
            .issues
            .iter()
            .any(|issue| issue.contains("unknown item member"))
    );

    document["version"] = Value::from(crate::game_settings::MAX_SUPPORTED_SCHEMA + 1);
    let future = equipped_item_snapshots(&document, 0).unwrap();
    assert!(future[0].issues.is_empty());

    set_equipment_item_level(&mut document, 0, "kinetic", 106).unwrap();
    assert_eq!(
        document.pointer("/state/characters/0/equipment/kinetic/level"),
        Some(&Value::from(106))
    );
    assert_eq!(
        document.pointer("/state/characters/0/equipment/kinetic/future_item_data/keep"),
        Some(&json!([1, 2, 3]))
    );

    *document
        .pointer_mut("/state/characters/0/equipment/kinetic/quantity")
        .unwrap() = json!({"future_quantity_shape": 1});
    let incompatible_known_field = equipped_item_snapshots(&document, 0).unwrap();
    assert!(
        incompatible_known_field[0]
            .issues
            .iter()
            .any(|issue| issue.contains("quantity must be"))
    );
    assert!(
        incompatible_known_field[0]
            .issues
            .iter()
            .all(|issue| !issue.contains("unknown item member"))
    );
}

#[test]
fn semantic_equipment_field_edits_do_not_touch_stored_inventory() {
    let mut document = json!({
        "version": 6,
        "state": {
            "characters": [{
                "equipment": {
                    "kinetic": {"level": 200, "flags": 2},
                    "energy": {"level": 106}
                },
                "inventory": [{"level": 75, "flags": 1}]
            }]
        }
    });

    set_equipment_item_level(&mut document, 0, "kinetic", 75).unwrap();
    let locked = super::super::inventory::set_inventory_locked_flag(Some(2), true);
    set_equipment_item_flags(&mut document, 0, "kinetic", locked).unwrap();

    assert_eq!(
        document.pointer("/state/characters/0/equipment/kinetic/level"),
        Some(&json!(75))
    );
    assert_eq!(
        document.pointer("/state/characters/0/equipment/kinetic/flags"),
        Some(&json!(3))
    );
    assert_eq!(
        document.pointer("/state/characters/0/equipment/energy/level"),
        Some(&json!(106))
    );
    assert_eq!(
        document.pointer("/state/characters/0/inventory/0"),
        Some(&json!({"level": 75, "flags": 1}))
    );

    let unchanged = document.clone();
    assert!(set_equipment_item_level(&mut document, 0, "future_slot", 75).is_err());
    assert_eq!(document, unchanged);
    assert!(set_equipment_item_flags(&mut document, 0, "kinetic", Some(8)).is_err());
    assert_eq!(document, unchanged);
}

#[test]
fn equipment_flag_mutation_follows_schema_introduction_and_is_atomic() {
    for version in 2..=6 {
        let mut document = json!({
            "version": version,
            "state": {
                "characters": [{
                    "equipment": {
                        "kinetic": {
                            "level": 106,
                            "flags": 2,
                            "future": {"preserved": true}
                        }
                    }
                }]
            }
        });
        let before = document.clone();
        let result = set_equipment_item_flags(&mut document, 0, "kinetic", Some(3));

        if version < 4 {
            assert!(
                result.is_err(),
                "schema {version} unexpectedly allowed flags"
            );
            assert_eq!(document, before);
        } else {
            result.unwrap();
            assert_eq!(
                document.pointer("/state/characters/0/equipment/kinetic/flags"),
                Some(&json!(3))
            );
            assert_eq!(
                document.pointer("/state/characters/0/equipment/kinetic/future/preserved"),
                Some(&Value::Bool(true))
            );
        }
    }
}

fn assert_default_subclass_abilities(
    document: &super::super::account_workspace::WorkspaceDocument,
) {
    for (field, expected) in [
        ("movement_ability", 6),
        ("grenade_ability", 7),
        ("super_ability", 10),
        ("melee_ability", 11),
        ("class_ability", 2),
    ] {
        assert_eq!(
            document.pointer(&format!("/state/characters/0/{field}")),
            Some(&json!(expected))
        );
    }
}

#[test]
fn subclass_equipping_updates_definition_and_default_abilities_atomically() {
    let mut document = super::super::account_workspace::WorkspaceDocument::json_only(json!({
        "version": 6,
        "state": {
            "characters": [{
                "class": 0,
                "movement_ability": 99,
                "grenade_ability": 99,
                "super_ability": 99,
                "melee_ability": 99,
                "class_ability": 99,
                "equipment": {
                    "subclass": {
                        "instance_soid": "0x0000000000000001",
                        "definition_hash": "0x00000001",
                        "level": 0,
                        "quantity": 1,
                        "plugs": null
                    }
                }
            }]
        }
    }));
    let choice = |entry, name: &str| AbilityChoice {
        entry,
        name: name.to_owned(),
    };
    let item = ItemDef {
        hash: 42,
        name: "Test subclass".to_owned(),
        type_name: "Subclass".to_owned(),
        bucket_hash: 3_284_755_031,
        class_type: 0,
        default_plugs: vec![Some("0x0000000A".to_owned()), None],
        sockets: Vec::new(),
        abilities: catalog::AbilityOptions {
            movement: vec![choice(6, "Lift")],
            grenade: vec![choice(7, "Grenade")],
            super_ability: vec![choice(10, "Super")],
            melee: vec![choice(11, "Melee")],
            class_ability: vec![choice(2, "Barricade")],
            attunements: Vec::new(),
            entry_perks: Default::default(),
            entry_icons: Default::default(),
            entry_descriptions: Default::default(),
            entry_entities: Default::default(),
            entry_rows: Default::default(),
            entry_modifiers: Default::default(),
            authored: false,
        },
    };

    equip_subclass_with_default_abilities(&mut document, 0, &item, false).unwrap();
    assert_eq!(
        document.pointer("/state/characters/0/equipment/subclass/definition_hash"),
        Some(&json!("0x0000002A"))
    );
    assert_eq!(
        document.pointer("/state/characters/0/equipment/subclass/plugs"),
        Some(&json!(["0x0000000A", null]))
    );
    assert_default_subclass_abilities(&document);

    let previous_subclass = document
        .pointer("/state/characters/0/equipment/subclass")
        .unwrap()
        .clone();
    let character = document
        .json_mut()
        .pointer_mut("/state/characters/0")
        .and_then(Value::as_object_mut)
        .unwrap();
    character.insert(
        "inventory".into(),
        json!([{
            "instance_soid": "0x0000000000000002",
            "definition_hash": "0x0000002A",
            "level": 0,
            "quantity": 1,
            "plugs": ["0x0000000B", null]
        }]),
    );
    for field in [
        "movement_ability",
        "grenade_ability",
        "super_ability",
        "melee_ability",
        "class_ability",
    ] {
        character.insert(field.into(), Value::from(99));
    }

    assert!(
        equip_inventory_item(
            &mut document,
            super::super::inventory::InventoryItemLocation {
                character_index: 0,
                item_index: 0,
            },
            "subclass",
            &item,
            false,
        )
        .unwrap()
    );
    assert_eq!(
        document.pointer("/state/characters/0/equipment/subclass/instance_soid"),
        Some(&json!("0x0000000000000002"))
    );
    assert_eq!(
        document.pointer("/state/characters/0/inventory/0"),
        Some(&previous_subclass)
    );
    assert_default_subclass_abilities(&document);

    let unchanged = document.clone();
    let mut wrong_bucket = item.clone();
    wrong_bucket.bucket_hash = 0;
    assert!(equip_subclass_with_default_abilities(&mut document, 0, &wrong_bucket, true,).is_err());
    assert_eq!(document, unchanged);

    let mut malformed = super::super::account_workspace::WorkspaceDocument::json_only(json!({
        "version": 6,
        "state": {"characters": [{"class": 0, "equipment": []}]}
    }));
    let original = malformed.clone();
    assert!(equip_subclass_with_default_abilities(&mut malformed, 0, &item, false,).is_err());
    assert_eq!(malformed, original);
}

#[test]
fn cross_class_override_controls_definition_and_stored_subclass_equips() {
    let mut document = super::super::account_workspace::WorkspaceDocument::json_only(json!({
        "version": 6,
        "state": {
            "characters": [{
                "class": 0,
                "movement_ability": 99,
                "grenade_ability": 99,
                "super_ability": 99,
                "melee_ability": 99,
                "class_ability": 99,
                "equipment": {
                    "subclass": {
                        "instance_soid": "0x0000000000000001",
                        "definition_hash": "0x00000001",
                        "level": 0,
                        "quantity": 1,
                        "plugs": null
                    }
                },
                "inventory": [{
                    "instance_soid": "0x0000000000000002",
                    "definition_hash": "0x0000002B",
                    "level": 0,
                    "quantity": 1,
                    "plugs": null
                }]
            }]
        }
    }));
    let item = ItemDef {
        hash: 42,
        name: "Foreign subclass".to_owned(),
        type_name: "Subclass".to_owned(),
        bucket_hash: 3_284_755_031,
        class_type: 1,
        default_plugs: Vec::new(),
        sockets: Vec::new(),
        abilities: catalog::AbilityOptions::default(),
    };

    let unchanged = document.clone();
    assert!(equip_subclass_with_default_abilities(&mut document, 0, &item, false,).is_err());
    assert_eq!(document, unchanged);
    equip_subclass_with_default_abilities(&mut document, 0, &item, true).unwrap();
    assert_eq!(
        document.pointer("/state/characters/0/equipment/subclass/definition_hash"),
        Some(&json!("0x0000002A"))
    );
    assert_default_subclass_abilities(&document);

    let stored_subclass = super::super::inventory::InventoryItemLocation {
        character_index: 0,
        item_index: 0,
    };
    let mut stored_item = item;
    stored_item.hash = 43;
    let unchanged = document.clone();
    assert!(
        equip_inventory_item(
            &mut document,
            stored_subclass,
            "subclass",
            &stored_item,
            false,
        )
        .is_err()
    );
    assert_eq!(document, unchanged);
    assert!(
        equip_inventory_item(
            &mut document,
            stored_subclass,
            "subclass",
            &stored_item,
            true,
        )
        .unwrap()
    );
    assert_eq!(
        document.pointer("/state/characters/0/equipment/subclass/definition_hash"),
        Some(&json!("0x0000002B"))
    );
}

#[test]
fn arcstrider_and_sentinel_subclass_edits_keep_the_base_super_lane() {
    let choice = |entry, name: &str| AbilityChoice {
        entry,
        name: name.to_owned(),
    };
    for (hash, class_type, name) in [(0x4F91_DC97, 1, "Arcstrider"), (0xC99B_33E9, 0, "Sentinel")] {
        let mut document = super::super::account_workspace::WorkspaceDocument::json_only(json!({
            "version": 6,
            "state": {
                "characters": [{
                    "soid": "0x9EAA300200100100",
                    "class": class_type,
                    "movement_ability": 4,
                    "grenade_ability": 9,
                    "super_ability": 20,
                    "melee_ability": 21,
                    "class_ability": 3,
                    "equipment": {
                        "subclass": {
                            "instance_soid": "0x4000000000000001",
                            "definition_hash": "0x00000001",
                            "level": 0,
                            "quantity": 1,
                            "plugs": null
                        }
                    }
                }]
            }
        }));
        let item = ItemDef {
            hash,
            name: name.to_owned(),
            type_name: "Subclass".to_owned(),
            bucket_hash: 3_284_755_031,
            class_type,
            default_plugs: Vec::new(),
            sockets: Vec::new(),
            abilities: catalog::AbilityOptions {
                movement: vec![choice(4, "Movement"), choice(6, "Preferred movement")],
                grenade: vec![choice(7, "Grenade")],
                // Put the Forsaken middle-path entries first to prove the helper still
                // selects the base-super/base-melee pair for these guard subclasses.
                super_ability: vec![choice(20, "Guard"), choice(10, "Base super")],
                melee: vec![choice(21, "Middle melee"), choice(11, "Base melee")],
                class_ability: vec![choice(2, "Class ability")],
                attunements: Vec::new(),
                entry_perks: Default::default(),
                entry_icons: Default::default(),
                entry_descriptions: Default::default(),
                entry_entities: Default::default(),
                entry_rows: Default::default(),
                entry_modifiers: Default::default(),
                authored: false,
            },
        };

        equip_subclass_with_default_abilities(&mut document, 0, &item, false).unwrap();
        assert_eq!(
            document.pointer("/state/characters/0/super_ability"),
            Some(&json!(10)),
            "{name} selected the wrong super lane"
        );
        assert_eq!(
            document.pointer("/state/characters/0/melee_ability"),
            Some(&json!(11)),
            "{name} selected the wrong melee lane"
        );
        assert_eq!(
            super::super::settings::validate_characters(&document),
            Ok(()),
            "{name} produced an invalid character"
        );
    }
}

#[test]
fn class_armor_restore_copies_opaque_fields_and_preserves_destination_identity() {
    let mut document = json!({
        "version": 8,
        "state": {"characters": [
            {
                "class": 1,
                "equipment": {
                    "helmet": {
                        "instance_soid": "0x0000000000000001",
                        "definition_hash": "0x0000002A",
                        "level": 106,
                        "quantity": 1,
                        "plugs": null,
                        "future_source": {"copy": true}
                    }
                }
            },
            {
                "class": 0,
                "equipment": {
                    "helmet": {
                        "instance_soid": "0x0000000000000002",
                        "definition_hash": "0x0000000B",
                        "level": 100,
                        "quantity": 1,
                        "plugs": [],
                        "future_destination": {"keep": true}
                    }
                }
            }
        ]}
    });

    assert!(restore_class_armor_from_character(&mut document, 0, 1).unwrap());
    assert_eq!(
        document.pointer("/state/characters/1/equipment/helmet/instance_soid"),
        Some(&json!("0x0000000000000002"))
    );
    assert_eq!(
        document.pointer("/state/characters/1/equipment/helmet/definition_hash"),
        Some(&json!("0x0000002A"))
    );
    assert_eq!(
        document.pointer("/state/characters/1/equipment/helmet/level"),
        Some(&json!(106))
    );
    assert_eq!(
        document.pointer("/state/characters/1/equipment/helmet/future_source/copy"),
        Some(&Value::Bool(true))
    );
    assert_eq!(
        document.pointer("/state/characters/1/equipment/helmet/future_destination/keep"),
        Some(&Value::Bool(true))
    );
}

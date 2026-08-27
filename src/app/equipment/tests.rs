use serde_json::json;

use super::*;

#[test]
fn equipment_picker_choice_assembly_keeps_more_than_five_hundred_items() {
    let items = (0_u64..620)
        .map(|index| ItemDef {
            hash: 10_000 + index,
            name: format!("Browse item {index:04}"),
            type_name: "Test weapon".into(),
            bucket_hash: 1_498_876_634,
            class_type: 3,
            default_plugs: Vec::new(),
            sockets: Vec::new(),
            abilities: catalog::AbilityOptions::default(),
        })
        .collect::<Vec<_>>();

    let choices = equipment_definition_choices(items.iter());
    assert_eq!(choices.len(), 620);
    assert_eq!(choices.first().unwrap().hash, 10_000);
    assert_eq!(choices.last().unwrap().hash, 10_619);
}

fn assert_primary_equipped_snapshots(snapshots: &[EquippedItemSnapshot]) {
    assert_eq!(
        snapshots
            .iter()
            .map(|snapshot| snapshot.slot)
            .collect::<Vec<_>>(),
        ["kinetic", "helmet", "subclass", "emote"]
    );
    let kinetic = &snapshots[0];
    assert_eq!(kinetic.slot_label, "Kinetic");
    assert_eq!(kinetic.bucket_hash, 1_498_876_634);
    assert_eq!(kinetic.definition_hash, Some(2));
    assert_eq!(kinetic.definition_text, "0x00000002");
    assert_eq!(kinetic.instance_soid, Some(1));
    assert_eq!(kinetic.instance_soid_text, "0x0000000000000001");
    assert_eq!(kinetic.level, Some(75));
    assert_eq!(kinetic.quantity, Some(1));
    assert_eq!(kinetic.plugs, EquippedItemPlugs::NativeDefaults);
    assert!(kinetic.issues.is_empty());
}

fn assert_secondary_equipped_snapshots(snapshots: &[EquippedItemSnapshot]) {
    assert_eq!(
        snapshots[1].plugs,
        EquippedItemPlugs::Authored(vec![
            EquippedPlugValue::Empty,
            EquippedPlugValue::Hash(6),
            EquippedPlugValue::Hash(7),
            EquippedPlugValue::Malformed("\"not-a-hash\"".to_owned()),
        ])
    );
    assert!(
        snapshots[1]
            .issues
            .iter()
            .any(|issue| issue.contains("plug 3"))
    );
    assert_eq!(snapshots[2].slot, "subclass");
    assert_eq!(snapshots[3].raw_item_text, "true");
    assert_eq!(snapshots[3].definition_hash, None);
    assert!(matches!(
        snapshots[3].plugs,
        EquippedItemPlugs::Malformed(ref raw) if raw == "true"
    ));
    assert_eq!(snapshots[3].issues, ["equipment row must be an object"]);
}

#[test]
fn equipped_snapshots_follow_slot_order_and_skip_missing_or_null_rows() {
    let document = json!({
        "state": {
            "characters": [{
                "equipment": {
                    "emote": true,
                    "subclass": {
                        "instance_soid": "0x0000000000000004",
                        "definition_hash": "0x00000005",
                        "level": 75,
                        "quantity": 1,
                        "plugs": []
                    },
                    "energy": null,
                    "helmet": {
                        "instance_soid": 3,
                        "definition_hash": 4,
                        "level": 76,
                        "quantity": 1,
                        "plugs": [null, "0x00000006", 7, "not-a-hash"]
                    },
                    "kinetic": {
                        "instance_soid": "0x0000000000000001",
                        "definition_hash": "0x00000002",
                        "level": 75,
                        "quantity": 1,
                        "plugs": null
                    }
                }
            }]
        }
    });

    let snapshots = equipped_item_snapshots(&document, 0).unwrap();
    assert_primary_equipped_snapshots(&snapshots);
    assert_secondary_equipped_snapshots(&snapshots);
}

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
fn equipped_snapshots_retain_invalid_fields_and_report_issues() {
    let document = json!({
        "state": {
            "characters": [{
                "equipment": {
                    "kinetic": {
                        "instance_soid": 0,
                        "definition_hash": "invalid",
                        "level": -1,
                        "quantity": 0,
                        "plugs": {"unexpected": true}
                    }
                }
            }]
        }
    });

    let snapshots = equipped_item_snapshots(&document, 0).unwrap();
    let snapshot = &snapshots[0];
    assert_eq!(snapshot.definition_hash, None);
    assert_eq!(snapshot.definition_text, "invalid");
    assert_eq!(snapshot.instance_soid, Some(0));
    assert_eq!(snapshot.level, Some(-1));
    assert_eq!(snapshot.quantity, Some(0));
    assert_eq!(
        snapshot.plugs,
        EquippedItemPlugs::Malformed("{\"unexpected\":true}".to_owned())
    );
    assert_eq!(snapshot.issues.len(), 5);
}

#[test]
fn inferred_item_level_uses_the_highest_positive_equipped_level() {
    let document = json!({
        "state": {
            "characters": [{
                "equipment": {
                    "ghost": {"level": 0},
                    "kinetic": {"level": 75},
                    "energy": {"level": 106},
                    "helmet": {"level": -1}
                }
            }]
        }
    });
    assert_eq!(inferred_item_level(&document, 0), 106);

    let unpowered = json!({
        "state": {"characters": [{"equipment": {"ghost": {"level": 0}}}]}
    });
    assert_eq!(inferred_item_level(&unpowered, 0), 106);
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

        if version < super::super::inventory::EQUIPMENT_FLAGS_SCHEMA_VERSION {
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
        },
    };

    equip_subclass_with_default_abilities(
        super::super::account_workspace::AccountWorkspace::json(),
        &mut document,
        0,
        &item,
    )
    .unwrap();
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
            super::super::account_workspace::AccountWorkspace::json(),
            &mut document,
            super::super::inventory::InventoryItemLocation {
                character_index: 0,
                item_index: 0,
            },
            "subclass",
            &item,
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
    assert!(
        equip_subclass_with_default_abilities(
            super::super::account_workspace::AccountWorkspace::json(),
            &mut document,
            0,
            &wrong_bucket,
        )
        .is_err()
    );
    assert_eq!(document, unchanged);

    let mut wrong_class = item.clone();
    wrong_class.class_type = 1;
    assert!(
        equip_subclass_with_default_abilities(
            super::super::account_workspace::AccountWorkspace::json(),
            &mut document,
            0,
            &wrong_class,
        )
        .is_err()
    );
    assert_eq!(document, unchanged);

    let mut malformed = super::super::account_workspace::WorkspaceDocument::json_only(json!({
        "version": 6,
        "state": {"characters": [{"class": 0, "equipment": []}]}
    }));
    let original = malformed.clone();
    assert!(
        equip_subclass_with_default_abilities(
            super::super::account_workspace::AccountWorkspace::json(),
            &mut malformed,
            0,
            &item,
        )
        .is_err()
    );
    assert_eq!(malformed, original);
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
            },
        };

        equip_subclass_with_default_abilities(
            super::super::account_workspace::AccountWorkspace::json(),
            &mut document,
            0,
            &item,
        )
        .unwrap();
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
        document.pointer("/state/characters/1/equipment/helmet/future_source/copy"),
        Some(&Value::Bool(true))
    );
    assert_eq!(
        document.pointer("/state/characters/1/equipment/helmet/future_destination/keep"),
        Some(&Value::Bool(true))
    );
}

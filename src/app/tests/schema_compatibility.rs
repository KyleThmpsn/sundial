//! Cross-version account compatibility: legacy behavior, current defaults, and future-field preservation.

use crate::app::{account_workspace::WorkspaceDocument, equipment, inventory, settings};
use serde_json::{Value, json};

fn document(version: u64) -> Value {
    json!({"version":version,"state":{"account":{"primary_soid":1},"characters":[{
        "soid":2,"class":0,"race":0,"gender":0,"inventory":[],"equipment":{
            "kinetic":{"instance_soid":3,"definition_hash":42,"level":0,"quantity":1,"plugs":null}
        }
    }]}})
}

#[test]
fn official_release_v6_account_validates_and_preserves_fields() {
    // Sunrise 0.3.2 / 4aebb148e92176c2b9d64a07b94068d759945853 defaults.
    let mut document: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/sunrise-v6-0.3.2-defaults.json"
    ))
    .unwrap();
    assert_eq!(document["version"], 6);
    assert_eq!(settings::validate_document(&document), Ok(()));
    let original = document.clone();
    for index in 0..3 {
        let rows = equipment::equipped_item_snapshots(&document, index).unwrap();
        assert!(!rows.is_empty());
        assert!(rows.iter().all(|item| item.issues.is_empty()), "{rows:?}");
    }
    equipment::set_equipment_item_flags(&mut document, 0, "kinetic", Some(1)).unwrap();
    document["state"]["characters"][0]["equipment"]["kinetic"] =
        original["state"]["characters"][0]["equipment"]["kinetic"].clone();
    let round_trip: Value =
        serde_json::from_slice(&serde_json::to_vec(&document).unwrap()).unwrap();
    assert_eq!(round_trip, original);
    assert_eq!(settings::validate_document(&round_trip), Ok(()));
}

#[test]
fn actual_upstream_v13_account_validates() {
    let document: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/sunrise-v13-a57dc9a9-defaults.json"
    ))
    .unwrap();
    assert_eq!(settings::validate_document(&document), Ok(()));
    for index in 0..3 {
        let rows = equipment::equipped_item_snapshots(&document, index).unwrap();
        let artifact = rows.iter().find(|item| item.slot == "artifact").unwrap();
        assert!(artifact.issues.is_empty());
        assert_eq!(artifact.bucket_hash, 0x59CA_1EA2);
    }
}

#[test]
fn retired_orbit_fields_survive_account_edits_without_validation() {
    for value in [
        json!("orbit_d2"),
        json!("invalid old name!"),
        json!({"legacy": true}),
        Value::Null,
    ] {
        let mut document: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/sunrise-v16-1120748-defaults.json"
        ))
        .unwrap();
        document["client"]["orbit_slice_set"] = value;
        let client = document["client"].clone();
        assert_eq!(settings::validate_document(&document), Ok(()));
        equipment::set_equipment_item_flags(&mut document, 0, "kinetic", Some(4)).unwrap();
        let round_trip: Value =
            serde_json::from_slice(&serde_json::to_vec(&document).unwrap()).unwrap();
        assert_eq!(round_trip["client"], client);
        assert_eq!(settings::validate_document(&round_trip), Ok(()));
    }
}

#[test]
fn masterwork_flags_follow_version_for_reads_writes_and_moves() {
    for version in [6, 8, 12, 13, 14] {
        for flags in [0_u8, 1, 3, 4, 5, 7, 8] {
            let mut document = document(version);
            let expected = flags <= if version >= 13 { 7 } else { 3 };
            let original = document.clone();
            assert_eq!(
                equipment::set_equipment_item_flags(&mut document, 0, "kinetic", Some(flags))
                    .is_ok(),
                expected
            );
            if !expected {
                assert_eq!(document, original);
                continue;
            }
            assert_eq!(settings::validate_characters(&document), Ok(()));
            assert!(
                equipment::equipped_item_snapshots(&document, 0).unwrap()[0]
                    .issues
                    .is_empty()
            );
            inventory::move_equipment_item_to_inventory(&mut document, 0, "kinetic").unwrap();
            let items = inventory::character_inventory(&document, 0)
                .unwrap()
                .unwrap();
            assert_eq!(items[0].flags, Some(flags));
            inventory::swap_inventory_item_with_equipment(
                &mut document,
                items[0].location,
                "kinetic",
            )
            .unwrap();
            assert_eq!(
                document["state"]["characters"][0]["equipment"]["kinetic"]["flags"],
                flags
            );
        }
    }
}

#[test]
fn artifact_slots_are_gated_through_creation_validation_and_snapshots() {
    for version in [6, 8, 12, 13, 14] {
        let mut document = document(version);
        let original = document.clone();
        let result = equipment::equip_definition(&mut document, 0, "artifact", 0x613A3DA6, &[]);
        if version < 13 {
            assert!(result.is_err());
            assert_eq!(document, original);
        } else {
            assert!(result.is_ok(), "{result:?}");
            let rows = equipment::equipped_item_snapshots(&document, 0).unwrap();
            let artifact = rows.iter().find(|item| item.slot == "artifact").unwrap();
            assert_ne!(artifact.instance_soid, Some(3));
            assert_eq!(settings::validate_characters(&document), Ok(()));
            inventory::move_equipment_item_to_inventory(&mut document, 0, "artifact").unwrap();
            let items = inventory::character_inventory(&document, 0)
                .unwrap()
                .unwrap();
            inventory::swap_inventory_item_with_equipment(
                &mut document,
                items[0].location,
                "artifact",
            )
            .unwrap();
        }
        let workspace = WorkspaceDocument::json_only(document);
        assert_eq!(
            workspace.equipment_slots().len(),
            if version >= 13 { 17 } else { 16 }
        );
    }
}

#[test]
fn v13_ignored_ability_fields_are_preserved_without_repair() {
    let mut raw = document(13);
    raw["state"]["characters"][0]["movement_ability"] = json!({"ignored":"keep exactly"});
    raw["state"]["characters"][0]["super_ability"] = 20.into();
    raw["state"]["characters"][0]["melee_ability"] = 21.into();
    raw["state"]["characters"][0]["equipment"]["subclass"] = json!({"instance_soid":4,"definition_hash":"0xC99B33E9","level":0,"quantity":1,"plugs":null});
    assert_eq!(settings::validate_characters(&raw), Ok(()));
    let mut document = WorkspaceDocument::json_only(raw);
    let original = document.clone();
    assert_eq!(settings::repair_known_ability_pairs(&mut document), Ok(0));
    assert_eq!(document, original);
}

#[test]
fn emote_wheel_authoring_is_gated_and_preserves_four_socket_lanes() {
    let hash = crate::account_contract::EMOTE_COLLECTION_DEFINITION_HASH;
    for version in [6, 8, 13, 14] {
        let mut document = document(version);
        let original = document.clone();
        let plugs = [
            Some("0x1".into()),
            None,
            Some("0x2".into()),
            Some("0x3".into()),
        ];
        let result = equipment::equip_definition(&mut document, 0, "emote", hash, &plugs);
        assert_eq!(result.is_ok(), version >= 13);
        if version < 13 {
            assert_eq!(document, original);
        } else {
            assert_eq!(
                document["state"]["characters"][0]["equipment"]["emote"]["plugs"],
                json!(["0x00000001", null, "0x00000002", "0x00000003"])
            );
            equipment::set_equipment_item_plug(&mut document, 0, "emote", 1, &plugs, Some(4))
                .unwrap();
            assert_eq!(
                document["state"]["characters"][0]["equipment"]["emote"]["plugs"][1],
                json!("0x00000004")
            );
        }
    }
}

#[test]
fn travelling_activity_edits_are_gated_bounded_and_lossless() {
    use crate::persistence::json_account::character_runtime::set_current_activity;
    for version in [6, 8, 13, 14] {
        let mut document = document(version);
        let original = document.clone();
        assert_eq!(
            set_current_activity(&mut document, 0, Some(json!("0xFFFF"))).is_ok(),
            version >= 13
        );
        if version < 13 {
            assert_eq!(document, original);
            continue;
        }
        let edited = document.clone();
        assert!(set_current_activity(&mut document, 0, Some(json!(65536))).is_err());
        assert_eq!(document, edited);
        assert_eq!(set_current_activity(&mut document, 0, None), Ok(true));
        assert_eq!(document, original);
    }
}

#[test]
fn equipping_a_v13_subclass_leaves_ignored_saved_abilities_untouched() {
    let mut raw = document(13);
    raw["state"]["characters"][0]["movement_ability"] = json!({"opaque":"keep"});
    raw["state"]["characters"][0]["super_ability"] = 20.into();
    raw["state"]["characters"][0]["melee_ability"] = 21.into();
    let abilities = raw["state"]["characters"][0].clone();
    let mut document = WorkspaceDocument::json_only(raw);
    let item = crate::catalog::ItemDef {
        hash: 0xC99B33E9,
        name: "Sentinel".into(),
        type_name: "Subclass".into(),
        bucket_hash: 3_284_755_031,
        class_type: 0,
        default_plugs: Vec::new(),
        sockets: Vec::new(),
        abilities: Default::default(),
    };
    equipment::equip_subclass_with_default_abilities(&mut document, 0, &item, false).unwrap();
    for key in [
        "movement_ability",
        "super_ability",
        "melee_ability",
        "grenade_ability",
        "class_ability",
    ] {
        assert_eq!(
            document["state"]["characters"][0].get(key),
            abilities.get(key)
        );
    }
}

#[test]
fn upstream_v16_account_edits_preserve_lore_defaults_and_retired_fields() {
    let mut document: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/sunrise-v16-1120748-defaults.json"
    ))
    .unwrap();
    assert_eq!(document["version"], 16);
    assert_eq!(settings::validate_document(&document), Ok(()));
    // Sunrise skips these retired keys regardless of their shape.
    document["state"]["characters"][0]["accepted"] = json!({"keep":true});
    document["state"]["account"]["record_rewards"] = json!({"keep":true});
    let original = document.clone();
    equipment::set_equipment_item_flags(&mut document, 0, "kinetic", Some(5)).unwrap();
    assert_eq!(settings::validate_document(&document), Ok(()));
    let mut expected = original;
    expected["state"]["characters"][0]["equipment"]["kinetic"]["flags"] = 5.into();
    let round_trip: Value =
        serde_json::from_slice(&serde_json::to_vec(&document).unwrap()).unwrap();
    assert_eq!(round_trip, expected);
    for index in 0..3 {
        assert!(
            equipment::equipped_item_snapshots(&round_trip, index)
                .unwrap()
                .iter()
                .all(|item| item.issues.is_empty())
        );
    }
}

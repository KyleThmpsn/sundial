use super::*;
use crate::investment::AuthoredSlotChange;
use serde_json::json;

fn item(id: u64, hash: u32) -> Value {
    json!({"instance_soid": format!("0x{id:016X}"), "definition_hash": hash, "level": 106, "quantity": 1, "plugs": [77, null], "flags": 1})
}
fn replacement(capacity: usize) -> AuthoredSlotReplacement {
    AuthoredSlotReplacement {
        changes: vec![AuthoredSlotChange {
            definition_hash: 100,
            previous_bucket: 0,
            incoming_bucket: 1,
        }],
        incoming_buckets: BTreeMap::from([(100, 1), (200, 1), (300, 3)]),
        weapon_capacities: [10, capacity, 10],
    }
}
fn document(version: u64) -> Value {
    json!({"version": version, "unknown": true, "state": {
        "account": {"primary_soid": "0x0000000000000001"},
        "characters": (0..3).map(|i| json!({"soid": format!("0x{:016X}", i + 2), "class": i, "future": {"keep": [1, 2]},
            "equipment": {"kinetic": item(1000 + i * 10, 100), "energy": item(1001 + i * 10, 200)},
            "inventory": [item(1002 + i * 10, 200)]})).collect::<Vec<_>>()
    }})
}

#[test]
fn slot_replacement_moves_complete_equipment_on_every_character_and_is_idempotent() {
    for version in [6, 8, 16] {
        let original = document(version);
        let mut updated = original.clone();
        let report = relocate(&mut updated, &BTreeSet::new(), Some(&replacement(3))).unwrap();
        assert_eq!(report.len(), 3);
        assert!(
            report
                .iter()
                .all(|row| row.outcome == AuthoredMoveOutcome::MovedToInventory)
        );
        let mut expected = original.clone();
        for i in 0..3 {
            let character = &mut expected["state"]["characters"][i];
            let item = character["equipment"]["kinetic"].take();
            character["inventory"].as_array_mut().unwrap().push(item);
        }
        assert_eq!(updated, expected, "v{version}");
        assert!(
            relocate(&mut updated, &BTreeSet::new(), Some(&replacement(3)))
                .unwrap()
                .is_empty()
        );
        assert_eq!(updated, expected);
    }
}

#[test]
fn slot_replacement_deletes_only_affected_equipment_when_destination_bucket_is_full() {
    let original = document(8);
    let mut updated = original.clone();
    let report = relocate(&mut updated, &BTreeSet::new(), Some(&replacement(2))).unwrap();
    assert_eq!(report.len(), 3);
    assert!(
        report
            .iter()
            .all(|row| row.outcome == AuthoredMoveOutcome::DeletedInventoryFull)
    );
    let mut expected = original;
    for character in expected["state"]["characters"].as_array_mut().unwrap() {
        character["equipment"]["kinetic"] = Value::Null;
    }
    assert_eq!(updated, expected);
}

#[test]
fn slot_replacement_checks_whole_inventory_and_does_not_treat_missing_metadata_as_full() {
    let mut updated = document(8);
    updated["state"]["characters"][0]["inventory"] =
        Value::Array((0..135).map(|i| item(2000 + i, 300)).collect());
    let report = relocate(&mut updated, &BTreeSet::new(), Some(&replacement(10))).unwrap();
    assert_eq!(report[0].outcome, AuthoredMoveOutcome::DeletedInventoryFull);
    assert_eq!(
        updated["state"]["characters"][0]["inventory"]
            .as_array()
            .unwrap()
            .len(),
        135
    );
    let mut original = document(8);
    let before = original.clone();
    let mut missing = replacement(10);
    missing.incoming_buckets.remove(&200);
    assert!(relocate(&mut original, &BTreeSet::new(), Some(&missing)).is_err());
    assert_eq!(original, before);
}

#[test]
fn slot_replacement_reserves_outgoing_space_before_moving_a_pair() {
    let mut updated = document(8);
    let mut changes = replacement(2);
    changes.changes.push(AuthoredSlotChange {
        definition_hash: 200,
        previous_bucket: 1,
        incoming_bucket: 0,
    });
    changes.incoming_buckets.insert(200, 0);
    changes.weapon_capacities = [2, 1, 10];
    let report = relocate(&mut updated, &BTreeSet::new(), Some(&changes)).unwrap();
    assert_eq!(report.len(), 6);
    assert!(
        report
            .iter()
            .all(|row| row.outcome == AuthoredMoveOutcome::MovedToInventory)
    );
}

#[test]
fn slot_replacement_blocks_overflow_of_already_stored_changed_copies() {
    let mut updated = document(8);
    updated["state"]["characters"][0]["inventory"] = json!([item(2000, 100), item(2001, 100)]);
    let original = updated.clone();
    assert!(
        relocate(&mut updated, &BTreeSet::new(), Some(&replacement(2)))
            .unwrap_err()
            .contains("stored weapons")
    );
    assert_eq!(updated, original);
}

#[test]
fn slot_replacement_selects_json_or_sqlite_at_runtime_without_touching_inactive_json() {
    use crate::investment::{
        preview_authored_account_replacement_with_slots, replace_authored_account_source,
    };
    let directory = crate::test_support::TestDirectory::new("slot-replacement-backends");
    let settings = directory.0.join("settings.json");
    let original = serde_json::to_vec(&document(8)).unwrap();
    std::fs::write(&settings, &original).unwrap();
    let slots = replacement(3);
    let json = preview_authored_account_replacement_with_slots(
        &directory.0,
        &BTreeSet::new(),
        &[],
        &[],
        Some(&slots),
    )
    .unwrap();
    assert_eq!(json.settings_path, settings);
    assert_eq!(json.slot_moves.len(), 3);
    assert_eq!(std::fs::read(&settings).unwrap(), original);
    replace_authored_account_source(&settings, &json.original_bytes, &json.cleaned_bytes).unwrap();
    assert_eq!(std::fs::read(&settings).unwrap(), json.cleaned_bytes);

    let inactive = br#"{"version":18,"state":{"characters":"inactive"},"unknown":true}"#;
    std::fs::write(&settings, inactive).unwrap();
    let database = crate::persistence::investment_path(&settings);
    crate::persistence::sqlite_account::tests::create_fixture(&database, 3);
    let mut slots = replacement(3);
    slots.incoming_buckets.insert(200, 16);
    slots.incoming_buckets.insert(300, 1);
    let sqlite = preview_authored_account_replacement_with_slots(
        &directory.0,
        &BTreeSet::new(),
        &[],
        &[],
        Some(&slots),
    )
    .unwrap();
    assert_eq!(sqlite.settings_path, database);
    assert_eq!(sqlite.slot_moves.len(), 1);
    replace_authored_account_source(&database, &sqlite.original_bytes, &sqlite.cleaned_bytes)
        .unwrap();
    assert_eq!(
        crate::investment::read_authored_account_source(&database).unwrap(),
        sqlite.cleaned_bytes
    );
    assert_eq!(std::fs::read(&settings).unwrap(), inactive);
}

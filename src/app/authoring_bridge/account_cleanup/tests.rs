use super::*;
use serde_json::json;

fn item(soid: u64, hash: u32) -> Value {
    json!({"instance_soid": format!("0x{soid:016X}"), "definition_hash": hash, "level": 106, "quantity": 1, "plugs": null})
}

#[test]
fn cleanup_preserves_unrelated_data_and_removes_both_unlock_lanes() {
    for version in [6, 8, 13] {
        let retained = item(102, 200);
        let mut plugged = item(104, 201);
        plugged["plugs"] = json!([300, 301]);
        let original = json!({
            "version": version, "unknown_setting": {"keep": true},
            "state": {
                "account": {"primary_soid": "0x0000000000000001", "profile_items": [{"definition_hash": 100, "quantity": 1}, {"definition_hash": 200, "quantity": 5}]},
                "characters": [{"soid": "0x0000000000000002", "class": 0,
                    "equipment": {"kinetic": item(101, 100), "energy": retained},
                    "inventory": [item(103, 100), plugged]},
                    {"soid": "0x0000000000000003", "class": 1, "equipment": {}, "inventory": [item(105, 100)]}],
                "unlocks": {"account_flag_runs": [[41, 3]], "profile_flag_runs": [[10, 1]], "objective_values": [[1, 12]], "account_progressions": [[1, 7, 8, 9]], "unknown": 12},
                "investment": {"family5_flag_overrides": [[200, 0], [201, 2]], "family5_value_overrides": [[13, 9]], "unknown": true}
            }
        });
        let hashes = BTreeSet::from([100, 300]);
        let unlocks = [AuthoredCollectionUnlock {
            definition_index: 200,
            bank: 1,
            slot: 42,
        }];
        let (cleaned, removed, plugs, flags, rewards) =
            clean(&original, &hashes, &unlocks).unwrap();
        assert_eq!(rewards, 0);
        assert_eq!(removed, BTreeMap::from([(100, 4)]));
        assert_eq!((plugs, flags), (1, 1));
        let mut expected = original.clone();
        expected["state"]["characters"][0]["equipment"]["kinetic"] = Value::Null;
        expected["state"]["characters"][0]["inventory"] =
            json!([original["state"]["characters"][0]["inventory"][1]]);
        expected["state"]["characters"][0]["inventory"][0]["plugs"] = json!([null, "0x0000012D"]);
        expected["state"]["characters"][1]["inventory"] = json!([]);
        expected["state"]["account"]["profile_items"] =
            json!([{"definition_hash": 200, "quantity": 5}]);
        expected["state"]["unlocks"]["account_flag_runs"] = json!([[41, 1], [43, 1]]);
        expected["state"]["investment"]["family5_flag_overrides"] = json!([[201, 2]]);
        assert_eq!(cleaned, expected, "v{version}");
        let (again, items, plugs, flags, _) = clean(&cleaned, &hashes, &unlocks).unwrap();
        assert_eq!(again, cleaned);
        assert!(items.is_empty());
        assert_eq!((plugs, flags), (0, 0));
    }
}

#[test]
fn malformed_and_future_accounts_are_not_silently_cleaned() {
    for original in [
        json!({"version": 99}),
        json!({"version": 13, "state": {"characters": {}}}),
        json!({"version": 8, "state": {"unlocks": {"account_flag_runs": "bad"}}}),
    ] {
        assert!(clean(&original, &BTreeSet::from([100]), &[]).is_err());
    }
}

#[test]
fn equipped_custom_plugs_and_reward_rules_are_removed_without_deleting_stock_items() {
    let mut stock = item(20, 200);
    stock["plugs"] = json!([300, 301]);
    let original = json!({"version": 13, "state": {
        "account": {"primary_soid": "0x0000000000000001", "dismantle_rewards": [
            {"definition_hash": 100, "quantity": 1}, {"definition_hash": 200, "quantity": 1}
        ]},
        "characters": [{"soid": "0x0000000000000002", "class": 0, "equipment": {"kinetic": stock}}]
    }});
    let (cleaned, items, plugs, flags, rewards) =
        clean(&original, &BTreeSet::from([100, 300]), &[]).unwrap();
    assert!(items.is_empty());
    assert_eq!((plugs, flags, rewards), (1, 0, 1));
    assert_eq!(
        cleaned["state"]["characters"][0]["equipment"]["kinetic"]["definition_hash"],
        200
    );
    assert_eq!(
        cleaned["state"]["characters"][0]["equipment"]["kinetic"]["plugs"],
        json!([null, 301])
    );
    assert_eq!(
        cleaned["state"]["account"]["dismantle_rewards"],
        json!([{"definition_hash": 200, "quantity": 1}])
    );
}

#[test]
fn automatic_cleanup_respects_the_selected_account_backend_build() {
    let directory = crate::test_support::TestDirectory::new("cleanup-backend-gate");
    let settings = directory.0.join("settings.json");
    std::fs::create_dir_all(directory.0.join("data")).unwrap();
    std::fs::write(
        directory.0.join("data").join("investment.sqlite3"),
        b"existing account database",
    )
    .unwrap();
    assert!(crate::investment::validate_authored_cleanup_backend(&settings).is_err());
    for version in [6, 8, 17] {
        std::fs::write(&settings, format!("{{\"version\":{version}}}")).unwrap();
        assert!(crate::investment::validate_authored_cleanup_backend(&settings).is_ok());
        assert!(
            crate::investment::validate_authored_cleanup_backend(
                &directory.0.join("data/investment.sqlite3")
            )
            .is_err()
        );
    }
    std::fs::write(&settings, br#"{"version":18}"#).unwrap();
    assert!(crate::investment::validate_authored_cleanup_backend(&settings).is_err());
}

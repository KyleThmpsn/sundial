use super::*;
use serde_json::json;

fn item(id: u64, hash: u32, plugs: Value) -> Value {
    json!({"instance_soid": format!("0x{id:016X}"), "definition_hash": hash,
        "level": 106, "quantity": 1, "flags": 1, "plugs": plugs})
}

fn document(version: u64, plugs: Value) -> Value {
    json!({"version": version, "unknown_setting": {"preserve": true}, "state": {
        "account": {"primary_soid": "0x0000000000000001"},
        "unknown_state": [1, 2],
        "characters": [
            {"soid": "0x0000000000000002", "class": 0, "unknown_character": true,
                "equipment": {"kinetic": item(10, 100, plugs.clone()), "energy": item(11, 100, Value::Null)},
                "inventory": [item(12, 100, plugs.clone()), item(13, 200, plugs.clone())]},
            {"soid": "0x0000000000000003", "class": 1,
                "equipment": {"kinetic": item(14, 100, plugs)}}
        ]
    }})
}

fn change(previous: usize, incoming: usize) -> AuthoredSocketChange {
    AuthoredSocketChange {
        definition_hash: 100,
        previous_socket_count: previous,
        default_plugs: (0..incoming)
            .map(|index| Some(500 + index as u32))
            .collect(),
    }
}

#[test]
fn growth_and_shrink_preserve_retained_selections_and_unknown_fields() {
    for version in [8, 16] {
        for (previous, incoming) in [(8, 9), (8, 12), (12, 8)] {
            let plugs = Value::Array(
                (0..previous)
                    .map(|index| {
                        if index == 1 {
                            Value::Null
                        } else {
                            json!(format!("0x{:x}", 300 + index))
                        }
                    })
                    .collect(),
            );
            let original = document(version, plugs.clone());
            let mut migrated = original.clone();
            let mut update = change(previous, incoming);
            if incoming > previous {
                update.default_plugs[incoming - 1] = None;
            }
            let resized = resize(
                &mut migrated,
                &BTreeSet::new(),
                std::slice::from_ref(&update),
            )
            .unwrap();
            assert_eq!(resized, BTreeMap::from([(100, 3)]));
            let mut expected = original;
            let mut expected_plugs = plugs.as_array().unwrap().clone();
            expected_plugs.truncate(incoming);
            expected_plugs.extend((previous..incoming).map(|index| {
                if index == incoming - 1 {
                    Value::Null
                } else {
                    json!(format!("0x{:08X}", 500 + index))
                }
            }));
            for path in [
                "/state/characters/0/equipment/kinetic/plugs",
                "/state/characters/0/inventory/0/plugs",
                "/state/characters/1/equipment/kinetic/plugs",
            ] {
                *expected.pointer_mut(path).unwrap() = json!(expected_plugs);
            }
            assert_eq!(migrated, expected, "v{version} {previous} to {incoming}");
            assert!(
                resize(&mut migrated, &BTreeSet::new(), &[update])
                    .unwrap()
                    .is_empty()
            );
            assert_eq!(migrated, expected);
        }
    }
}

#[test]
fn malformed_socket_lists_and_conflicting_changes_fail_without_partial_updates() {
    for bad_plugs in [
        json!([300]),
        json!("malformed"),
        json!(vec![4294967295_u64; 8]),
    ] {
        let mut original = document(16, json!(vec![300; 8]));
        original["state"]["characters"][1]["equipment"]["kinetic"]["plugs"] = bad_plugs;
        let mut proposed = original.clone();
        assert!(resize(&mut proposed, &BTreeSet::new(), &[change(8, 9)]).is_err());
        assert_eq!(proposed, original);
    }
    let original = document(16, json!(vec![300; 8]));
    for changes in [vec![change(8, 9), change(8, 10)], vec![change(8, 13)]] {
        let mut proposed = original.clone();
        assert!(resize(&mut proposed, &BTreeSet::new(), &changes).is_err());
        assert_eq!(proposed, original);
    }
    assert!(
        resize(
            &mut original.clone(),
            &BTreeSet::from([100]),
            &[change(8, 9)]
        )
        .is_err()
    );
    let mut future = original;
    future["version"] = json!(99);
    assert!(resize(&mut future, &BTreeSet::new(), &[change(8, 9)]).is_err());
}

#[test]
fn native_defaults_and_unaffected_items_need_no_socket_rewrite() {
    let original = document(16, Value::Null);
    let mut proposed = original.clone();
    assert!(
        resize(&mut proposed, &BTreeSet::new(), &[change(8, 9)])
            .unwrap()
            .is_empty()
    );
    assert_eq!(proposed, original);
}

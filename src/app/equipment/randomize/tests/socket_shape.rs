use super::*;
use crate::catalog::{ItemStackability, SocketDef};
use serde_json::json;

fn catalog(socket_count: usize) -> Catalog {
    let item = ItemDef {
        hash: 100,
        name: "Socket Test Weapon".into(),
        type_name: "Auto Rifle".into(),
        bucket_hash: SLOTS[0].2,
        class_type: 3,
        default_plugs: vec![Some("0x65".into()); socket_count],
        sockets: vec![SocketDef::default(); socket_count],
        abilities: Default::default(),
    };
    let other_family = ItemDef {
        hash: 200,
        name: "Other Weapon Family".into(),
        type_name: "Pulse Rifle".into(),
        bucket_hash: SLOTS[1].2,
        class_type: 3,
        default_plugs: vec![None],
        sockets: vec![SocketDef {
            socket_type: 77,
            allowed: vec![202],
            ..Default::default()
        }],
        abilities: Default::default(),
    };
    Catalog::for_test_with_inventory(
        vec![item, other_family],
        HashMap::new(),
        HashMap::from([(
            100,
            InventoryMetadata {
                scope: InventoryScope::Character,
                native_bucket_id: 0,
                stackability: ItemStackability::Instanced,
                max_stack_size: Some(1),
                bucket_capacity: Some(10),
            },
        )]),
    )
}

fn document(version: u64) -> account::WorkspaceDocument {
    let mut document = emote_document(version);
    document.json_mut()["state"]["characters"][0]["equipment"]["kinetic"] = json!({
        "instance_soid": "0x0000000000000003",
        "definition_hash": "0x00000064",
        "level": 100,
        "quantity": 1,
        "plugs": null,
        "flags": 0,
    });
    document
}

#[test]
fn wrong_socket_count_cannot_equip_or_add_a_candidate() {
    let catalog = catalog(2);
    for version in [8, 16] {
        for count in [
            0,
            1,
            3,
            inventory::MAX_ITEM_PLUGS,
            inventory::MAX_ITEM_PLUGS + 1,
        ] {
            let candidate = Candidate {
                item_hash: 100,
                plugs: vec![Some(101); count],
            };
            let mut document = document(version);
            let original = document.clone();
            for discard_replaced in [false, true] {
                let error =
                    apply_candidate(&mut document, &catalog, 0, &candidate, discard_replaced)
                        .unwrap_err();
                assert!(error.contains("requires 2"), "{error}");
                assert_eq!(document, original);
            }
            let error =
                add_candidate_to_inventory(&mut document, &catalog, 0, &candidate).unwrap_err();
            assert!(error.contains("requires 2"), "{error}");
            assert_eq!(document, original);
        }
    }
}

#[test]
fn importing_extra_sockets_cannot_publish_a_new_invalid_instance() {
    let catalog = catalog(2);
    let request = ItemBuilderRequest {
        item_hash: 100,
        authored_plugs: Some(json!([101, null, 102])),
    };
    let (_, _, candidate) = candidate_from_builder_request(&catalog, &request).unwrap();
    assert_eq!(candidate.plugs, vec![Some(101), None, Some(102)]);
    let mut document = document(16);
    let original = document.clone();
    assert!(apply_candidate(&mut document, &catalog, 0, &candidate, false).is_err());
    assert!(add_candidate_to_inventory(&mut document, &catalog, 0, &candidate).is_err());
    assert_eq!(document, original);
    assert_eq!(request.authored_plugs, Some(json!([101, null, 102])));
}

#[test]
fn imported_unknown_plugs_cannot_equip_or_add_a_candidate() {
    let catalog = catalog(2);
    for hash in [100, 0x0BAD_C0DE] {
        assert!(!catalog.contains_plug(hash));
        let request = ItemBuilderRequest {
            item_hash: 100,
            authored_plugs: Some(json!([101, hash])),
        };
        let (_, _, candidate) = candidate_from_builder_request(&catalog, &request).unwrap();
        assert_eq!(candidate.plugs, vec![Some(101), Some(hash)]);
        for version in [8, 16] {
            let mut document = document(version);
            let original = document.clone();
            for discard_replaced in [false, true] {
                let error =
                    apply_candidate(&mut document, &catalog, 0, &candidate, discard_replaced)
                        .unwrap_err();
                assert!(error.contains("socket 2"), "{error}");
                assert!(error.contains("installed plug catalog"), "{error}");
                assert_eq!(document, original);
            }
            let error =
                add_candidate_to_inventory(&mut document, &catalog, 0, &candidate).unwrap_err();
            assert!(error.contains("socket 2"), "{error}");
            assert!(error.contains("installed plug catalog"), "{error}");
            assert_eq!(document, original);
        }
    }
}

#[test]
fn installed_plugs_from_other_families_remain_valid_candidates() {
    let catalog = catalog(2);
    let item = catalog.item(100).unwrap();
    assert!(!catalog.socket_options(&item.sockets[0]).contains(&202));
    assert!(catalog.all_plug_options().contains(&202));
    let request = ItemBuilderRequest {
        item_hash: 100,
        authored_plugs: Some(json!([202, null])),
    };
    let (_, _, candidate) = candidate_from_builder_request(&catalog, &request).unwrap();
    for version in [8, 16] {
        let mut document = document(version);
        apply_candidate(&mut document, &catalog, 0, &candidate, false).unwrap();
        let equipped = account::equipped_item_snapshots(&document, 0).unwrap();
        assert_eq!(
            equipped_plugs_value(&equipped[0].plugs),
            Some(json!([202, null]))
        );
        add_candidate_to_inventory(&mut document, &catalog, 0, &candidate).unwrap();
        let held = account::character_inventory(&document, 0).unwrap().unwrap();
        assert_eq!(
            held[1].plugs,
            inventory::ItemPlugs::Authored(vec![Some(202), None])
        );
    }
}

#[test]
fn importing_more_than_twelve_sockets_preserves_the_previous_preview() {
    let catalog = catalog(inventory::MAX_ITEM_PLUGS);
    let mut state = WorkspaceState {
        candidate: Some(Candidate {
            item_hash: 100,
            plugs: vec![None; inventory::MAX_ITEM_PLUGS],
        }),
        ..Default::default()
    };
    let request = ItemBuilderRequest {
        item_hash: 100,
        authored_plugs: Some(json!(vec![101; inventory::MAX_ITEM_PLUGS + 1])),
    };
    let original = request.authored_plugs.clone();
    assert!(open_builder_request(&catalog, &mut state, &request).is_err());
    assert_eq!(
        state.candidate.unwrap().plugs,
        vec![None; inventory::MAX_ITEM_PLUGS]
    );
    assert_eq!(request.authored_plugs, original);
}

#[test]
fn native_defaults_and_explicit_empty_sockets_survive_both_apply_paths() {
    for version in [8, 16] {
        for socket_count in [0, 2] {
            let catalog = catalog(socket_count);
            if socket_count > 0 {
                assert!(catalog.contains_plug(101));
                assert!(!catalog.all_plug_options().contains(&101));
            }
            for authored_plugs in [Value::Null, json!(vec![None::<u32>; socket_count])] {
                let request = ItemBuilderRequest {
                    item_hash: 100,
                    authored_plugs: Some(authored_plugs.clone()),
                };
                let (_, _, candidate) = candidate_from_builder_request(&catalog, &request).unwrap();
                let expected = if authored_plugs.is_null() {
                    vec![Some(101); socket_count]
                } else {
                    vec![None; socket_count]
                };
                assert_eq!(candidate.plugs, expected);
                let mut document = document(version);
                let previous =
                    document.json()["state"]["characters"][0]["equipment"]["kinetic"].clone();
                apply_candidate(&mut document, &catalog, 0, &candidate, false).unwrap();
                assert_eq!(
                    document.json()["state"]["characters"][0]["inventory"][0],
                    previous
                );
                let equipped = account::equipped_item_snapshots(&document, 0).unwrap();
                let plugs = equipped_plugs_value(&equipped[0].plugs).unwrap();
                assert_eq!(
                    plugs
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(parse_unsigned_value)
                        .collect::<Vec<_>>(),
                    expected,
                );
                add_candidate_to_inventory(&mut document, &catalog, 0, &candidate).unwrap();
                let held = account::character_inventory(&document, 0).unwrap().unwrap();
                assert_eq!(
                    held[1].plugs,
                    inventory::ItemPlugs::Authored(
                        expected
                            .into_iter()
                            .map(|hash| hash.map(|hash| hash as u32))
                            .collect()
                    ),
                );
            }
        }
    }
}

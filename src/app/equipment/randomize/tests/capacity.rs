use super::*;
use crate::catalog::ItemStackability;
use serde_json::json;

fn catalog() -> Catalog {
    let mut items = Vec::new();
    let mut metadata = HashMap::new();
    for (hash, slot, bucket, capacity) in [(99, "artifact", 49, 4), (100, "ghost", 8, 10)] {
        items.push(ItemDef {
            hash,
            name: slot.into(),
            type_name: slot.into(),
            bucket_hash: slot_definition(slot).unwrap().2,
            class_type: 3,
            default_plugs: vec![],
            sockets: vec![],
            abilities: Default::default(),
        });
        metadata.insert(
            hash,
            InventoryMetadata {
                scope: InventoryScope::Character,
                native_bucket_id: bucket,
                stackability: ItemStackability::Instanced,
                max_stack_size: Some(1),
                bucket_capacity: Some(capacity),
            },
        );
    }
    let emotes = emote_catalog();
    let hash = crate::account_contract::EMOTE_COLLECTION_DEFINITION_HASH;
    items.push(emotes.item(hash).unwrap().clone());
    metadata.insert(hash, *emotes.inventory_metadata(hash).unwrap());
    Catalog::for_test_with_inventory(items, HashMap::new(), metadata)
}

#[test]
fn full_loadout_respects_each_native_bucket_and_retained_locked_items() {
    let catalog = catalog();
    for locked_count in [0, 2, 3] {
        let mut document = emote_document(16);
        document.json_mut()["state"]["characters"][0]["equipment"]["artifact"] = json!({
            "instance_soid": 50, "definition_hash": 99, "level": 0,
            "quantity": 1, "plugs": null, "flags": 1,
        });
        let locked = (0..locked_count)
            .map(|index| {
                json!({
                    "instance_soid": 60 + index, "definition_hash": 99, "level": 0,
                    "quantity": 1, "plugs": null, "flags": 1,
                })
            })
            .collect::<Vec<_>>();
        document.json_mut()["state"]["characters"][0]["inventory"] = json!(locked);
        let equipped = document.json()["state"]["characters"][0]["equipment"]["artifact"].clone();
        for _ in 0..2 {
            randomize_full_loadout(
                &mut document,
                &catalog,
                0,
                PlugSelectionMode::Supported,
                false,
                LoadoutOptions {
                    equipment_flair: true,
                    replace_held_inventory: true,
                    ..Default::default()
                },
            )
            .unwrap();
            let inventory = account::character_inventory(&document, 0).unwrap().unwrap();
            assert_eq!(
                inventory
                    .iter()
                    .filter(|item| item.definition_hash == 99)
                    .count(),
                3
            );
            assert_eq!(
                inventory
                    .iter()
                    .filter(|item| item.definition_hash == 100)
                    .count(),
                9
            );
            assert_eq!(
                document.json()["state"]["characters"][0]["equipment"]["artifact"],
                equipped
            );
            for (index, item) in locked.iter().enumerate() {
                assert_eq!(
                    &document.json()["state"]["characters"][0]["inventory"][index],
                    item
                );
            }
        }
    }
}

#[test]
fn missing_native_capacity_cannot_fall_back_to_ten_items() {
    let source = catalog();
    let emote_hash = crate::account_contract::EMOTE_COLLECTION_DEFINITION_HASH;
    let catalog = Catalog::for_test_with_inventory(
        vec![
            source.item(99).unwrap().clone(),
            source.item(emote_hash).unwrap().clone(),
        ],
        HashMap::new(),
        HashMap::from([(emote_hash, *source.inventory_metadata(emote_hash).unwrap())]),
    );
    let mut document = emote_document(16);
    let before = document.clone();
    let error = randomize_full_loadout(
        &mut document,
        &catalog,
        0,
        PlugSelectionMode::Supported,
        false,
        LoadoutOptions {
            equipment_flair: true,
            replace_held_inventory: true,
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(error.contains("inventory placement metadata"), "{error}");
    assert_eq!(document, before);
}

#[test]
fn single_random_item_cannot_fill_empty_equipment_slot_when_bucket_is_full() {
    let item = ItemDef {
        hash: 101,
        name: "Test weapon".into(),
        type_name: "Weapon".into(),
        bucket_hash: slot_definition("kinetic").unwrap().2,
        class_type: 3,
        default_plugs: vec![],
        sockets: vec![],
        abilities: Default::default(),
    };
    let catalog = Catalog::for_test_with_inventory(
        vec![item],
        HashMap::new(),
        HashMap::from([(
            101,
            InventoryMetadata {
                scope: InventoryScope::Character,
                native_bucket_id: 0,
                stackability: ItemStackability::Instanced,
                max_stack_size: Some(1),
                bucket_capacity: Some(10),
            },
        )]),
    );
    let roll = Candidate {
        item_hash: 101,
        plugs: vec![],
    };
    for version in [8, 16] {
        let mut document = emote_document(version);
        document.json_mut()["state"]["characters"][0]["inventory"] = json!(
            (0..10)
                .map(|index| json!({
                    "instance_soid": 100 + index, "definition_hash": 101,
                    "level": 106, "quantity": 1, "plugs": null,
                }))
                .collect::<Vec<_>>()
        );
        let before = document.clone();
        for discard_replaced in [false, true] {
            let error =
                apply_candidate(&mut document, &catalog, 0, &roll, discard_replaced).unwrap_err();
            assert!(error.contains("contains 11 items"), "{error}");
            assert_eq!(document, before);
        }
        account::remove_character_inventory_items(&mut document, 0, [0]).unwrap();
        apply_candidate(&mut document, &catalog, 0, &roll, false).unwrap();
        assert_eq!(
            account::character_inventory(&document, 0)
                .unwrap()
                .unwrap()
                .len(),
            9
        );
    }
}

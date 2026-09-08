//! Equipment randomizer behavior and invariant tests.

mod capacity;
mod native_sanity;
mod socket_shape;

use crate::app::account_workspace as account;
use std::collections::HashMap;

use super::*;

fn emote_catalog() -> Catalog {
    use crate::account_contract::{EMOTE_BUCKET_HASH, EMOTE_COLLECTION_DEFINITION_HASH};
    use crate::catalog::{ItemStackability, SocketDef};
    let collection = ItemDef {
        hash: EMOTE_COLLECTION_DEFINITION_HASH,
        name: "Emote Collection".into(),
        type_name: "Emote".into(),
        bucket_hash: EMOTE_BUCKET_HASH,
        class_type: 3,
        default_plugs: vec![Some("0x65".into()); 4],
        sockets: (0..4)
            .map(|_| SocketDef {
                socket_type: 42,
                allowed: vec![101, 102, 103, 104],
                ..Default::default()
            })
            .collect(),
        abilities: Default::default(),
    };
    let legacy = ItemDef {
        hash: 7,
        name: "Legacy Emote".into(),
        sockets: vec![],
        default_plugs: vec![],
        ..collection.clone()
    };
    let other = ItemDef {
        hash: 8,
        name: "Other Item".into(),
        bucket_hash: SLOTS[0].2,
        sockets: vec![SocketDef {
            socket_type: 77,
            allowed: vec![999],
            ..Default::default()
        }],
        default_plugs: vec![Some("0x3E7".into())],
        ..collection.clone()
    };
    Catalog::for_test_with_inventory(
        vec![collection, legacy, other],
        HashMap::new(),
        [EMOTE_COLLECTION_DEFINITION_HASH, 7]
            .into_iter()
            .map(|hash| {
                (
                    hash,
                    InventoryMetadata {
                        scope: InventoryScope::Character,
                        native_bucket_id: if hash == EMOTE_COLLECTION_DEFINITION_HASH {
                            12
                        } else {
                            41
                        },
                        stackability: ItemStackability::Instanced,
                        max_stack_size: Some(1),
                        bucket_capacity: Some(if hash == EMOTE_COLLECTION_DEFINITION_HASH {
                            1
                        } else {
                            10
                        }),
                    },
                )
            })
            .collect(),
    )
}

fn emote_document(version: u64) -> account::WorkspaceDocument {
    let defaults: Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/sunrise-v13-a57dc9a9-defaults.json"
    ))
    .unwrap();
    let mut document = serde_json::json!({
        "version":version,"state":{"account":{"primary_soid":1,"settings":{}},"characters":[{
            "soid":2,"class":0,"race":0,"gender":0,"inventory":[],"equipment":{},
            "future_field":{"preserve":true}
        }]}
    });
    document["state"]["account"]["settings"] = defaults["state"]["account"]["settings"].clone();
    account::WorkspaceDocument::json_only(document)
}

#[test]
fn emote_loadout_uses_collection_rolls_only_on_v13_and_later() {
    let catalog = emote_catalog();
    for version in [8, 12, 13, 16] {
        let mut document = emote_document(version);
        randomize_full_loadout(
            &mut document,
            &catalog,
            0,
            PlugSelectionMode::AnyPlug,
            false,
            LoadoutOptions {
                equipment_flair: true,
                replace_held_inventory: true,
                ..Default::default()
            },
        )
        .unwrap();
        let equipped = account::equipped_item_snapshots(&document, 0).unwrap();
        let emote = equipped.iter().find(|item| item.slot == "emote").unwrap();
        let held = account::character_inventory(&document, 0).unwrap().unwrap();
        let expected = if version >= 13 {
            crate::account_contract::EMOTE_COLLECTION_DEFINITION_HASH
        } else {
            7
        };
        assert_eq!(emote.definition_hash, Some(expected));
        assert_eq!(
            held.len(),
            if version >= 13 {
                0
            } else {
                HELD_ITEMS_PER_SLOT
            }
        );
        assert!(
            held.iter()
                .all(|item| u64::from(item.definition_hash) == expected)
        );
        if version >= 13 {
            let emote_plugs = equipped_plugs_value(&emote.plugs).unwrap();
            for plugs in std::iter::once(emote_plugs)
                .chain(held.iter().map(|item| inventory_plugs_value(&item.plugs)))
            {
                let plugs = plugs.as_array().unwrap();
                assert_eq!(plugs.len(), 4);
                assert!(plugs.iter().all(|plug| {
                    parse_unsigned_value(plug).is_some_and(|hash| (101..=104).contains(&hash))
                }));
            }
        }
        assert_eq!(
            document.json()["state"]["characters"][0]["future_field"],
            serde_json::json!({"preserve":true})
        );
    }
}

#[test]
fn missing_v13_emote_collection_does_not_partially_replace_the_loadout() {
    let source = emote_catalog();
    let catalog = Catalog::for_test(vec![source.item(7).unwrap().clone()], HashMap::new());
    let mut document = emote_document(16);
    let before = document.clone();
    let result = randomize_full_loadout(
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
    );
    assert!(result.unwrap_err().contains("Emote Collection"));
    assert_eq!(document, before);
}

#[test]
#[ignore = "requires SUNDIAL_TEST_INSTALL with the supported native packages"]
fn native_emote_loadout_respects_collection_capacity_and_randomizes_four_choices() {
    let install = std::path::PathBuf::from(std::env::var_os("SUNDIAL_TEST_INSTALL").unwrap());
    let cache = crate::test_support::TestDirectory::new("emote-native-catalog");
    let catalog =
        Catalog::load_or_scan_with_progress(&install, cache.0.join("catalog.json"), false, |_| {})
            .unwrap();
    let mut document = emote_document(16);
    randomize_full_loadout(
        &mut document,
        &catalog,
        0,
        PlugSelectionMode::AnyPlug,
        false,
        LoadoutOptions {
            equipment_flair: true,
            replace_held_inventory: true,
            ..Default::default()
        },
    )
    .unwrap();
    let hash = crate::account_contract::EMOTE_COLLECTION_DEFINITION_HASH;
    let definition = catalog.item(hash).unwrap();
    let equipped = account::equipped_item_snapshots(&document, 0).unwrap();
    let emote = equipped.iter().find(|item| item.slot == "emote").unwrap();
    assert_eq!(emote.definition_hash, Some(hash));
    let held = account::character_inventory(&document, 0).unwrap().unwrap();
    let emotes = held
        .iter()
        .filter(|item| {
            catalog
                .item(u64::from(item.definition_hash))
                .is_some_and(|item| item.bucket_hash == crate::account_contract::EMOTE_BUCKET_HASH)
        })
        .collect::<Vec<_>>();
    assert_eq!(
        catalog
            .inventory_metadata(hash)
            .unwrap()
            .authored_row_capacity(),
        Some(1)
    );
    assert!(
        emotes.is_empty(),
        "The equipped collection fills its one native row"
    );
    for plugs in std::iter::once(equipped_plugs_value(&emote.plugs).unwrap())
        .chain(emotes.iter().map(|item| inventory_plugs_value(&item.plugs)))
    {
        let plugs = plugs.as_array().unwrap();
        assert_eq!(plugs.len(), 4);
        for (index, plug) in plugs.iter().enumerate() {
            let hash = parse_unsigned_value(plug).unwrap();
            assert!(
                catalog
                    .socket_options(&definition.sockets[index])
                    .contains(&hash)
            );
        }
    }
    let mut bucket_counts = HashMap::<u8, (usize, usize)>::new();
    for hash in equipped
        .iter()
        .filter_map(|item| item.definition_hash)
        .chain(held.iter().map(|item| u64::from(item.definition_hash)))
    {
        let metadata = catalog.inventory_metadata(hash).unwrap();
        let capacity = usize::from(metadata.authored_row_capacity().unwrap());
        let count = bucket_counts
            .entry(metadata.native_bucket_id)
            .or_insert((0, capacity));
        count.0 += 1;
        assert!(
            count.0 <= capacity,
            "{} exceeds {capacity} rows",
            metadata.bucket_label()
        );
    }
    assert_eq!(
        bucket_counts.get(&49),
        Some(&(4, 4)),
        "Seasonal artifacts must include one equipped and three stored items"
    );
    assert_eq!(bucket_counts.get(&12), Some(&(1, 1)));
}

#[test]
fn emote_loadout_preserves_locked_items_and_obeys_inventory_replacement() {
    let catalog = emote_catalog();
    for replace in [false, true] {
        let mut document = emote_document(16);
        let equipped = serde_json::json!({"instance_soid":3,"definition_hash":7,"level":0,"quantity":1,"plugs":null,"flags":1});
        let locked = serde_json::json!({"instance_soid":4,"definition_hash":7,"level":0,"quantity":1,"plugs":null,"flags":1});
        let unlocked = serde_json::json!({"instance_soid":5,"definition_hash":7,"level":0,"quantity":1,"plugs":null,"flags":0});
        let mut raw = document.json().clone();
        raw["state"]["characters"][0]["equipment"]["emote"] = equipped.clone();
        raw["state"]["characters"][0]["inventory"] = serde_json::json!([locked, unlocked]);
        // Leave equipment unlocked for the no-replacement case so there is still a selected change.
        if !replace {
            raw["state"]["characters"][0]["equipment"]["emote"]["flags"] = 0.into();
        }
        document = account::WorkspaceDocument::json_only(raw);
        randomize_full_loadout(
            &mut document,
            &catalog,
            0,
            PlugSelectionMode::Supported,
            false,
            LoadoutOptions {
                equipment_flair: true,
                replace_held_inventory: replace,
                keep_locked_items: true,
                ..Default::default()
            },
        )
        .unwrap();
        let held = document.json()["state"]["characters"][0]["inventory"]
            .as_array()
            .unwrap();
        assert_eq!(held[0], locked);
        if replace {
            assert_eq!(
                document.json()["state"]["characters"][0]["equipment"]["emote"],
                equipped
            );
            assert_eq!(
                held.len(),
                2,
                "One locked legacy emote and one collection in its separate one-slot bucket"
            );
            assert!(
                held[1..]
                    .iter()
                    .all(|item| parse_unsigned_value(&item["definition_hash"])
                        == Some(crate::account_contract::EMOTE_COLLECTION_DEFINITION_HASH))
            );
        } else {
            assert_eq!(held, &vec![locked, unlocked]);
        }
    }
}

#[test]
fn replacing_flair_preserves_unrecognized_inventory_items() {
    let mut document = account::WorkspaceDocument::json_only(serde_json::json!({
        "version": 8, "state": {"characters": [{"inventory": [{
            "instance_soid": 2, "definition_hash": 999, "level": 100,
            "quantity": 1, "plugs": null
        }]}]}
    }));
    let before = document.clone();
    let catalog = Catalog::for_test(Vec::new(), HashMap::new());
    clear_selected_inventory(
        &mut document,
        &catalog,
        0,
        LoadoutOptions {
            equipment_flair: true,
            replace_held_inventory: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(document, before);
    assert_eq!(loadout_scope_for_bucket(u64::MAX), None);
}

#[test]
fn exotic_detection_uses_rarity_not_perk_sockets() {
    use crate::catalog::{ItemPackageMetadata, ItemRarity, SocketDef};
    let mut item = ItemDef {
        hash: 1,
        name: "Custom weapon".into(),
        type_name: "Sidearm".into(),
        bucket_hash: SLOTS[0].2,
        class_type: 3,
        default_plugs: vec![],
        sockets: vec![],
        abilities: Default::default(),
    };
    let catalog = Catalog::for_test(
        vec![item.clone()],
        HashMap::from([(
            1,
            ItemPackageMetadata {
                rarity: ItemRarity::Exotic,
                ..Default::default()
            },
        )]),
    );
    assert!(is_exotic(&catalog, &item));
    item.sockets.push(SocketDef {
        socket_type: 377,
        ..Default::default()
    });
    let catalog = Catalog::for_test(
        vec![item.clone()],
        HashMap::from([(
            1,
            ItemPackageMetadata {
                rarity: ItemRarity::Legendary,
                ..Default::default()
            },
        )]),
    );
    assert!(!is_exotic(&catalog, &item));
}

#[test]
fn seeded_random_choices_are_repeatable() {
    let mut first = Rng::from_seed(42);
    let mut second = Rng::from_seed(42);
    let options = [3, 5, 7, 11];
    assert_eq!(first.pick(&options), second.pick(&options));
    assert_eq!(first.pick(&options), second.pick(&options));
}

#[test]
fn hash_choices_skip_the_engine_empty_marker() {
    let mut rng = Rng::from_seed(7);
    let no_definition_hash = u64::from(NO_DEFINITION_HASH.get());
    assert_eq!(rng.pick_valid_hash(&[no_definition_hash, 7]), Some(7));
    assert_eq!(rng.pick_valid_hash(&[no_definition_hash]), None);
}

#[test]
fn loadout_scopes_default_off_and_partition_every_slot() {
    let options = LoadoutOptions::default();
    assert!(!options.any());
    assert!(!options.replace_held_inventory);
    assert!(options.keep_locked_items);
    assert_eq!(loadout_scope_for_slot("kinetic"), LoadoutScope::Weapons);
    assert_eq!(loadout_scope_for_slot("helmet"), LoadoutScope::Armor);
    assert_eq!(loadout_scope_for_slot("subclass"), LoadoutScope::Subclass);
    assert_eq!(loadout_scope_for_slot("ship"), LoadoutScope::EquipmentFlair);
}

#[test]
fn unchecked_loadout_scopes_are_excluded_without_affecting_the_others() {
    let options = LoadoutOptions {
        weapons: false,
        armor: true,
        equipment_flair: false,
        subclass: true,
        replace_held_inventory: false,
        keep_locked_items: true,
    };
    let selected = SLOTS
        .iter()
        .filter(|(slot, _, _)| options.includes(loadout_scope_for_slot(slot)))
        .map(|(slot, _, _)| *slot)
        .collect::<Vec<_>>();
    assert_eq!(selected.len(), ARMOR_SLOTS.len() + 1);
    assert!(selected.contains(&"helmet"));
    assert!(selected.contains(&SUBCLASS_SLOT));
    assert!(!selected.contains(&"kinetic"));
    assert!(!selected.contains(&"ship"));
}

#[test]
fn loadout_lock_detection_only_uses_the_locked_flag() {
    assert!(loadout_item_is_locked(Some(
        inventory::INVENTORY_FLAG_LOCKED
    )));
    assert!(loadout_item_is_locked(Some(
        inventory::INVENTORY_FLAG_LOCKED | inventory::INVENTORY_FLAG_TRACKED
    )));
    assert!(!loadout_item_is_locked(None));
    assert!(!loadout_item_is_locked(Some(
        inventory::INVENTORY_FLAG_TRACKED
    )));
}

#[test]
fn random_item_add_respects_native_bucket_capacity() {
    assert!(bucket_has_room_for_add(9, 0, 10));
    assert!(!bucket_has_room_for_add(10, 0, 10));
    assert!(!bucket_has_room_for_add(9, 1, 10));
    assert!(bucket_has_room_for_add(8, 1, 10));
}

#[test]
fn random_item_search_preserves_equipped_and_inventory_rolls() {
    let document = account::WorkspaceDocument::json_only(serde_json::json!({
    "version": 6,
    "state": {
        "characters": [{
            "equipment": {
                "kinetic": {
                    "instance_soid": 1,
                    "definition_hash": 11,
                    "level": 100,
                    "quantity": 1,
                    "plugs": [101, null, 103]
                }
            },
            "inventory": [{
                "instance_soid": 2,
                "definition_hash": 22,
                "level": 100,
                "quantity": 1,
                "plugs": [201, 202]
            }]
        }]
    }
    }));
    let matching_hashes = HashSet::from([11, 22]);

    let choices = matching_item_instances(&document, 0, &matching_hashes);

    assert_eq!(choices.len(), 2);
    assert_eq!(choices[0].request.item_hash, 11);
    assert_eq!(
        choices[0].request.authored_plugs,
        Some(serde_json::json!([101, null, 103]))
    );
    assert_eq!(choices[0].location, "Equipped · Kinetic");
    assert_eq!(choices[1].request.item_hash, 22);
    assert_eq!(
        choices[1].request.authored_plugs,
        Some(serde_json::json!([201, 202]))
    );
    assert_eq!(choices[1].location, "Inventory · item 1");
}

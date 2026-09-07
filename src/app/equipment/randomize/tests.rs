//! Equipment randomizer behavior and invariant tests.

use crate::app::account_workspace as account;

use super::*;

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
fn loadout_inventory_plan_matches_panoptes_distribution() {
    let options = LoadoutOptions {
        weapons: true,
        armor: true,
        equipment_flair: true,
        subclass: true,
        replace_held_inventory: true,
        keep_locked_items: true,
    };
    let equipment_slots = SLOTS
        .iter()
        .filter(|(slot, _, _)| options.includes(loadout_scope_for_slot(slot)))
        .count();
    let held_items = SLOTS
        .iter()
        .filter(|(slot, _, _)| {
            options.includes(loadout_scope_for_slot(slot))
                && !matches!(*slot, SUBCLASS_SLOT | CLAN_BANNER_SLOT)
        })
        .count()
        * HELD_ITEMS_PER_SLOT;
    assert_eq!(equipment_slots, 16);
    assert_eq!(held_items, 126);
    assert!(held_items <= inventory::CHARACTER_INVENTORY_CAPACITY);
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

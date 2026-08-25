//! Behavioral coverage for the inventory document contract and mutation facade.

use super::document::{LEGACY_DISMANTLE_REWARD_CAPACITY, NO_DEFINITION_HASH};
use super::*;
use serde_json::json;

fn item(soid: u64, hash: u32) -> Value {
    json!({
        "instance_soid": format_instance_soid(soid),
        "definition_hash": format_definition_hash_hex(hash),
        "level": 106,
        "quantity": 1,
        "plugs": null
    })
}

fn document(version: u64) -> Value {
    json!({
        "version": version,
        "state": {
            "account": {
                "primary_soid": "0x9EAA300100100100",
                "profile_items": []
            },
            "characters": [{
                "soid": "0x9EAA300200100100",
                "class": 0,
                "equipment": {},
                "inventory": []
            }]
        }
    })
}

fn add_character(document: &mut Value, soid: u64, class_type: u64) {
    document
        .pointer_mut("/state/characters")
        .and_then(Value::as_array_mut)
        .expect("test document has a characters array")
        .push(json!({
            "soid": format_instance_soid(soid),
            "class": class_type,
            "equipment": {},
            "inventory": []
        }));
}

#[test]
fn schema_modes_are_explicit_about_mutability() {
    let future_version = MAX_SUPPORTED_SCHEMA + 1;
    let future = SchemaMode::Future(future_version);

    assert_eq!(schema_mode(&json!({})), SchemaMode::MissingOrInvalid);
    assert_eq!(
        schema_mode(&json!({"version": 1})),
        SchemaMode::Unsupported(1)
    );
    assert_eq!(
        schema_mode(&json!({"version": 3})),
        SchemaMode::PreInventory(3)
    );
    for version in 6..=MAX_SUPPORTED_SCHEMA {
        assert_eq!(
            schema_mode(&json!({"version": version})),
            SchemaMode::Inventory(version)
        );
    }
    assert_eq!(schema_mode(&json!({"version": future_version})), future);
    assert!(!future.is_read_only());
    assert!(future.is_future());
    assert!(SchemaMode::Unsupported(1).is_read_only());
    assert!(!SchemaMode::Unsupported(1).can_mutate_profile_items());
    assert!(!SchemaMode::MissingOrInvalid.can_mutate_equipment());
    assert!(!SchemaMode::Unsupported(1).can_mutate_equipment());
    assert!(future.can_mutate_profile_items());
    assert!(future.can_mutate_character_inventory());
    assert!(future.can_mutate_equipment());
    assert!(!SchemaMode::MissingOrInvalid.supports_equipment_flags());
    assert!(!SchemaMode::Unsupported(1).supports_equipment_flags());
    assert!(future.supports_equipment_flags());
    assert!(future.can_mutate_equipment_flags());
    assert_eq!(future.profile_item_capacity(), Some(PROFILE_ITEM_CAPACITY));
    for version in 2..=5 {
        let mode = schema_mode(&json!({"version": version}));
        assert!(mode.can_mutate_profile_items());
        assert!(!mode.can_mutate_character_inventory());
        assert!(mode.can_mutate_equipment());
        assert_eq!(mode.supports_equipment_flags(), version >= 4);
        assert_eq!(mode.can_mutate_equipment_flags(), version >= 4);
        assert_eq!(mode.supports_dismantle_rewards(), version >= 5);
    }
    let current = SchemaMode::Inventory(MAX_SUPPORTED_SCHEMA);
    assert!(current.can_mutate_profile_items());
    assert!(current.can_mutate_character_inventory());
    assert!(current.can_mutate_equipment());
    assert!(current.supports_equipment_flags());
    assert!(current.can_mutate_equipment_flags());
    assert!(current.supports_dismantle_rewards());
    assert_eq!(profile_item_capacity(3), 32);
    assert_eq!(profile_item_capacity(4), 701);
}

#[test]
fn reading_missing_sections_never_materializes_them() {
    let document = json!({
        "version": 6,
        "state": {"account": {}, "characters": [{"soid": 1}]}
    });
    let before = document.clone();
    assert_eq!(profile_items(&document).unwrap(), None);
    assert_eq!(character_inventory(&document, 0).unwrap(), None);
    assert_eq!(document, before);
}

#[test]
fn profile_actions_preserve_order_and_unknown_fields() {
    let mut document = document(3);
    *document
        .pointer_mut("/state/account/profile_items")
        .unwrap() = json!([
        {"definition_hash": "0x00000001", "quantity": 2, "future": [1, 2]},
        {"definition_hash": 2, "quantity": 3}
    ]);

    apply_profile_item_action(
        &mut document,
        ProfileItemLocation { index: 0 },
        ProfileItemAction::SetQuantity(9),
    )
    .unwrap();
    assert_eq!(
        document.pointer("/state/account/profile_items/0/future"),
        Some(&json!([1, 2]))
    );
    assert_eq!(
        document.pointer("/state/account/profile_items/1/definition_hash"),
        Some(&Value::from(2))
    );

    let added = add_profile_item(&mut document, 3, 4).unwrap();
    assert_eq!(added.index, 2);
    assert_eq!(
        document.pointer("/state/account/profile_items/2"),
        Some(&json!({"definition_hash": "0x00000003", "quantity": 4}))
    );

    apply_profile_item_action(
        &mut document,
        ProfileItemLocation { index: 1 },
        ProfileItemAction::Remove,
    )
    .unwrap();
    assert_eq!(
        document.pointer("/state/account/profile_items/1/definition_hash"),
        Some(&Value::String("0x00000003".into()))
    );
}

#[test]
fn malformed_profile_sections_are_rejected_without_mutation() {
    let mut document = document(6);
    *document
        .pointer_mut("/state/account/profile_items")
        .unwrap() = Value::String("bad".into());
    let before = document.clone();
    let error = add_profile_item(&mut document, 1, 1).unwrap_err();
    assert_eq!(error.path(), "/state/account/profile_items");
    assert!(error.message().contains("array"));
    assert_eq!(document, before);
}

#[test]
fn profile_hashes_reject_the_engine_sentinel_without_mutation() {
    let mut parsed = document(6);
    *parsed.pointer_mut("/state/account/profile_items").unwrap() = json!([{
        "definition_hash": format_definition_hash_hex(NO_DEFINITION_HASH),
        "quantity": 1
    }]);
    let error = validate_document_items(&parsed).unwrap_err();
    assert_eq!(
        error.path(),
        "/state/account/profile_items/0/definition_hash"
    );

    let mut added = document(6);
    let before = added.clone();
    assert!(add_profile_item(&mut added, NO_DEFINITION_HASH, 1).is_err());
    assert_eq!(added, before);

    add_profile_item(&mut added, 1, 1).unwrap();
    let before = added.clone();
    assert!(
        apply_profile_item_action(
            &mut added,
            ProfileItemLocation { index: 0 },
            ProfileItemAction::SetDefinitionHash(NO_DEFINITION_HASH),
        )
        .is_err()
    );
    assert_eq!(added, before);
}

#[test]
fn profile_capacity_follows_schema_history() {
    let mut legacy = document(3);
    *legacy.pointer_mut("/state/account/profile_items").unwrap() = Value::Array(
        (0..LEGACY_PROFILE_ITEM_CAPACITY)
            .map(|index| json!({"definition_hash": index, "quantity": 1}))
            .collect(),
    );
    let before = legacy.clone();
    assert!(add_profile_item(&mut legacy, 99, 1).is_err());
    assert_eq!(legacy, before);

    let mut newer = document(4);
    *newer.pointer_mut("/state/account/profile_items").unwrap() = Value::Array(
        (0..LEGACY_PROFILE_ITEM_CAPACITY)
            .map(|index| json!({"definition_hash": index, "quantity": 1}))
            .collect(),
    );
    assert!(add_profile_item(&mut newer, 99, 1).is_ok());
}

#[test]
fn future_schema_does_not_assume_the_last_known_profile_capacity() {
    let mut future = document(MAX_SUPPORTED_SCHEMA + 1);
    *future.pointer_mut("/state/account/profile_items").unwrap() = Value::Array(
        (1..=PROFILE_ITEM_CAPACITY + 1)
            .map(|hash| json!({"definition_hash": hash, "quantity": 1}))
            .collect(),
    );

    assert_eq!(
        profile_items(&future).unwrap().unwrap().len(),
        PROFILE_ITEM_CAPACITY + 1
    );
    assert_eq!(validate_document_items(&future), Ok(()));

    future["version"] = Value::from(MAX_SUPPORTED_SCHEMA);
    assert!(profile_items(&future).is_err());
}

#[test]
fn unsupported_and_pre_inventory_schemas_keep_unsupported_edits_read_only() {
    let mut unsupported = document(1);
    let before = unsupported.clone();
    assert!(add_profile_item(&mut unsupported, 1, 1).is_err());
    assert!(validate_document_items(&unsupported).is_err());
    assert_eq!(unsupported, before);

    let mut old = document(5);
    *old.pointer_mut("/state/characters/0/inventory").unwrap() =
        Value::Array(vec![item(GENERATED_INSTANCE_SOID_START, 1)]);
    assert_eq!(character_inventory(&old, 0).unwrap().unwrap().len(), 1);
    let before = old.clone();
    assert!(
        apply_inventory_item_action(
            &mut old,
            InventoryItemLocation {
                character_index: 0,
                item_index: 0
            },
            InventoryItemAction::SetQuantity(2)
        )
        .is_err()
    );
    assert_eq!(old, before);
}

#[test]
fn future_schema_edits_known_item_fields_and_preserves_opaque_data() {
    let future_version = MAX_SUPPORTED_SCHEMA + 73;
    let inventory_soid = GENERATED_INSTANCE_SOID_START;
    let equipment_soid = GENERATED_INSTANCE_SOID_START + 1;
    let opaque_slot_soid = GENERATED_INSTANCE_SOID_START + 2;
    let mut future = document(future_version);

    *future.pointer_mut("/state/account/profile_items").unwrap() = json!([{
        "definition_hash": "0x00000001",
        "quantity": 2,
        "future_profile_data": {"keep": [1, 2, 3]}
    }]);
    future
        .pointer_mut("/state/account")
        .unwrap()
        .as_object_mut()
        .unwrap()
        .insert(
            "dismantle_rewards".into(),
            json!({"future_layout": ["leave", "untouched"]}),
        );

    let mut stored_item = item(inventory_soid, 2);
    stored_item["future_item_data"] = json!({"keep": true});
    *future.pointer_mut("/state/characters/0/inventory").unwrap() = Value::Array(vec![stored_item]);

    let mut equipped_item = item(equipment_soid, 3);
    equipped_item["future_equipment_data"] = json!([4, 5, 6]);
    *future.pointer_mut("/state/characters/0/equipment").unwrap() = json!({
        "kinetic": equipped_item,
        "future_slot": {
            "instance_soid": format_instance_soid(opaque_slot_soid),
            "opaque": {"keep": "all of this"}
        },
        "future_scalar_slot": ["an", "unknown", "shape"]
    });
    future["future_root_data"] = json!({"keep": true});

    assert_eq!(validate_document_items(&future), Ok(()));
    assert!(
        collect_used_soids(&future)
            .unwrap()
            .contains(&opaque_slot_soid)
    );

    let added = add_inventory_item(&mut future, 0, NewInventoryItem::single(4, 106)).unwrap();
    assert_eq!(
        added,
        InventoryItemLocation {
            character_index: 0,
            item_index: 1,
        }
    );
    assert_eq!(
        future.pointer("/state/characters/0/inventory/1/instance_soid"),
        Some(&Value::String(format_instance_soid(
            GENERATED_INSTANCE_SOID_START + 3
        )))
    );

    apply_profile_item_action(
        &mut future,
        ProfileItemLocation { index: 0 },
        ProfileItemAction::SetQuantity(9),
    )
    .unwrap();
    apply_inventory_item_action(
        &mut future,
        InventoryItemLocation {
            character_index: 0,
            item_index: 0,
        },
        InventoryItemAction::SetQuantity(7),
    )
    .unwrap();

    assert_eq!(
        future.pointer("/state/account/profile_items/0/quantity"),
        Some(&Value::from(9))
    );
    assert_eq!(
        future.pointer("/state/characters/0/inventory/0/quantity"),
        Some(&Value::from(7))
    );
    assert_eq!(
        future.pointer("/state/account/profile_items/0/future_profile_data/keep"),
        Some(&json!([1, 2, 3]))
    );
    assert_eq!(
        future.pointer("/state/characters/0/inventory/0/future_item_data/keep"),
        Some(&Value::Bool(true))
    );
    assert_eq!(
        future.pointer("/state/characters/0/equipment/kinetic/future_equipment_data"),
        Some(&json!([4, 5, 6]))
    );
    assert_eq!(
        future.pointer("/state/characters/0/equipment/future_slot/opaque/keep"),
        Some(&Value::String("all of this".into()))
    );
    assert_eq!(
        future.pointer("/state/characters/0/equipment/future_scalar_slot"),
        Some(&json!(["an", "unknown", "shape"]))
    );
    assert_eq!(
        future.pointer("/state/account/dismantle_rewards/future_layout"),
        Some(&json!(["leave", "untouched"]))
    );
    assert_eq!(
        future.pointer("/future_root_data/keep"),
        Some(&Value::Bool(true))
    );

    assert!(
        swap_inventory_item_with_equipment(
            &mut future,
            InventoryItemLocation {
                character_index: 0,
                item_index: 0,
            },
            "kinetic",
        )
        .unwrap()
    );
    assert_eq!(
        future.pointer("/state/characters/0/equipment/kinetic/future_item_data/keep"),
        Some(&Value::Bool(true))
    );
    assert_eq!(
        future.pointer("/state/characters/0/inventory/0/future_equipment_data"),
        Some(&json!([4, 5, 6]))
    );
    assert_eq!(
        future.pointer("/state/characters/0/equipment/future_scalar_slot"),
        Some(&json!(["an", "unknown", "shape"]))
    );
    assert_eq!(validate_document_items(&future), Ok(()));

    let encoded = super::super::settings::encode_settings(&future).unwrap();
    let reparsed: Value = serde_json::from_str(&encoded).unwrap();
    assert_eq!(reparsed, future);
}

#[test]
fn future_schema_rejects_malformed_known_fields_without_mutation() {
    let mut future = document(MAX_SUPPORTED_SCHEMA + 91);
    let mut malformed = item(GENERATED_INSTANCE_SOID_START, 1);
    malformed["quantity"] = json!({"future_quantity_shape": 1});
    malformed["unknown_item_data"] = json!({"keep": true});
    *future.pointer_mut("/state/characters/0/inventory").unwrap() = Value::Array(vec![malformed]);

    let before = future.clone();
    let error = apply_inventory_item_action(
        &mut future,
        InventoryItemLocation {
            character_index: 0,
            item_index: 0,
        },
        InventoryItemAction::SetLevel(107),
    )
    .unwrap_err();

    assert!(error.path().ends_with("/quantity"));
    assert_eq!(future, before);
}

#[test]
fn explicit_v6_add_materializes_only_the_inventory_leaf() {
    let mut document = document(6);
    let character = document
        .pointer_mut("/state/characters/0")
        .unwrap()
        .as_object_mut()
        .unwrap();
    character.remove("inventory");
    character.insert("future_character_data".into(), json!({"keep": true}));

    let location = add_inventory_item(&mut document, 0, NewInventoryItem::single(42, 106)).unwrap();
    assert_eq!(
        location,
        InventoryItemLocation {
            character_index: 0,
            item_index: 0
        }
    );
    let created = document.pointer("/state/characters/0/inventory/0").unwrap();
    assert_eq!(created.get("plugs"), Some(&Value::Null));
    assert!(created.get("flags").is_none());
    assert_eq!(created.get("quantity"), Some(&Value::from(1)));
    assert_eq!(
        document.pointer("/state/characters/0/future_character_data/keep"),
        Some(&Value::Bool(true))
    );
}

#[test]
fn item_state_edits_preserve_unrelated_flags() {
    assert_eq!(
        set_inventory_locked_flag(None, true),
        Some(INVENTORY_FLAG_LOCKED)
    );
    assert_eq!(
        set_inventory_locked_flag(Some(INVENTORY_FLAG_LOCKED), false),
        None
    );
    assert_eq!(
        set_inventory_locked_flag(Some(INVENTORY_FLAG_TRACKED), true),
        Some(INVENTORY_FLAG_LOCKED | INVENTORY_FLAG_TRACKED)
    );
    assert_eq!(
        set_inventory_locked_flag(Some(INVENTORY_FLAG_MASK), false),
        Some(INVENTORY_FLAG_TRACKED)
    );
}

#[test]
fn inventory_actions_preserve_identity_order_and_unknown_fields() {
    let mut document = document(6);
    *document
        .pointer_mut("/state/characters/0/inventory")
        .unwrap() = json!([
        {
            "instance_soid": "0x4000000000000001",
            "definition_hash": "0x00000001",
            "level": 106,
            "quantity": 1,
            "plugs": null,
            "flags": 1,
            "future": {"keep": true}
        },
        {
            "instance_soid": "0x4000000000000002",
            "definition_hash": "0x00000002",
            "level": 106,
            "quantity": 1,
            "plugs": []
        }
    ]);
    let first = InventoryItemLocation {
        character_index: 0,
        item_index: 0,
    };
    apply_inventory_item_action(
        &mut document,
        first,
        InventoryItemAction::SetDefinitionHash(3),
    )
    .unwrap();
    apply_inventory_item_action(
        &mut document,
        first,
        InventoryItemAction::SetPlugs(ItemPlugs::Authored(vec![Some(4), None])),
    )
    .unwrap();
    apply_inventory_item_action(&mut document, first, InventoryItemAction::SetFlags(None)).unwrap();
    let edited = document.pointer("/state/characters/0/inventory/0").unwrap();
    assert_eq!(
        edited.get("instance_soid"),
        Some(&Value::String("0x4000000000000001".into()))
    );
    assert_eq!(edited.pointer("/future/keep"), Some(&Value::Bool(true)));
    assert_eq!(edited.get("plugs"), Some(&json!(["0x00000004", null])));
    assert!(edited.get("flags").is_none());
    assert_eq!(
        document.pointer("/state/characters/0/inventory/1/instance_soid"),
        Some(&Value::String("0x4000000000000002".into()))
    );
}

#[test]
fn equipping_a_stored_item_swaps_the_complete_authored_rows() {
    let mut document = document(6);
    let mut equipped = item(1, 10);
    equipped["equipped_only"] = json!({"preserved": true});
    let mut stored = item(2, 20);
    stored["stored_only"] = json!([1, 2, 3]);
    let untouched = item(3, 30);
    *document
        .pointer_mut("/state/characters/0/equipment")
        .unwrap() = json!({"kinetic": equipped.clone()});
    *document
        .pointer_mut("/state/characters/0/inventory")
        .unwrap() = Value::Array(vec![stored.clone(), untouched.clone()]);

    let replaced = swap_inventory_item_with_equipment(
        &mut document,
        InventoryItemLocation {
            character_index: 0,
            item_index: 0,
        },
        "kinetic",
    )
    .unwrap();

    assert!(replaced);
    assert_eq!(
        document.pointer("/state/characters/0/equipment/kinetic"),
        Some(&stored)
    );
    assert_eq!(
        document.pointer("/state/characters/0/inventory/0"),
        Some(&equipped)
    );
    assert_eq!(
        document.pointer("/state/characters/0/inventory/1"),
        Some(&untouched)
    );
}

#[test]
fn equipping_into_an_empty_slot_removes_only_the_moved_inventory_row() {
    let mut document = document(6);
    let stored = item(1, 10);
    let untouched = item(2, 20);
    *document
        .pointer_mut("/state/characters/0/equipment")
        .unwrap() = json!({"energy": null});
    *document
        .pointer_mut("/state/characters/0/inventory")
        .unwrap() = Value::Array(vec![stored.clone(), untouched.clone()]);

    let replaced = swap_inventory_item_with_equipment(
        &mut document,
        InventoryItemLocation {
            character_index: 0,
            item_index: 0,
        },
        "energy",
    )
    .unwrap();

    assert!(!replaced);
    assert_eq!(
        document.pointer("/state/characters/0/equipment/energy"),
        Some(&stored)
    );
    assert_eq!(
        document.pointer("/state/characters/0/inventory"),
        Some(&Value::Array(vec![untouched]))
    );
}

#[test]
fn failed_inventory_equipment_swaps_are_atomic() {
    let mut document = document(6);
    *document
        .pointer_mut("/state/characters/0/equipment")
        .unwrap() = json!({
        "kinetic": {
            "instance_soid": "0x4000000000000001",
            "definition_hash": "0x0000000A",
            "level": 106,
            "quantity": 1
        }
    });
    *document
        .pointer_mut("/state/characters/0/inventory")
        .unwrap() = Value::Array(vec![item(2, 20)]);
    let before = document.clone();

    assert!(
        swap_inventory_item_with_equipment(
            &mut document,
            InventoryItemLocation {
                character_index: 0,
                item_index: 0,
            },
            "kinetic",
        )
        .is_err()
    );
    assert_eq!(document, before);
}

#[test]
fn moving_between_characters_preserves_the_complete_authored_row() {
    let mut document = document(6);
    add_character(&mut document, 0x9EAA_3002_0020_0100, 1);
    let mut moved = item(1, 10);
    moved["flags"] = json!(1);
    moved["future"] = json!({"preserved": true});
    let source_untouched = item(2, 20);
    let destination_untouched = item(3, 30);
    *document
        .pointer_mut("/state/characters/0/inventory")
        .unwrap() = json!([moved.clone(), source_untouched.clone()]);
    *document
        .pointer_mut("/state/characters/1/inventory")
        .unwrap() = json!([destination_untouched.clone()]);

    let destination = move_inventory_item_to_character(
        &mut document,
        InventoryItemLocation {
            character_index: 0,
            item_index: 0,
        },
        1,
    )
    .unwrap();

    assert_eq!(
        destination,
        InventoryItemLocation {
            character_index: 1,
            item_index: 1,
        }
    );
    assert_eq!(
        document.pointer("/state/characters/0/inventory"),
        Some(&json!([source_untouched]))
    );
    assert_eq!(
        document.pointer("/state/characters/1/inventory"),
        Some(&json!([destination_untouched, moved]))
    );
}

#[test]
fn moving_between_characters_creates_a_missing_destination_inventory() {
    let mut document = document(6);
    add_character(&mut document, 0x9EAA_3002_0020_0100, 1);
    let moved = item(1, 10);
    *document
        .pointer_mut("/state/characters/0/inventory")
        .unwrap() = json!([moved.clone()]);
    document
        .pointer_mut("/state/characters/1")
        .and_then(Value::as_object_mut)
        .unwrap()
        .remove("inventory");

    move_inventory_item_to_character(
        &mut document,
        InventoryItemLocation {
            character_index: 0,
            item_index: 0,
        },
        1,
    )
    .unwrap();

    assert_eq!(
        document.pointer("/state/characters/0/inventory"),
        Some(&json!([]))
    );
    assert_eq!(
        document.pointer("/state/characters/1/inventory"),
        Some(&json!([moved]))
    );
}

#[test]
fn failed_moves_between_characters_are_atomic() {
    let mut document = document(6);
    add_character(&mut document, 0x9EAA_3002_0020_0100, 1);
    *document
        .pointer_mut("/state/characters/0/inventory")
        .unwrap() = json!([item(1, 10)]);
    let before_same_character = document.clone();

    let same_character_error = move_inventory_item_to_character(
        &mut document,
        InventoryItemLocation {
            character_index: 0,
            item_index: 0,
        },
        0,
    )
    .unwrap_err();
    assert!(same_character_error.message().contains("must be different"));
    assert_eq!(document, before_same_character);

    *document
        .pointer_mut("/state/characters/1/inventory")
        .unwrap() = Value::Array(
        (0..CHARACTER_INVENTORY_CAPACITY)
            .map(|index| item(index as u64 + 2, 20))
            .collect(),
    );
    let before_full_character = document.clone();

    let full_character_error = move_inventory_item_to_character(
        &mut document,
        InventoryItemLocation {
            character_index: 0,
            item_index: 0,
        },
        1,
    )
    .unwrap_err();
    assert!(full_character_error.message().contains("inventory is full"));
    assert_eq!(document, before_full_character);
}

#[test]
fn unequipping_moves_the_complete_authored_row_to_inventory() {
    let mut document = document(6);
    let mut equipped = item(1, 10);
    equipped["equipped_only"] = json!({"preserved": true});
    let untouched = item(2, 20);
    *document
        .pointer_mut("/state/characters/0/equipment")
        .unwrap() = json!({"kinetic": equipped.clone()});
    *document
        .pointer_mut("/state/characters/0/inventory")
        .unwrap() = Value::Array(vec![untouched.clone()]);

    move_equipment_item_to_inventory(&mut document, 0, "kinetic").unwrap();

    assert_eq!(
        document.pointer("/state/characters/0/equipment/kinetic"),
        Some(&Value::Null)
    );
    assert_eq!(
        document.pointer("/state/characters/0/inventory"),
        Some(&Value::Array(vec![untouched, equipped]))
    );
}

#[test]
fn unequipping_creates_a_missing_inventory_array() {
    let mut document = document(6);
    let equipped = item(1, 10);
    document
        .pointer_mut("/state/characters/0")
        .and_then(Value::as_object_mut)
        .unwrap()
        .remove("inventory");
    *document
        .pointer_mut("/state/characters/0/equipment")
        .unwrap() = json!({"energy": equipped.clone()});

    move_equipment_item_to_inventory(&mut document, 0, "energy").unwrap();

    assert_eq!(
        document.pointer("/state/characters/0/inventory"),
        Some(&Value::Array(vec![equipped]))
    );
    assert_eq!(
        document.pointer("/state/characters/0/equipment/energy"),
        Some(&Value::Null)
    );
}

#[test]
fn failed_unequips_are_atomic() {
    let mut document = document(6);
    *document
        .pointer_mut("/state/characters/0/equipment")
        .unwrap() = json!({"heavy": item(1, 10)});
    *document
        .pointer_mut("/state/characters/0/inventory")
        .unwrap() = Value::Array(
        (0..CHARACTER_INVENTORY_CAPACITY)
            .map(|index| item(index as u64 + 2, 20))
            .collect(),
    );
    let before = document.clone();

    let error = move_equipment_item_to_inventory(&mut document, 0, "heavy").unwrap_err();

    assert!(
        error.message().contains("inventory is full"),
        "unexpected error: {error}"
    );
    assert_eq!(document, before);
}

#[test]
fn strict_v6_validation_reports_unknown_item_members_without_deleting_them() {
    let mut inventory_document = document(6);
    let mut row = item(1, 1);
    row["future"] = json!({"keep": true});
    *inventory_document
        .pointer_mut("/state/characters/0/inventory")
        .unwrap() = Value::Array(vec![row]);

    let error = validate_document_items(&inventory_document).unwrap_err();
    assert_eq!(error.path(), "/state/characters/0/inventory/0/future");
    apply_inventory_item_action(
        &mut inventory_document,
        InventoryItemLocation {
            character_index: 0,
            item_index: 0,
        },
        InventoryItemAction::SetQuantity(2),
    )
    .unwrap();
    assert_eq!(
        inventory_document.pointer("/state/characters/0/inventory/0/future/keep"),
        Some(&Value::Bool(true))
    );

    let mut profile = document(6);
    *profile.pointer_mut("/state/account/profile_items").unwrap() =
        json!([{"definition_hash": 1, "quantity": 1, "future": true}]);
    assert_eq!(validate_document_items(&profile), Ok(()));
}

#[test]
fn inventory_validation_covers_required_bounds() {
    let invalid_values = [
        ("instance_soid", Value::from(0)),
        ("definition_hash", Value::from(u64::from(u32::MAX) + 1)),
        ("level", Value::from(-1)),
        ("quantity", Value::from(0)),
        ("flags", Value::from(8)),
    ];
    for (key, value) in invalid_values {
        let mut document = document(6);
        let mut row = item(1, 1);
        row.as_object_mut().unwrap().insert(key.into(), value);
        *document
            .pointer_mut("/state/characters/0/inventory")
            .unwrap() = Value::Array(vec![row]);
        let error = validate_document_items(&document).unwrap_err();
        assert!(error.path().ends_with(key), "unexpected error: {error}");
    }

    let mut too_many_plugs = document(6);
    let mut row = item(1, 1);
    row["plugs"] = Value::Array(vec![Value::Null; MAX_ITEM_PLUGS + 1]);
    *too_many_plugs
        .pointer_mut("/state/characters/0/inventory")
        .unwrap() = Value::Array(vec![row]);
    assert!(validate_document_items(&too_many_plugs).is_err());

    let mut too_many_items = document(6);
    *too_many_items
        .pointer_mut("/state/characters/0/inventory")
        .unwrap() = Value::Array(
        (0..=CHARACTER_INVENTORY_CAPACITY)
            .map(|index| item(index as u64 + 1, 1))
            .collect(),
    );
    assert!(validate_document_items(&too_many_items).is_err());

    let mut quoted_flags = document(6);
    let mut row = item(1, 1);
    row["flags"] = Value::String("0x3".into());
    *quoted_flags
        .pointer_mut("/state/characters/0/inventory")
        .unwrap() = Value::Array(vec![row]);
    assert_eq!(
        character_inventory(&quoted_flags, 0).unwrap().unwrap()[0].flags,
        Some(3)
    );
}

#[test]
fn document_validation_requires_the_account_primary_soid() {
    let mut document = document(6);
    document
        .pointer_mut("/state/account")
        .unwrap()
        .as_object_mut()
        .unwrap()
        .remove("primary_soid");
    let error = validate_document_items(&document).unwrap_err();
    assert_eq!(error.path(), "/state/account/primary_soid");
}

#[test]
fn schemas_five_through_seven_dismantle_rewards_follow_legacy_constraints() {
    for version in 5..=7 {
        let mut valid = document(version);
        *valid
            .pointer_mut("/state/account")
            .unwrap()
            .as_object_mut()
            .unwrap()
            .entry("dismantle_rewards")
            .or_insert(Value::Null) = json!([
            {
                "definition_hash": "0x00000001",
                "quantity": 1,
                "future": {"preserved": true}
            },
            {"definition_hash": 2, "quantity": i32::MAX}
        ]);
        assert_eq!(validate_document_items(&valid), Ok(()));
        assert_eq!(
            valid.pointer("/state/account/dismantle_rewards/0/future/preserved"),
            Some(&Value::Bool(true))
        );
    }

    let invalid = [
        (json!("not an array"), "/state/account/dismantle_rewards"),
        (json!(["not an object"]), "/dismantle_rewards/0"),
        (
            json!([{"quantity": 1}]),
            "/dismantle_rewards/0/definition_hash",
        ),
        (
            json!([{"definition_hash": 1}]),
            "/dismantle_rewards/0/quantity",
        ),
        (
            json!([{"definition_hash": 0, "quantity": 1}]),
            "/dismantle_rewards/0/definition_hash",
        ),
        (
            json!([{
            "definition_hash": format_definition_hash_hex(NO_DEFINITION_HASH),
                "quantity": 1
            }]),
            "/dismantle_rewards/0/definition_hash",
        ),
        (
            json!([{
                "definition_hash": u64::from(u32::MAX) + 1,
                "quantity": 1
            }]),
            "/dismantle_rewards/0/definition_hash",
        ),
        (
            json!([{"definition_hash": 1, "quantity": 0}]),
            "/dismantle_rewards/0/quantity",
        ),
        (
            json!([{"definition_hash": 1, "quantity": "0x1"}]),
            "/dismantle_rewards/0/quantity",
        ),
        (
            json!([{
                "definition_hash": 1,
                "quantity": i64::from(i32::MAX) + 1
            }]),
            "/dismantle_rewards/0/quantity",
        ),
        (
            json!([
                {"definition_hash": 1, "quantity": 1},
                {"definition_hash": 1, "quantity": 2}
            ]),
            "/dismantle_rewards/1/definition_hash",
        ),
        (
            Value::Array(
                (1..=LEGACY_DISMANTLE_REWARD_CAPACITY + 1)
                    .map(|hash| json!({"definition_hash": hash, "quantity": 1}))
                    .collect(),
            ),
            "/state/account/dismantle_rewards",
        ),
    ];
    for version in 5..=7 {
        for (rewards, expected_path_suffix) in &invalid {
            let mut candidate = document(version);
            candidate
                .pointer_mut("/state/account")
                .unwrap()
                .as_object_mut()
                .unwrap()
                .insert("dismantle_rewards".into(), rewards.clone());
            let before = candidate.clone();
            let error = validate_document_items(&candidate).unwrap_err();
            assert!(
                error.path().ends_with(expected_path_suffix),
                "unexpected error for schema {version}: {error}"
            );
            assert_eq!(candidate, before);
        }
    }

    let mut legacy = document(4);
    legacy
        .pointer_mut("/state/account")
        .unwrap()
        .as_object_mut()
        .unwrap()
        .insert("dismantle_rewards".into(), json!({"future": true}));
    assert_eq!(validate_document_items(&legacy), Ok(()));
}

#[test]
fn schema_eight_validates_filtered_dismantle_reward_policies() {
    let mut valid = document(8);
    valid
        .pointer_mut("/state/account")
        .unwrap()
        .as_object_mut()
        .unwrap()
        .insert(
            "dismantle_rewards".into(),
            json!([
                {"definition_hash": 1, "quantity": 25, "rarity": "common"},
                {"definition_hash": 1, "quantity": 50, "rarity": "uncommon"},
                {
                    "definition_hash": 2,
                    "quantity": 3,
                    "rarity": ["legendary", "exotic"],
                    "class": "weapon"
                },
                {
                    "definition_hash": 2,
                    "quantity": 4,
                    "rarity": ["legendary", "exotic"],
                    "class": "armor",
                    "masterworked": false
                },
                {"definition_hash": 2, "quantity": 5, "masterworked": true},
                {
                    "definition_hash": 3,
                    "quantity": i32::MAX,
                    "future": {"preserved": true}
                }
            ]),
        );
    let before = valid.clone();
    assert_eq!(validate_document_items(&valid), Ok(()));
    assert_eq!(valid, before);
    let rewards = valid
        .pointer("/state/account/dismantle_rewards")
        .cloned()
        .unwrap();
    add_profile_item(&mut valid, 4, 1).unwrap();
    assert_eq!(
        valid.pointer("/state/account/dismantle_rewards"),
        Some(&rewards)
    );

    let invalid = [
        (
            json!([
                {"definition_hash": 1, "quantity": 1, "rarity": ["rare", "legendary"]},
                {"definition_hash": 1, "quantity": 2, "rarity": ["legendary", "rare"]}
            ]),
            "/dismantle_rewards/1/definition_hash",
        ),
        (
            json!([{"definition_hash": 1, "quantity": 1, "rarity": []}]),
            "/dismantle_rewards/0/rarity",
        ),
        (
            json!([{"definition_hash": 1, "quantity": 1, "rarity": ["rare", "rare"]}]),
            "/dismantle_rewards/0/rarity/1",
        ),
        (
            json!([{"definition_hash": 1, "quantity": 1, "rarity": "mythic"}]),
            "/dismantle_rewards/0/rarity",
        ),
        (
            json!([{"definition_hash": 1, "quantity": 1, "class": "ghost"}]),
            "/dismantle_rewards/0/class",
        ),
        (
            json!([{"definition_hash": 1, "quantity": 1, "masterworked": 1}]),
            "/dismantle_rewards/0/masterworked",
        ),
        (
            Value::Array(
                (1..=FILTERED_DISMANTLE_REWARD_CAPACITY + 1)
                    .map(|hash| json!({"definition_hash": hash, "quantity": 1}))
                    .collect(),
            ),
            "/state/account/dismantle_rewards",
        ),
    ];
    for (rewards, expected_path_suffix) in invalid {
        let mut candidate = document(8);
        candidate
            .pointer_mut("/state/account")
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("dismantle_rewards".into(), rewards);
        let before = candidate.clone();
        let error = validate_document_items(&candidate).unwrap_err();
        assert!(
            error.path().ends_with(expected_path_suffix),
            "unexpected error: {error}"
        );
        assert_eq!(candidate, before);
    }
}

#[test]
fn dismantle_policy_actions_preserve_unknown_members_and_are_atomic() {
    let mut document = document(8);
    document
        .pointer_mut("/state/account")
        .unwrap()
        .as_object_mut()
        .unwrap()
        .insert(
            "dismantle_rewards".into(),
            json!([{
                "definition_hash": 1,
                "quantity": 1,
                "opaque": {"keep": true}
            }]),
        );

    apply_dismantle_reward_action(
        &mut document,
        DismantleRewardLocation { index: 0 },
        DismantleRewardAction::SetPolicy {
            definition_hash: 1,
            quantity: 7,
            rarities: vec![DismantleRarity::Rare, DismantleRarity::Legendary],
            gear_class: Some(DismantleGearClass::Weapon),
            masterworked: Some(true),
        },
    )
    .unwrap();
    assert_eq!(
        document.pointer("/state/account/dismantle_rewards/0"),
        Some(&json!({
            "definition_hash": "0x00000001",
            "quantity": 7,
            "rarity": ["rare", "legendary"],
            "class": "weapon",
            "masterworked": true,
            "opaque": {"keep": true}
        }))
    );

    let added = add_dismantle_reward(&mut document, 1).unwrap();
    assert_eq!(added, DismantleRewardLocation { index: 1 });
    let before = document.clone();
    let error = apply_dismantle_reward_action(
        &mut document,
        added,
        DismantleRewardAction::SetPolicy {
            definition_hash: 1,
            quantity: 9,
            rarities: vec![DismantleRarity::Legendary, DismantleRarity::Rare],
            gear_class: Some(DismantleGearClass::Weapon),
            masterworked: Some(true),
        },
    )
    .unwrap_err();
    assert!(
        error
            .message()
            .contains("filter combinations must be unique")
    );
    assert_eq!(document, before);

    apply_dismantle_reward_action(&mut document, added, DismantleRewardAction::Remove).unwrap();
    assert_eq!(
        document
            .pointer("/state/account/dismantle_rewards")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(1)
    );
}

#[test]
fn legacy_dismantle_policies_do_not_create_filtered_duplicates() {
    let mut document = document(7);
    document
        .pointer_mut("/state/account")
        .unwrap()
        .as_object_mut()
        .unwrap()
        .insert(
            "dismantle_rewards".into(),
            json!([{"definition_hash": 1, "quantity": 1}]),
        );
    let before = document.clone();
    assert!(add_dismantle_reward(&mut document, 1).is_err());
    assert_eq!(document, before);
}

#[test]
fn document_validation_rejects_duplicate_soids_with_both_locations() {
    let duplicate = 0x4000_0000_0000_1234;
    let mut equipment_inventory = document(6);
    *equipment_inventory
        .pointer_mut("/state/characters/0/equipment")
        .unwrap() = json!({"kinetic": item(duplicate, 1)});
    *equipment_inventory
        .pointer_mut("/state/characters/0/inventory")
        .unwrap() = Value::Array(vec![item(duplicate, 2)]);

    let error = validate_document_items(&equipment_inventory).unwrap_err();
    assert_eq!(
        error.path(),
        "/state/characters/0/inventory/0/instance_soid"
    );
    assert!(
        error
            .message()
            .contains("/state/characters/0/equipment/kinetic/instance_soid")
    );

    let mut account_character = document(6);
    let account_soid = account_character
        .pointer("/state/account/primary_soid")
        .unwrap()
        .clone();
    *account_character
        .pointer_mut("/state/characters/0/soid")
        .unwrap() = account_soid;

    let error = validate_document_items(&account_character).unwrap_err();
    assert_eq!(error.path(), "/state/characters/0/soid");
    assert!(error.message().contains("/state/account/primary_soid"));
}

#[test]
fn soid_allocation_scans_account_characters_equipment_and_inventory() {
    let start = GENERATED_INSTANCE_SOID_START;
    let mut document = json!({
        "version": 6,
        "state": {
            "account": {"primary_soid": format_instance_soid(start)},
            "characters": [
                {
                    "soid": format_instance_soid(start + 1),
                    "equipment": {"kinetic": item(start + 2, 1)},
                    "inventory": [item(start + 3, 2)]
                },
                {
                    "soid": "0x9EAA300200100101",
                    "equipment": {},
                    "inventory": []
                }
            ]
        }
    });
    assert_eq!(allocate_instance_soid(&document).unwrap(), start + 4);

    let location = add_inventory_item(
        &mut document,
        1,
        NewInventoryItem {
            definition_hash: 3,
            level: 106,
            quantity: 2,
        },
    )
    .unwrap();
    assert_eq!(location.item_index, 0);
    assert_eq!(
        document.pointer("/state/characters/1/inventory/0/instance_soid"),
        Some(&Value::String(format_instance_soid(start + 4)))
    );
}

#[test]
fn allocation_errors_on_malformed_identity_sources_and_exhaustion() {
    let malformed = json!({
        "state": {"characters": [{"equipment": {"kinetic": {"instance_soid": 0}}}]}
    });
    let error = collect_used_soids(&malformed).unwrap_err();
    assert!(error.path().ends_with("instance_soid"));

    let exhausted = json!({
        "state": {"account": {"primary_soid": u64::MAX}}
    });
    assert!(next_available_instance_soid(&exhausted, u64::MAX).is_err());
}

#[test]
fn failed_actions_leave_documents_untouched() {
    let mut document = document(6);
    *document
        .pointer_mut("/state/characters/0/inventory")
        .unwrap() = Value::Array(vec![item(1, 1)]);
    let before = document.clone();
    assert!(
        apply_inventory_item_action(
            &mut document,
            InventoryItemLocation {
                character_index: 0,
                item_index: 0
            },
            InventoryItemAction::SetQuantity(0)
        )
        .is_err()
    );
    assert_eq!(document, before);

    let before = document.clone();
    assert!(
        apply_inventory_item_action(
            &mut document,
            InventoryItemLocation {
                character_index: 0,
                item_index: 0
            },
            InventoryItemAction::SetPlugs(ItemPlugs::Authored(vec![None; MAX_ITEM_PLUGS + 1]))
        )
        .is_err()
    );
    assert_eq!(document, before);
}

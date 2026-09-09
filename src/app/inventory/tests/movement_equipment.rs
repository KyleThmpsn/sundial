use super::*;

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

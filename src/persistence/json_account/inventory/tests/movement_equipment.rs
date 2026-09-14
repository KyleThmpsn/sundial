use super::*;

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

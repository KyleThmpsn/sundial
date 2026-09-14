use super::*;

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

fn assert_future_schema_data_before_swap(document: &Value) {
    assert_eq!(
        document.pointer("/state/account/profile_items/0/quantity"),
        Some(&Value::from(9))
    );
    assert_eq!(
        document.pointer("/state/characters/0/inventory/0/quantity"),
        Some(&Value::from(7))
    );
    assert_eq!(
        document.pointer("/state/account/profile_items/0/future_profile_data/keep"),
        Some(&json!([1, 2, 3]))
    );
    assert_eq!(
        document.pointer("/state/characters/0/inventory/0/future_item_data/keep"),
        Some(&Value::Bool(true))
    );
    assert_eq!(
        document.pointer("/state/characters/0/equipment/kinetic/future_equipment_data"),
        Some(&json!([4, 5, 6]))
    );
    assert_eq!(
        document.pointer("/state/characters/0/equipment/future_slot/opaque/keep"),
        Some(&Value::String("all of this".into()))
    );
    assert_eq!(
        document.pointer("/state/characters/0/equipment/future_scalar_slot"),
        Some(&json!(["an", "unknown", "shape"]))
    );
    assert_eq!(
        document.pointer("/state/account/dismantle_rewards/future_layout"),
        Some(&json!(["leave", "untouched"]))
    );
    assert_eq!(
        document.pointer("/future_root_data/keep"),
        Some(&Value::Bool(true))
    );
}

fn assert_future_schema_data_after_swap(document: &Value) {
    assert_eq!(
        document.pointer("/state/characters/0/equipment/kinetic/future_item_data/keep"),
        Some(&Value::Bool(true))
    );
    assert_eq!(
        document.pointer("/state/characters/0/inventory/0/future_equipment_data"),
        Some(&json!([4, 5, 6]))
    );
    assert_eq!(
        document.pointer("/state/characters/0/equipment/future_scalar_slot"),
        Some(&json!(["an", "unknown", "shape"]))
    );
    assert_eq!(validate_document_items(document), Ok(()));
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

    assert_future_schema_data_before_swap(&future);

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
    assert_future_schema_data_after_swap(&future);

    let encoded = crate::persistence::json_document::encoding::encode_settings(&future).unwrap();
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

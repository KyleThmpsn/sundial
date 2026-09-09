use super::*;

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

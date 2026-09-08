use super::*;
use crate::catalog::{Catalog, ItemStackability};
use serde_json::json;
use std::collections::HashMap;

fn catalog() -> Catalog {
    Catalog::for_test_with_inventory(
        vec![],
        HashMap::new(),
        HashMap::from([(
            99,
            InventoryMetadata {
                scope: InventoryScope::Character,
                native_bucket_id: 49,
                stackability: ItemStackability::Instanced,
                max_stack_size: Some(1),
                bucket_capacity: Some(4),
            },
        )]),
    )
}

fn document(stored_counts: &[usize]) -> WorkspaceDocument {
    WorkspaceDocument::json_only(json!({
        "version": 16,
        "state": {"account": {}, "characters": stored_counts.iter().enumerate().map(|(ci, count)| json!({
            "soid": ci + 1, "class": 1, "race": 0, "gender": 0,
            "equipment": {"artifact": {
                "instance_soid": 1000 + ci * 100, "definition_hash": 99,
                "level": 0, "quantity": 1, "plugs": null,
            }},
            "inventory": (0..*count).map(|index| json!({
                "instance_soid": 1001 + ci * 100 + index, "definition_hash": 99,
                "level": 0, "quantity": 1, "plugs": null,
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>()},
    }))
}

#[test]
fn save_gate_counts_equipped_and_stored_artifacts_per_character() {
    let catalog = catalog();
    let valid = document(&[3, 3]);
    assert!(validate_new_bucket_overflows(&valid, &document(&[0, 0]), &catalog).is_ok());
    let error = crate::app::account_validation::validate_new_account_catalog_issues(
        &document(&[3, 4]),
        &valid,
        &catalog,
        false,
    )
    .unwrap_err();
    assert!(
        error.contains("Character 2 seasonal artifacts contains 5 items"),
        "{error}"
    );
    assert!(error.contains("only 4"), "{error}");
}

#[test]
fn existing_overflow_can_be_repaired_but_not_increased() {
    let catalog = catalog();
    let before = document(&[9]);
    for repaired in [document(&[9]), document(&[8]), document(&[3])] {
        assert!(validate_new_bucket_overflows(&repaired, &before, &catalog).is_ok());
    }
    assert!(validate_new_bucket_overflows(&document(&[10]), &before, &catalog).is_err());
}

#[test]
fn profile_capacity_counts_stacks_instead_of_their_quantities() {
    let catalog = Catalog::for_test_with_inventory(
        vec![],
        HashMap::new(),
        HashMap::from([(
            99,
            InventoryMetadata {
                scope: InventoryScope::Profile,
                native_bucket_id: 15,
                stackability: ItemStackability::Stackable,
                max_stack_size: Some(999),
                bucket_capacity: Some(2),
            },
        )]),
    );
    let mut candidate = document(&[]);
    let before = candidate.clone();
    candidate.json_mut()["state"]["account"]["profile_items"] = json!([
        {"definition_hash": 99, "quantity": 999},
        {"definition_hash": 99, "quantity": 999},
    ]);
    assert!(validate_new_bucket_overflows(&candidate, &before, &catalog).is_ok());
    candidate.json_mut()["state"]["account"]["profile_items"]
        .as_array_mut()
        .unwrap()
        .push(json!({"definition_hash": 99, "quantity": 1}));
    let error = validate_new_bucket_overflows(&candidate, &before, &catalog).unwrap_err();
    assert!(error.contains("contains 3 items"), "{error}");
    assert!(error.contains("only 2"), "{error}");
}

#[test]
fn filling_empty_equipment_slot_is_atomic_at_native_capacity() {
    for version in [13, 16] {
        let mut candidate = document(&[4]);
        candidate.json_mut()["version"] = json!(version);
        candidate.json_mut()["state"]["characters"][0]["equipment"]["artifact"] = json!(null);
        let before = candidate.clone();
        let error = apply_with_bucket_limits(&mut candidate, &catalog(), |updated| {
            account::equip_definition(updated, 0, "artifact", 99, &[])
        })
        .unwrap_err();
        assert!(error.contains("contains 5 items"), "{error}");
        assert_eq!(candidate, before);

        account::remove_character_inventory_items(&mut candidate, 0, [0]).unwrap();
        apply_with_bucket_limits(&mut candidate, &catalog(), |updated| {
            account::equip_definition(updated, 0, "artifact", 99, &[])
        })
        .unwrap();
        // Replacing the equipped definition does not consume another bucket slot.
        apply_with_bucket_limits(&mut candidate, &catalog(), |updated| {
            account::equip_definition(updated, 0, "artifact", 99, &[])
        })
        .unwrap();
        assert_eq!(
            account::character_inventory(&candidate, 0)
                .unwrap()
                .unwrap()
                .len(),
            3
        );
    }
}

#[test]
fn transfer_to_full_bucket_leaves_both_characters_unchanged() {
    let mut candidate = document(&[1, 3]);
    let before = candidate.clone();
    let error = apply_with_bucket_limits(&mut candidate, &catalog(), |updated| {
        account::move_inventory_item_to_character(
            updated,
            crate::app::inventory::InventoryItemLocation {
                character_index: 0,
                item_index: 0,
            },
            1,
        )
        .map_err(|error| error.to_string())
    })
    .unwrap_err();
    assert!(error.contains("Character 2"), "{error}");
    assert_eq!(candidate, before);
}

#[test]
fn definition_swap_into_full_native_bucket_is_atomic() {
    let mut candidate = document(&[4]);
    candidate.json_mut()["state"]["characters"][0]["inventory"][3]["definition_hash"] = json!(100);
    let before = candidate.clone();
    let error = apply_with_bucket_limits(&mut candidate, &catalog(), |updated| {
        account::apply_inventory_item_action(
            updated,
            crate::app::inventory::InventoryItemLocation {
                character_index: 0,
                item_index: 3,
            },
            crate::app::inventory::InventoryItemAction::SetDefinitionHash(99),
        )
        .map_err(|error| error.to_string())
    })
    .unwrap_err();
    assert!(error.contains("contains 5 items"), "{error}");
    assert_eq!(candidate, before);
}

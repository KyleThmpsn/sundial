//! Focused policy and regression tests for the inventory page feature.

use std::collections::HashMap;

use crate::{
    app::inventory::{
        InventoryItemAction, InventoryItemLocation, InventoryItemSnapshot, ItemPlugs,
    },
    catalog::{InventoryDefinition, InventoryMetadata, InventoryScope, ItemStackability},
};
use serde_json::json;

use super::{
    buckets::{bucket_has_room, profile_swap_candidate},
    definitions::{character_definition_choices, profile_definition_choices},
    interactions::{
        apply_inventory_actions_atomic, bucket_picker_open_request_key,
        inventory_item_ui_identities, take_bucket_picker_open_request,
    },
    model::BucketUsage,
};

#[test]
fn profile_browse_keeps_all_results_across_named_buckets() {
    let modifications = InventoryMetadata {
        scope: InventoryScope::Profile,
        native_bucket_id: 13,
        stackability: ItemStackability::Stackable,
        max_stack_size: Some(1),
        bucket_capacity: Some(200),
    };
    let glimmer = InventoryMetadata {
        scope: InventoryScope::Profile,
        native_bucket_id: 21,
        stackability: ItemStackability::Stackable,
        max_stack_size: Some(999_999),
        bucket_capacity: Some(1),
    };
    let many_alphabetical_rows = (0_u64..120).map(|hash| InventoryDefinition {
        hash,
        name: "Alphabetical modification",
        type_name: "Modification",
        metadata: &modifications,
        item: None,
    });
    let currency = std::iter::once(InventoryDefinition {
        hash: 1_000,
        name: "Glimmer",
        type_name: "Currency",
        metadata: &glimmer,
        item: None,
    });

    let choices = profile_definition_choices(many_alphabetical_rows.chain(currency));
    assert_eq!(choices.len(), 121);
    assert!(
        choices
            .iter()
            .any(|choice| choice.group.as_deref() == Some("Glimmer"))
    );
}

#[test]
fn shared_item_swap_candidates_stay_in_the_current_bucket() {
    let metadata = |native_bucket_id| InventoryMetadata {
        scope: InventoryScope::Profile,
        native_bucket_id,
        stackability: ItemStackability::Stackable,
        max_stack_size: Some(999_999),
        bucket_capacity: Some(10),
    };
    let current = metadata(15);
    let other = metadata(21);
    let usage = BucketUsage {
        counts: HashMap::from([(15, 1), (21, 1)]),
        unresolved_count: 0,
        occupancy_complete: true,
    };

    assert!(profile_swap_candidate(
        &current,
        Some(15),
        100,
        &usage,
        false
    ));
    assert!(!profile_swap_candidate(
        &other,
        Some(15),
        100,
        &usage,
        false
    ));
    assert!(profile_swap_candidate(&other, None, 100, &usage, true));
}

#[test]
fn character_browse_keeps_all_results_and_orders_native_buckets() {
    let metadata = |native_bucket_id| InventoryMetadata {
        scope: InventoryScope::Character,
        native_bucket_id,
        stackability: ItemStackability::Instanced,
        max_stack_size: Some(1),
        bucket_capacity: Some(200),
    };
    let kinetic = metadata(0);
    let chest = metadata(5);
    let artifact = metadata(49);
    let many_chest_rows = (0_u64..120).map(|hash| InventoryDefinition {
        hash,
        name: "Chest item",
        type_name: "Chest armor",
        metadata: &chest,
        item: None,
    });
    let edge_buckets = [
        InventoryDefinition {
            hash: 1_000,
            name: "Kinetic item",
            type_name: "Kinetic weapon",
            metadata: &kinetic,
            item: None,
        },
        InventoryDefinition {
            hash: 1_001,
            name: "Artifact item",
            type_name: "Seasonal artifact",
            metadata: &artifact,
            item: None,
        },
    ];

    let choices = character_definition_choices(many_chest_rows.chain(edge_buckets));
    assert_eq!(choices.len(), 122);
    let group_position = |group| {
        choices
            .iter()
            .position(|choice| choice.group.as_deref() == Some(group))
            .unwrap()
    };
    assert!(group_position("Kinetic weapons") < group_position("Chest armor"));
    assert!(group_position("Chest armor") < group_position("Seasonal artifacts"));
}

#[test]
fn character_item_ui_ids_survive_index_shifts_and_disambiguate_bad_soids() {
    let snapshot = |item_index, instance_soid| InventoryItemSnapshot {
        location: InventoryItemLocation {
            character_index: 0,
            item_index,
        },
        instance_soid,
        definition_hash: 1,
        level: 1,
        quantity: 1,
        plugs: ItemPlugs::NativeDefaults,
        flags: None,
    };
    let before = inventory_item_ui_identities(&[snapshot(0, 10), snapshot(1, 20)]);
    let after = inventory_item_ui_identities(&[snapshot(0, 20)]);
    assert_eq!(before[1], after[0]);

    let duplicates = inventory_item_ui_identities(&[snapshot(0, 20), snapshot(1, 20)]);
    assert_ne!(duplicates[0], duplicates[1]);
    assert_eq!(duplicates[0].duplicate_ordinal, Some(0));
    assert_eq!(duplicates[1].duplicate_ordinal, Some(1));
}

#[test]
fn bucket_picker_open_request_waits_for_the_originating_click_frame() {
    let picker_key = "character-inventory:0:add:1:4";
    let request_key = bucket_picker_open_request_key(picker_key);
    let mut searches = HashMap::from([(request_key.clone(), String::new())]);

    assert!(!take_bucket_picker_open_request(
        &mut searches,
        picker_key,
        true
    ));
    assert!(searches.contains_key(&request_key));
    assert!(take_bucket_picker_open_request(
        &mut searches,
        picker_key,
        false
    ));
    assert!(!searches.contains_key(&request_key));
    assert!(!take_bucket_picker_open_request(
        &mut searches,
        picker_key,
        false
    ));
}

#[test]
fn bucket_capacity_counts_only_present_rows_and_allows_same_bucket_replacement() {
    let metadata = InventoryMetadata {
        scope: InventoryScope::Character,
        native_bucket_id: 4,
        bucket_capacity: Some(3),
        ..InventoryMetadata::default()
    };
    let one_row_free = BucketUsage {
        // Present equipment and inventory rows are both included in this count.
        counts: HashMap::from([(4, 2)]),
        unresolved_count: 0,
        occupancy_complete: true,
    };
    assert!(bucket_has_room(&metadata, &one_row_free, None, false));

    let full = BucketUsage {
        counts: HashMap::from([(4, 3)]),
        unresolved_count: 0,
        occupancy_complete: true,
    };
    assert!(bucket_has_room(&metadata, &full, Some(4), false));
    assert!(!bucket_has_room(&metadata, &full, None, false));

    let unresolved = BucketUsage {
        counts: HashMap::new(),
        unresolved_count: 3,
        occupancy_complete: true,
    };
    assert!(bucket_has_room(&metadata, &unresolved, Some(4), false));
    assert!(!bucket_has_room(&metadata, &unresolved, None, false));
    assert!(bucket_has_room(&metadata, &unresolved, None, true));
}

#[test]
fn multi_field_inventory_edits_are_atomic() {
    let mut document = super::super::account_workspace::WorkspaceDocument::json_only(json!({
    "version": 6,
    "state": {
        "account": {},
        "characters": [{
            "soid": 1,
            "equipment": {},
            "inventory": [{
                "instance_soid": "0x4000000000000001",
                "definition_hash": "0x0000002A",
                "level": 106,
                "quantity": 1,
                "plugs": null
            }]
        }]
    }
    }));
    let before = document.clone();
    let error = apply_inventory_actions_atomic(
        &mut document,
        InventoryItemLocation {
            character_index: 0,
            item_index: 0,
        },
        vec![
            InventoryItemAction::SetQuantity(2),
            InventoryItemAction::SetFlags(Some(8)),
        ],
    )
    .unwrap_err();

    assert!(error.contains("flags"));
    assert_eq!(document, before);
}

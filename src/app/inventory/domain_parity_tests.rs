//! Differential tests between legacy JSON mutations and the storage-neutral account path.

use serde_json::{Value, json};
use sundial_account as domain;

use crate::persistence::json_account::JsonProfileAdapter;

use super::legacy_profile_tests as legacy;
use super::*;

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

#[test]
fn adapter_capabilities_match_every_current_json_schema_mode() {
    for version in 2..=8 {
        let document = document(version);
        let mode = schema_mode(&document);
        let capabilities = JsonProfileAdapter::load(&document).unwrap().capabilities();

        assert_eq!(
            capabilities.profile_items_writable,
            mode.can_mutate_profile_items()
        );
        assert_eq!(
            capabilities.profile_item_capacity,
            mode.profile_item_capacity()
        );
        assert!(capabilities.enforce_loaded_profile_item_capacity);
        assert_eq!(
            capabilities.dismantle_rewards_writable,
            mode.can_mutate_dismantle_rewards()
        );
        assert_eq!(
            capabilities.dismantle_reward_capacity,
            mode.dismantle_reward_capacity()
        );
        assert_eq!(
            capabilities.filtered_dismantle_rewards,
            mode.supports_filtered_dismantle_rewards()
        );
    }

    let future = document(crate::game_settings::MAX_SUPPORTED_SCHEMA + 1);
    let capabilities = JsonProfileAdapter::load(&future).unwrap().capabilities();
    assert!(capabilities.profile_items_writable);
    assert_eq!(
        capabilities.profile_item_capacity,
        Some(PROFILE_ITEM_CAPACITY)
    );
    assert!(!capabilities.enforce_loaded_profile_item_capacity);
    assert!(!capabilities.dismantle_rewards_writable);
}

#[test]
fn future_profile_capacity_matches_legacy_without_rejecting_loaded_rows() {
    let mut production = document(crate::game_settings::MAX_SUPPORTED_SCHEMA + 1);
    *production
        .pointer_mut("/state/account/profile_items")
        .unwrap() = Value::Array(vec![
        json!({
            "definition_hash": 11,
            "quantity": 1
        });
        PROFILE_ITEM_CAPACITY
    ]);
    let mut legacy_document = production.clone();
    let source = production.clone();

    let production_result = add_profile_item(&mut production, 22, 1);
    let legacy_result = legacy::add_profile_item(&mut legacy_document, 22, 1);

    assert_eq!(production_result, legacy_result);
    assert_eq!(production, source);
    assert_eq!(legacy_document, source);

    *production
        .pointer_mut("/state/account/profile_items")
        .unwrap() = Value::Array(vec![
        json!({
            "definition_hash": 11,
            "quantity": 1
        });
        PROFILE_ITEM_CAPACITY + 1
    ]);
    let adapter = JsonProfileAdapter::load_profile_items(&production).unwrap();
    assert_eq!(
        adapter.state().profile_items().len(),
        PROFILE_ITEM_CAPACITY + 1
    );
}

#[test]
fn profile_edits_match_legacy_json_exactly() {
    let mut legacy = document(8);
    *legacy.pointer_mut("/state/account/profile_items").unwrap() = json!([{
        "definition_hash": 11,
        "quantity": 1,
        "future": {"keep": true}
    }]);
    let source = legacy.clone();
    let adapter = JsonProfileAdapter::load(&source).unwrap();
    let id = adapter.state().profile_items()[0].id;

    let (_, projected) = adapter
        .apply_profile_item(
            &source,
            domain::ProfileItemCommand::SetQuantity { id, quantity: 9 },
        )
        .unwrap();
    legacy::apply_profile_item_action(
        &mut legacy,
        ProfileItemLocation { index: 0 },
        ProfileItemAction::SetQuantity(9),
    )
    .unwrap();

    assert_eq!(projected, legacy);
}

#[test]
fn profile_edits_leave_malformed_sibling_dismantle_data_opaque() {
    let mut legacy = document(8);
    *legacy.pointer_mut("/state/account/profile_items").unwrap() = json!([{
        "definition_hash": 11,
        "quantity": 1
    }]);
    legacy
        .pointer_mut("/state/account")
        .unwrap()
        .as_object_mut()
        .unwrap()
        .insert("dismantle_rewards".into(), Value::String("opaque".into()));
    let source = legacy.clone();
    let adapter = JsonProfileAdapter::load_profile_items(&source).unwrap();
    let id = adapter.state().profile_items()[0].id;

    let (_, projected) = adapter
        .apply_profile_item(
            &source,
            domain::ProfileItemCommand::SetQuantity { id, quantity: 9 },
        )
        .unwrap();
    legacy::apply_profile_item_action(
        &mut legacy,
        ProfileItemLocation { index: 0 },
        ProfileItemAction::SetQuantity(9),
    )
    .unwrap();

    assert_eq!(projected, legacy);
    assert_eq!(
        projected.pointer("/state/account/dismantle_rewards"),
        Some(&Value::String("opaque".into()))
    );
}

#[test]
fn profile_hash_edits_only_rewrite_the_selected_field() {
    let mut legacy = document(8);
    *legacy.pointer_mut("/state/account/profile_items").unwrap() = json!([{
        "definition_hash": 11,
        "quantity": 2,
        "future": {"keep": true}
    }]);
    let source = legacy.clone();
    let adapter = JsonProfileAdapter::load(&source).unwrap();
    let id = adapter.state().profile_items()[0].id;

    let (_, projected) = adapter
        .apply_profile_item(
            &source,
            domain::ProfileItemCommand::SetDefinitionHash {
                id,
                definition_hash: domain::DefinitionHash::new(22),
            },
        )
        .unwrap();
    legacy::apply_profile_item_action(
        &mut legacy,
        ProfileItemLocation { index: 0 },
        ProfileItemAction::SetDefinitionHash(22),
    )
    .unwrap();

    assert_eq!(projected, legacy);
    assert_eq!(
        projected.pointer("/state/account/profile_items/0/quantity"),
        Some(&Value::from(2))
    );
}

#[test]
fn profile_add_and_remove_match_legacy_json_exactly() {
    let mut legacy = document(8);
    *legacy.pointer_mut("/state/account/profile_items").unwrap() = json!([
        {"definition_hash": 11, "quantity": 1},
        {"definition_hash": 22, "quantity": 2, "future": "keep"}
    ]);
    let mut projected = legacy.clone();
    let adapter = JsonProfileAdapter::load(&projected).unwrap();
    let new_id = adapter.next_entity_id();

    let (adapter, next_projected) = adapter
        .apply_profile_item(
            &projected,
            domain::ProfileItemCommand::Add(domain::ProfileItem {
                id: new_id,
                definition_hash: domain::DefinitionHash::new(33),
                quantity: 3,
            }),
        )
        .unwrap();
    projected = next_projected;
    legacy::add_profile_item(&mut legacy, 33, 3).unwrap();
    assert_eq!(projected, legacy);

    let removed_id = adapter.state().profile_items()[0].id;
    let (_, projected) = adapter
        .apply_profile_item(
            &projected,
            domain::ProfileItemCommand::Remove { id: removed_id },
        )
        .unwrap();
    legacy::apply_profile_item_action(
        &mut legacy,
        ProfileItemLocation { index: 0 },
        ProfileItemAction::Remove,
    )
    .unwrap();

    assert_eq!(projected, legacy);
}

#[test]
fn filtered_dismantle_policy_allocation_matches_legacy_json_exactly() {
    let mut legacy = document(8);
    legacy
        .pointer_mut("/state/account")
        .and_then(Value::as_object_mut)
        .unwrap()
        .insert(
            "dismantle_rewards".into(),
            json!([{"definition_hash": 44, "quantity": 1}]),
        );
    let source = legacy.clone();
    let adapter = JsonProfileAdapter::load(&source).unwrap();

    let (_, projected) = adapter
        .apply_dismantle_reward(
            &source,
            domain::DismantleRewardCommand::AddForDefinition {
                id: adapter.next_entity_id(),
                definition_hash: domain::DefinitionHash::new(44),
            },
        )
        .unwrap();
    legacy::add_dismantle_reward(&mut legacy, 44).unwrap();

    assert_eq!(projected, legacy);
}

#[test]
fn legacy_dismantle_add_and_remove_match_exactly() {
    for version in 5..=7 {
        let mut legacy = document(version);
        legacy
            .pointer_mut("/state/account")
            .and_then(Value::as_object_mut)
            .unwrap()
            .insert(
                "dismantle_rewards".into(),
                json!([{
                    "definition_hash": 44,
                    "quantity": 1,
                    "future": {"keep": true}
                }]),
            );
        let mut projected = legacy.clone();
        let adapter = JsonProfileAdapter::load(&projected).unwrap();
        let removed_id = adapter.state().dismantle_rewards()[0].id;

        let (adapter, next_projected) = adapter
            .apply_dismantle_reward(
                &projected,
                domain::DismantleRewardCommand::AddForDefinition {
                    id: adapter.next_entity_id(),
                    definition_hash: domain::DefinitionHash::new(55),
                },
            )
            .unwrap();
        projected = next_projected;
        legacy::add_dismantle_reward(&mut legacy, 55).unwrap();
        assert_eq!(projected, legacy, "schema {version} add differed");

        let (_, projected) = adapter
            .apply_dismantle_reward(
                &projected,
                domain::DismantleRewardCommand::Remove { id: removed_id },
            )
            .unwrap();
        legacy::apply_dismantle_reward_action(
            &mut legacy,
            DismantleRewardLocation { index: 0 },
            DismantleRewardAction::Remove,
        )
        .unwrap();
        assert_eq!(projected, legacy, "schema {version} remove differed");
    }
}

#[test]
fn dismantle_policy_rewrites_match_legacy_and_preserve_unknown_members() {
    let mut legacy = document(8);
    legacy
        .pointer_mut("/state/account")
        .and_then(Value::as_object_mut)
        .unwrap()
        .insert(
            "dismantle_rewards".into(),
            json!([{
                "definition_hash": 44,
                "quantity": 1,
                "future": {"keep": true}
            }]),
        );
    let source = legacy.clone();
    let adapter = JsonProfileAdapter::load(&source).unwrap();
    let id = adapter.state().dismantle_rewards()[0].id;

    let (_, projected) = adapter
        .apply_dismantle_reward(
            &source,
            domain::DismantleRewardCommand::SetPolicy(domain::DismantleReward {
                id,
                definition_hash: domain::DefinitionHash::new(55),
                quantity: 7,
                rarities: vec![
                    domain::DismantleRarity::Rare,
                    domain::DismantleRarity::Legendary,
                ],
                gear_class: Some(domain::DismantleGearClass::Weapon),
                masterworked: Some(true),
            }),
        )
        .unwrap();
    legacy::apply_dismantle_reward_action(
        &mut legacy,
        DismantleRewardLocation { index: 0 },
        DismantleRewardAction::SetPolicy {
            definition_hash: 55,
            quantity: 7,
            rarities: vec![DismantleRarity::Rare, DismantleRarity::Legendary],
            gear_class: Some(DismantleGearClass::Weapon),
            masterworked: Some(true),
        },
    )
    .unwrap();

    assert_eq!(projected, legacy);
}

#[test]
fn invalid_profile_commands_are_atomic_in_both_paths() {
    let mut legacy = document(8);
    *legacy.pointer_mut("/state/account/profile_items").unwrap() =
        json!([{"definition_hash": 11, "quantity": 1}]);
    let source = legacy.clone();
    let adapter = JsonProfileAdapter::load(&source).unwrap();
    let adapter_before = adapter.clone();
    let id = adapter.state().profile_items()[0].id;

    assert!(
        adapter
            .apply_profile_item(
                &source,
                domain::ProfileItemCommand::SetQuantity { id, quantity: 0 },
            )
            .is_err()
    );
    assert!(
        legacy::apply_profile_item_action(
            &mut legacy,
            ProfileItemLocation { index: 0 },
            ProfileItemAction::SetQuantity(0),
        )
        .is_err()
    );
    assert_eq!(adapter, adapter_before);
    assert_eq!(legacy, source);
}

#[test]
fn duplicate_dismantle_policies_are_atomic_in_both_paths() {
    let mut legacy = document(8);
    legacy
        .pointer_mut("/state/account")
        .and_then(Value::as_object_mut)
        .unwrap()
        .insert(
            "dismantle_rewards".into(),
            json!([
                {"definition_hash": 44, "quantity": 1, "rarity": "rare"},
                {"definition_hash": 44, "quantity": 2, "rarity": "legendary"}
            ]),
        );
    let source = legacy.clone();
    let adapter = JsonProfileAdapter::load(&source).unwrap();
    let adapter_before = adapter.clone();
    let id = adapter.state().dismantle_rewards()[1].id;

    assert!(
        adapter
            .apply_dismantle_reward(
                &source,
                domain::DismantleRewardCommand::SetPolicy(domain::DismantleReward {
                    id,
                    definition_hash: domain::DefinitionHash::new(44),
                    quantity: 2,
                    rarities: vec![domain::DismantleRarity::Rare],
                    gear_class: None,
                    masterworked: None,
                }),
            )
            .is_err()
    );
    assert!(
        legacy::apply_dismantle_reward_action(
            &mut legacy,
            DismantleRewardLocation { index: 1 },
            DismantleRewardAction::SetPolicy {
                definition_hash: 44,
                quantity: 2,
                rarities: vec![DismantleRarity::Rare],
                gear_class: None,
                masterworked: None,
            },
        )
        .is_err()
    );
    assert_eq!(adapter, adapter_before);
    assert_eq!(legacy, source);
}

#[test]
fn future_profile_edits_leave_opaque_dismantle_layouts_untouched() {
    let mut legacy = document(crate::game_settings::MAX_SUPPORTED_SCHEMA + 1);
    *legacy.pointer_mut("/state/account/profile_items").unwrap() = json!([{
        "definition_hash": 11,
        "quantity": 1,
        "future": {"keep": true}
    }]);
    legacy
        .pointer_mut("/state/account")
        .and_then(Value::as_object_mut)
        .unwrap()
        .insert(
            "dismantle_rewards".into(),
            json!({"future_layout": [1, 2, 3]}),
        );
    let source = legacy.clone();
    let adapter = JsonProfileAdapter::load(&source).unwrap();
    let id = adapter.state().profile_items()[0].id;

    let (_, projected) = adapter
        .apply_profile_item(
            &source,
            domain::ProfileItemCommand::SetQuantity { id, quantity: 9 },
        )
        .unwrap();
    legacy::apply_profile_item_action(
        &mut legacy,
        ProfileItemLocation { index: 0 },
        ProfileItemAction::SetQuantity(9),
    )
    .unwrap();

    assert_eq!(projected, legacy);
}

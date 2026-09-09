use super::*;

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

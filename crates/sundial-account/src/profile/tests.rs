use std::num::NonZeroU64;

use super::*;

const WRITABLE_FILTERED: ProfileCapabilities = ProfileCapabilities {
    profile_items_writable: true,
    profile_item_capacity: Some(701),
    enforce_loaded_profile_item_capacity: true,
    dismantle_rewards_writable: true,
    dismantle_reward_capacity: Some(32),
    filtered_dismantle_rewards: true,
    combined_dismantle_gear_class: false,
};

fn id(value: u64) -> EntityId {
    EntityId::new(NonZeroU64::new(value).expect("test IDs are nonzero"))
}

fn profile_item(id_value: u64, hash: u32, quantity: i32) -> ProfileItem {
    ProfileItem {
        id: id(id_value),
        definition_hash: DefinitionHash::new(hash),
        quantity,
        instance_soid: None,
    }
}

fn reward(id_value: u64, hash: u32) -> DismantleReward {
    DismantleReward {
        id: id(id_value),
        definition_hash: DefinitionHash::new(hash),
        quantity: 1,
        rarities: Vec::new(),
        gear_class: None,
        masterworked: None,
    }
}

#[test]
fn profile_commands_use_stable_ids_after_rows_are_removed() {
    let mut state = ProfileState::try_new(
        WRITABLE_FILTERED,
        vec![profile_item(1, 11, 1), profile_item(2, 22, 2)],
        Vec::new(),
    )
    .unwrap();

    state
        .apply_profile_item(WRITABLE_FILTERED, ProfileItemCommand::Remove { id: id(1) })
        .unwrap();
    state
        .apply_profile_item(
            WRITABLE_FILTERED,
            ProfileItemCommand::SetQuantity {
                id: id(2),
                quantity: 9,
            },
        )
        .unwrap();

    assert_eq!(state.profile_items(), &[profile_item(2, 22, 9)]);
}

#[test]
fn failed_profile_commands_leave_state_untouched() {
    let mut state =
        ProfileState::try_new(WRITABLE_FILTERED, vec![profile_item(1, 11, 1)], Vec::new()).unwrap();
    let before = state.clone();

    let error = state
        .apply_profile_item(
            WRITABLE_FILTERED,
            ProfileItemCommand::SetQuantity {
                id: id(1),
                quantity: 0,
            },
        )
        .unwrap_err();

    assert_eq!(error, AccountError::InvalidQuantity);
    assert_eq!(state, before);
}

#[test]
fn profile_capacity_and_duplicate_ids_are_enforced_before_mutation() {
    let capabilities = ProfileCapabilities {
        profile_item_capacity: Some(1),
        ..WRITABLE_FILTERED
    };
    let mut state =
        ProfileState::try_new(capabilities, vec![profile_item(1, 11, 1)], Vec::new()).unwrap();
    let before = state.clone();

    let duplicate = state
        .apply_profile_item(
            capabilities,
            ProfileItemCommand::Add(profile_item(1, 22, 1)),
        )
        .unwrap_err();
    assert_eq!(
        duplicate,
        AccountError::DuplicateEntityId(EntityKind::ProfileItem)
    );
    assert_eq!(state, before);

    let full = state
        .apply_profile_item(
            capabilities,
            ProfileItemCommand::Add(profile_item(2, 22, 1)),
        )
        .unwrap_err();
    assert_eq!(
        full,
        AccountError::CapacityExceeded {
            entity: EntityKind::ProfileItem,
            capacity: 1,
        }
    );
    assert_eq!(state, before);
}

#[test]
fn loaded_capacity_tolerance_does_not_disable_the_command_capacity() {
    let capabilities = ProfileCapabilities {
        profile_item_capacity: Some(1),
        enforce_loaded_profile_item_capacity: false,
        ..WRITABLE_FILTERED
    };
    let mut state = ProfileState::try_new(
        capabilities,
        vec![profile_item(1, 11, 1), profile_item(2, 22, 1)],
        Vec::new(),
    )
    .unwrap();
    let before = state.clone();

    let error = state
        .apply_profile_item(
            capabilities,
            ProfileItemCommand::Add(profile_item(3, 33, 1)),
        )
        .unwrap_err();

    assert_eq!(
        error,
        AccountError::CapacityExceeded {
            entity: EntityKind::ProfileItem,
            capacity: 1,
        }
    );
    assert_eq!(state, before);
}

#[test]
fn unfiltered_dismantle_formats_have_one_policy_per_definition() {
    let capabilities = ProfileCapabilities {
        filtered_dismantle_rewards: false,
        ..WRITABLE_FILTERED
    };
    let mut state = ProfileState::default();
    state
        .apply_dismantle_reward(
            capabilities,
            DismantleRewardCommand::AddForDefinition {
                id: id(1),
                definition_hash: DefinitionHash::new(44),
            },
        )
        .unwrap();
    let before = state.clone();

    let error = state
        .apply_dismantle_reward(
            capabilities,
            DismantleRewardCommand::AddForDefinition {
                id: id(2),
                definition_hash: DefinitionHash::new(44),
            },
        )
        .unwrap_err();

    assert_eq!(error, AccountError::NoAvailableDismantlePolicy);
    assert_eq!(state, before);
}

#[test]
fn combined_gear_class_capability_participates_in_generated_policies() {
    let capabilities = ProfileCapabilities {
        combined_dismantle_gear_class: true,
        ..WRITABLE_FILTERED
    };
    let mut state = ProfileState::default();
    for id_value in 1..=10 {
        state
            .apply_dismantle_reward(
                capabilities,
                DismantleRewardCommand::AddForDefinition {
                    id: id(id_value),
                    definition_hash: DefinitionHash::new(44),
                },
            )
            .unwrap();
    }

    assert!(
        state
            .dismantle_rewards()
            .iter()
            .any(|reward| reward.gear_class == Some(DismantleGearClass::Both))
    );
}

#[test]
fn unsupported_dismantle_filters_fail_atomically() {
    let capabilities = ProfileCapabilities {
        filtered_dismantle_rewards: false,
        ..WRITABLE_FILTERED
    };
    let mut state = ProfileState::try_new(capabilities, Vec::new(), vec![reward(1, 44)]).unwrap();
    let before = state.clone();
    let mut replacement = reward(1, 44);
    replacement.rarities.push(DismantleRarity::Legendary);

    let error = state
        .apply_dismantle_reward(capabilities, DismantleRewardCommand::SetPolicy(replacement))
        .unwrap_err();

    assert_eq!(error, AccountError::UnsupportedDismantleFilters);
    assert_eq!(state, before);
}

#[test]
fn read_only_capabilities_reject_commands_without_mutation() {
    let capabilities = ProfileCapabilities {
        profile_items_writable: false,
        dismantle_rewards_writable: false,
        ..WRITABLE_FILTERED
    };
    let mut state = ProfileState::try_new(
        capabilities,
        vec![profile_item(1, 11, 1)],
        vec![reward(2, 22)],
    )
    .unwrap();
    let before = state.clone();

    assert_eq!(
        state.apply_profile_item(capabilities, ProfileItemCommand::Remove { id: id(1) }),
        Err(AccountError::ReadOnly(EntityKind::ProfileItem))
    );
    assert_eq!(
        state.apply_dismantle_reward(capabilities, DismantleRewardCommand::Remove { id: id(2) }),
        Err(AccountError::ReadOnly(EntityKind::DismantleReward))
    );
    assert_eq!(state, before);
}

#[test]
fn loaded_state_rejects_duplicate_ids_policies_and_rarities() {
    assert_eq!(
        ProfileState::try_new(
            WRITABLE_FILTERED,
            vec![profile_item(1, 11, 1), profile_item(1, 22, 1)],
            Vec::new(),
        ),
        Err(AccountError::DuplicateEntityId(EntityKind::ProfileItem))
    );

    assert_eq!(
        ProfileState::try_new(
            WRITABLE_FILTERED,
            Vec::new(),
            vec![reward(1, 44), reward(2, 44)],
        ),
        Err(AccountError::DuplicateDismantlePolicy)
    );

    let mut repeated_rarity = reward(1, 44);
    repeated_rarity.rarities = vec![DismantleRarity::Rare, DismantleRarity::Rare];
    assert_eq!(
        ProfileState::try_new(WRITABLE_FILTERED, Vec::new(), vec![repeated_rarity],),
        Err(AccountError::DuplicateDismantleRarity)
    );
}

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

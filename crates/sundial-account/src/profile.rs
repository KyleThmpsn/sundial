//! Profile inventory and dismantle-reward state and commands.

use std::collections::BTreeSet;

use crate::validation::{validate_authored_definition_hash, validate_positive_quantity};
use crate::{AccountError, AccountResult, DefinitionHash, EntityId, EntityKind};

/// Adapter-derived rules for the loaded account format.
///
/// Format versions stay in adapters. The domain receives only the behavior the loaded format
/// supports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProfileCapabilities {
    pub profile_items_writable: bool,
    pub profile_item_capacity: Option<usize>,
    pub enforce_loaded_profile_item_capacity: bool,
    pub dismantle_rewards_writable: bool,
    pub dismantle_reward_capacity: Option<usize>,
    pub filtered_dismantle_rewards: bool,
    /// Whether a format distinguishes an explicit weapon-and-armor mask from no filter.
    pub combined_dismantle_gear_class: bool,
}

/// The storage-neutral portion of account state currently owned by this domain slice.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProfileState {
    profile_items: Vec<ProfileItem>,
    dismantle_rewards: Vec<DismantleReward>,
}

impl ProfileState {
    pub fn try_new(
        capabilities: ProfileCapabilities,
        profile_items: Vec<ProfileItem>,
        dismantle_rewards: Vec<DismantleReward>,
    ) -> AccountResult<Self> {
        validate_loaded_entities(
            &profile_items,
            capabilities.profile_item_capacity,
            capabilities.enforce_loaded_profile_item_capacity,
            EntityKind::ProfileItem,
            validate_profile_item,
        )?;
        validate_loaded_entities(
            &dismantle_rewards,
            capabilities.dismantle_reward_capacity,
            true,
            EntityKind::DismantleReward,
            |reward| validate_dismantle_reward(reward, capabilities),
        )?;
        let mut policies = BTreeSet::new();
        if dismantle_rewards
            .iter()
            .any(|reward| !policies.insert(reward.policy_key()))
        {
            return Err(AccountError::DuplicateDismantlePolicy);
        }

        Ok(Self {
            profile_items,
            dismantle_rewards,
        })
    }

    #[must_use]
    pub fn profile_items(&self) -> &[ProfileItem] {
        &self.profile_items
    }

    #[must_use]
    pub fn dismantle_rewards(&self) -> &[DismantleReward] {
        &self.dismantle_rewards
    }

    pub fn apply_profile_item(
        &mut self,
        capabilities: ProfileCapabilities,
        command: ProfileItemCommand,
    ) -> AccountResult<()> {
        if !capabilities.profile_items_writable {
            return Err(AccountError::ReadOnly(EntityKind::ProfileItem));
        }

        match command {
            ProfileItemCommand::Add(item) => {
                validate_profile_item(&item)?;
                ensure_unique_entity_id(&self.profile_items, item.id, EntityKind::ProfileItem)?;
                ensure_capacity(
                    self.profile_items.len(),
                    capabilities.profile_item_capacity,
                    EntityKind::ProfileItem,
                )?;
                self.profile_items.push(item);
            }
            ProfileItemCommand::SetDefinitionHash {
                id,
                definition_hash,
            } => {
                validate_authored_definition_hash(definition_hash)?;
                let item = find_profile_item_mut(&mut self.profile_items, id)?;
                item.definition_hash = definition_hash;
            }
            ProfileItemCommand::SetQuantity { id, quantity } => {
                validate_positive_quantity(quantity)?;
                let item = find_profile_item_mut(&mut self.profile_items, id)?;
                item.quantity = quantity;
            }
            ProfileItemCommand::Remove { id } => {
                let index = self
                    .profile_items
                    .iter()
                    .position(|item| item.id == id)
                    .ok_or(AccountError::EntityNotFound(EntityKind::ProfileItem))?;
                self.profile_items.remove(index);
            }
        }
        Ok(())
    }

    pub fn apply_dismantle_reward(
        &mut self,
        capabilities: ProfileCapabilities,
        command: DismantleRewardCommand,
    ) -> AccountResult<()> {
        if !capabilities.dismantle_rewards_writable {
            return Err(AccountError::ReadOnly(EntityKind::DismantleReward));
        }

        match command {
            DismantleRewardCommand::AddForDefinition {
                id,
                definition_hash,
            } => {
                validate_dismantle_definition_hash(definition_hash)?;
                ensure_unique_entity_id(&self.dismantle_rewards, id, EntityKind::DismantleReward)?;
                ensure_capacity(
                    self.dismantle_rewards.len(),
                    capabilities.dismantle_reward_capacity,
                    EntityKind::DismantleReward,
                )?;
                let (rarities, gear_class, masterworked) = self
                    .next_dismantle_policy(
                        definition_hash,
                        capabilities.filtered_dismantle_rewards,
                        capabilities.combined_dismantle_gear_class,
                    )
                    .ok_or(AccountError::NoAvailableDismantlePolicy)?;
                self.dismantle_rewards.push(DismantleReward {
                    id,
                    definition_hash,
                    quantity: 1,
                    rarities,
                    gear_class,
                    masterworked,
                });
            }
            DismantleRewardCommand::SetPolicy(replacement) => {
                validate_dismantle_reward(&replacement, capabilities)?;
                let index = self
                    .dismantle_rewards
                    .iter()
                    .position(|reward| reward.id == replacement.id)
                    .ok_or(AccountError::EntityNotFound(EntityKind::DismantleReward))?;
                let duplicate =
                    self.dismantle_rewards
                        .iter()
                        .enumerate()
                        .any(|(candidate_index, reward)| {
                            candidate_index != index
                                && reward.policy_key() == replacement.policy_key()
                        });
                if duplicate {
                    return Err(AccountError::DuplicateDismantlePolicy);
                }
                self.dismantle_rewards[index] = replacement;
            }
            DismantleRewardCommand::Remove { id } => {
                let index = self
                    .dismantle_rewards
                    .iter()
                    .position(|reward| reward.id == id)
                    .ok_or(AccountError::EntityNotFound(EntityKind::DismantleReward))?;
                self.dismantle_rewards.remove(index);
            }
        }
        Ok(())
    }

    fn next_dismantle_policy(
        &self,
        definition_hash: DefinitionHash,
        filtered: bool,
        combined_gear_class: bool,
    ) -> Option<(
        Vec<DismantleRarity>,
        Option<DismantleGearClass>,
        Option<bool>,
    )> {
        let occupied = self
            .dismantle_rewards
            .iter()
            .map(DismantleReward::policy_key)
            .collect::<BTreeSet<_>>();
        let rarity_masks = if filtered { 0..32 } else { 0..1 };
        let gear_classes: &[Option<DismantleGearClass>] = if filtered && combined_gear_class {
            &[
                None,
                Some(DismantleGearClass::Weapon),
                Some(DismantleGearClass::Armor),
                Some(DismantleGearClass::Both),
            ]
        } else if filtered {
            &[
                None,
                Some(DismantleGearClass::Weapon),
                Some(DismantleGearClass::Armor),
            ]
        } else {
            &[None]
        };
        let masterwork_filters: &[Option<bool>] = if filtered {
            &[None, Some(false), Some(true)]
        } else {
            &[None]
        };

        for rarity_mask in rarity_masks {
            let rarities = DismantleRarity::ALL
                .into_iter()
                .enumerate()
                .filter_map(|(index, rarity)| (rarity_mask & (1 << index) != 0).then_some(rarity))
                .collect::<Vec<_>>();
            for &gear_class in gear_classes {
                for &masterworked in masterwork_filters {
                    let key = DismantlePolicyKey {
                        definition_hash,
                        rarity_mask: rarity_mask_of(&rarities),
                        gear_class,
                        masterworked,
                    };
                    if !occupied.contains(&key) {
                        return Some((rarities, gear_class, masterworked));
                    }
                }
            }
        }
        None
    }
}

/// A profile-scoped material stack.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileItem {
    pub id: EntityId,
    pub definition_hash: DefinitionHash,
    pub quantity: i32,
}

/// A mutation of profile-scoped material stacks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProfileItemCommand {
    Add(ProfileItem),
    SetDefinitionHash {
        id: EntityId,
        definition_hash: DefinitionHash,
    },
    SetQuantity {
        id: EntityId,
        quantity: i32,
    },
    Remove {
        id: EntityId,
    },
}

/// Item rarity filters used by dismantle rewards.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum DismantleRarity {
    Common,
    Uncommon,
    Rare,
    Legendary,
    Exotic,
}

impl DismantleRarity {
    pub const ALL: [Self; 5] = [
        Self::Common,
        Self::Uncommon,
        Self::Rare,
        Self::Legendary,
        Self::Exotic,
    ];

    const fn bit(self) -> u8 {
        match self {
            Self::Common => 1 << 1,
            Self::Uncommon => 1 << 2,
            Self::Rare => 1 << 3,
            Self::Legendary => 1 << 4,
            Self::Exotic => 1 << 5,
        }
    }
}

/// Gear-class filters used by dismantle rewards.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum DismantleGearClass {
    Weapon,
    Armor,
    Both,
}

/// A filtered material reward granted when matching gear is dismantled.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DismantleReward {
    pub id: EntityId,
    pub definition_hash: DefinitionHash,
    pub quantity: i32,
    pub rarities: Vec<DismantleRarity>,
    pub gear_class: Option<DismantleGearClass>,
    pub masterworked: Option<bool>,
}

impl DismantleReward {
    fn policy_key(&self) -> DismantlePolicyKey {
        DismantlePolicyKey {
            definition_hash: self.definition_hash,
            rarity_mask: rarity_mask_of(&self.rarities),
            gear_class: self.gear_class,
            masterworked: self.masterworked,
        }
    }
}

/// A mutation of dismantle-reward policies.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DismantleRewardCommand {
    AddForDefinition {
        id: EntityId,
        definition_hash: DefinitionHash,
    },
    SetPolicy(DismantleReward),
    Remove {
        id: EntityId,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct DismantlePolicyKey {
    definition_hash: DefinitionHash,
    rarity_mask: u8,
    gear_class: Option<DismantleGearClass>,
    masterworked: Option<bool>,
}

trait Identified {
    fn entity_id(&self) -> EntityId;
}

impl Identified for ProfileItem {
    fn entity_id(&self) -> EntityId {
        self.id
    }
}

impl Identified for DismantleReward {
    fn entity_id(&self) -> EntityId {
        self.id
    }
}

fn ensure_unique_entity_id<T: Identified>(
    entities: &[T],
    id: EntityId,
    kind: EntityKind,
) -> AccountResult<()> {
    if entities.iter().any(|entity| entity.entity_id() == id) {
        Err(AccountError::DuplicateEntityId(kind))
    } else {
        Ok(())
    }
}

fn validate_loaded_entities<T: Identified>(
    entities: &[T],
    capacity: Option<usize>,
    enforce_capacity: bool,
    kind: EntityKind,
    validate: impl Fn(&T) -> AccountResult<()>,
) -> AccountResult<()> {
    if enforce_capacity
        && let Some(capacity) = capacity
        && entities.len() > capacity
    {
        return Err(AccountError::CapacityExceeded {
            entity: kind,
            capacity,
        });
    }
    let mut ids = BTreeSet::new();
    for entity in entities {
        if !ids.insert(entity.entity_id()) {
            return Err(AccountError::DuplicateEntityId(kind));
        }
        validate(entity)?;
    }
    Ok(())
}

fn ensure_capacity(
    current_length: usize,
    capacity: Option<usize>,
    entity: EntityKind,
) -> AccountResult<()> {
    if let Some(capacity) = capacity
        && current_length >= capacity
    {
        return Err(AccountError::CapacityExceeded { entity, capacity });
    }
    Ok(())
}

fn find_profile_item_mut(
    items: &mut [ProfileItem],
    id: EntityId,
) -> AccountResult<&mut ProfileItem> {
    items
        .iter_mut()
        .find(|item| item.id == id)
        .ok_or(AccountError::EntityNotFound(EntityKind::ProfileItem))
}

fn validate_profile_item(item: &ProfileItem) -> AccountResult<()> {
    validate_authored_definition_hash(item.definition_hash)?;
    validate_positive_quantity(item.quantity)
}

fn validate_dismantle_reward(
    reward: &DismantleReward,
    capabilities: ProfileCapabilities,
) -> AccountResult<()> {
    validate_dismantle_definition_hash(reward.definition_hash)?;
    validate_positive_quantity(reward.quantity)?;
    if !capabilities.filtered_dismantle_rewards
        && (!reward.rarities.is_empty()
            || reward.gear_class.is_some()
            || reward.masterworked.is_some())
    {
        return Err(AccountError::UnsupportedDismantleFilters);
    }
    if reward.gear_class == Some(DismantleGearClass::Both)
        && !capabilities.combined_dismantle_gear_class
    {
        return Err(AccountError::UnsupportedDismantleFilters);
    }
    let mut rarities = BTreeSet::new();
    if reward
        .rarities
        .iter()
        .any(|rarity| !rarities.insert(*rarity))
    {
        return Err(AccountError::DuplicateDismantleRarity);
    }
    Ok(())
}

fn validate_dismantle_definition_hash(hash: DefinitionHash) -> AccountResult<()> {
    if hash.get() == 0 {
        Err(AccountError::InvalidDefinitionHash)
    } else {
        validate_authored_definition_hash(hash)
    }
}

fn rarity_mask_of(rarities: &[DismantleRarity]) -> u8 {
    rarities.iter().fold(0, |mask, rarity| mask | rarity.bit())
}

#[cfg(test)]
mod tests {
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
            ProfileState::try_new(WRITABLE_FILTERED, vec![profile_item(1, 11, 1)], Vec::new())
                .unwrap();
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
    fn filtered_dismantle_adds_choose_the_first_unoccupied_policy() {
        let mut state = ProfileState::default();
        for id_value in 1..=3 {
            state
                .apply_dismantle_reward(
                    WRITABLE_FILTERED,
                    DismantleRewardCommand::AddForDefinition {
                        id: id(id_value),
                        definition_hash: DefinitionHash::new(44),
                    },
                )
                .unwrap();
        }

        assert_eq!(state.dismantle_rewards()[0], reward(1, 44));
        assert_eq!(
            state.dismantle_rewards()[1].masterworked,
            Some(false),
            "masterwork filters vary before gear-class filters"
        );
        assert_eq!(state.dismantle_rewards()[2].masterworked, Some(true));
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

        assert_eq!(
            state.dismantle_rewards()[9].gear_class,
            Some(DismantleGearClass::Both)
        );
    }

    #[test]
    fn unsupported_dismantle_filters_fail_atomically() {
        let capabilities = ProfileCapabilities {
            filtered_dismantle_rewards: false,
            ..WRITABLE_FILTERED
        };
        let mut state =
            ProfileState::try_new(capabilities, Vec::new(), vec![reward(1, 44)]).unwrap();
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
            state
                .apply_dismantle_reward(capabilities, DismantleRewardCommand::Remove { id: id(2) }),
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
}

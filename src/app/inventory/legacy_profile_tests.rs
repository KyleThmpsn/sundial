//! Frozen legacy JSON profile mutations used only as differential-test oracles.

use std::collections::BTreeSet;

use serde_json::{Map, Value};

use super::*;

pub(super) fn add_profile_item(
    document: &mut Value,
    definition_hash: u32,
    quantity: i32,
) -> InventoryResult<ProfileItemLocation> {
    let mode = require_profile_mutation(document)?;
    validate_inventory_definition_hash(
        definition_hash,
        "/state/account/profile_items/<new>/definition_hash",
    )?;
    validate_positive_i32(quantity, "/state/account/profile_items/<new>/quantity")?;

    let existing = profile_items(document)?;
    let length = existing.as_ref().map_or(0, Vec::len);
    let capacity = mode
        .profile_item_capacity()
        .expect("writable schemas always have a known profile capacity");
    if length >= capacity {
        return Err(InventoryError::new(
            "/state/account/profile_items",
            format!("profile_items is full for this schema (maximum {capacity})"),
        ));
    }
    ensure_account_object(document)?;

    let mut item = Map::new();
    item.insert(
        "definition_hash".into(),
        Value::String(format_definition_hash_hex(definition_hash)),
    );
    item.insert("quantity".into(), Value::from(quantity));

    let account = account_object_mut(document)?;
    match account.get_mut("profile_items") {
        Some(Value::Array(items)) => items.push(Value::Object(item)),
        Some(_) => unreachable!("profile_items shape was validated before mutation"),
        None => {
            account.insert(
                "profile_items".into(),
                Value::Array(vec![Value::Object(item)]),
            );
        }
    }
    Ok(ProfileItemLocation { index: length })
}

pub(super) fn apply_profile_item_action(
    document: &mut Value,
    location: ProfileItemLocation,
    action: ProfileItemAction,
) -> InventoryResult<()> {
    require_profile_mutation(document)?;
    let snapshots = profile_items(document)?.ok_or_else(|| {
        InventoryError::new(
            "/state/account/profile_items",
            "profile_items is missing; add an item before editing a row",
        )
    })?;
    if location.index >= snapshots.len() {
        return Err(InventoryError::new(
            format!("/state/account/profile_items/{}", location.index),
            "profile item index is out of range",
        ));
    }
    match &action {
        ProfileItemAction::SetDefinitionHash(hash) => validate_inventory_definition_hash(
            *hash,
            &format!(
                "/state/account/profile_items/{}/definition_hash",
                location.index
            ),
        )?,
        ProfileItemAction::SetQuantity(quantity) => validate_positive_i32(
            *quantity,
            &format!("/state/account/profile_items/{}/quantity", location.index),
        )?,
        ProfileItemAction::Remove => {}
    }

    let items = profile_array_mut(document)?;
    match action {
        ProfileItemAction::Remove => {
            items.remove(location.index);
        }
        ProfileItemAction::SetDefinitionHash(hash) => {
            let item = items[location.index]
                .as_object_mut()
                .expect("profile row shape was validated before mutation");
            item.insert(
                "definition_hash".into(),
                Value::String(format_definition_hash_hex(hash)),
            );
        }
        ProfileItemAction::SetQuantity(quantity) => {
            let item = items[location.index]
                .as_object_mut()
                .expect("profile row shape was validated before mutation");
            item.insert("quantity".into(), Value::from(quantity));
        }
    }
    Ok(())
}

pub(super) fn add_dismantle_reward(
    document: &mut Value,
    definition_hash: u32,
) -> InventoryResult<DismantleRewardLocation> {
    let mode = require_dismantle_reward_mutation(document)?;
    validate_inventory_definition_hash(
        definition_hash,
        "/state/account/dismantle_rewards/<new>/definition_hash",
    )?;
    let existing = dismantle_rewards(document)?.unwrap_or_default();
    let capacity = mode
        .dismantle_reward_capacity()
        .expect("writable dismantle schemas have a known capacity");
    if existing.len() >= capacity {
        return Err(InventoryError::new(
            "/state/account/dismantle_rewards",
            format!("dismantle_rewards is full for this schema (maximum {capacity})"),
        ));
    }

    let occupied = existing
        .iter()
        .map(dismantle_policy_key)
        .collect::<BTreeSet<_>>();
    let mut selected = None;
    let rarity_masks = if mode.supports_filtered_dismantle_rewards() {
        0..32
    } else {
        0..1
    };
    let gear_classes: &[Option<DismantleGearClass>] = if mode.supports_filtered_dismantle_rewards()
    {
        &[
            None,
            Some(DismantleGearClass::Weapon),
            Some(DismantleGearClass::Armor),
        ]
    } else {
        &[None]
    };
    let masterwork_filters: &[Option<bool>] = if mode.supports_filtered_dismantle_rewards() {
        &[None, Some(false), Some(true)]
    } else {
        &[None]
    };
    'policies: for rarity_mask in rarity_masks {
        let rarities = DismantleRarity::ALL
            .into_iter()
            .enumerate()
            .filter_map(|(index, rarity)| (rarity_mask & (1 << index) != 0).then_some(rarity))
            .collect::<Vec<_>>();
        for &gear_class in gear_classes {
            for &masterworked in masterwork_filters {
                let candidate = (
                    definition_hash,
                    rarity_mask_of(&rarities),
                    gear_class.map_or(0, DismantleGearClass::mask),
                    masterworked.map_or(0, |value| if value { 1 } else { 2 }),
                );
                if !occupied.contains(&candidate) {
                    selected = Some((rarities, gear_class, masterworked));
                    break 'policies;
                }
            }
        }
    }
    let Some((rarities, gear_class, masterworked)) = selected else {
        return Err(InventoryError::new(
            "/state/account/dismantle_rewards",
            "every supported filter combination for this material is already present",
        ));
    };

    let mut candidate = document.clone();
    ensure_account_object(&candidate)?;
    let mut reward = Map::new();
    write_dismantle_policy(
        &mut reward,
        definition_hash,
        1,
        &rarities,
        gear_class,
        masterworked,
        mode.supports_filtered_dismantle_rewards(),
    );
    let account = account_object_mut(&mut candidate)?;
    match account.get_mut("dismantle_rewards") {
        Some(Value::Array(rewards)) => rewards.push(Value::Object(reward)),
        Some(_) => unreachable!("dismantle rewards were validated before mutation"),
        None => {
            account.insert(
                "dismantle_rewards".into(),
                Value::Array(vec![Value::Object(reward)]),
            );
        }
    }
    validate_document_items(&candidate)?;
    *document = candidate;
    Ok(DismantleRewardLocation {
        index: existing.len(),
    })
}

pub(super) fn apply_dismantle_reward_action(
    document: &mut Value,
    location: DismantleRewardLocation,
    action: DismantleRewardAction,
) -> InventoryResult<()> {
    let mode = require_dismantle_reward_mutation(document)?;
    let snapshots = dismantle_rewards(document)?.ok_or_else(|| {
        InventoryError::new(
            "/state/account/dismantle_rewards",
            "dismantle_rewards is missing; add a policy before editing a row",
        )
    })?;
    if location.index >= snapshots.len() {
        return Err(InventoryError::new(
            format!("/state/account/dismantle_rewards/{}", location.index),
            "dismantle reward index is out of range",
        ));
    }
    if let DismantleRewardAction::SetPolicy {
        definition_hash,
        quantity,
        rarities,
        gear_class,
        masterworked,
    } = &action
    {
        validate_inventory_definition_hash(
            *definition_hash,
            &format!(
                "/state/account/dismantle_rewards/{}/definition_hash",
                location.index
            ),
        )?;
        validate_positive_i32(
            *quantity,
            &format!(
                "/state/account/dismantle_rewards/{}/quantity",
                location.index
            ),
        )?;
        if !mode.supports_filtered_dismantle_rewards()
            && (!rarities.is_empty() || gear_class.is_some() || masterworked.is_some())
        {
            return Err(InventoryError::new(
                format!("/state/account/dismantle_rewards/{}", location.index),
                "dismantle filters require settings schema 8",
            ));
        }
    }

    let mut candidate = document.clone();
    let rewards = candidate
        .pointer_mut("/state/account/dismantle_rewards")
        .and_then(Value::as_array_mut)
        .expect("dismantle rewards were validated before mutation");
    match action {
        DismantleRewardAction::Remove => {
            rewards.remove(location.index);
        }
        DismantleRewardAction::SetPolicy {
            definition_hash,
            quantity,
            rarities,
            gear_class,
            masterworked,
        } => {
            let reward = rewards[location.index]
                .as_object_mut()
                .expect("dismantle reward row shape was validated before mutation");
            write_dismantle_policy(
                reward,
                definition_hash,
                quantity,
                &rarities,
                gear_class,
                masterworked,
                mode.supports_filtered_dismantle_rewards(),
            );
        }
    }
    validate_document_items(&candidate)?;
    *document = candidate;
    Ok(())
}

fn require_dismantle_reward_mutation(document: &Value) -> InventoryResult<SchemaMode> {
    let mode = require_readable_schema(document)?;
    if mode.can_mutate_dismantle_rewards() {
        Ok(mode)
    } else {
        Err(read_only_schema_error(mode, "dismantle rewards"))
    }
}

fn dismantle_policy_key(snapshot: &DismantleRewardSnapshot) -> (u32, u8, u8, u8) {
    (
        snapshot.definition_hash,
        rarity_mask_of(&snapshot.rarities),
        snapshot.gear_class.map_or(0, DismantleGearClass::mask),
        snapshot
            .masterworked
            .map_or(0, |value| if value { 1 } else { 2 }),
    )
}

fn rarity_mask_of(rarities: &[DismantleRarity]) -> u8 {
    rarities.iter().fold(0, |mask, rarity| mask | rarity.bit())
}

fn write_dismantle_policy(
    reward: &mut Map<String, Value>,
    definition_hash: u32,
    quantity: i32,
    rarities: &[DismantleRarity],
    gear_class: Option<DismantleGearClass>,
    masterworked: Option<bool>,
    filtered: bool,
) {
    reward.insert(
        "definition_hash".into(),
        Value::String(format_definition_hash_hex(definition_hash)),
    );
    reward.insert("quantity".into(), Value::from(quantity));
    if filtered && !rarities.is_empty() {
        let value = if rarities.len() == 1 {
            Value::String(rarities[0].token().into())
        } else {
            Value::Array(
                rarities
                    .iter()
                    .map(|rarity| Value::String(rarity.token().into()))
                    .collect(),
            )
        };
        reward.insert("rarity".into(), value);
    } else {
        reward.remove("rarity");
    }
    if filtered {
        if let Some(gear_class) = gear_class {
            reward.insert("class".into(), Value::String(gear_class.token().into()));
        } else {
            reward.remove("class");
        }
        if let Some(masterworked) = masterworked {
            reward.insert("masterworked".into(), Value::Bool(masterworked));
        } else {
            reward.remove("masterworked");
        }
    } else {
        reward.remove("class");
        reward.remove("masterworked");
    }
}

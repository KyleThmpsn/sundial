use std::collections::BTreeMap;

#[cfg(feature = "sqlite-account")]
use std::collections::BTreeSet;

use serde_json::{Map, Value};
#[cfg(feature = "sqlite-account")]
use sundial_account::{
    AccountSettingKey, AccountSettingValue, DismantleReward, ItemInstance, ProfileItem,
};

#[cfg(feature = "sqlite-account")]
use crate::persistence::sqlite_account::SqliteAccountDocument;

pub(super) fn account_members_except_settings(
    account: Option<&Map<String, Value>>,
) -> BTreeMap<&str, &Value> {
    account
        .into_iter()
        .flat_map(|account| account.iter())
        .filter(|(key, _)| key.as_str() != "settings")
        .map(|(key, value)| (key.as_str(), value))
        .collect()
}

#[cfg(feature = "sqlite-account")]
pub(super) fn sqlite_change_summaries(
    before: &SqliteAccountDocument,
    after: &SqliteAccountDocument,
    limit: usize,
) -> Vec<String> {
    let mut changes = Vec::new();
    summarize_profile_items(before, after, limit, &mut changes);
    summarize_dismantle_rewards(before, after, limit, &mut changes);
    summarize_characters(before, after, limit, &mut changes);
    summarize_account_settings(before, after, limit, &mut changes);
    changes
}

#[cfg(feature = "sqlite-account")]
fn summarize_profile_items(
    before: &SqliteAccountDocument,
    after: &SqliteAccountDocument,
    limit: usize,
    changes: &mut Vec<String>,
) {
    let before = before
        .profile()
        .profile_items()
        .iter()
        .map(|item| (item.id.get(), item))
        .collect::<BTreeMap<_, _>>();
    let after = after
        .profile()
        .profile_items()
        .iter()
        .map(|item| (item.id.get(), item))
        .collect::<BTreeMap<_, _>>();
    for id in before
        .keys()
        .chain(after.keys())
        .copied()
        .collect::<BTreeSet<_>>()
    {
        summarize_profile_item(
            &format!("state.sqlite3/profile_items/{id}"),
            before.get(&id).copied(),
            after.get(&id).copied(),
            limit,
            changes,
        );
    }
}

#[cfg(feature = "sqlite-account")]
fn summarize_profile_item(
    path: &str,
    before: Option<&ProfileItem>,
    after: Option<&ProfileItem>,
    limit: usize,
    changes: &mut Vec<String>,
) {
    match (before, after) {
        (None, Some(item)) => push_summary(
            changes,
            limit,
            format!(
                "{path}: added hash {} ×{}",
                item.definition_hash.get(),
                item.quantity
            ),
        ),
        (Some(item), None) => push_summary(
            changes,
            limit,
            format!(
                "{path}: removed hash {} ×{}",
                item.definition_hash.get(),
                item.quantity
            ),
        ),
        (Some(before), Some(after)) => {
            push_field_change(
                changes,
                limit,
                &format!("{path}/definition_hash"),
                before.definition_hash.get(),
                after.definition_hash.get(),
            );
            push_field_change(
                changes,
                limit,
                &format!("{path}/quantity"),
                before.quantity,
                after.quantity,
            );
        }
        (None, None) => {}
    }
}

#[cfg(feature = "sqlite-account")]
fn summarize_dismantle_rewards(
    before: &SqliteAccountDocument,
    after: &SqliteAccountDocument,
    limit: usize,
    changes: &mut Vec<String>,
) {
    let before = before
        .profile()
        .dismantle_rewards()
        .iter()
        .map(|reward| (reward.id.get(), reward))
        .collect::<BTreeMap<_, _>>();
    let after = after
        .profile()
        .dismantle_rewards()
        .iter()
        .map(|reward| (reward.id.get(), reward))
        .collect::<BTreeMap<_, _>>();
    for id in before
        .keys()
        .chain(after.keys())
        .copied()
        .collect::<BTreeSet<_>>()
    {
        let path = format!("state.sqlite3/dismantle_rewards/{id}");
        match (before.get(&id).copied(), after.get(&id).copied()) {
            (None, Some(reward)) => push_summary(
                changes,
                limit,
                format!("{path}: added {}", dismantle_reward_label(reward)),
            ),
            (Some(reward), None) => push_summary(
                changes,
                limit,
                format!("{path}: removed {}", dismantle_reward_label(reward)),
            ),
            (Some(before), Some(after)) if before != after => push_summary(
                changes,
                limit,
                format!(
                    "{path}: {} -> {}",
                    dismantle_reward_label(before),
                    dismantle_reward_label(after)
                ),
            ),
            _ => {}
        }
    }
}

#[cfg(feature = "sqlite-account")]
fn dismantle_reward_label(reward: &DismantleReward) -> String {
    format!(
        "hash {} ×{} · rarities {:?} · class {:?} · masterworked {:?}",
        reward.definition_hash.get(),
        reward.quantity,
        reward.rarities,
        reward.gear_class,
        reward.masterworked
    )
}

#[cfg(feature = "sqlite-account")]
fn summarize_characters(
    before: &SqliteAccountDocument,
    after: &SqliteAccountDocument,
    limit: usize,
    changes: &mut Vec<String>,
) {
    let before = before
        .characters()
        .characters()
        .iter()
        .map(|character| (character.id.get(), character))
        .collect::<BTreeMap<_, _>>();
    let after = after
        .characters()
        .characters()
        .iter()
        .map(|character| (character.id.get(), character))
        .collect::<BTreeMap<_, _>>();
    for id in before
        .keys()
        .chain(after.keys())
        .copied()
        .collect::<BTreeSet<_>>()
    {
        let path = format!("state.sqlite3/characters/{id}");
        match (before.get(&id).copied(), after.get(&id).copied()) {
            (None, Some(character)) => push_summary(
                changes,
                limit,
                format!("{path}: added character SOID {:?}", character.soid),
            ),
            (Some(character), None) => push_summary(
                changes,
                limit,
                format!("{path}: removed character SOID {:?}", character.soid),
            ),
            (Some(before), Some(after)) => {
                push_field_change(
                    changes,
                    limit,
                    &format!("{path}/metadata"),
                    format!("{:?}", before.metadata),
                    format!("{:?}", after.metadata),
                );
                summarize_items(
                    &format!("{path}/inventory"),
                    &before.inventory,
                    &after.inventory,
                    limit,
                    changes,
                );
                for slot in before
                    .equipment
                    .keys()
                    .chain(after.equipment.keys())
                    .collect::<BTreeSet<_>>()
                {
                    summarize_item(
                        &format!("{path}/equipment/{}", slot.as_str()),
                        before.equipment.get(slot).and_then(Option::as_ref),
                        after.equipment.get(slot).and_then(Option::as_ref),
                        limit,
                        changes,
                    );
                }
            }
            (None, None) => {}
        }
    }
}

#[cfg(feature = "sqlite-account")]
fn summarize_items(
    path: &str,
    before: &[ItemInstance],
    after: &[ItemInstance],
    limit: usize,
    changes: &mut Vec<String>,
) {
    let before = before
        .iter()
        .map(|item| (item.id.get(), item))
        .collect::<BTreeMap<_, _>>();
    let after = after
        .iter()
        .map(|item| (item.id.get(), item))
        .collect::<BTreeMap<_, _>>();
    for id in before
        .keys()
        .chain(after.keys())
        .copied()
        .collect::<BTreeSet<_>>()
    {
        summarize_item(
            &format!("{path}/{id}"),
            before.get(&id).copied(),
            after.get(&id).copied(),
            limit,
            changes,
        );
    }
}

#[cfg(feature = "sqlite-account")]
fn summarize_item(
    path: &str,
    before: Option<&ItemInstance>,
    after: Option<&ItemInstance>,
    limit: usize,
    changes: &mut Vec<String>,
) {
    match (before, after) {
        (None, Some(item)) => push_summary(
            changes,
            limit,
            format!(
                "{path}: added hash {} · Power {} · quantity {}",
                item.definition_hash.get(),
                item.level,
                item.quantity
            ),
        ),
        (Some(item), None) => push_summary(
            changes,
            limit,
            format!(
                "{path}: removed hash {} · Power {} · quantity {}",
                item.definition_hash.get(),
                item.level,
                item.quantity
            ),
        ),
        (Some(before), Some(after)) => {
            push_field_change(
                changes,
                limit,
                &format!("{path}/definition_hash"),
                before.definition_hash.get(),
                after.definition_hash.get(),
            );
            push_field_change(
                changes,
                limit,
                &format!("{path}/power"),
                before.level,
                after.level,
            );
            push_field_change(
                changes,
                limit,
                &format!("{path}/quantity"),
                before.quantity,
                after.quantity,
            );
            push_field_change(
                changes,
                limit,
                &format!("{path}/plugs"),
                format!("{:?}", before.plugs),
                format!("{:?}", after.plugs),
            );
            push_field_change(
                changes,
                limit,
                &format!("{path}/flags"),
                format!("{:?}", before.flags),
                format!("{:?}", after.flags),
            );
        }
        (None, None) => {}
    }
}

#[cfg(feature = "sqlite-account")]
fn summarize_account_settings(
    before: &SqliteAccountDocument,
    after: &SqliteAccountDocument,
    limit: usize,
    changes: &mut Vec<String>,
) {
    for key in before
        .settings()
        .values()
        .keys()
        .chain(after.settings().values().keys())
        .collect::<BTreeSet<_>>()
    {
        let before = before.settings().values().get(key);
        let after = after.settings().values().get(key);
        if before != after {
            push_summary(
                changes,
                limit,
                format!(
                    "state.sqlite3/account_settings/{}: {} -> {}",
                    account_setting_key_label(key),
                    account_setting_value_label(before),
                    account_setting_value_label(after)
                ),
            );
        }
    }
}

#[cfg(feature = "sqlite-account")]
fn account_setting_key_label(key: &AccountSettingKey) -> String {
    match key {
        AccountSettingKey::Preference { group, name } => {
            format!("{}/{name}", format!("{group:?}").to_ascii_lowercase())
        }
        AccountSettingKey::KeyBinding { action, slot } => {
            format!(
                "key_bindings/{action}/{}",
                format!("{slot:?}").to_ascii_lowercase()
            )
        }
    }
}

#[cfg(feature = "sqlite-account")]
fn account_setting_value_label(value: Option<&AccountSettingValue>) -> String {
    match value {
        Some(AccountSettingValue::Boolean(value)) => value.to_string(),
        Some(AccountSettingValue::Unsigned(value)) => value.to_string(),
        Some(AccountSettingValue::Decimal(value)) => value.get().to_string(),
        Some(AccountSettingValue::Text(value)) => format!("{value:?}"),
        Some(AccountSettingValue::InputCode(value)) => format!("input code {value}"),
        Some(AccountSettingValue::Unassigned) => "unassigned".to_owned(),
        None => "missing".to_owned(),
    }
}

#[cfg(feature = "sqlite-account")]
fn push_field_change<T: PartialEq + std::fmt::Display>(
    changes: &mut Vec<String>,
    limit: usize,
    path: &str,
    before: T,
    after: T,
) {
    if before != after {
        push_summary(changes, limit, format!("{path}: {before} -> {after}"));
    }
}

#[cfg(feature = "sqlite-account")]
fn push_summary(changes: &mut Vec<String>, limit: usize, summary: String) {
    if changes.len() < limit {
        changes.push(summary);
    }
}

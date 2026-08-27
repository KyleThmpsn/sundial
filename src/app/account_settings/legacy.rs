//! Frozen test-only behavior of guided account-setting JSON writes.

use serde_json::{Map, Number, Value};
use sundial_account::{
    AccountSettingGroup, AccountSettingKey, AccountSettingValue, AccountSettingsCommand,
    KeyBindingSlot,
};

pub(super) fn apply_commands(
    document: &mut Value,
    commands: Vec<AccountSettingsCommand>,
) -> Result<bool, String> {
    let mut changed = false;
    for command in commands {
        let AccountSettingsCommand::Set { key, value } = command;
        let replacement = encode(value)?;
        let target = target_mut(document, &key)?;
        if *target != replacement {
            *target = replacement;
            changed = true;
        }
    }
    Ok(changed)
}

pub(super) fn insert_target(
    settings: &mut Map<String, Value>,
    key: &AccountSettingKey,
    value: Value,
) {
    match key {
        AccountSettingKey::Preference { group, name } => {
            if *group == AccountSettingGroup::Root {
                settings.insert(name.to_string(), value);
            } else {
                let group = settings
                    .entry(group_name(*group).to_owned())
                    .or_insert_with(|| Value::Object(Map::new()))
                    .as_object_mut()
                    .expect("test setting groups are objects");
                group.insert(name.to_string(), value);
            }
        }
        AccountSettingKey::KeyBinding { action, slot } => {
            let bindings = settings
                .entry("key_bindings")
                .or_insert_with(|| Value::Object(Map::new()))
                .as_object_mut()
                .expect("test key bindings are an object");
            let binding = bindings
                .entry(action.to_string())
                .or_insert_with(|| Value::Object(Map::new()))
                .as_object_mut()
                .expect("test bindings are objects");
            binding.insert(slot_name(*slot).into(), value);
        }
    }
}

fn target_mut<'a>(
    document: &'a mut Value,
    key: &AccountSettingKey,
) -> Result<&'a mut Value, String> {
    let settings = document
        .pointer_mut("/state/account/settings")
        .and_then(Value::as_object_mut)
        .ok_or("account settings must be an object")?;
    match key {
        AccountSettingKey::Preference { group, name } => {
            let values = if *group == AccountSettingGroup::Root {
                settings
            } else {
                settings
                    .get_mut(group_name(*group))
                    .and_then(Value::as_object_mut)
                    .ok_or("account setting group must be an object")?
            };
            values
                .get_mut(name.as_ref())
                .ok_or_else(|| format!("account setting {name} is missing"))
        }
        AccountSettingKey::KeyBinding { action, slot } => settings
            .get_mut("key_bindings")
            .and_then(Value::as_object_mut)
            .and_then(|bindings| bindings.get_mut(action.as_ref()))
            .and_then(Value::as_object_mut)
            .and_then(|binding| binding.get_mut(slot_name(*slot)))
            .ok_or_else(|| format!("key binding {action} is missing")),
    }
}

fn encode(value: AccountSettingValue) -> Result<Value, String> {
    match value {
        AccountSettingValue::Boolean(value) => Ok(Value::Bool(value)),
        AccountSettingValue::Unsigned(value) => Ok(Value::from(value)),
        AccountSettingValue::Decimal(value) => Number::from_f64(value.get())
            .map(Value::Number)
            .ok_or("account setting decimal must be finite".into()),
        AccountSettingValue::Text(value) => Ok(Value::String(value.into_string())),
        AccountSettingValue::InputCode(value) => Ok(Value::from(value)),
        AccountSettingValue::Unassigned => Ok(Value::Null),
    }
}

const fn group_name(group: AccountSettingGroup) -> &'static str {
    match group {
        AccountSettingGroup::Root => "",
        AccountSettingGroup::Controls => "controls",
        AccountSettingGroup::Audio => "audio",
        AccountSettingGroup::Display => "display",
        AccountSettingGroup::Interface => "interface",
        AccountSettingGroup::Social => "social",
    }
}

const fn slot_name(slot: KeyBindingSlot) -> &'static str {
    match slot {
        KeyBindingSlot::Primary => "primary",
        KeyBindingSlot::Secondary => "secondary",
    }
}

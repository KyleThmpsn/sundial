//! Lossless JSON projection for operation-scoped account-setting commands.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Number, Value};
use sundial_account::{
    AccountError, AccountSettingGroup, AccountSettingKey, AccountSettingValue,
    AccountSettingsCapabilities, AccountSettingsCommand, AccountSettingsState, FiniteF64,
    KeyBindingSlot,
};

use crate::game_settings::MIN_SUPPORTED_SCHEMA;

use super::{JsonAccountError, schema_version};

type JsonAccountSettingsResult<T> = Result<T, JsonAccountError>;

/// A focused JSON projection containing only the settings named by one command batch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct JsonAccountSettingsAdapter {
    source_schema_version: u64,
    capabilities: AccountSettingsCapabilities,
    state: AccountSettingsState,
    original_values: BTreeMap<AccountSettingKey, Option<AccountSettingValue>>,
    changed_keys: BTreeSet<AccountSettingKey>,
}

impl JsonAccountSettingsAdapter {
    pub(crate) fn load_for_commands(
        document: &Value,
        commands: &[AccountSettingsCommand],
    ) -> JsonAccountSettingsResult<Self> {
        let source_schema_version = schema_version(document)?;
        if source_schema_version < MIN_SUPPORTED_SCHEMA {
            return Err(JsonAccountError::format(
                "/version",
                format!(
                    "settings schema {source_schema_version} predates supported schema {MIN_SUPPORTED_SCHEMA}"
                ),
            ));
        }
        let capabilities = AccountSettingsCapabilities {
            writable: true,
            named_key_bindings_writable: source_schema_version >= 3,
            numeric_key_bindings_writable: false,
            extended_field_of_view: source_schema_version >= 16,
        };
        let settings = account_settings(document)?;
        let keys = commands
            .iter()
            .map(|command| match command {
                AccountSettingsCommand::Set { key, .. } => key.clone(),
            })
            .collect::<BTreeSet<_>>();
        let mut values = BTreeMap::new();
        let mut original_values = BTreeMap::new();
        for key in keys {
            let fallback = default_value(&key)?;
            let raw = setting_value(settings, &key)?;
            let decoded = decode_value(&key, raw);
            let loaded = match decoded.clone() {
                Some(value) => {
                    let one = BTreeMap::from([(key.clone(), value.clone())]);
                    match AccountSettingsState::try_new(capabilities, one) {
                        Ok(_) => value,
                        Err(AccountError::InvalidAccountSettingValue) => fallback,
                        Err(error) => return Err(error.into()),
                    }
                }
                None => fallback,
            };
            original_values.insert(key.clone(), decoded.filter(|value| value == &loaded));
            values.insert(key, loaded);
        }
        let state = AccountSettingsState::try_new(capabilities, values)?;
        Ok(Self {
            source_schema_version,
            capabilities,
            state,
            original_values,
            changed_keys: BTreeSet::new(),
        })
    }

    pub(crate) fn apply(
        &self,
        document: &Value,
        commands: Vec<AccountSettingsCommand>,
    ) -> JsonAccountSettingsResult<(Self, Value, bool)> {
        let mut candidate = self.clone();
        candidate
            .state
            .apply_all(candidate.capabilities, commands)?;
        candidate.changed_keys = candidate
            .state
            .values()
            .iter()
            .filter_map(|(key, value)| {
                (candidate.original_values.get(key).and_then(Option::as_ref) != Some(value))
                    .then_some(key.clone())
            })
            .collect();
        let changed = !candidate.changed_keys.is_empty();
        let projected = candidate.project(document)?;
        Ok((candidate, projected, changed))
    }

    fn project(&self, document: &Value) -> JsonAccountSettingsResult<Value> {
        if schema_version(document)? != self.source_schema_version {
            return Err(JsonAccountError::format(
                "/version",
                "the JSON schema changed after the account settings projection was loaded",
            ));
        }
        let mut candidate = document.clone();
        let settings = account_settings_mut(&mut candidate)?;
        for key in &self.changed_keys {
            let value = self
                .state
                .values()
                .get(key)
                .expect("changed settings remain in the operation-scoped state");
            *setting_value_mut(settings, key)? = encode_value(value);
        }
        Ok(candidate)
    }
}

fn account_settings(document: &Value) -> JsonAccountSettingsResult<&Map<String, Value>> {
    document
        .pointer("/state/account/settings")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            JsonAccountError::format(
                "/state/account/settings",
                "account settings must be an object",
            )
        })
}

fn account_settings_mut(
    document: &mut Value,
) -> JsonAccountSettingsResult<&mut Map<String, Value>> {
    document
        .pointer_mut("/state/account/settings")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| {
            JsonAccountError::format(
                "/state/account/settings",
                "account settings must be an object",
            )
        })
}

fn preference_group(
    settings: &Map<String, Value>,
    group: AccountSettingGroup,
) -> JsonAccountSettingsResult<&Map<String, Value>> {
    let name = group_name(group)
        .ok_or_else(|| JsonAccountError::from(AccountError::UnknownAccountSetting))?;
    settings
        .get(name)
        .and_then(Value::as_object)
        .ok_or_else(|| {
            JsonAccountError::format(
                format!("/state/account/settings/{name}"),
                "account setting group must be an object",
            )
        })
}

fn preference_group_mut(
    settings: &mut Map<String, Value>,
    group: AccountSettingGroup,
) -> JsonAccountSettingsResult<&mut Map<String, Value>> {
    let name = group_name(group)
        .ok_or_else(|| JsonAccountError::from(AccountError::UnknownAccountSetting))?;
    settings
        .get_mut(name)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| {
            JsonAccountError::format(
                format!("/state/account/settings/{name}"),
                "account setting group must be an object",
            )
        })
}

fn group_name(group: AccountSettingGroup) -> Option<&'static str> {
    match group {
        AccountSettingGroup::Root => None,
        AccountSettingGroup::Controls => Some("controls"),
        AccountSettingGroup::Audio => Some("audio"),
        AccountSettingGroup::Display => Some("display"),
        AccountSettingGroup::Interface => Some("interface"),
        AccountSettingGroup::Social => Some("social"),
    }
}

fn setting_value<'a>(
    settings: &'a Map<String, Value>,
    key: &AccountSettingKey,
) -> JsonAccountSettingsResult<&'a Value> {
    match key {
        AccountSettingKey::Preference { group, name } => {
            let values = if *group == AccountSettingGroup::Root {
                settings
            } else {
                preference_group(settings, *group)?
            };
            values.get(name.as_ref()).ok_or_else(|| {
                JsonAccountError::format(setting_path(key), "account setting is missing")
            })
        }
        AccountSettingKey::KeyBinding { action, slot } => settings
            .get("key_bindings")
            .and_then(Value::as_object)
            .and_then(|bindings| bindings.get(action.as_ref()))
            .and_then(Value::as_object)
            .and_then(|binding| binding.get(binding_slot_name(*slot)))
            .ok_or_else(|| JsonAccountError::format(setting_path(key), "key binding is missing")),
    }
}

fn setting_value_mut<'a>(
    settings: &'a mut Map<String, Value>,
    key: &AccountSettingKey,
) -> JsonAccountSettingsResult<&'a mut Value> {
    let path = setting_path(key);
    match key {
        AccountSettingKey::Preference { group, name } => {
            let values = if *group == AccountSettingGroup::Root {
                settings
            } else {
                preference_group_mut(settings, *group)?
            };
            values
                .get_mut(name.as_ref())
                .ok_or_else(|| JsonAccountError::format(path, "account setting is missing"))
        }
        AccountSettingKey::KeyBinding { action, slot } => settings
            .get_mut("key_bindings")
            .and_then(Value::as_object_mut)
            .and_then(|bindings| bindings.get_mut(action.as_ref()))
            .and_then(Value::as_object_mut)
            .and_then(|binding| binding.get_mut(binding_slot_name(*slot)))
            .ok_or_else(|| JsonAccountError::format(path, "key binding is missing")),
    }
}

fn setting_path(key: &AccountSettingKey) -> String {
    match key {
        AccountSettingKey::Preference { group, name } => group_name(*group).map_or_else(
            || format!("/state/account/settings/{name}"),
            |group| format!("/state/account/settings/{group}/{name}"),
        ),
        AccountSettingKey::KeyBinding { action, slot } => format!(
            "/state/account/settings/key_bindings/{action}/{}",
            binding_slot_name(*slot)
        ),
    }
}

const fn binding_slot_name(slot: KeyBindingSlot) -> &'static str {
    match slot {
        KeyBindingSlot::Primary => "primary",
        KeyBindingSlot::Secondary => "secondary",
    }
}

fn decode_value(key: &AccountSettingKey, value: &Value) -> Option<AccountSettingValue> {
    match key {
        AccountSettingKey::KeyBinding { .. } => match value {
            Value::Null => Some(AccountSettingValue::Unassigned),
            Value::String(value) => Some(AccountSettingValue::text(value)),
            _ => None,
        },
        AccountSettingKey::Preference { group, name }
            if *group == AccountSettingGroup::Root && name.as_ref() == "key_binding_source" =>
        {
            value.as_str().map(AccountSettingValue::text)
        }
        AccountSettingKey::Preference { group, name }
            if *group == AccountSettingGroup::Controls
                && name.as_ref() == "ads_sensitivity_modifier" =>
        {
            value
                .as_f64()
                .and_then(FiniteF64::new)
                .map(AccountSettingValue::Decimal)
        }
        AccountSettingKey::Preference { group, name } if is_boolean_preference(*group, name) => {
            value.as_bool().map(AccountSettingValue::Boolean)
        }
        AccountSettingKey::Preference { .. } => value.as_u64().map(AccountSettingValue::Unsigned),
    }
}

fn is_boolean_preference(group: AccountSettingGroup, name: &str) -> bool {
    match group {
        AccountSettingGroup::Controls => matches!(
            name,
            "controller_invert_vertical"
                | "controller_auto_look_centering"
                | "controller_vibration"
                | "controller_swap_shoulders"
                | "controller_invert_horizontal"
                | "mouse_invert_vertical"
                | "mouse_invert_horizontal"
                | "unidentified_toggle"
                | "mouse_aim_smoothing"
        ),
        AccountSettingGroup::Audio => name == "mute_when_unfocused",
        AccountSettingGroup::Display => name == "show_fps",
        AccountSettingGroup::Interface => name == "display_hints",
        AccountSettingGroup::Social => matches!(
            name,
            "prefer_good_connection"
                | "show_real_names"
                | "clan_invite_notifications"
                | "profanity_filter"
                | "voice_chat_enabled"
        ),
        AccountSettingGroup::Root => false,
    }
}

fn default_value(key: &AccountSettingKey) -> JsonAccountSettingsResult<AccountSettingValue> {
    let value = match key {
        AccountSettingKey::KeyBinding { .. } => AccountSettingValue::Unassigned,
        AccountSettingKey::Preference { group, name }
            if *group == AccountSettingGroup::Root && name.as_ref() == "key_binding_source" =>
        {
            AccountSettingValue::text("computer")
        }
        AccountSettingKey::Preference { group, name }
            if *group == AccountSettingGroup::Controls
                && name.as_ref() == "ads_sensitivity_modifier" =>
        {
            AccountSettingValue::Decimal(
                FiniteF64::new(1.0).expect("the account setting default is finite"),
            )
        }
        AccountSettingKey::Preference { group, name } if is_boolean_preference(*group, name) => {
            AccountSettingValue::Boolean(false)
        }
        AccountSettingKey::Preference { group, name }
            if (*group == AccountSettingGroup::Controls
                && name.as_ref() == "mouse_look_sensitivity") =>
        {
            AccountSettingValue::Unsigned(1)
        }
        AccountSettingKey::Preference { group, name }
            if *group == AccountSettingGroup::Display && name.as_ref() == "field_of_view" =>
        {
            AccountSettingValue::Unsigned(85)
        }
        AccountSettingKey::Preference { .. } => AccountSettingValue::Unsigned(0),
    };
    let one = BTreeMap::from([(key.clone(), value.clone())]);
    AccountSettingsState::try_new(
        AccountSettingsCapabilities {
            writable: true,
            named_key_bindings_writable: true,
            numeric_key_bindings_writable: false,
            extended_field_of_view: true,
        },
        one,
    )?;
    Ok(value)
}

fn encode_value(value: &AccountSettingValue) -> Value {
    match value {
        AccountSettingValue::Boolean(value) => Value::Bool(*value),
        AccountSettingValue::Unsigned(value) => Value::from(*value),
        AccountSettingValue::Decimal(value) => Value::Number(
            Number::from_f64(value.get()).expect("storage-neutral decimal settings are finite"),
        ),
        AccountSettingValue::Text(value) => Value::String(value.to_string()),
        AccountSettingValue::InputCode(value) => Value::from(*value),
        AccountSettingValue::Unassigned => Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::game_settings::MAX_SUPPORTED_SCHEMA;

    fn document(version: u64) -> Value {
        json!({
            "version": version,
            "state": {"account": {"settings": {
                "display": {"brightness": 3, "show_fps": false, "future": {"keep": true}},
                "key_bindings": {"fire": {"primary": "f", "secondary": null}},
                "future_group": {"keep": true}
            }}},
            "future_root": {"keep": true}
        })
    }

    fn set(key: AccountSettingKey, value: AccountSettingValue) -> AccountSettingsCommand {
        AccountSettingsCommand::Set { key, value }
    }

    #[test]
    fn focused_projection_changes_only_requested_fields() {
        let source = document(8);
        let key = AccountSettingKey::preference(AccountSettingGroup::Display, "brightness");
        let commands = vec![set(key, AccountSettingValue::Unsigned(6))];
        let adapter = JsonAccountSettingsAdapter::load_for_commands(&source, &commands).unwrap();
        let (_, projected, changed) = adapter.apply(&source, commands).unwrap();

        assert!(changed);
        assert_eq!(
            projected.pointer("/state/account/settings/display/brightness"),
            Some(&Value::from(6))
        );
        assert_eq!(
            projected.pointer("/state/account/settings/display/future/keep"),
            Some(&Value::Bool(true))
        );
        assert_eq!(
            projected.pointer("/future_root/keep"),
            Some(&Value::Bool(true))
        );
    }

    #[test]
    fn invalid_current_values_can_be_repaired_without_loading_unrelated_settings() {
        let mut source = document(MAX_SUPPORTED_SCHEMA + 1);
        *source
            .pointer_mut("/state/account/settings/display/brightness")
            .unwrap() = Value::from(999);
        *source
            .pointer_mut("/state/account/settings/key_bindings")
            .unwrap() = Value::String("future layout".into());
        let key = AccountSettingKey::preference(AccountSettingGroup::Display, "brightness");
        let commands = vec![set(key, AccountSettingValue::Unsigned(2))];

        let adapter = JsonAccountSettingsAdapter::load_for_commands(&source, &commands).unwrap();
        let (_, projected, changed) = adapter.apply(&source, commands).unwrap();

        assert!(changed);
        assert_eq!(
            projected.pointer("/state/account/settings/display/brightness"),
            Some(&Value::from(2))
        );
        assert_eq!(
            projected.pointer("/state/account/settings/key_bindings"),
            source.pointer("/state/account/settings/key_bindings")
        );
    }
}

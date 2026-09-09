//! Normalized account preferences. Unknown columns and reserved native values stay untouched.
use super::{SqliteAccountDocument, SqliteAccountError, contract::KEY_BINDING_ACTIONS};
use rusqlite::{Connection, params, types::Value};
use std::collections::BTreeMap;
use sundial_account::{
    AccountSettingGroup, AccountSettingKey, AccountSettingValue, AccountSettingsState, FiniteF64,
    KeyBindingSlot,
};
const GROUPS: &[(AccountSettingGroup, &str)] = &[
    (AccountSettingGroup::Root, "account_preferences"),
    (AccountSettingGroup::Controls, "account_controls"),
    (AccountSettingGroup::Audio, "account_audio"),
    (AccountSettingGroup::Display, "account_display"),
    (AccountSettingGroup::Interface, "account_interface"),
    (AccountSettingGroup::Social, "account_social"),
];
const BOOLEANS: &[&str] = &[
    "controller_invert_vertical",
    "controller_auto_look_centering",
    "controller_vibration",
    "controller_swap_shoulders",
    "controller_invert_horizontal",
    "mouse_invert_vertical",
    "mouse_invert_horizontal",
    "unidentified_toggle",
    "mouse_aim_smoothing",
    "mute_when_unfocused",
    "show_fps",
    "display_hints",
    "prefer_good_connection",
    "show_real_names",
    "clan_invite_notifications",
    "profanity_filter",
    "voice_chat_enabled",
];

pub(super) fn load(connection: &Connection) -> Result<AccountSettingsState, SqliteAccountError> {
    let mut values = BTreeMap::new();
    for &(group, table) in GROUPS {
        let mut statement = connection
            .prepare(&format!("SELECT * FROM {table} WHERE id=1"))
            .map_err(|error| SqliteAccountError::sqlite("read settings from", error))?;
        let names = statement
            .column_names()
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let row_values = statement
            .query_row([], |row| {
                (0..names.len())
                    .map(|i| row.get::<_, Value>(i))
                    .collect::<Result<Vec<_>, _>>()
            })
            .map_err(|error| SqliteAccountError::sqlite("read settings from", error))?;
        for (name, value) in names.iter().zip(row_values) {
            let Some(key) = AccountSettingKey::known_preference(name) else {
                continue;
            };
            if !matches!(&key, AccountSettingKey::Preference { group: key_group, .. } if *key_group == group)
            {
                continue;
            }
            let value = match value {
                Value::Integer(value) if name == "key_binding_source" => match value {
                    0 => AccountSettingValue::text("account"),
                    1 => AccountSettingValue::text("computer"),
                    _ => return Err(invalid(name)),
                },
                Value::Integer(value) if BOOLEANS.contains(&name.as_str()) => {
                    if !(0..=1).contains(&value) {
                        return Err(invalid(name));
                    }
                    AccountSettingValue::Boolean(value != 0)
                }
                Value::Integer(value) => {
                    AccountSettingValue::Unsigned(u64::try_from(value).map_err(|_| invalid(name))?)
                }
                Value::Real(value) => AccountSettingValue::Decimal(
                    FiniteF64::new(value).ok_or_else(|| invalid(name))?,
                ),
                _ => return Err(invalid(name)),
            };
            values.insert(key, value);
        }
    }
    let mut statement = connection
        .prepare(
            "SELECT action, primary_code, secondary_code FROM account_key_bindings ORDER BY action",
        )
        .map_err(|error| SqliteAccountError::sqlite("read key bindings from", error))?;
    let mut rows = statement
        .query([])
        .map_err(|error| SqliteAccountError::sqlite("read key bindings from", error))?;
    let mut count = 0;
    while let Some(row) = rows
        .next()
        .map_err(|error| SqliteAccountError::sqlite("read key bindings from", error))?
    {
        let action: usize = row
            .get(0)
            .map_err(|error| SqliteAccountError::sqlite("read key bindings from", error))?;
        if action != count {
            return Err(invalid("account_key_bindings.action"));
        }
        let name = KEY_BINDING_ACTIONS
            .get(action)
            .ok_or_else(|| invalid("account_key_bindings.action"))?;
        for (index, slot) in [(1, KeyBindingSlot::Primary), (2, KeyBindingSlot::Secondary)] {
            let code: i64 = row
                .get(index)
                .map_err(|error| SqliteAccountError::sqlite("read key bindings from", error))?;
            let value = if code == -1 {
                AccountSettingValue::Unassigned
            } else {
                AccountSettingValue::InputCode(
                    u16::try_from(code).map_err(|_| invalid("account_key_bindings.code"))?,
                )
            };
            values.insert(AccountSettingKey::key_binding(*name, slot), value);
        }
        count += 1;
    }
    if count != KEY_BINDING_ACTIONS.len() {
        return Err(invalid("account_key_bindings"));
    }
    AccountSettingsState::try_new(SqliteAccountDocument::settings_capabilities(), values)
        .map_err(Into::into)
}

pub(super) fn save(
    connection: &Connection,
    settings: &AccountSettingsState,
) -> Result<(), SqliteAccountError> {
    for (key, value) in settings.values() {
        match key {
            AccountSettingKey::Preference { group, name } => {
                let table = GROUPS
                    .iter()
                    .find(|(g, _)| g == group)
                    .ok_or_else(|| invalid(name))?
                    .1;
                if AccountSettingKey::known_preference(name).as_ref() != Some(key) {
                    return Err(invalid(name));
                }
                let value = match value {
                    AccountSettingValue::Boolean(value) => Value::Integer(i64::from(*value)),
                    AccountSettingValue::Unsigned(value) => {
                        Value::Integer(i64::try_from(*value).map_err(|_| invalid(name))?)
                    }
                    AccountSettingValue::Decimal(value) => Value::Real(value.get()),
                    AccountSettingValue::Text(value) if name.as_ref() == "key_binding_source" => {
                        Value::Integer(match value.as_ref() {
                            "account" => 0,
                            "computer" => 1,
                            _ => return Err(invalid(name)),
                        })
                    }
                    _ => return Err(invalid(name)),
                };
                connection
                    .execute(
                        &format!("UPDATE {table} SET \"{name}\"=? WHERE id=1"),
                        [value],
                    )
                    .map_err(|error| SqliteAccountError::sqlite("write settings to", error))?;
            }
            AccountSettingKey::KeyBinding { action, slot } => {
                let index = KEY_BINDING_ACTIONS
                    .iter()
                    .position(|a| *a == action.as_ref())
                    .ok_or_else(|| invalid(action))?;
                let column = match slot {
                    KeyBindingSlot::Primary => "primary_code",
                    KeyBindingSlot::Secondary => "secondary_code",
                };
                let code = match value {
                    AccountSettingValue::Unassigned => -1,
                    AccountSettingValue::InputCode(code) => i64::from(*code),
                    _ => return Err(invalid(action)),
                };
                connection
                    .execute(
                        &format!("UPDATE account_key_bindings SET {column}=? WHERE action=?"),
                        params![code, index],
                    )
                    .map_err(|error| SqliteAccountError::sqlite("write key bindings to", error))?;
            }
        }
    }
    Ok(())
}
fn invalid(name: &str) -> SqliteAccountError {
    SqliteAccountError::invalid_data(name, "invalid normalized account preference")
}

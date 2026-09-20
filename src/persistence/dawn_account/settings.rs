//! Writes edited settings back to a Dawn player-state database.
//!
//! `settings_values` is one flat key/value table with a CHECK that exactly one of
//! `integer_value` and `real_value` is non-NULL, and `key_bindings` is one row per action with two
//! nullable input codes. Neither is Sunrise's shape, so neither borrows Sunrise's rules.
//!
//! Two rules make this safe. Every statement is an UPDATE of a row that was read, never a DELETE
//! or an INSERT: Dawn seeds both tables itself, and most of what it stores has no model key here,
//! so rewriting either table from the model would drop whatever this build does not name. And each
//! value goes back to the column it came from, recorded at read time, because a row that satisfies
//! the CHECK in the wrong column still breaks Dawn's typed read at boot.

use std::collections::BTreeMap;

use rusqlite::params;
use sundial_account::{
    AccountSettingKey, AccountSettingValue, AccountSettingsState, KeyBindingSlot,
};

use super::error::DawnAccountError;

/// Which column of `settings_values` a key was stored in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SettingColumn {
    Integer,
    Real,
}

/// Conversion between one storage-neutral value and Dawn's representation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum SettingEncoding {
    #[default]
    Direct,
    BindingSource,
    VerticalSyncMode,
}

/// The exact Dawn row and representation behind one storage-neutral preference.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SettingLocation {
    pub key: String,
    pub column: SettingColumn,
    pub encoding: SettingEncoding,
}

/// Where each modelled setting came from, so an edit goes back to exactly that row and column.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct SettingsIndex {
    /// The verbatim Dawn key, column and encoding behind each model key. These are recorded rather
    /// than rebuilt because some PC rows project into different storage-neutral groups.
    pub preferences: BTreeMap<AccountSettingKey, SettingLocation>,
    /// The `key_bindings.action` ordinal each action name was read from.
    pub actions: BTreeMap<String, i64>,
}

/// Writes the settings that differ from the ones loaded, and nothing else.
pub(super) fn save(
    transaction: &rusqlite::Transaction<'_>,
    index: &SettingsIndex,
    loaded: &AccountSettingsState,
    current: &AccountSettingsState,
) -> Result<(), DawnAccountError> {
    for (key, value) in current.values() {
        if loaded.values().get(key) == Some(value) {
            continue;
        }
        match key {
            AccountSettingKey::Preference { .. } => {
                let Some(location) = index.preferences.get(key) else {
                    // The model only ever holds keys the reader took from this database, so a key
                    // with no recorded row would mean writing one Dawn never had.
                    return Err(DawnAccountError::Unwritable(format!(
                        "setting {key:?} was not read from this database and cannot be written"
                    )));
                };
                write_preference(transaction, location, value)?;
            }
            AccountSettingKey::KeyBinding { action, slot } => {
                let Some(ordinal) = index.actions.get(action.as_ref()) else {
                    return Err(DawnAccountError::Unwritable(format!(
                        "key binding {action} was not read from this database and cannot be written"
                    )));
                };
                write_key_binding(transaction, *ordinal, *slot, value)?;
            }
        }
    }
    Ok(())
}

fn write_preference(
    transaction: &rusqlite::Transaction<'_>,
    location: &SettingLocation,
    value: &AccountSettingValue,
) -> Result<(), DawnAccountError> {
    let key = location.key.as_str();
    let column = location.column;
    let value = match (location.encoding, value) {
        (SettingEncoding::Direct, value) => value.clone(),
        (SettingEncoding::BindingSource, AccountSettingValue::Text(value)) => {
            AccountSettingValue::Boolean(match value.as_ref() {
                "account" => false,
                "computer" => true,
                _ => {
                    return Err(DawnAccountError::Unwritable(format!(
                        "setting {key} cannot hold binding source {value:?}"
                    )));
                }
            })
        }
        (SettingEncoding::VerticalSyncMode, AccountSettingValue::Unsigned(value))
            if *value <= 2 =>
        {
            AccountSettingValue::Unsigned(*value)
        }
        (encoding, value) => {
            return Err(DawnAccountError::Unwritable(format!(
                "setting {key} uses {encoding:?} and cannot hold {value:?}"
            )));
        }
    };
    let updated = match (column, value) {
        (SettingColumn::Integer, AccountSettingValue::Boolean(set)) => transaction.execute(
            "UPDATE settings_values SET integer_value=?2,real_value=NULL WHERE key=?1",
            params![key, i64::from(set)],
        )?,
        (SettingColumn::Integer, AccountSettingValue::Unsigned(number)) => {
            let Ok(number) = i64::try_from(number) else {
                return Err(DawnAccountError::Unwritable(format!(
                    "setting {key} holds {number}, which is larger than Dawn stores"
                )));
            };
            transaction.execute(
                "UPDATE settings_values SET integer_value=?2,real_value=NULL WHERE key=?1",
                params![key, number],
            )?
        }
        (SettingColumn::Real, AccountSettingValue::Decimal(number)) => transaction.execute(
            "UPDATE settings_values SET real_value=?2,integer_value=NULL WHERE key=?1",
            params![key, number.get()],
        )?,
        // Dawn reads each key at one fixed type. Moving a key to the other column would still
        // satisfy the table's CHECK and then fail Dawn's own typed read at boot, so refuse.
        (column, value) => {
            return Err(DawnAccountError::Unwritable(format!(
                "setting {key} is stored as {column:?} and cannot hold {value:?}"
            )));
        }
    };
    if updated != 1 {
        return Err(DawnAccountError::Unwritable(format!(
            "setting {key} is no longer in player-state.db"
        )));
    }
    Ok(())
}

fn write_key_binding(
    transaction: &rusqlite::Transaction<'_>,
    action: i64,
    slot: KeyBindingSlot,
    value: &AccountSettingValue,
) -> Result<(), DawnAccountError> {
    // Dawn stores an unbound half as NULL and keeps the row, so an unassigned slot clears the
    // column rather than removing the action.
    let code = match value {
        AccountSettingValue::Unassigned => None,
        AccountSettingValue::InputCode(code) => Some(i64::from(*code)),
        value => {
            return Err(DawnAccountError::Unwritable(format!(
                "key binding action {action} cannot hold {value:?}"
            )));
        }
    };
    let statement = match slot {
        KeyBindingSlot::Primary => "UPDATE key_bindings SET primary_input=?2 WHERE action=?1",
        KeyBindingSlot::Secondary => "UPDATE key_bindings SET secondary_input=?2 WHERE action=?1",
    };
    if transaction.execute(statement, params![action, code])? != 1 {
        return Err(DawnAccountError::Unwritable(format!(
            "key binding action {action} is no longer in player-state.db"
        )));
    }
    Ok(())
}

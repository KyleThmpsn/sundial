//! Versioned per-character runtime state, separate from legacy appearance/ability metadata.

use crate::hash::parse_unsigned_value;
use serde_json::{Map, Value};

pub(crate) const CURRENT_ACTIVITY: &str = "current_activity_index";

pub(crate) fn validate_character(character: &Value) -> Result<(), String> {
    if let Some(value) = character.get(CURRENT_ACTIVITY)
        && parse_unsigned_value(value).is_none_or(|n| n > u64::from(u16::MAX))
    {
        return Err(
            "current_activity_index must be an unsigned 16-bit index or hexadecimal string".into(),
        );
    }
    Ok(())
}

pub(crate) fn set_current_activity(
    document: &mut Value,
    index: usize,
    value: Option<Value>,
) -> Result<bool, String> {
    if !document
        .get("version")
        .and_then(Value::as_u64)
        .is_some_and(crate::account_contract::supports_v13)
    {
        return Err("Character runtime state requires JSON schema 13 or newer".into());
    }
    let mut probe = serde_json::json!({});
    if let Some(value) = &value {
        probe[CURRENT_ACTIVITY] = value.clone();
    }
    validate_character(&probe)?;
    let character = document
        .pointer_mut("/state/characters")
        .and_then(Value::as_array_mut)
        .and_then(|rows| rows.get_mut(index))
        .and_then(Value::as_object_mut)
        .ok_or("Character is missing or malformed")?;
    if character.get(CURRENT_ACTIVITY) == value.as_ref() {
        return Ok(false);
    }
    match value {
        Some(value) => {
            character.insert(CURRENT_ACTIVITY.into(), value);
        }
        None => {
            character.remove(CURRENT_ACTIVITY);
        }
    }
    Ok(true)
}

/// Optional fields shared by account validation and the Sunrise preferences editor.
pub(crate) fn validate_details(character: &Map<String, Value>) -> Result<(), String> {
    for key in ["preview_available", "content_bypass"] {
        if character.get(key).is_some_and(|value| !value.is_boolean()) {
            return Err(format!("{key} must be boolean"));
        }
    }
    if character.get("appearance_value").is_some_and(|value| {
        value
            .as_f64()
            .is_none_or(|number| !(number as f32).is_finite())
    }) {
        return Err("appearance_value must be a finite float".into());
    }
    if character
        .get("last_orbited_destination")
        .is_some_and(|value| {
            parse_unsigned_value(value).is_none_or(|hash| hash > u64::from(u32::MAX))
        })
    {
        return Err(
            "last_orbited_destination must be a 32-bit integer or hexadecimal string".into(),
        );
    }
    Ok(())
}

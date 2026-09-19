//! Validation of per-character Sunrise preferences.

use crate::hash::parse_unsigned_value;
use serde_json::{Map, Value};

/// Optional fields shared by account validation and the Sunrise preferences editor.
pub(crate) fn validate(character: &Map<String, Value>) -> Result<(), String> {
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

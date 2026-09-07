//! JSON-format normalization applied before the account document reaches guided editors.

use serde_json::Value;

use crate::game_settings::MAX_SUPPORTED_SCHEMA;

const VERTICAL_SYNC_INTERVAL_KEY: &str = "vertical_sync_interval";
const FIELD_OF_VIEW_KEY: &str = "field_of_view";
const KEY_BINDING_SOURCE_KEY: &str = "key_binding_source";

/// Materializes preferences introduced with JSON schema 8 at their effective defaults for every
/// supported schema that retains that layout.
pub(crate) fn ensure_schema_v8_preferences(document: &mut Value) -> bool {
    let Some(version) = document.get("version").and_then(Value::as_u64) else {
        return false;
    };
    if !(8..=MAX_SUPPORTED_SCHEMA).contains(&version) {
        return false;
    }

    let Some(settings) = document
        .pointer_mut("/state/account/settings")
        .and_then(Value::as_object_mut)
    else {
        return false;
    };

    let mut changed = false;
    if let Some(display) = settings.get_mut("display").and_then(Value::as_object_mut) {
        if !display.contains_key(VERTICAL_SYNC_INTERVAL_KEY) {
            display.insert(VERTICAL_SYNC_INTERVAL_KEY.into(), Value::from(0));
            changed = true;
        }
        if !display.contains_key(FIELD_OF_VIEW_KEY) {
            display.insert(FIELD_OF_VIEW_KEY.into(), Value::from(85));
            changed = true;
        }
    }
    if !settings.contains_key(KEY_BINDING_SOURCE_KEY) {
        settings.insert(
            KEY_BINDING_SOURCE_KEY.into(),
            Value::String("computer".into()),
        );
        changed = true;
    }
    changed
}

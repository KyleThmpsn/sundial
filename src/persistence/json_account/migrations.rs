//! JSON-format normalization applied before the account document reaches guided editors.

use serde_json::Value;

const VERTICAL_SYNC_INTERVAL_KEY: &str = "vertical_sync_interval";
const FIELD_OF_VIEW_KEY: &str = "field_of_view";
const KEY_BINDING_SOURCE_KEY: &str = "key_binding_source";

/// Materializes preferences introduced with JSON schema 8 at their effective defaults.
pub(crate) fn ensure_schema_v8_preferences(document: &mut Value) -> bool {
    if document.get("version").and_then(Value::as_u64) != Some(8) {
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

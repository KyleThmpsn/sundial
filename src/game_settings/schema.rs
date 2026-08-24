//! Supported schema policy and schema-gated preference defaults.

use serde_json::{Map, Value};

pub(crate) const MIN_SUPPORTED_SCHEMA: u64 = 2;
pub(crate) const MAX_SUPPORTED_SCHEMA: u64 = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct SettingsSchema(pub(super) u64);

impl SettingsSchema {
    pub(super) fn from_document(document: &Value) -> Result<Self, String> {
        match schema_version(document) {
            Some(version) if (MIN_SUPPORTED_SCHEMA..=MAX_SUPPORTED_SCHEMA).contains(&version) => {
                Ok(Self(version))
            }
            Some(version) => Err(format!(
                "Project Sunrise settings schema version {version} has not been tested with this Sundial release"
            )),
            None => Err("Project Sunrise settings schema version is missing or invalid".into()),
        }
    }

    pub(super) fn for_editing(document: &Value) -> Option<Self> {
        schema_version(document)
            .filter(|version| *version >= MIN_SUPPORTED_SCHEMA)
            .map(Self)
    }

    pub(super) const fn key_binding_format(self) -> KeyBindingFormat {
        if self.0 == 2 {
            KeyBindingFormat::Numeric
        } else {
            KeyBindingFormat::Named
        }
    }

    pub(super) const fn named_key_bindings_editable(self) -> bool {
        matches!(self.key_binding_format(), KeyBindingFormat::Named)
    }
}

pub(crate) fn schema_version(document: &Value) -> Option<u64> {
    document.get("version").and_then(Value::as_u64)
}

pub(crate) fn future_schema_version(document: &Value) -> Option<u64> {
    schema_version(document).filter(|version| *version > MAX_SUPPORTED_SCHEMA)
}

pub(super) fn key_bindings_editable(document: &Value) -> bool {
    SettingsSchema::for_editing(document).is_some_and(SettingsSchema::named_key_bindings_editable)
}

pub(super) const VERTICAL_SYNC_INTERVAL_KEY: &str = "vertical_sync_interval";
pub(super) const FIELD_OF_VIEW_KEY: &str = "field_of_view";
pub(super) const KEY_BINDING_SOURCE_KEY: &str = "key_binding_source";
pub(super) const ORBIT_SLICE_SET_PATH: &str = "/client/orbit_slice_set";

pub(crate) fn ensure_schema_v8_preferences(document: &mut Value) -> bool {
    if schema_version(document) != Some(8) {
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

// Older schemas only expose these preferences when they already contain them. Schema v8 files are
// populated with their effective defaults before reaching the editor.
pub(super) fn show_presence_gated_preference(values: &Map<String, Value>, key: &str) -> bool {
    values.contains_key(key)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum KeyBindingFormat {
    Numeric,
    Named,
}

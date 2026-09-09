//! Project Sunrise JSON account-state adapter.

mod account_settings;
mod character;
pub(crate) mod character_runtime;
mod error;
mod migrations;
mod profile;

use std::num::NonZeroU64;

use serde_json::Value;
use sundial_account::EntityId;

pub(crate) use account_settings::JsonAccountSettingsAdapter;
pub(crate) use character::JsonCharacterAdapter;
pub(crate) use error::JsonAccountError;
pub(crate) use migrations::ensure_schema_v8_preferences;
pub(crate) use profile::JsonProfileAdapter;

pub(crate) type JsonProfileError = JsonAccountError;

fn take_entity_id(next_id: &mut u64) -> Result<EntityId, JsonAccountError> {
    let id = NonZeroU64::new(*next_id).ok_or(JsonAccountError::EntityIdentityExhausted)?;
    *next_id = next_id
        .checked_add(1)
        .ok_or(JsonAccountError::EntityIdentityExhausted)?;
    Ok(EntityId::new(id))
}

fn schema_version(document: &Value) -> Result<u64, JsonAccountError> {
    document
        .get("version")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            JsonAccountError::format("/version", "settings schema version is missing or invalid")
        })
}

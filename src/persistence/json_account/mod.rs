//! Project Sunrise JSON account-state adapter.

mod account_settings;
mod character;
mod error;
mod migrations;
mod profile;

pub(crate) use account_settings::JsonAccountSettingsAdapter;
pub(crate) use character::JsonCharacterAdapter;
pub(crate) use error::JsonAccountError;
pub(crate) use migrations::ensure_schema_v8_preferences;
pub(crate) use profile::JsonProfileAdapter;

pub(crate) type JsonProfileError = JsonAccountError;

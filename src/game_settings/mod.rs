//! Project Sunrise game-settings editing and validation.
//!
//! The facade preserves the application-facing API while the implementation is organized by
//! schema, page, preference, input, widget, and validation responsibilities.

pub(crate) mod dawn;
mod key_bindings;
mod page;
mod preferences;
pub(crate) mod runtime;
mod schema;
mod validation;
mod widgets;

pub(super) use key_bindings::KeyBindingUiState;
pub(crate) use key_bindings::{named_input_code, native_input_name};
pub(super) use page::{PageContext, Tab, draw_page};
pub(crate) use schema::requires_sqlite_account;
pub(crate) use schema::{MAX_SUPPORTED_SCHEMA, MIN_SUPPORTED_SCHEMA};
pub(super) use schema::{future_schema_version, key_bindings_editable, schema_version};
pub(super) use validation::{validate, validate_non_account};

#[cfg(test)]
mod tests;

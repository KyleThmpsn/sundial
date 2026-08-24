//! Project Sunrise game-settings editing and validation.
//!
//! The facade preserves the application-facing API while the implementation is organized by
//! schema, page, preference, input, widget, and validation responsibilities.

mod key_bindings;
mod page;
mod preferences;
mod schema;
mod validation;
mod widgets;

pub(super) use key_bindings::KeyBindingUiState;
pub(super) use page::{PlayerTools, Tab, draw_page};
pub(crate) use schema::{MAX_SUPPORTED_SCHEMA, MIN_SUPPORTED_SCHEMA, ensure_schema_v8_preferences};
pub(super) use schema::{future_schema_version, schema_version};
pub(super) use validation::validate;

#[cfg(test)]
mod tests;

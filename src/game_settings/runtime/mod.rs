//! Current Sunrise configuration. Omitted fields and unknown members remain untouched.

mod activity;
mod activity_page;
mod character_page;
mod entitlements;
pub(crate) use entitlements::validate_native as validate_native_entitlements;
pub(crate) fn validate_native_details(document: &serde_json::Value) -> Result<(), String> {
    validate_native_entitlements(document)?;
    character_page::validate(document)?;
    if !document
        .pointer("/state/account/profile_setup_completed")
        .is_some_and(serde_json::Value::is_boolean)
    {
        return Err("Profile Setup Completed must be boolean".into());
    }
    Ok(())
}
mod fields;
mod page;
mod services;
#[cfg(test)]
mod tests;

use crate::persistence::json_fields::{optional_value, write_value};
pub(crate) use activity::validate as validate_activity;
use fields::FIELDS;
pub(super) use page::draw;
use serde_json::Value;

pub(crate) fn available(document: &Value) -> bool {
    document
        .get("version")
        .and_then(Value::as_u64)
        .is_some_and(|version| version >= 16)
}

pub(crate) fn validate(document: &Value, json_account: bool) -> Result<(), String> {
    if !available(document) {
        return Ok(());
    }
    for field in FIELDS
        .iter()
        .chain(services::FIELDS)
        .copied()
        .map(|field| field.for_document(document))
        .filter(|field| json_account || !field.account_owned())
    {
        if let Some(value) = optional_value(document, field.path)? {
            field.validate(value)?;
        }
    }
    services::validate(document, json_account)?;
    validate_activity(document)?;
    Ok(())
}

fn set_field(
    document: &mut Value,
    path: &str,
    value: Value,
    account_available: bool,
) -> Result<bool, String> {
    if !available(document) {
        return Err("These settings require schema 16 or newer".into());
    }
    let field = FIELDS
        .iter()
        .chain(services::FIELDS)
        .copied()
        .map(|field| field.for_document(document))
        .find(|field| field.path == path)
        .ok_or("Unknown runtime setting")?;
    if field.account_owned() && !account_available {
        return Err("The active account is unavailable".into());
    }
    field.validate(&value)?;
    if optional_value(document, path)? == Some(&value) {
        return Ok(false);
    }
    let mut candidate = document.clone();
    write_value(&mut candidate, path, value)?;
    *document = candidate;
    Ok(true)
}

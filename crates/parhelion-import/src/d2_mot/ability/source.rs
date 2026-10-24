//! Checked identities in retained modern gameplay traces.

use anyhow::{Context, Result, ensure};
use serde_json::Value;

pub(crate) fn number(value: &Value, label: &str) -> Result<u32> {
    u32::try_from(value.as_u64().with_context(|| format!("Missing {label}"))?)
        .with_context(|| format!("Invalid {label}"))
}

pub(crate) fn tag(value: &str) -> Result<u32> {
    ensure!(
        value.len() == 8 && value.bytes().all(|c| c.is_ascii_hexdigit()),
        "Invalid source tag or class {value}"
    );
    let value = u32::from_str_radix(value, 16)?;
    ensure!(
        value != 0 && value != u32::MAX,
        "Source tag or class is a sentinel"
    );
    Ok(value)
}

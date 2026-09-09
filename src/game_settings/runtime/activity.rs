//! Activity fallback and arrival-row validation, matching Sunrise's current parser.

use super::optional_value;
use crate::hash::parse_unsigned_value;
use serde_json::Value;

pub(super) const DESTINATION: &str = "/state/activity/default_destination";
pub(super) const ARRIVALS: &str = "/state/activity/arrival_overrides";
pub(super) const ARRIVAL_CAPACITY: usize = 64;

pub(crate) fn validate(document: &Value) -> Result<(), String> {
    if let Some(value) = optional_value(document, DESTINATION)? {
        validate_destination(value)?;
    }
    if let Some(value) = optional_value(document, ARRIVALS)? {
        let rows = value
            .as_array()
            .ok_or("Activity arrival overrides must be an array")?;
        if rows.len() > ARRIVAL_CAPACITY {
            return Err("At most 64 activity arrival overrides are supported".into());
        }
        for (index, row) in rows.iter().enumerate() {
            validate_arrival(row)
                .map_err(|error| format!("Arrival override {}: {error}", index + 1))?;
        }
    }
    Ok(())
}

fn name(row: &Value) -> Result<(), String> {
    row.get("package_name")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty() && name.len() <= 40)
        .map(|_| ())
        .ok_or_else(|| "Package name must contain 1â€“40 bytes".into())
}

fn integer(row: &Value, key: &str, min: i64, max: i64) -> Result<i64, String> {
    row.get(key)
        .and_then(Value::as_i64)
        .filter(|n| (min..=max).contains(n))
        .ok_or_else(|| format!("{key} must be an integer from {min} to {max}"))
}

fn hash(row: &Value, key: &str, max: u64) -> Result<u64, String> {
    row.get(key)
        .and_then(parse_unsigned_value)
        .filter(|n| *n <= max)
        .ok_or_else(|| format!("{key} must be an unsigned value no greater than {max}"))
}

pub(super) fn validate_destination(row: &Value) -> Result<(), String> {
    name(row)?;
    integer(row, "reason", -1, 14)?;
    integer(row, "source_activity_index", -1, 4094)?;
    integer(row, "activity_index", 0, 4094)?;
    let count = integer(row, "bubble_count", 1, 64)? as u32;
    let initial = integer(row, "initial_slice_set", 0, 511)? as u32;
    let mask = hash(row, "stateful_bubble_mask", u64::MAX)?;
    hash(row, "spawn_set_hash", u64::from(u32::MAX))?;
    let allowed = if count == 64 {
        u64::MAX
    } else {
        (1_u64 << count) - 1
    };
    if mask & !allowed != 0 {
        return Err("Stateful bubble mask contains bits beyond the bubble count".into());
    }
    let initial_bubble = initial / 8;
    if initial_bubble >= count || mask & (1_u64 << initial_bubble) == 0 {
        return Err(
            "Initial slice set must belong to a stateful bubble within the bubble count".into(),
        );
    }
    Ok(())
}

pub(super) fn validate_arrival(row: &Value) -> Result<(), String> {
    name(row)?;
    if row.get("bubble").is_some() {
        integer(row, "bubble", 0, 63)?;
    }
    if row.get("slice_set").is_some() {
        integer(row, "slice_set", 0, 511)?;
    }
    if row.get("spawn_set_hash").is_some() {
        hash(row, "spawn_set_hash", u64::from(u32::MAX))?;
    }
    let from_launch = match row.get("current_activity_from_launch") {
        None => false,
        Some(value) => value
            .as_bool()
            .ok_or("current_activity_from_launch must be true or false")?,
    };
    if !["bubble", "slice_set", "spawn_set_hash"]
        .iter()
        .any(|key| row.get(*key).is_some())
        && !from_launch
    {
        return Err("Choose at least one arrival override".into());
    }
    Ok(())
}

pub(super) fn set_destination(document: &mut Value, value: Value) -> Result<bool, String> {
    if !super::available(document) {
        return Err("Activity editing requires schema 16 or newer".into());
    }
    validate_destination(&value)?;
    if optional_value(document, DESTINATION)? == Some(&value) {
        return Ok(false);
    }
    super::write_value(document, DESTINATION, value)?;
    Ok(true)
}

pub(super) fn set_arrival(
    document: &mut Value,
    index: usize,
    row: Option<Value>,
) -> Result<bool, String> {
    if !super::available(document) {
        return Err("Activity editing requires schema 16 or newer".into());
    }
    let mut rows = match optional_value(document, ARRIVALS)? {
        None => Vec::new(),
        Some(value) => value
            .as_array()
            .ok_or("Activity arrival overrides must be an array")?
            .clone(),
    };
    if index > rows.len() || (index == rows.len() && row.is_none()) {
        return Err("Arrival override no longer exists".into());
    }
    match row {
        Some(row) => {
            validate_arrival(&row)?;
            if index == rows.len() {
                if rows.len() >= ARRIVAL_CAPACITY {
                    return Err("Arrival override table is full".into());
                }
                rows.push(row);
            } else {
                if rows[index] == row {
                    return Ok(false);
                }
                rows[index] = row;
            }
        }
        None => {
            rows.remove(index);
        }
    }
    super::write_value(document, ARRIVALS, Value::Array(rows))?;
    Ok(true)
}

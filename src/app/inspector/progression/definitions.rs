//! Display language for package unlock definitions and Family 5 values.

use crate::{catalog::UnlockDefinition, hash::format_hash_hex};

pub(in crate::app) const fn flag_override_state_label(value: u8) -> &'static str {
    match value {
        0 => "0 · clear",
        1 => "1 · logical value 1",
        2 => "2 · set",
        _ => "Invalid",
    }
}

pub(in crate::app) const fn flag_override_state_help() -> &'static str {
    "Sunrise logical unlock-flag value: 0 clear, 1 logical value 1, 2 set."
}

pub(in crate::app) fn definition_hash_hex_text(definition: &UnlockDefinition) -> String {
    format_hash_hex(definition.hash)
}

pub(in crate::app) fn definition_name(definition: &UnlockDefinition) -> Option<&str> {
    definition
        .name
        .as_deref()
        .filter(|name| !name.trim().is_empty())
}

pub(in crate::app) fn definition_identity(index: usize, definition: &UnlockDefinition) -> String {
    format!("#{index}: {}", definition_hash_hex_text(definition))
}

pub(super) fn definition_name_tooltip(definition: &UnlockDefinition) -> Option<String> {
    let mut lines = Vec::new();
    if let Some(name) = definition_name(definition) {
        lines.push(format!("Name: {name}"));
    }
    if let Some(description) = definition
        .description
        .as_deref()
        .filter(|description| !description.trim().is_empty())
    {
        lines.push(format!("Description: {description}"));
    }
    (!lines.is_empty()).then(|| lines.join("\n"))
}

pub(in crate::app) fn definition_metadata_tooltip(definition: &UnlockDefinition) -> String {
    let mut lines: Vec<String> = definition_name_tooltip(definition)
        .map(|tooltip| tooltip.lines().map(str::to_owned).collect())
        .unwrap_or_default();
    lines.push(format!("Code: 0x{:04X}", definition.code));
    if let Some(slot) = definition.compact_slot {
        lines.push(format!("Compact slot: {slot}"));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::flag_override_state_label;

    #[test]
    fn family5_flag_states_use_sunrise_logical_value_terms() {
        assert_eq!(flag_override_state_label(0), "0 · clear");
        assert_eq!(flag_override_state_label(1), "1 · logical value 1");
        assert_eq!(flag_override_state_label(2), "2 · set");
    }
}

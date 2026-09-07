//! Sunrise-compatible formatting and schema-specific save-size limits.
use crate::game_settings;
use serde_json::Value;

pub(in crate::app) struct PreparedSettings {
    pub(super) encoded: String,
    pub(in crate::app) encoded_bytes: usize,
    pub(in crate::app) size_limit_bytes: usize,
    pub(in crate::app) compacted: bool,
}

#[derive(Clone, Copy)]
struct SettingsSizeLimitTier {
    through_schema: u64,
    bytes: usize,
}

// This records the lowest cap known to have shipped with each schema. Schema 5 briefly shipped
// with 128 KiB before Sunrise raised the cap without changing the schema, so keeping it in the
// 128 KiB tier preserves compatibility with every v5 build. Unknown future schemas inherit the
// latest known cap until a new boundary is added here.
const SETTINGS_SIZE_LIMITS: &[SettingsSizeLimitTier] = &[
    SettingsSizeLimitTier {
        through_schema: 3,
        bytes: 64 * 1024,
    },
    SettingsSizeLimitTier {
        through_schema: 5,
        bytes: 128 * 1024,
    },
    SettingsSizeLimitTier {
        through_schema: u64::MAX,
        bytes: 1024 * 1024,
    },
];

pub(in crate::app) fn settings_size_limit_for_schema(schema: Option<u64>) -> usize {
    let schema = schema.unwrap_or_default();
    SETTINGS_SIZE_LIMITS
        .iter()
        .find(|tier| schema <= tier.through_schema)
        .expect("the final settings-size tier must cover every schema")
        .bytes
}

pub(in crate::app) fn prepare_settings(document: &Value) -> Result<PreparedSettings, String> {
    let size_limit_bytes = settings_size_limit_for_schema(game_settings::schema_version(document));
    let mut encoded = encode_settings(document)?;
    encoded.push_str("\r\n");
    let compacted = encoded.len() > size_limit_bytes;
    if compacted {
        encoded = serde_json::to_string(document)
            .map_err(|e| format!("Could not compact settings JSON: {e}"))?;
        encoded.push_str("\r\n");
    }
    let encoded_bytes = encoded.len();
    if encoded_bytes > size_limit_bytes {
        return Err(format!(
            "Settings JSON is {encoded_bytes} bytes after compaction, above the supported {size_limit_bytes}-byte Sunrise limit"
        ));
    }
    Ok(PreparedSettings {
        encoded,
        encoded_bytes,
        size_limit_bytes,
        compacted,
    })
}

pub(in crate::app) fn encode_settings(document: &Value) -> Result<String, String> {
    const MAX_INLINE_WIDTH: usize = 80;

    fn contains_object(value: &Value) -> bool {
        match value {
            Value::Object(_) => true,
            Value::Array(array) => array.iter().any(contains_object),
            _ => false,
        }
    }

    fn current_column(output: &str) -> usize {
        output
            .rsplit_once('\n')
            .map_or(output.len(), |(_, line)| line.len())
    }

    fn is_dense_table(path: &[String]) -> bool {
        matches!(path, [state, section, _] if state == "state" && (section == "investment" || section == "unlocks"))
    }

    fn is_entitlements(path: &[String]) -> bool {
        matches!(path, [server, entitlements] if server == "server" && entitlements == "entitlements")
    }

    fn is_key_binding(path: &[String]) -> bool {
        matches!(path, [state, account, settings, bindings, _]
            if state == "state"
                && account == "account"
                && settings == "settings"
                && bindings == "key_bindings")
    }

    fn is_profile_items(path: &[String]) -> bool {
        matches!(path, [state, account, items]
            if state == "state" && account == "account" && items == "profile_items")
    }

    fn write_inline(value: &Value, spaces: bool, output: &mut String) -> Result<(), String> {
        let separator = if spaces { ", " } else { "," };
        match value {
            Value::Object(object) => {
                output.push('{');
                if spaces && !object.is_empty() {
                    output.push(' ');
                }
                for (index, (key, child)) in object.iter().enumerate() {
                    if index != 0 {
                        output.push_str(separator);
                    }
                    output.push_str(
                        &serde_json::to_string(key)
                            .map_err(|e| format!("Could not encode setting name: {e}"))?,
                    );
                    output.push_str(if spaces { ": " } else { ":" });
                    write_inline(child, spaces, output)?;
                }
                if spaces && !object.is_empty() {
                    output.push(' ');
                }
                output.push('}');
            }
            Value::Array(array) => {
                output.push('[');
                for (index, child) in array.iter().enumerate() {
                    if index != 0 {
                        output.push_str(separator);
                    }
                    write_inline(child, spaces, output)?;
                }
                output.push(']');
            }
            _ => output.push_str(
                &serde_json::to_string(value)
                    .map_err(|e| format!("Could not encode setting: {e}"))?,
            ),
        }
        Ok(())
    }

    fn write_dense_table(value: &Value, output: &mut String) -> Result<(), String> {
        let Value::Array(array) = value else {
            return write_inline(value, false, output);
        };
        output.push('[');
        for (index, child) in array.iter().enumerate() {
            if index != 0 {
                // Sunrise separates rows in its dense pair tables, while keeping each row compact.
                output.push_str(if child.is_array() { ", " } else { "," });
            }
            write_inline(child, false, output)?;
        }
        output.push(']');
        Ok(())
    }

    fn write_profile_items(
        array: &[Value],
        indent: usize,
        output: &mut String,
    ) -> Result<(), String> {
        output.push_str("[\n");
        for (index, child) in array.iter().enumerate() {
            let object = child
                .as_object()
                .ok_or("Sunrise profile_items entries must be objects")?;
            output.push_str(&" ".repeat(indent));
            output.push_str("{\n");
            for (field_index, (key, value)) in object.iter().enumerate() {
                output.push_str(&" ".repeat(indent));
                output.push_str(
                    &serde_json::to_string(key)
                        .map_err(|e| format!("Could not encode setting name: {e}"))?,
                );
                output.push_str(": ");
                write_inline(value, true, output)?;
                if field_index + 1 != object.len() {
                    output.push(',');
                }
                output.push('\n');
            }
            output.push_str(&" ".repeat(indent));
            output.push('}');
            if index + 1 != array.len() {
                output.push(',');
            }
            output.push('\n');
        }
        output.push_str(&" ".repeat(indent));
        output.push(']');
        Ok(())
    }

    fn write_value(
        value: &Value,
        indent: usize,
        path: &mut Vec<String>,
        legacy_profile_items: bool,
        output: &mut String,
    ) -> Result<(), String> {
        match value {
            Value::Object(_) if is_key_binding(path) => write_inline(value, true, output)?,
            Value::Object(object) if !object.is_empty() => {
                output.push_str("{\n");
                for (index, (key, child)) in object.iter().enumerate() {
                    output.push_str(&" ".repeat(indent + 2));
                    output.push_str(
                        &serde_json::to_string(key)
                            .map_err(|e| format!("Could not encode setting name: {e}"))?,
                    );
                    output.push_str(": ");
                    path.push(key.clone());
                    write_value(child, indent + 2, path, legacy_profile_items, output)?;
                    path.pop();
                    if index + 1 != object.len() {
                        output.push(',');
                    }
                    output.push('\n');
                }
                output.push_str(&" ".repeat(indent));
                output.push('}');
            }
            Value::Array(_) if is_dense_table(path) => write_dense_table(value, output)?,
            Value::Array(array)
                if legacy_profile_items
                    && is_profile_items(path)
                    && !array.is_empty()
                    && array.iter().all(Value::is_object) =>
            {
                write_profile_items(array, indent, output)?;
            }
            Value::Array(array) if array.iter().any(contains_object) => {
                output.push_str("[\n");
                for (index, child) in array.iter().enumerate() {
                    output.push_str(&" ".repeat(indent + 2));
                    if is_entitlements(path) {
                        write_inline(child, true, output)?;
                    } else {
                        write_value(child, indent + 2, path, legacy_profile_items, output)?;
                    }
                    if index + 1 != array.len() {
                        output.push(',');
                    }
                    output.push('\n');
                }
                output.push_str(&" ".repeat(indent));
                output.push(']');
            }
            Value::Array(array) => {
                let mut inline = String::new();
                write_inline(value, true, &mut inline)?;
                if array.is_empty() || current_column(output) + inline.len() <= MAX_INLINE_WIDTH {
                    output.push_str(&inline);
                } else {
                    output.push_str("[\n");
                    for (index, child) in array.iter().enumerate() {
                        output.push_str(&" ".repeat(indent + 2));
                        write_value(child, indent + 2, path, legacy_profile_items, output)?;
                        if index + 1 != array.len() {
                            output.push(',');
                        }
                        output.push('\n');
                    }
                    output.push_str(&" ".repeat(indent));
                    output.push(']');
                }
            }
            _ => output.push_str(
                &serde_json::to_string(value)
                    .map_err(|e| format!("Could not encode setting: {e}"))?,
            ),
        }
        Ok(())
    }

    let mut output = String::new();
    let legacy_profile_items =
        matches!(game_settings::schema_version(document), None | Some(0..=3));
    write_value(
        document,
        0,
        &mut Vec::new(),
        legacy_profile_items,
        &mut output,
    )?;
    Ok(output.replace('\n', "\r\n"))
}

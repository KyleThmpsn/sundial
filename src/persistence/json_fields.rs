//! Lossless access to optional fields with object-only parent paths.

use serde_json::{Map, Value};

/// Distinguishes an omitted field from a malformed parent without replacing either.
pub(crate) fn optional_value<'a>(
    document: &'a Value,
    path: &str,
) -> Result<Option<&'a Value>, String> {
    let mut value = document;
    for key in path.trim_start_matches('/').split('/') {
        let object = value
            .as_object()
            .ok_or_else(|| format!("{path} has a non-object parent"))?;
        let Some(child) = object.get(key) else {
            return Ok(None);
        };
        value = child;
    }
    Ok(Some(value))
}

/// Writes a known path into a candidate, creating only absent object parents.
pub(crate) fn write_value(document: &mut Value, path: &str, value: Value) -> Result<(), String> {
    optional_value(document, path)?;
    let mut keys = path.trim_start_matches('/').split('/').peekable();
    let mut target = document;
    while let Some(key) = keys.next() {
        let object = target
            .as_object_mut()
            .ok_or_else(|| format!("{path} has a non-object parent"))?;
        if keys.peek().is_none() {
            object.insert(key.to_owned(), value);
            return Ok(());
        }
        target = object
            .entry(key)
            .or_insert_with(|| Value::Object(Map::new()));
    }
    Err("Cannot replace the document root".into())
}

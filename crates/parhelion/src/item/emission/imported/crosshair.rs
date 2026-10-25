//! An imported weapon's own hip-fire crosshair.
//!
//! The import records the source item's client tuple and, for a type key Shadowkeep lacks, rows
//! naming a converted crosshair scene among the graph's private assets. The rows join the native
//! crosshair table unless an earlier weapon already added the same bucket and key. The item's
//! strings record takes the source's type keys only when the table holds both for its bucket, so
//! a key with no row never reaches the client.
use super::*;
use crate::weapon::crosshair::{Row, SUB_ROW_SIZE, TABLE, Table};

fn word(record: &Value, name: &str) -> AuthoringResult<u32> {
    record[name]
        .as_str()
        .and_then(|text| u32::from_str_radix(text, 16).ok())
        .ok_or_else(|| invalid(format!("Imported crosshair {name} is missing")))
}

fn sub_row(text: &str) -> AuthoringResult<[u8; SUB_ROW_SIZE]> {
    if text.len() != SUB_ROW_SIZE * 2 {
        return Err(invalid("Imported crosshair style row has the wrong size"));
    }
    let mut row = [0; SUB_ROW_SIZE];
    for (i, byte) in row.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16)
            .map_err(|_| invalid("Imported crosshair style row is not hexadecimal"))?;
    }
    Ok(row)
}

pub(super) fn apply(
    manager: &sundial::package_authoring::PackageManager,
    emission: &mut PackageEmission,
    symbols: &BTreeMap<String, TagHash>,
    graph: &Value,
    previous: &BTreeMap<u32, Vec<u8>>,
    item: u32,
) -> AuthoringResult<Option<ReplacementSpec>> {
    let record = &graph["crosshair"];
    if !matches!(record["status"].as_str(), Some("native" | "converted")) {
        return Ok(None);
    }
    let original = match previous.get(&TABLE.0) {
        Some(data) => data.clone(),
        None => manager
            .read_tag(TABLE)
            .map_err(|error| invalid(error.to_string()))?,
    };
    let mut table = Table::parse(&original)?;
    let mut added = false;
    for row in record["rows"].as_array().into_iter().flatten() {
        let pair = row["pair"]
            .as_str()
            .and_then(|symbol| symbols.get(symbol))
            .ok_or_else(|| invalid("Imported crosshair pair was not allocated"))?;
        let subs = row["subs"]
            .as_array()
            .ok_or_else(|| invalid("Imported crosshair style rows are missing"))?
            .iter()
            .map(|text| sub_row(text.as_str().unwrap_or_default()))
            .collect::<AuthoringResult<Vec<_>>>()?;
        added |= table.insert(Row {
            hash: word(row, "hash")?,
            bucket: word(row, "bucket")?,
            key: word(row, "key")?,
            pair: pair.0,
            subs,
        })?;
    }
    let (first, second) = (word(record, "first")?, word(record, "second")?);
    let ordinal = strings_ordinal(emission, item)?;
    let strings = &mut emission
        .host_new_tags
        .get_mut(ordinal)
        .ok_or_else(|| invalid("Authored item strings missing"))?
        .payload;
    let (bucket, ..) = item_string_type_keys(strings)?;
    if table.has(bucket, first) && table.has(bucket, second) {
        set_item_string_type_keys(strings, first, second)?;
    } else {
        eprintln!(
            "Imported crosshair {first:08X}/{second:08X} has no row for bucket {bucket:08X}; the base crosshair stays"
        );
    }
    if !added {
        return Ok(None);
    }
    Ok(Some(ReplacementSpec {
        tag: TABLE,
        payload: table.serialize()?,
    }))
}

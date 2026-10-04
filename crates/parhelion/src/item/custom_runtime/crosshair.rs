//! The crosshair keys an imported weapon's runtime content takes from its converted crosshair.
use super::*;
use parhelion_import::GraphReference;

/// The type key and style key of the converted crosshair row for the item's own bucket. Only a
/// converted crosshair has rows the build adds. A source key Shadowkeep already has keeps the
/// base weapon's runtime keys.
pub(in crate::item) fn load(graph: &GraphReference) -> AuthoringResult<Option<(u32, u32)>> {
    let value: serde_json::Value = serde_json::from_slice(
        &std::fs::read(graph.directory.join("asset-graph.json"))
            .map_err(|error| invalid(format!("Imported crosshair graph: {error}")))?,
    )
    .map_err(|error| invalid(format!("Imported crosshair graph: {error}")))?;
    let record = &value["crosshair"];
    if record["status"] != "converted" {
        return Ok(None);
    }
    let hex = |field: &serde_json::Value| {
        field
            .as_str()
            .and_then(|text| u32::from_str_radix(text, 16).ok())
            .ok_or_else(|| invalid("Imported crosshair key is not hexadecimal"))
    };
    let bucket = hex(&record["bucket"])?;
    let keys = [hex(&record["first"])?, hex(&record["second"])?];
    let rows = record["rows"]
        .as_array()
        .ok_or_else(|| invalid("Imported crosshair rows are missing"))?;
    let mut selected = None;
    for row in rows {
        let key = hex(&row["key"])?;
        if hex(&row["bucket"])? != bucket || !keys.contains(&key) {
            continue;
        }
        if selected.is_some() {
            return Err(invalid(
                "Imported crosshair has more than one row for the item's bucket",
            ));
        }
        // A style sub-row starts with its style key, stored little-endian.
        let text = row["subs"]
            .as_array()
            .and_then(|subs| subs.first())
            .and_then(|sub| sub.as_str())
            .and_then(|sub| sub.get(..8))
            .ok_or_else(|| invalid("Imported crosshair row has no style row"))?;
        let mut bytes = [0; 4];
        for (i, byte) in bytes.iter_mut().enumerate() {
            *byte = text
                .get(i * 2..i * 2 + 2)
                .and_then(|pair| u8::from_str_radix(pair, 16).ok())
                .ok_or_else(|| invalid("Imported crosshair style row is not hexadecimal"))?;
        }
        let style = u32::from_le_bytes(bytes);
        selected = Some((key, style));
    }
    Ok(selected)
}

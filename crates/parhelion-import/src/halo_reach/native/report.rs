//! Keep inherited runtime behavior visible beside the translated source data.
use serde_json::{Value, json};

// Material properties are source evidence, not an executable child of a debris
// presentation. Store them once at the graph root to keep portable JSON shallow.
fn material_properties(value: &mut Value, properties: &mut Vec<Value>) {
    match value {
        Value::Object(object) => {
            if let Some(source) = object.remove("source_properties") {
                let index = properties
                    .iter()
                    .position(|entry| *entry == source)
                    .unwrap_or_else(|| {
                        properties.push(source);
                        properties.len() - 1
                    });
                object.insert("source_properties_index".into(), json!(index));
            }
            for child in object.values_mut() {
                material_properties(child, properties);
            }
        }
        Value::Array(rows) => {
            for row in rows {
                material_properties(row, properties);
            }
        }
        _ => {}
    }
}

pub(super) fn compact(path: &std::path::Path) -> anyhow::Result<()> {
    let graph: Value = serde_json::from_slice(&std::fs::read(path)?)?;
    std::fs::write(path, serde_json::to_vec(&graph)?)?;
    Ok(())
}

pub(super) fn behavior(graph: &mut Value) {
    let mut properties = Vec::new();
    material_properties(graph, &mut properties);
    if !properties.is_empty() {
        graph["source_material_properties"] = json!(properties);
    }
    let mut retained = serde_json::Map::new();
    let character = graph["animation"].get("character").is_some();
    if let Some(vehicle) = graph.get("vehicle") {
        let mut rows = vec![
            "Native interaction and mount state selection",
            if character {
                "Native rider action selection and arm and leg constraints"
            } else {
                "Native rider animation and arm and leg constraints"
            },
            "Native vehicle weapon timing, projectile damage and impact effects",
        ];
        if vehicle.get("motion").is_none() {
            rows.push("Native hover motion and handling");
        } else {
            rows.push("Native physical integration and contact response with translated source parameters");
        }
        retained.insert("vehicle".into(), json!(rows));
    } else {
        retained.insert("weapon".into(), json!([
            "Native weapon controller, action timing and socket behavior with the recipe's explicit overrides",
            if character { "Native character action selection and untranslated actions" }
            else { "Native third-person character animation and action selection" },
        ]));
    }
    for (name, section) in [
        ("first_person", &graph["animation"]["first_person"]),
        ("character", &graph["animation"]["character"]),
        ("projectile", &graph["projectile"]["conversion"]),
        ("shards", &graph["projectile"]["conversion"]["shards"]),
        ("melee", &graph["melee"]),
        ("handling_audio", &graph["handling_audio"]),
    ] {
        if let Some(rows) = section.get("retained_native_behavior") {
            retained.insert(name.into(), rows.clone());
        }
    }
    graph["retained_native_behavior"] = Value::Object(retained);
    graph["limits"] = json!([
        "Source behavior is translated only where the corresponding report section records its implementation. Retained native behavior is listed separately.",
        "Package and native-code readback do not establish live rendering, animation transitions, collision or gameplay acceptance.",
    ]);
}

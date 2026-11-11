//! Source weapon attachments and object functions beside what the native weapon carries for them.
//!
//! A Reach object's attachments (`0x20`-byte rows at object `+0x104`) place a light, effect or
//! looping sound at a marker, scaled by the object function or export named at `+0x18` and
//! `+0x1C`. Its functions (`0x40`-byte rows at `+0xEC`) map an input to a named output. The
//! firearms' attached lights scale by `illumination`, which rises while the weapon fires, so they
//! are firing lights. The native firing flow carries its own firing light events, which the rebuilt
//! flow keeps at the source muzzle. Their light records (`80807140`) compute their color at run
//! time, and the firing flow's private copies store the source light's color over it, as its
//! `firing_lights` receipt records. A weapon's ammunition display is a material animation: its
//! decimal frame functions read the `primary_ammunition_ones` and `primary_ammunition_tens`
//! exports, which the live display channel translates, and no object function takes part.
use crate::halo_reach::Scene;
use serde_json::{Value, json};

/// Whether `value` holds the string `text` anywhere.
fn mentions(value: &Value, text: &str) -> bool {
    match value {
        Value::String(string) => string == text,
        Value::Array(rows) => rows.iter().any(|row| mentions(row, text)),
        Value::Object(object) => object.values().any(|row| mentions(row, text)),
        _ => false,
    }
}

fn status(rows: &[Value]) -> &'static str {
    let count = |name: &str| rows.iter().filter(|row| row["status"] == name).count();
    let omitted = count("omitted");
    if rows.is_empty() {
        "absent"
    } else if omitted == rows.len() {
        "omitted"
    } else if omitted > 0 {
        "partial"
    } else if count("translated") == rows.len() {
        "translated"
    } else {
        "native"
    }
}

pub(super) fn report(scene: &Scene, graph: &mut Value) {
    let firing = &graph["firing_effect"];
    let kept = matches!(firing["status"].as_str(), Some("translated" | "partial"));
    let muzzle = firing["muzzle"]
        .as_str()
        .unwrap_or("primary_trigger")
        .to_owned();
    let mut attachments = Vec::new();
    for (index, attachment) in scene.object.attachments.iter().enumerate() {
        let Some(group) = attachment["tag"]["group"].as_str() else {
            continue;
        };
        let marker = attachment["marker"].as_str().unwrap_or("");
        let scale = attachment["primary_scale"].as_str().unwrap_or("");
        let path = attachment["tag"]["path"].as_str().unwrap_or("");
        let (status, reason) = match group {
            "ligh"
                if scale == "illumination"
                    && firing["firing_lights"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .any(|row| {
                            row["status"] == "translated"
                                && row["source"]["datum"] == attachment["tag"]["datum"]
                        }) =>
            {
                (
                    "translated",
                    format!(
                        "Private copies of the donor's firing light records store this light's color over the color their expressions derive from the donor's object vector, at the donor's brightness scale, reach and duration{}",
                        if marker == muzzle || marker.is_empty() {
                            String::new()
                        } else {
                            format!(", and the light stands at the muzzle rather than {marker}")
                        }
                    ),
                )
            }
            "ligh" if scale == "illumination" => (
                "native",
                format!(
                    "{} The donor's light records compute their color at run time from their expressions, and no private copy took this light's color, so its color, range and intensity are not converted{}",
                    if kept {
                        "The donor's firing light events stay in the rebuilt firing flow at the source muzzle."
                    } else {
                        "The donor's firing light stays."
                    },
                    if marker == muzzle || marker.is_empty() {
                        String::new()
                    } else {
                        format!(", and the donor light stands at the muzzle rather than {marker}")
                    }
                ),
            ),
            "ligh" => (
                "omitted",
                format!("A persistent attached light needs a native light host on the weapon and its {scale} function"),
            ),
            "effe" => (
                "omitted",
                format!("A persistent attached effect needs a native effect host on the weapon and its {scale} function"),
            ),
            "lsnd" if mentions(&graph["audio"], path) => (
                "translated",
                "The audio translation plays this looping sound as its firing loop".to_owned(),
            ),
            "lsnd" => (
                "omitted",
                "The audio translation converts only the attached loop it selects as the firing loop"
                    .to_owned(),
            ),
            _ => ("omitted", format!("Attachment group {group} has no translation")),
        };
        attachments.push(
            json!({"index":index,"source":attachment["tag"],"marker":marker,
            "primary_scale":scale,"secondary_scale":attachment["secondary_scale"],
            "status":status,"reason":reason}),
        );
    }
    let display = graph["object_inputs"].get("32CC5402").is_some();
    let mut functions = Vec::new();
    for function in &scene.object.functions {
        let output = function["output"].as_str().unwrap_or("");
        let scaled = attachments
            .iter()
            .filter(|row| {
                !output.is_empty()
                    && (row["primary_scale"] == output || row["secondary_scale"] == output)
            })
            .map(|row| row["index"].clone())
            .collect::<Vec<_>>();
        functions.push(json!({"input":function["input"],"output":output,"flags":function["flags"],
            "attachments":scaled,"status":"omitted",
            "reason":if scaled.is_empty() {
                "No translated presentation reads this function"
            } else {
                "Nothing evaluates it, since the attachments it scales keep native or no presentation"
            }}));
    }
    let attachment_status = status(&attachments);
    graph["weapon_attachments"] = json!({"status":attachment_status,"attachments":attachments,
        "reason":match attachment_status {
            "absent" => "The source weapon has no attachments",
            "translated" => "Every source attachment is translated",
            _ => "Firing lights use the donor's firing light events, in the source color where a private copy took it. Persistent lights, effects and other looping sounds stay pending",
        },
        "gameplay_verified":false});
    graph["object_functions"] = json!({"status":if functions.is_empty() { "absent" } else { "omitted" },
    "functions":functions,
    "reason":"Object functions are recorded. No converted presentation evaluates them",
    "ammunition_display":if display {
        json!({"status":"translated","channel":"32CC5402",
            "reason":"The material's decimal frame functions read the primary_ammunition_ones and primary_ammunition_tens exports through the live display channel. No object function drives the display."})
    } else {
        Value::Null
    }});
}

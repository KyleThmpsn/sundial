//! Resolve the shared character palette by its relationship to equipment rigs.
use super::*;

fn hierarchy(a: &[Value], b: &[Value]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(a, b)| a["name_hash"] == b["name_hash"] && a["parent"] == b["parent"])
}

/// A first-person equipment skeleton contains a character prefix followed by the
/// independently declared equipment skeleton. Validate the entire suffix before using
/// that boundary. Neither a particular inventory item nor a fixed bone count is assumed.
fn prefix(rig: &Value) -> Option<Vec<Value>> {
    let skeletons = rig["skeletons"]
        .as_array()?
        .iter()
        .filter(|s| matches!(s["class"].as_str(), Some("808081DE" | "80808546")))
        .collect::<Vec<_>>();
    for weapon in skeletons
        .iter()
        .filter(|s| s["entity"] == rig["runtime_entity"])
    {
        let bones = weapon["bones"].as_array()?;
        if bones.is_empty() {
            continue;
        }
        for attachment in skeletons
            .iter()
            .filter(|s| s["entity"] != rig["runtime_entity"])
        {
            let full = attachment["bones"].as_array()?;
            let Some(start) = full.len().checked_sub(bones.len()).filter(|n| *n > 0) else {
                continue;
            };
            let suffix = &full[start..];
            let matches = bones.iter().zip(suffix).enumerate().all(|(index, (a, b))| {
                a["name_hash"] == b["name_hash"]
                    && if index == 0 {
                        b["parent"]
                            .as_i64()
                            .is_some_and(|parent| parent >= 0 && parent < start as i64)
                    } else {
                        a["parent"].as_i64().is_some_and(|parent| {
                            b["parent"].as_i64() == Some(parent + start as i64)
                        })
                    }
            });
            if matches {
                return Some(full[..start].to_vec());
            }
        }
    }
    None
}

pub(super) fn inspect(r: &mut Reader, modern: bool) -> Result<Value> {
    let patterns = patterns(r, modern)?;
    let entities = entities(r, modern, &patterns.iter().map(|(key, _)| *key).collect())?;
    let mut visited = BTreeSet::new();
    let mut inspected = Vec::new();
    let mut shared = None;
    for &(key, content) in &patterns {
        let Some(targets) = entities.get(&key).filter(|e| e.len() == 1) else {
            continue;
        };
        let entity = *targets.first().unwrap();
        if !visited.insert((entity, content)) {
            continue;
        }
        let Ok(rig) = runtime(r, entity, content, modern) else {
            continue;
        };
        if shared.is_none() {
            shared = prefix(&rig);
        }
        inspected.push(rig);
        let Some(prefix) = &shared else {
            continue;
        };
        // A standalone runtime with exactly this prefix is the character palette.
        // Keep its real FK payload, so conversion and staging audits validate the same rig.
        for rig in &inspected {
            let Some(skeletons) = rig["skeletons"].as_array() else {
                continue;
            };
            for skeleton in skeletons {
                if !matches!(skeleton["class"].as_str(), Some("808081DE" | "80808546")) {
                    continue;
                }
                if skeleton["entity"] != rig["runtime_entity"] {
                    continue;
                }
                let Some(bones) = skeleton["bones"].as_array() else {
                    continue;
                };
                if !hierarchy(prefix, bones) {
                    continue;
                }
                let components = rig["components"]
                    .as_array()
                    .context("character components")?
                    .iter()
                    .filter(|row| row["owner"] == skeleton["owner"])
                    .collect::<Vec<_>>();
                let mut skeleton = skeleton.clone();
                skeleton["shared_character"] = json!(true);
                return Ok(json!({"runtime_entity":rig["runtime_entity"],
                    "components":components,"skeletons":[skeleton],"shared_character":true}));
            }
        }
    }
    anyhow::bail!(
        "No standalone character skeleton matches the validated equipment attachment prefix"
    )
}

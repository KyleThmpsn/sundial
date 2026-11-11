//! Particle event bindings in a converted graph, checked against their systems' transform rows.
//!
//! Native system start (build 86657, `exe+1207173`) gives each particle system a table with one
//! 16-byte entry per transform row of its definition (`+0x47`) and then writes one entry per
//! locator its event row binds, without comparing the two. A row binding more locators than its
//! system's definition has rows writes past the table, over the next heap block's header, and the
//! client later frees that block with a wrong size. Graph nodes name their systems and definitions
//! through patches, so the check covers every event whose system and definition the graph carries.
//! Events naming stock systems keep their native bindings and are not read here.
use crate::tiger::payload::Payload;
use anyhow::{Result, ensure};
use serde_json::Value;
use std::{collections::BTreeMap, fs, path::Path};

/// Flow definition field holding the event rows, their row class and the classes read below.
const FLOW_EVENTS: usize = 0x168;
const EVENT_ROW: u32 = 0x8080_93e6;
const PARTICLE_EVENT: u32 = 0x8080_6cc6;
const SYSTEM_ROW: u32 = 0x8080_6cc8;
const BYTES: u32 = 0x8080_0009;
/// Particle definition byte counting its transform binding rows.
const TRANSFORM_ROWS: usize = 0x47;

/// Fails when a particle event row in the graph at `directory` binds more locators than its
/// system's definition has transform rows, naming each such row.
pub(crate) fn check(directory: &Path) -> Result<()> {
    let graph: Value = serde_json::from_slice(&fs::read(directory.join("asset-graph.json"))?)?;
    let mut nodes = BTreeMap::new();
    collect(&graph, &mut nodes);
    let mut problems = Vec::new();
    for (symbol, node) in &nodes {
        let Some(payload) = read(directory, node) else {
            continue;
        };
        let patches = patches(node);
        for event in particle_events(&payload) {
            let Ok(rows) = payload.array(event + 0x40, 24, Some(SYSTEM_ROW)) else {
                continue;
            };
            for row in rows {
                let Ok(locators) = payload.array(row, 1, Some(BYTES)) else {
                    continue;
                };
                let Some(system) = patches.get(&(row + 16)) else {
                    continue;
                };
                let Some(rows) = transform_rows(directory, &nodes, system) else {
                    continue;
                };
                if locators.len() > usize::from(rows) {
                    problems.push(format!(
                        "{symbol}: the event at {event:#x} binds {} locators to {system}, whose definition has {rows} transform rows",
                        locators.len()
                    ));
                }
            }
        }
    }
    ensure!(
        problems.is_empty(),
        "Particle events bind more locators than their systems have transform rows, which overruns the client's particle heap:\n{}",
        problems.join("\n")
    );
    Ok(())
}

/// Every node in the manifest, at any depth, by symbol.
fn collect<'a>(value: &'a Value, nodes: &mut BTreeMap<String, &'a Value>) {
    match value {
        Value::Object(map) => {
            if let (Some(Value::String(symbol)), Some(Value::String(_)), Some(Value::Array(_))) =
                (map.get("symbol"), map.get("file"), map.get("patches"))
            {
                nodes.insert(symbol.clone(), value);
            }
            for item in map.values() {
                collect(item, nodes);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect(item, nodes);
            }
        }
        _ => {}
    }
}

fn read(directory: &Path, node: &Value) -> Option<Payload> {
    let file = node["file"].as_str()?;
    file.ends_with(".bin")
        .then(|| fs::read(directory.join(file)).ok().map(Payload))
        .flatten()
}

fn patches(node: &Value) -> BTreeMap<usize, String> {
    node["patches"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|patch| {
            Some((
                usize::try_from(patch["offset"].as_u64()?).ok()?,
                patch["symbol"].as_str()?.to_owned(),
            ))
        })
        .collect()
}

/// The particle events of a flow payload, none for any other payload.
fn particle_events(payload: &Payload) -> Vec<usize> {
    let Ok(root) = payload.pointer(24) else {
        return Vec::new();
    };
    let Ok(rows) = payload.array(root + FLOW_EVENTS, 24, Some(EVENT_ROW)) else {
        return Vec::new();
    };
    rows.into_iter()
        .filter_map(|row| payload.pointer(row + 16).ok())
        .filter(|&event| event >= 4 && payload.u32(event - 4).ok() == Some(PARTICLE_EVENT))
        .collect()
}

/// The transform row count of the graph's own system `symbol`, through its definition patch.
fn transform_rows(directory: &Path, nodes: &BTreeMap<String, &Value>, symbol: &str) -> Option<u8> {
    let system = nodes.get(symbol)?;
    let definition = patches(system).remove(&0)?;
    read(directory, nodes.get(&definition)?)?
        .u8(TRANSFORM_ROWS)
        .ok()
}

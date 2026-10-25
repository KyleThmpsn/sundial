//! The hip-fire crosshair an item names, rebuilt from native parts when Shadowkeep has none for its
//! key.
//!
//! An item's strings record names its crosshair through its client tuple: the inventory bucket
//! and two type keys, at modern record +0xC0 and native +0xB8. The crosshair table holds a row per
//! bucket and key that names a crosshair pair: the crosshair entity and an entity every crosshair
//! shares. Rows both versions hold for the same bucket and key are twins, with the same row hash
//! and the same design, so a key Shadowkeep has needs no conversion. A key it lacks, such as the
//! glaive's, gets a native scene built in `scene` and rows the package build adds.
mod scene;
mod twins;

use crate::d2_mot::{
    localization,
    payload::Payload,
    reader::{Reader, write_json},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path};

pub const NATIVE_TABLE: u32 = 0x80B4_79A9;
const MODERN_TABLE: u32 = 0x80C0_D789;
const MODERN_STRINGS: u32 = 0x8080_549F;
const MODERN_TUPLE: usize = 0xC0;
const ROW_STRIDE: usize = 0x28;
const SUB_ROW_STRIDE: usize = 0x14;
/// Parhelion's private asset packages. A native row naming a pair in one was added by an
/// installed build, not by Shadowkeep.
const PRIVATE_PACKAGES: std::ops::RangeInclusive<u16> = 0x0AA0..=0x0CFF;

/// One crosshair table row: row hash, bucket, key, crosshair pair and its sub-rows.
#[derive(Clone, Debug)]
pub(crate) struct Row {
    pub hash: u32,
    pub bucket: u32,
    pub key: u32,
    pub pair: u32,
    pub subs: Vec<Vec<u8>>,
}

/// Rows of either version's crosshair table, which share the layout.
pub(crate) fn rows(r: &mut Reader, table: u32) -> Result<Vec<Row>> {
    let p = r.tag(table, None)?;
    let mut rows = Vec::new();
    for at in p.array(8, ROW_STRIDE, None)? {
        let subs = p
            .array(at + 0x10, SUB_ROW_STRIDE, None)?
            .into_iter()
            .map(|sub| Ok(p.bytes::<SUB_ROW_STRIDE>(sub)?.to_vec()))
            .collect::<Result<Vec<_>>>()?;
        rows.push(Row {
            hash: p.u32(at)?,
            bucket: p.u32(at + 4)?,
            key: p.u32(at + 8)?,
            pair: p.u32(at + 0xC)?,
            subs,
        });
    }
    ensure!(!rows.is_empty(), "crosshair table {table:08X} has no rows");
    Ok(rows)
}

fn hex(value: u32) -> String {
    format!("{value:08X}")
}

/// Record the source item's crosshair, and convert the crosshair of each source key Shadowkeep
/// lacks into native nodes in the graph. The record goes into the graph as `crosshair`.
pub fn apply(
    modern: &Path,
    native: &Path,
    source: &Path,
    work: &Path,
    graph: &Path,
) -> Result<Value> {
    let report: Value = serde_json::from_slice(&fs::read(source.join("report.json"))?)?;
    let item = u32::try_from(
        report["item_hash"]
            .as_u64()
            .context("crosshair source item")?,
    )?;
    let index = usize::try_from(
        report["item_index"]
            .as_u64()
            .context("crosshair source index")?,
    )?;
    let mut m = Reader::new(modern, &work.join("source"), true)?;
    let mut n = Reader::new(native, &work.join("native"), false)?;
    let strings = localization::item_strings(&mut m, item, index)?;
    let strings = m.tag(strings, Some(MODERN_STRINGS))?;
    let tuple = [
        strings.u32(MODERN_TUPLE)?,
        strings.u32(MODERN_TUPLE + 4)?,
        strings.u32(MODERN_TUPLE + 8)?,
    ];
    let modern_rows = rows(&mut m, MODERN_TABLE)?;
    let mut native_rows = rows(&mut n, NATIVE_TABLE)?;
    native_rows.retain(|row| !PRIVATE_PACKAGES.contains(&tiger_pkg::TagHash(row.pair).pkg_id()));
    let missing = modern_rows
        .iter()
        .filter(|row| tuple[1..].contains(&row.key))
        .filter(|row| {
            !native_rows
                .iter()
                .any(|native| native.bucket == row.bucket && native.key == row.key)
        })
        .cloned()
        .collect::<Vec<_>>();
    let manifest = graph.join("asset-graph.json");
    let mut document: Value = serde_json::from_slice(&fs::read(&manifest)?)?;
    // A repeated conversion replaces the nodes an earlier one added.
    document["nodes"]
        .as_array_mut()
        .context("graph nodes")?
        .retain(|node| {
            !node["symbol"]
                .as_str()
                .is_some_and(|s| s.starts_with("crosshair-"))
        });
    let mut record = json!({"bucket":hex(tuple[0]),"first":hex(tuple[1]),"second":hex(tuple[2]),
        "status":"native","rows":[],"gameplay_verified":false});
    if !missing.is_empty() {
        for row in &missing {
            ensure!(
                !native_rows.iter().any(|native| native.hash == row.hash),
                "crosshair row hash {:08X} is already native",
                row.hash
            );
        }
        let twins = twins::discover(&mut m, &mut n, &modern_rows, &native_rows)?;
        let nodes = document["nodes"].as_array_mut().context("graph nodes")?;
        let mut pairs: BTreeMap<u32, String> = BTreeMap::new();
        let mut scenes = Vec::new();
        for row in &missing {
            if pairs.contains_key(&row.pair) {
                continue;
            }
            let prefix = format!("crosshair-{}", pairs.len());
            let (symbol, scene) =
                scene::convert(&mut m, &mut n, &twins, row.pair, &prefix, graph, nodes)?;
            pairs.insert(row.pair, symbol);
            scenes.push(scene);
        }
        record["rows"] = missing
            .iter()
            .map(|row| {
                json!({"hash":hex(row.hash),"bucket":hex(row.bucket),"key":hex(row.key),
                    "pair":pairs[&row.pair],"subs":row.subs.iter().map(hex::encode).collect::<Vec<_>>()})
            })
            .collect::<Vec<_>>()
            .into();
        record["scenes"] = scenes.into();
        record["status"] = json!("converted");
    }
    document["crosshair"] = record.clone();
    write_json(&manifest, &document)?;
    m.finish()?;
    n.finish()?;
    Ok(record)
}

/// Record why the source crosshair could not be converted. The item keeps its base's crosshair.
pub fn record_failure(graph: &Path, error: &anyhow::Error) -> Result<()> {
    let manifest = graph.join("asset-graph.json");
    let mut document: Value = serde_json::from_slice(&fs::read(&manifest)?)?;
    document["crosshair"] = json!({"status":"unconverted","reason":format!("{error:#}"),"rows":[]});
    write_json(&manifest, &document)
}

/// Words of a payload, for scans over 4-aligned values.
fn words(p: &Payload) -> impl Iterator<Item = (usize, u32)> + '_ {
    (0..p.0.len().saturating_sub(3))
        .step_by(4)
        .map(|at| (at, u32::from_le_bytes(p.0[at..at + 4].try_into().unwrap())))
}

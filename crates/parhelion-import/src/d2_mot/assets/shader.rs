//! A reusable private dye bundle. Modern GPU programs are never relabeled as native.
use crate::d2_mot::{
    dye_bundle, dyes,
    profile::hash,
    reader::{Reader, write_json},
    shadowkeep,
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path};
fn unique_channels(rows: &[Value]) -> Result<Vec<Value>> {
    let mut channels = BTreeMap::new();
    for row in rows {
        let channel = row["channel"].as_u64().context("channel")?;
        let mut value = row.clone();
        value
            .as_object_mut()
            .context("dye row")?
            .remove("descriptor");
        if let Some(previous) = channels.insert(channel, value.clone()) {
            ensure!(
                previous == value,
                "conflicting dye layers for channel {channel}; explicit precedence mapping required"
            );
        }
    }
    Ok(channels.into_values().collect())
}
pub fn export(p: &Value, modern: &Path, native: &Path, out: &Path) -> Result<Value> {
    let source = out.join("source");
    let carrier = out.join("native");
    let seed = out.join("seed");
    let bundle = out.join("preset");
    let mut r = Reader::new(modern, &source, true)?;
    let (_, tag) = super::item::find(&mut r, hash(p, "source_item")?)?;
    let modern_dyes = dyes::inspect(&mut r, tag, true)?;
    r.finish()?;
    write_json(&source.join("dyes-all.json"), &modern_dyes)?;
    let rows = unique_channels(modern_dyes.as_array().context("modern dyes")?)?;
    ensure!(!rows.is_empty(), "source item has no dye channels");
    let channels: Vec<_> = rows.iter().map(|r| r["channel"].clone()).collect();
    write_json(&source.join("dyes.json"), &json!(rows))?;
    let mut r = Reader::new(native, &carrier, false)?;
    let native_report = shadowkeep::extract(&mut r, hash(p, "native_item")?)?;
    let native_tag = u32::from_str_radix(
        native_report["item_tag"]
            .as_str()
            .context("native item tag")?,
        16,
    )?;
    let native_dyes = dyes::inspect(&mut r, native_tag, false)?;
    let native_rows = native_dyes.as_array().context("native dyes")?;
    let fallback = native_rows
        .first()
        .context("native item has no material template")?;
    // Channel IDs choose presentation slots; every copied scope is checked by dye_bundle.
    // Missing native channels use an explicitly supplied carrier's legacy-only parameters.
    let mut templates = vec![];
    let mut borrowed = vec![];
    for row in &rows {
        let same = native_rows.iter().find(|n| n["channel"] == row["channel"]);
        let mut template = same.unwrap_or(fallback).clone();
        if same.is_none() {
            borrowed.push(row["channel"].clone());
        }
        template["channel"] = row["channel"].clone();
        templates.push(template);
    }
    write_json(&carrier.join("dyes.json"), &json!(templates))?;
    r.finish()?;
    fs::create_dir_all(&seed)?;
    write_json(
        &seed.join("asset-graph.json"),
        &json!({"kind":"shader_preset","nodes":[],"dye_key_base":hash(p,"dye_key_base")?,"installable":false}),
    )?;
    let mut r = Reader::new(native, &bundle, false)?;
    let graph = dye_bundle::build(&mut r, &source, &carrier, &seed)?;
    r.finish()?;
    let report = json!({"schema":1,"kind":"shader","profile":p,"preset":bundle,"channels":channels,"legacy_template_channels":borrowed,"private_nodes":graph["nodes"].as_array().unwrap().len(),"installed":false,"standalone_installable":false,"limits":["native legacy-only material parameters retained","animated modern shader programs not translated","recipient must expose compatible dye slots"]});
    write_json(&out.join("asset.json"), &report)?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identical_layers_collapse_but_conflicting_layers_fail() {
        let a = json!({"channel":0,"descriptor":40,"manifest":"A","found":[1]});
        let mut b = a.clone();
        b["descriptor"] = json!(56);
        assert_eq!(unique_channels(&[a.clone(), b.clone()]).unwrap().len(), 1);
        b["found"] = json!([2]);
        assert!(unique_channels(&[a, b]).is_err());
    }
}

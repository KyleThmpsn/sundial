//! A reusable private dye bundle. Modern GPU programs are never relabeled as native.
use crate::d2_mot::{
    dye_bundle, dyes,
    profile::hash,
    reader::{Reader, write_json},
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
    let (index, tag) = super::item::find(&mut r, hash(p, "source_item")?)?;
    let source_hash = hash(p, "source_item")?;
    let name = crate::d2_mot::localization::item_name(&mut r, source_hash, index, 0)?;
    let flavor = crate::d2_mot::localization::item_label(&mut r, source_hash, index, 0, 0xA4)?;
    let rarity = r.tag(tag, None)?.u8(0xA0)?;
    crate::d2_mot::icon::export(&mut r, source_hash, index)?;
    write_json(
        &source.join("presentation.json"),
        &json!({"name":name["name"],"flavor":flavor["name"],"rarity":rarity}),
    )?;
    let modern_dyes = dyes::inspect(&mut r, tag, true)?;
    let modern_context = crate::d2_mot::tfx::context(&mut r, true)?;
    write_json(&source.join("render-context.json"), &modern_context)?;
    r.finish()?;
    write_json(&source.join("dyes-all.json"), &modern_dyes)?;
    let rows = unique_channels(modern_dyes.as_array().context("modern dyes")?)?;
    ensure!(!rows.is_empty(), "source item has no dye channels");
    let channels: Vec<_> = rows.iter().map(|r| r["channel"].clone()).collect();
    write_json(&source.join("dyes.json"), &json!(rows))?;
    let mut r = Reader::new(native, &carrier, false)?;
    let native_tag = super::item::find_native(&mut r, hash(p, "native_item")?)?;
    let native_item = r.tag(native_tag, Some(0x80807BEA))?;
    if p["kind"].as_str() == Some("shader") {
        ensure!(
            native_item.u8(0xB8)? == 14,
            "Native registration template is not a shader"
        );
    }
    let native_dyes = dyes::inspect(&mut r, native_tag, false)?;
    let native_context = crate::d2_mot::tfx::context(&mut r, false)?;
    write_json(&carrier.join("render-context.json"), &native_context)?;
    let native_rows = native_dyes.as_array().context("native dyes")?;
    let fallback = native_rows
        .first()
        .context("native item has no material template")?;
    // Channel IDs choose presentation slots; every copied scope is checked by dye_bundle.
    // Templates provide native record metadata only. Renderer defaults supply legacy-only values.
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
    let report = json!({"schema":1,"kind":"shader","profile":p,"preset":bundle,"channels":channels,"native_metadata_channels":borrowed,"private_nodes":graph["nodes"].as_array().unwrap().len(),"material_programs":graph["material_programs"],"installed":false,"standalone_installable":false,"limits":["Native lighting and shared gear rendering determine the final appearance. Verify it in game."]});
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

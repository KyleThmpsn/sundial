//! Reusable, explicitly targeted imports. Unsupported layouts fail before staging.
use crate::d2_mot::{
    bundle, convert, extract,
    reader::{Reader, outside, write_json},
    shadowkeep,
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{fs, path::Path};
pub fn hash(p: &Value, key: &str) -> Result<u32> {
    let v = &p[key];
    let n = if let Some(s) = v.as_str() {
        u32::from_str_radix(s.trim_start_matches("0x"), 16)?
    } else {
        u32::try_from(v.as_u64().with_context(|| format!("missing {key}"))?)?
    };
    ensure!(![0, u32::MAX, 0x811C9DC5].contains(&n), "invalid {key}");
    Ok(n)
}
pub fn prepare(profile: &Path, modern: &Path, native: &Path, out: &Path) -> Result<Value> {
    prepare_reusing(profile, modern, native, out, None, &mut |_| {})
}

pub(crate) struct Extracted<'a> {
    pub source: &'a Path,
    pub native: &'a Path,
}

pub(crate) fn prepare_reusing(
    profile: &Path,
    modern: &Path,
    native: &Path,
    out: &Path,
    extracted: Option<Extracted<'_>>,
    progress: &mut dyn FnMut(String),
) -> Result<Value> {
    let p: Value = serde_json::from_slice(&fs::read(profile)?)?;
    for key in p.as_object().context("profile must be an object")?.keys() {
        ensure!(
            [
                "source_item",
                "native_item",
                "target_item",
                "art_key",
                "model_index",
                "convert_dyes",
                "dye_key_base",
                "plated",
                "ornament",
                "rig"
            ]
            .contains(&key.as_str()),
            "unknown profile setting {key}"
        );
    }
    for key in ["convert_dyes", "plated"] {
        if let Some(v) = p.get(key) {
            ensure!(v.is_boolean(), "{key} must be boolean");
        }
    }
    if let Some(v) = p.get("model_index") {
        ensure!(
            v.as_u64().is_some(),
            "model_index must be a nonnegative integer"
        );
    }
    let source = hash(&p, "source_item")?;
    let donor = hash(&p, "native_item")?;
    let target = hash(&p, "target_item")?;
    let key = hash(&p, "art_key")?;
    ensure!(
        source != target && donor != target,
        "target must be an authored private item"
    );
    let index = p["model_index"].as_u64().unwrap_or(0) as usize;
    // Check both installed source trees before creating any output.
    let out = outside(
        &outside(out, modern.parent().context("modern path")?)?,
        native.parent().context("native path")?,
    )?;
    fs::create_dir_all(&out)?;
    let modern_out = extracted
        .as_ref()
        .map_or_else(|| out.join("source"), |paths| paths.source.to_owned());
    let native_out = extracted
        .as_ref()
        .map_or_else(|| out.join("native"), |paths| paths.native.to_owned());
    let mapped = out.join("mapped");
    let graph = out.join("graph");
    let source_report = if extracted.is_some() {
        serde_json::from_slice::<Value>(&fs::read(modern_out.join("report.json"))?)?
    } else {
        eprintln!("Extracting modern item {source:08X}");
        progress("Extracting source models and textures…".into());
        let mut r = Reader::new(modern, &modern_out, true)?;
        if let Some(link) = p.get("ornament") {
            let weapon = hash(link, "source_weapon")?;
            let sockets = crate::d2_mot::ornaments::discover(&mut r, weapon)?;
            let index = link["source_socket"]
                .as_u64()
                .context("ornament source socket")? as usize;
            let socket = sockets["sockets"]
                .as_array()
                .context("source sockets")?
                .get(index)
                .context("source socket outside table")?;
            let choices = socket["choices"].as_array().context("source choices")?;
            ensure!(
                choices.iter().any(|choice| choice["hash"] == source
                    && choice["art_indices"]
                        .as_array()
                        .is_some_and(|art| !art.is_empty())),
                "ornament is not an appearance choice on the source weapon"
            );
        }
        let report = extract::extract_with_progress(&mut r, source, None, progress)?;
        write_json(&modern_out.join("report.json"), &report)?;
        r.finish()?;
        report
    };
    ensure!(
        source_report["item_hash"].as_u64() == Some(u64::from(source)),
        "Cached source identity differs"
    );
    ensure!(
        source_report["models"]
            .as_array()
            .context("models")?
            .get(index)
            .is_some(),
        "selected model missing"
    );
    let native_report = if extracted.is_some() {
        serde_json::from_slice::<Value>(&fs::read(native_out.join("template-report.json"))?)?
    } else {
        eprintln!("Extracting native carrier {donor:08X}");
        progress("Extracting donor models and materials…".into());
        let mut r = Reader::new(native, &native_out, false)?;
        let report = shadowkeep::extract(&mut r, donor)?;
        write_json(&native_out.join("template-report.json"), &report)?;
        r.finish()?;
        report
    };
    ensure!(
        hash(&native_report, "item")? == donor,
        "Cached donor identity differs"
    );
    progress("Converting mesh geometry and texture plates…".into());
    eprintln!("Converting selected rigid mesh");
    let rig_mapping = p
        .get("rig")
        .map(|config| {
            crate::d2_mot::rig_convert::compatible_map(
                config,
                &source_report["item_tag"],
                &native_report["item_tag"],
            )
        })
        .transpose()?;
    let bones = rig_mapping
        .as_ref()
        .map(|mapping| serde_json::from_value::<Vec<u16>>(mapping["bone_map"].clone()))
        .transpose()?;
    if let Some(mapping) = &rig_mapping {
        write_json(&out.join("rig_mapping.json"), mapping)?;
    }
    convert::convert_mapped(
        &modern_out,
        &native_out,
        &mapped,
        index,
        p["plated"].as_bool().unwrap_or(false),
        bones.as_deref(),
    )?;
    let config = json!({"item_hash":target,"art_key":key,"source_model_index":index,"native_template":native_out,"native_item":donor});
    progress("Building native model and material resources…".into());
    let mut r = Reader::new(native, &graph, false)?;
    let mut graph_report = bundle::build_configured(&mut r, &modern_out, &mapped, Some(&config))?;
    if let Some(mapping) = rig_mapping {
        graph_report["rig_mapping"] = mapping;
        write_json(&graph.join("asset-graph.json"), &graph_report)?;
    }
    if let Some(link) = p.get("ornament") {
        graph_report["ornament"] = link.clone();
        graph_report["ornament"]["name"] = source_report["name"].clone();
        graph_report["ornament_localization"] = source_report["localization"].clone();
        graph_report["ornament_icon_png"] = json!(modern_out.join("item-icon.png"));
        write_json(&graph.join("asset-graph.json"), &graph_report)?;
    }
    r.finish()?;
    drop(r);
    let mut final_graph = graph.clone();
    if p["convert_dyes"].as_bool().unwrap_or(false) {
        let base = hash(&p, "dye_key_base")?;
        graph_report["dye_key_base"] = json!(base);
        write_json(&graph.join("asset-graph.json"), &graph_report)?;
        let md = out.join("modern-dyes");
        let nd = out.join("native-dyes");
        progress("Converting shader and dye channels…".into());
        eprintln!("Converting compatible dye channels");
        for (packages, output, report, modern) in [
            (modern, &md, &source_report, true),
            (native, &nd, &native_report, false),
        ] {
            let mut r = Reader::new(packages, output, modern)?;
            let report = crate::d2_mot::dyes::inspect(&mut r, hash(report, "item_tag")?, modern)?;
            write_json(&output.join("dyes.json"), &report)?;
            r.finish()?;
        }
        progress("Building final materials…".into());
        final_graph = out.join("material-graph");
        let mut r = Reader::new(native, &final_graph, false)?;
        graph_report = crate::d2_mot::dye_bundle::build(&mut r, &md, &nd, &graph)?;
        r.finish()?;
    }
    progress("Saving converted model…".into());
    write_json(&out.join("profile.json"), &p)?;
    Ok(
        json!({"graph":final_graph,"source_icon":modern_out.join("item-icon.png"),"source_item":source,"native_item":donor,"target_item":target,"private_nodes":graph_report["nodes"].as_array().unwrap().len(),"installed":false,"limits":"rigid bone-zero mesh; matching native material stages; native donor animations and shader programs; in-game test required"}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_sentinel_and_overflow() {
        assert_eq!(
            hash(&json!({"item":"0x50EE7278"}), "item").unwrap(),
            0x50EE7278
        );
        assert!(hash(&json!({"item":"FFFFFFFF"}), "item").is_err());
        assert!(hash(&json!({"item":4294967296u64}), "item").is_err());
    }
}

//! Mounted source acceptance follows persisted talent and equipment references.
use anyhow::{Context, Result, ensure};
use parhelion_import::{d1_roi::Source, tiger::payload::Payload};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, env, fs, path::PathBuf};

#[test]
#[ignore = "Requires configured PARHELION_D1_ROI_PACKAGES, PARHELION_D1_ROI_WEAPONS and fresh PARHELION_D1_ROI_OUTPUT"]
#[allow(clippy::cognitive_complexity)]
fn mounted_weapons_preserve_selectable_art_and_executable_talent_references() -> Result<()> {
    let packages =
        PathBuf::from(env::var_os("PARHELION_D1_ROI_PACKAGES").context("Source packages")?);
    let output = PathBuf::from(env::var_os("PARHELION_D1_ROI_OUTPUT").context("Fresh output")?);
    let requested = env::var("PARHELION_D1_ROI_WEAPONS")?
        .split(',')
        .map(|s| u32::from_str_radix(s.trim().trim_start_matches("0x"), 16))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    ensure!(!requested.is_empty(), "Required weapon corpus is empty");
    ensure!(!output.exists(), "Use a fresh output directory");
    let source = Source::open(&packages, |_, _| {})?;
    source.write_inspection(&output)?;
    let mut receipts = Vec::new();
    for hash in requested {
        let directory = output.join(format!("weapon-{hash:08X}"));
        let path = source.export_weapon(hash, &directory)?;
        let report: Value = serde_json::from_slice(&fs::read(path)?)?;
        let mut assets = BTreeMap::new();
        for asset in report["assets"]
            .as_array()
            .context("Persisted source assets")?
        {
            let tag = asset["tag"].as_u64().context("Asset identity")? as u32;
            let bytes = fs::read(directory.join(asset["file"].as_str().context("Asset path")?))?;
            ensure!(hex::encode(Sha256::digest(&bytes)) == asset["sha256"]);
            ensure!(
                assets.insert(tag, Payload(bytes)).is_none(),
                "Repeated export identity"
            );
        }
        let definition = &assets[&(report["item"]["definition"]
            .as_u64()
            .context("Definition")? as u32)];
        ensure!(definition.u32(0x78)? == hash);
        let shared = &assets[&(report["talents"]["shared_grid"]
            .as_u64()
            .context("Shared grid")? as u32)];
        let shared_nodes = shared.array(0x10, 64, Some(0x80801728))?;
        let stats = &assets[&(report["talents"]["stat_table"]
            .as_u64()
            .context("Stat table")? as u32)];
        let stat_rows = stats.array(8, 16, Some(0x80801870))?;
        let nodes = report["talents"]["nodes"]
            .as_array()
            .context("Talent nodes")?;
        ensure!(nodes.len() == shared_nodes.len() && !nodes.is_empty());
        let mut actions = Vec::new();
        let mut adjusted = 0;
        for (node, at) in nodes.iter().zip(shared_nodes) {
            let steps = shared.array(at + 24, 144, None)?;
            ensure!(node["steps"].as_array().context("Talent steps")?.len() == steps.len());
            for (step, at) in node["steps"].as_array().unwrap().iter().zip(steps) {
                for (variant, at) in step["variants"]
                    .as_array()
                    .context("Talent variants")?
                    .iter()
                    .zip(shared.array(at, 88, None)?)
                {
                    let ordinals = shared.array(at + 32, 2, Some(0x80804628))?;
                    let perks = variant["perks"].as_array().context("Finished perks")?;
                    ensure!(ordinals.len() == perks.len());
                    // Exported adjustments follow the variant's own rows and the stat table.
                    let rows = shared.array(at + 16, 16, Some(0x80801787))?;
                    let exported = variant["stat_adjustments"]
                        .as_array()
                        .context("Stat adjustments")?;
                    ensure!(rows.len() == exported.len());
                    for (adjustment, at) in exported.iter().zip(rows) {
                        let ordinal = shared.u8(at + 12)?;
                        ensure!(adjustment["index"] == ordinal);
                        ensure!(adjustment["value"] == i32::from_le_bytes(shared.bytes::<4>(at)?));
                        ensure!(adjustment["hash"] == stats.u32(stat_rows[usize::from(ordinal)])?);
                        adjusted += 1;
                    }
                    for (perk, at) in perks.iter().zip(ordinals) {
                        ensure!(perk["ordinal"] == shared.u16(at)?);
                        if let Some(action) = perk["action"].as_u64() {
                            let action = action as u32;
                            let controller = &assets[&action];
                            ensure!(source.reference(action)? == 0x80802711);
                            // Native lowering must be able to read the actual pointed records,
                            // including conditions that are intentionally unsupported later.
                            for (field, stride) in [(0x20, 8), (0x38, 8), (0x48, 8), (0x58, 8)] {
                                for row in controller.array(field, stride, None)? {
                                    let target = controller.pointer(row)?;
                                    let minimum = if field == 0x38 { 3 } else { 8 };
                                    ensure!(target >= 4 && target + minimum <= controller.0.len());
                                    ensure!(controller.u32(target - 4)? >> 16 == 0x8080);
                                }
                            }
                            actions.push(action);
                        }
                    }
                }
            }
        }
        ensure!(
            !actions.is_empty(),
            "Required weapon has no executable talent trace"
        );
        let mut art = Vec::new();
        for part in report["art"]["parts"]
            .as_array()
            .context("Selectable art")?
        {
            let parent = part["parent"].as_u64().context("Art parent")? as u32;
            let entity = assets[&parent].u32(16)?;
            ensure!(part["entity"] == entity);
            let owners = assets[&entity].array(0x20, 12, Some(0x80800715))?;
            for row in owners {
                let owner = &assets[&assets[&entity].u32(row)?];
                let resource = owner.pointer(24)?;
                if matches!(owner.u32(resource - 4)?, 0x80801A9C | 0x80801BF6) {
                    let model = &assets[&owner.u32(resource + 0x1C4)?];
                    for mesh in model.array(16, 160, Some(0x80801BBF))? {
                        for offset in [0x30, 0x34, 0x38, 0x3C, 0x40] {
                            let tag = model.u32(mesh + offset)?;
                            if matches!(tag, 0 | u32::MAX) {
                                continue;
                            }
                            ensure!(assets.contains_key(&tag));
                            ensure!(assets.contains_key(&source.reference(tag)?));
                        }
                    }
                }
            }
            art.push(json!({"parent":parent,"entity":entity}));
        }
        ensure!(!art.is_empty(), "Required weapon has no source art");
        receipts.push(json!({"source_item":hash,"source_actions":actions,
            "stat_adjustments":adjusted,"art":art,"export":directory}));
    }
    fs::write(
        output.join("weapon-readback.json"),
        serde_json::to_vec_pretty(&json!({
            "builds":source.report().builds,"package_stamp":source.report().stamp,
            "weapons":receipts,"native_conversion":false
        }))?,
    )?;
    Ok(())
}

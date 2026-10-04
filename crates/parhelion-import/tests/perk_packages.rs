//! Opt-in package-to-artifact verification. Never opens an executable or game process.
use std::{env, fs, path::PathBuf};

use parhelion_import::d2_mot::{gameplay::perks, reader::Reader};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

#[test]
#[ignore = "Requires PARHELION_IMPORT_MODERN_PACKAGES and PARHELION_IMPORT_PERK_OUTPUT"]
#[expect(
    clippy::cognitive_complexity,
    reason = "End-to-end verification keeps the ordered workflow and its independent assertions together"
)]
fn source_perks_reach_their_runtime_payloads() -> anyhow::Result<()> {
    let packages =
        PathBuf::from(env::var_os("PARHELION_IMPORT_MODERN_PACKAGES").expect("modern packages"));
    let output =
        PathBuf::from(env::var_os("PARHELION_IMPORT_PERK_OUTPUT").expect("artifact output"));
    let mut reader = Reader::new(&packages, &output, true)?;
    let mut reports = Vec::new();
    for (hash, directory) in [
        (2077819806, "eager"),
        (1618208178, "enhanced"),
        (1528281896, "outlaw"),
    ] {
        let report = perks::trace(&mut reader, hash)?;
        assert_eq!(report["plug_hash"], hash);
        let actions = report["perks"].as_array().expect("perk actions");
        assert!(
            actions
                .iter()
                .any(|p| !p["states"].as_array().expect("states").is_empty())
        );
        fs::create_dir_all(output.join(directory))?;
        fs::write(
            output.join(directory).join("perk-trace.json"),
            serde_json::to_vec_pretty(&report)?,
        )?;
        reports.push(report);
    }
    let eager = reports[0]["perks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["perk_hash"] == 0x25F8291Au32)
        .expect("Eager Edge runtime identity");
    let states = eager["states"].as_array().unwrap();
    let conditions: Vec<_> = states
        .iter()
        .flat_map(|s| s["condition_groups"].as_array().unwrap())
        .flat_map(|g| g["conditions"].as_array().unwrap())
        .collect();
    assert!(!conditions.is_empty());
    for report in reports.iter().take(2) {
        let controller = report["perks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|perk| {
                perk["auxiliary"]
                    .as_array()
                    .is_some_and(|records| records.iter().any(|record| record["key"] == "86C440E8"))
            })
            .expect("source Eager Edge auxiliary controller");
        let auxiliary = controller["auxiliary"].as_array().unwrap();
        assert_eq!(auxiliary.len(), 1);
        let record = &auxiliary[0];
        assert_eq!(record["class"], "8080B843");
        assert_eq!(record["state_reference_class"], "8080B848");
        assert_eq!(record["unknown_integer"], 150);
        assert_eq!(record["state_keys"], json!(["43F19C55"]));
        assert_eq!(record["source_record"].as_str().unwrap().len(), 112);
        let state_offsets = controller["states"]
            .as_array()
            .unwrap()
            .iter()
            .map(|state| state["offset"].as_u64().unwrap())
            .collect::<Vec<_>>();
        assert!(
            record["state_offsets"]
                .as_array()
                .unwrap()
                .iter()
                .all(|offset| state_offsets.contains(&offset.as_u64().unwrap()))
        );
        let condition49 = controller["states"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|state| state["condition_groups"].as_array().unwrap())
            .flat_map(|group| group["conditions"].as_array().unwrap())
            .find(|condition| condition["class"] == "8080BDCF")
            .expect("source condition 49");
        assert_eq!(condition49["source_kind"], 49);
        assert_eq!(condition49["source_record"].as_str().unwrap().len(), 80);
        assert_eq!(condition49["native_kind"], 41);
        assert_eq!(condition49["native_class"], "808029E1");
        assert_eq!(condition49["native_node"]["kind"], 41);
        assert_eq!(
            condition49["native_node"]["bytes"].as_str().unwrap().len(),
            82
        );
    }
    for tag in [0x80C30682u32, 0x80C30AC9] {
        let action = reader.tag(tag, Some(0x8080B835))?;
        let controller = perks::controller::read(&action)?;
        let condition49 = controller
            .states
            .iter()
            .flat_map(|state| &state.transitions)
            .flat_map(|transition| &transition.conditions)
            .find(|condition| condition.class == 0x8080BDCF)
            .expect("paired source condition 49");
        assert_eq!(action.u32(condition49.offset + 40)?, 0x808030E3);
        assert_eq!(condition49.source_record.as_ref().unwrap().len(), 80);
    }
    let effects: Vec<_> = states
        .iter()
        .flat_map(|s| s["effects"].as_array().unwrap())
        .collect();
    for suffix in [
        "sword_enhanced_lunge.pattern.tft",
        "sword_enhanced_lunge_angular_tracking.pattern.tft",
    ] {
        let effect = effects
            .iter()
            .find(|e| e["path"].as_str().is_some_and(|p| p.ends_with(suffix)))
            .expect("source-authored sword attachment");
        assert!(!effect["expression"]["code"].as_str().unwrap().is_empty());
        assert!(
            !effect["expression"]["constant_bits"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        let modifiers: Vec<_> = effect["entity"]["components"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|c| c["modifiers"].as_array().into_iter().flatten())
            .collect();
        assert!(!modifiers.is_empty());
        for modifier in &modifiers {
            let bits = u32::from_str_radix(modifier["amount_bits"].as_str().unwrap(), 16)?;
            assert!(f32::from_bits(bits).is_finite());
            assert_eq!(
                (modifier["amount"].as_f64().unwrap() as f32).to_bits(),
                bits
            );
        }
        let mapped = modifiers
            .iter()
            .map(|modifier| {
                (
                    modifier["component"].as_u64().unwrap(),
                    modifier["input"].as_u64().unwrap(),
                    modifier["native_component"].as_u64(),
                    modifier["native_input"].as_u64(),
                )
            })
            .collect::<Vec<_>>();
        if suffix.contains("angular_tracking") {
            assert_eq!(
                mapped.iter().map(|row| (row.0, row.1)).collect::<Vec<_>>(),
                [(12, 4), (12, 5)]
            );
        } else {
            assert_eq!(mapped, [(12, 3, Some(11), Some(3))]);
        }
    }
    reader.finish()?;
    let manifest: Value = serde_json::from_slice(&fs::read(output.join("source-manifest.json"))?)?;
    let mut verified = Vec::new();
    for (tag, record) in manifest["tags"].as_object().unwrap() {
        let bytes = fs::read(record["path"].as_str().unwrap())?;
        assert_eq!(record["size"].as_u64(), Some(bytes.len() as u64));
        verified.push(
            json!({"tag": tag, "class": record["reference"], "size": bytes.len(),
            "sha256": hex::encode(Sha256::digest(&bytes))}),
        );
    }
    fs::write(
        output.join("verified-source.json"),
        serde_json::to_vec_pretty(&json!({
            "process_access": false, "installable": false, "plugs": reports, "payloads": verified
        }))?,
    )?;
    println!("Verified source perk artifacts at {}", output.display());
    Ok(())
}

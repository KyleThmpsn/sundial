//! Configured source bank export, persisted clips and the native pose decoder.
use anyhow::{Context, Result, ensure};
use parhelion_import::{
    marathon,
    tiger::{animation::clip, payload::Payload, reader::Reader},
};
use serde_json::{Value, json};
use std::{collections::BTreeSet, env, fs, path::PathBuf};

fn configured(name: &str) -> Result<PathBuf> {
    env::var_os(name)
        .map(PathBuf::from)
        .with_context(|| format!("Set {name}"))
}

#[test]
#[ignore = "Requires configured Marathon source packages, cases and a fresh artifact directory"]
fn source_banks_survive_export_and_native_pose_decoding() -> Result<()> {
    let packages = configured("PARHELION_MARATHON_PACKAGES")?;
    let output = parhelion_import::tiger::reader::outside(
        &configured("PARHELION_MARATHON_ANIMATION_OUTPUT")?,
        packages.parent().context("Source root")?,
    )?;
    ensure!(!output.exists(), "Use a fresh animation artifact directory");
    let cases: Vec<Value> = serde_json::from_slice(&fs::read(configured(
        "PARHELION_MARATHON_ANIMATION_CASES",
    )?)?)?;
    ensure!(!cases.is_empty(), "Required animation corpus is empty");
    fs::create_dir_all(&output)?;
    fs::create_dir(output.join("poses"))?;
    let mut source = Reader::marathon(&packages, &output.join("source"))?;
    let mut evidence = Vec::new();
    let mut decoded = BTreeSet::new();
    for (index, case) in cases.iter().enumerate() {
        let directory = output.join(format!("case-{index}"));
        fs::create_dir(&directory)?;
        let parts = case["source_assembly"]
            .as_array()
            .context("Source assembly")?;
        let report = marathon::inspect_animation(&mut source, parts, &directory)?;
        fs::write(
            directory.join("animation.json"),
            serde_json::to_vec_pretty(&report)?,
        )?;
        let saved: Value = serde_json::from_slice(&fs::read(directory.join("animation.json"))?)?;
        let actual: BTreeSet<_> = saved["clips"]
            .as_array()
            .context("Exported clips")?
            .iter()
            .map(|c| c["source"].as_u64().context("Source clip"))
            .collect::<Result<_>>()?;
        let expected: BTreeSet<_> = case["expected_source_clips"]
            .as_array()
            .context("Independent source clip references")?
            .iter()
            .map(|c| c.as_u64().context("Expected source clip"))
            .collect::<Result<_>>()?;
        ensure!(
            !expected.is_empty() && actual == expected,
            "Export lost or borrowed source clips"
        );
        let mut clips = Vec::new();
        for entry in saved["clips"].as_array().unwrap() {
            if entry["status"] != "converted" {
                ensure!(
                    entry["reason"].is_string(),
                    "Unconverted source clip has no reason"
                );
                continue;
            }
            let payload = Payload(fs::read(
                directory.join(entry["file"].as_str().context("Clip file")?),
            )?);
            let slots = usize::from(payload.u16(0x140)?);
            let frames = usize::from(payload.u16(0x13C)?);
            let source = entry["source"].as_u64().context("Source clip identity")?;
            let pose_file = format!("poses/{source:08X}.json");
            if decoded.insert(source) {
                let defaults = vec![[0., 0., 0., 1., 0., 0., 0., 1.]; slots];
                let samples = (0..frames)
                    .map(|frame| clip::sample(&payload, &defaults, frame))
                    .collect::<Result<Vec<_>>>()?;
                fs::write(output.join(&pose_file), serde_json::to_vec(&samples)?)?;
            }
            clips.push(json!({"source":source,"file":entry["file"],"frames":frames,"slots":slots,"pose_file":pose_file}));
        }
        ensure!(
            !clips.is_empty(),
            "Required source bank produced no decodable clips"
        );
        fs::write(
            directory.join("native-poses.json"),
            serde_json::to_vec(&clips)?,
        )?;
        evidence.push(json!({"case":index,"source_clips":actual,"decoded_clips":clips.len(),"controller_status":saved["controller_status"]}));
    }
    source.finish()?;
    fs::write(
        output.join("verified-source-animation.json"),
        serde_json::to_vec_pretty(&json!({
            "cases":evidence,"unique_decoded_clips":decoded.len(),"limits":"Source references and native channel decoding with identity defaults for absent channels. Controller execution, source activation, final rig poses and gameplay are unverified."
        }))?,
    )?;
    Ok(())
}

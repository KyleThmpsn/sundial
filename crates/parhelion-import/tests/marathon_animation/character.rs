//! Source character selection is explicit and verified against independently decoded package links.
use crate::*;
use sha2::{Digest, Sha256};

#[test]
#[ignore = "Requires configured source character cases and a fresh artifact directory"]
fn character_animation_export_keeps_selected_rigs_and_routes() -> Result<()> {
    let packages = configured("PARHELION_MARATHON_PACKAGES")?;
    let source_stamp = parhelion_import::tiger::reader::package_stamp(&packages)?;
    let output = parhelion_import::tiger::reader::outside(
        &configured("PARHELION_MARATHON_CHARACTER_OUTPUT")?,
        packages.parent().context("Source root")?,
    )?;
    ensure!(!output.exists(), "Use a fresh character artifact directory");
    let case_bytes = fs::read(configured("PARHELION_MARATHON_CHARACTER_CASES")?)?;
    let cases: Vec<Value> = serde_json::from_slice(&case_bytes)?;
    ensure!(!cases.is_empty(), "Required character corpus is empty");
    fs::create_dir_all(&output)?;
    let mut source = Reader::marathon(&packages, &output.join("source"))?;
    let mut evidence = Vec::new();
    for (index, case) in cases.iter().enumerate() {
        let directory = output.join(format!("case-{index}"));
        fs::create_dir(&directory)?;
        let entity = u32::try_from(case["entity"].as_u64().context("Source character entity")?)?;
        let profile = u32::try_from(case["profile"].as_u64().context("Source weapon profile")?)?;
        let report =
            marathon::inspect_character_animation(&mut source, entity, profile, &directory)?;
        let file = directory.join("character-animation.json");
        fs::write(&file, serde_json::to_vec_pretty(&report)?)?;
        let saved: Value = serde_json::from_slice(&fs::read(&file)?)?;
        ensure!(
            saved["selection"]["driver"]["lookup"]["owner"] == case["lookup"],
            "Character lookup binding differs"
        );
        ensure!(
            saved["selection"]["driver"]["skeleton"]["owner"] == case["skeleton"],
            "Character skeleton binding differs"
        );
        ensure!(
            saved["selection"]["driver"]["controls"]["owner"] == case["controls"],
            "Character control binding differs"
        );
        let actual = saved["clips"]
            .as_array()
            .context("Exported character clips")?
            .iter()
            .map(|c| c["source"].as_u64().context("Source clip"))
            .collect::<Result<BTreeSet<_>>>()?;
        let expected = case["clips"]
            .as_array()
            .context("Independent character clip closure")?
            .iter()
            .map(|c| c.as_u64().context("Expected clip"))
            .collect::<Result<BTreeSet<_>>>()?;
        ensure!(
            !expected.is_empty() && actual == expected,
            "Character export borrowed or lost clips"
        );
        let probes = case["routes"]
            .as_array()
            .context("Independent character routes")?;
        ensure!(!probes.is_empty(), "Character route corpus is empty");
        for probe in probes {
            let name = probe["action"]
                .as_u64()
                .context("Character action")?
                .to_string();
            ensure!(
                saved["selection"]["routes"][&name]["default_names"] == probe["descriptor_names"],
                "Character source route differs"
            );
        }
        let mut converted = 0;
        for entry in saved["clips"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| c["status"] == "converted")
        {
            let tag = entry["source"].as_u64().context("Converted source clip")?;
            let original = fs::read(
                directory.join(
                    entry["source_file"]
                        .as_str()
                        .context("Original clip file")?,
                ),
            )?;
            ensure!(
                hex::encode(Sha256::digest(&original))
                    == case["clip_sha256"][tag.to_string()]
                        .as_str()
                        .context("Independent source clip digest")?,
                "Exported original character clip differs from the source payload"
            );
            let payload = Payload(fs::read(
                directory.join(entry["file"].as_str().context("Native clip")?),
            )?);
            let defaults = vec![[0., 0., 0., 1., 0., 0., 0., 1.]; usize::from(payload.u16(0x140)?)];
            let frames = usize::from(payload.u16(0x13C)?);
            ensure!(frames > 0, "Converted character clip has no frames");
            for frame in [0, frames / 2, frames - 1] {
                let pose = clip::sample(&payload, &defaults, frame)?;
                ensure!(
                    pose.iter().flatten().all(|v| v.is_finite()),
                    "Character channels are not finite"
                );
            }
            converted += 1;
        }
        ensure!(
            converted > 0,
            "Required character bank has no decodable clip candidates"
        );
        evidence.push(json!({"entity":entity,"profile":profile,"clips":actual.len(),"route_probes":probes.len()}));
    }
    source.finish()?;
    ensure!(
        parhelion_import::tiger::reader::package_stamp(&packages)? == source_stamp,
        "Mounted character inputs changed"
    );
    fs::write(
        output.join("verified-character-animation.json"),
        serde_json::to_vec_pretty(&json!({
            "revision":env::var("SUNDIAL_TEST_REVISION")?,
            "test_binary_sha256":hex::encode(Sha256::digest(fs::read(env::current_exe()?)?)),
        "expectations_sha256":hex::encode(Sha256::digest(case_bytes)),"source_stamp":source_stamp,"cases":evidence,
            "limits":"Serialized source bindings, routes and native clip channel decoding. Character retargeting and live first-person or third-person playback are unverified."
        }))?,
    )?;
    Ok(())
}

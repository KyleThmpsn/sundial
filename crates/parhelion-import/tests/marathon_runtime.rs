//! Configured graph export checked against an independent package decoder's complete topology.
use anyhow::{Context, Result, ensure};
use parhelion_import::{
    marathon,
    tiger::{
        entity::links::Graph,
        payload::Payload,
        reader::{Reader, package_stamp},
    },
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, env, fs, path::PathBuf};

fn configured(name: &str) -> Result<PathBuf> {
    env::var_os(name)
        .map(PathBuf::from)
        .with_context(|| format!("Set {name}"))
}

#[test]
#[ignore = "Requires configured Marathon packages, independent graph expectations and fresh output"]
fn runtime_graphs_survive_export_and_reference_readback() -> Result<()> {
    let revision = env::var("SUNDIAL_TEST_REVISION")?;
    let test_binary_sha256 = hex::encode(Sha256::digest(fs::read(env::current_exe()?)?));
    let packages = configured("PARHELION_MARATHON_PACKAGES")?;
    let source_stamp = package_stamp(&packages)?;
    let output = parhelion_import::tiger::reader::outside(
        &configured("PARHELION_MARATHON_RUNTIME_OUTPUT")?,
        packages.parent().context("Source root")?,
    )?;
    ensure!(!output.exists(), "Use a fresh runtime artifact directory");
    let expectation_bytes = fs::read(configured("PARHELION_MARATHON_RUNTIME_CASES")?)?;
    let expected: Value = serde_json::from_slice(&expectation_bytes)?;
    let cases = expected["entities"]
        .as_array()
        .context("Independent entity expectations")?;
    ensure!(!cases.is_empty(), "Required graph corpus is empty");
    let entities = cases
        .iter()
        .map(|case| {
            Ok(u32::try_from(
                case["entity"].as_u64().context("Source entity")?,
            )?)
        })
        .collect::<Result<Vec<_>>>()?;
    let mut source = Reader::marathon(&packages, &output.join("source"))?;
    let report = marathon::inspect_runtime(&mut source, &entities, &output.join("runtime"))?;
    let report_file = output.join("runtime.json");
    fs::write(&report_file, serde_json::to_vec_pretty(&report)?)?;
    let saved: Value = serde_json::from_slice(&fs::read(&report_file)?)?;
    let actual = saved["entities"]
        .as_array()
        .context("Exported entity reports")?;
    ensure!(
        actual.len() == cases.len(),
        "Entity export coverage differs"
    );
    let mut receipts = Vec::new();
    for case in cases {
        let actual = actual
            .iter()
            .find(|entry| entry["entity"] == case["entity"])
            .context("Expected entity was not exported")?;
        ensure!(
            actual["graph"] == case["graph"],
            "Exported connection topology differs for {}",
            case["entity"]
        );
        ensure!(
            actual["interfaces"] == case["interfaces"],
            "Provider dispatch differs for {}",
            case["entity"]
        );
        ensure!(
            case.get("animation_driver").is_some(),
            "Independent driver expectations are absent"
        );
        ensure!(
            actual["animation_driver"] == case["animation_driver"],
            "Animation selected an unconnected or different component for {}",
            case["entity"]
        );
        let payloads = actual["payloads"].as_array().context("Exported payloads")?;
        let expected_payloads = case["payloads"]
            .as_object()
            .context("Independent payload digests")?;
        ensure!(
            payloads.len() == expected_payloads.len(),
            "Exported reference closure differs"
        );
        let mut exported = BTreeMap::new();
        for payload in payloads {
            let tag = payload["tag"].as_u64().context("Payload tag")?;
            let bytes = fs::read(
                output
                    .join("runtime")
                    .join(payload["file"].as_str().context("Payload file")?),
            )?;
            let sha = hex::encode(Sha256::digest(&bytes));
            let expected_sha = expected_payloads
                .get(&format!("{tag:08X}"))
                .context("Unexpected reference export")?;
            ensure!(
                expected_sha.as_str() == Some(sha.as_str()),
                "Exported source payload {tag:08X} differs from independent readback"
            );
            exported.insert(u32::try_from(tag)?, Payload(bytes));
        }
        let entity = u32::try_from(case["entity"].as_u64().context("Expected entity")?)?;
        let graph =
            Graph::read_marathon(exported.get(&entity).context("Exported entity payload")?)?;
        graph.validate_owners(&exported)?;
        receipts.push(json!({"entity":case["entity"],"payloads":payloads.len(),
            "animation_driver":actual["animation_driver"],
            "connections":actual["graph"]["connections"].as_array().unwrap().len(),
            "named_connections":actual["graph"]["named_connections"].as_array().unwrap().len()}));
    }
    // The inspector is read-only even on invalid requests, and an empty survey cannot succeed.
    ensure!(
        marathon::inspect_runtime(&mut source, &[], &output.join("empty")).is_err(),
        "Empty graph survey succeeded"
    );
    source.finish()?;
    let source_manifest: Value =
        serde_json::from_slice(&fs::read(source.output.join("source-manifest.json"))?)?;
    ensure!(
        package_stamp(&packages)? == source_stamp,
        "Mounted source changed"
    );
    fs::write(
        output.join("verified-runtime-graphs.json"),
        serde_json::to_vec_pretty(&json!({
            "revision":revision,"test_binary_sha256":test_binary_sha256,
            "converter_revision":parhelion_import::CONVERTER_REVISION,
            "source":source_manifest,"source_stamp":source_stamp,
            "cases":receipts,"expectations_sha256":hex::encode(Sha256::digest(&expectation_bytes)),
            "report_sha256":hex::encode(Sha256::digest(fs::read(report_file)?)),
            "limits":"Independent source topology, dispatch metadata and persisted reference closure. No source method, selector, target cache or gameplay was executed."
        }))?,
    )?;
    Ok(())
}

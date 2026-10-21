//! Public catalog intake compared with an independently decoded source witness.
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, env, fs, path::PathBuf, time::Instant};

fn configured(name: &str) -> Result<PathBuf> {
    env::var_os(name)
        .map(PathBuf::from)
        .with_context(|| format!("Set {name}"))
}

fn identity(row: &Value) -> Result<(String, u64)> {
    Ok((
        row["cache"].as_str().context("Cache identity")?.to_owned(),
        row["datum"].as_u64().context("Object datum")?,
    ))
}

#[test]
#[ignore = "Requires PARHELION_REACH_MAPS, PARHELION_REACH_CATALOG_WITNESS and PARHELION_REACH_CATALOG_OUTPUT"]
fn catalog_preserves_source_objects_and_damage_destinations() -> Result<()> {
    let maps = configured("PARHELION_REACH_MAPS")?;
    let witness_bytes = fs::read(configured("PARHELION_REACH_CATALOG_WITNESS")?)?;
    let witness: Value = serde_json::from_slice(&witness_bytes)?;
    let expected = witness["objects"]
        .as_array()
        .filter(|r| !r.is_empty())
        .context("Required independent object corpus is empty")?;
    let output = configured("PARHELION_REACH_CATALOG_OUTPUT")?;
    ensure!(!output.exists(), "Use a fresh catalog artifact directory");
    fs::create_dir_all(&output)?;
    let started = Instant::now();
    let catalog = parhelion_import::halo_reach::catalog(&maps)?;
    let seconds = started.elapsed().as_secs_f64();
    let bytes = serde_json::to_vec_pretty(&catalog)?;
    fs::write(output.join("catalog.json"), &bytes)?;
    let reopened: Value = serde_json::from_slice(&fs::read(output.join("catalog.json"))?)?;
    let objects = reopened["objects"].as_array().context("Catalog objects")?;
    let mut actual = BTreeMap::new();
    for row in objects {
        let key = identity(&json!({"cache":row["cache"],"datum":row["object"]["tag"]["datum"]}))?;
        ensure!(
            actual.insert(key, &row["object"]).is_none(),
            "Duplicate catalog identity"
        );
    }
    ensure!(
        actual.len() == expected.len(),
        "Catalog object coverage differs"
    );
    let mut transfer_count = 0;
    let mut inactive_outside = 0;
    for row in expected {
        let key = identity(row)?;
        let object = actual
            .remove(&key)
            .with_context(|| format!("Missing {key:?}"))?;
        ensure!(
            object["tag"]["path"] == row["path"] && object["tag"]["group"] == row["group"],
            "Object identity differs for {key:?}"
        );
        if row["group"] != "vehi" {
            continue;
        }
        let sections = object["gameplay"]["damage"]["sections"]
            .as_array()
            .context("Vehicle sections")?;
        ensure!(
            sections.len() as u64 == row["sections"].as_u64().context("Witness sections")?,
            "Section count differs for {key:?}"
        );
        let mut transfers = Vec::new();
        for (index, section) in sections.iter().enumerate() {
            for field in ["damage_transfers", "health_transfers"] {
                transfers.push(json!({"section":index,"field":field,"rows":section[field]}));
            }
            for (response, value) in section["responses"]
                .as_array()
                .context("Responses")?
                .iter()
                .enumerate()
            {
                transfers.push(json!({"section":index,"response":response,"field":"damage_transfers","rows":value["damage_transfers"]}));
            }
        }
        ensure!(
            json!(transfers) == row["transfers"],
            "Raw transfer fields differ for {key:?}"
        );
        for group in &transfers {
            for transfer in group["rows"].as_array().context("Transfers")? {
                transfer_count += 1;
                if transfer["flags"].as_u64().context("Flags")? & 1 == 0
                    && transfer["section"].as_i64().context("Section")? >= sections.len() as i64
                {
                    inactive_outside += 1;
                }
            }
        }
    }
    let caches = reopened["caches"].as_array().context("Catalog caches")?;
    let coverage = caches
        .iter()
        .map(|c| json!({"cache":c["cache"],"build":c["build"],"exclusion":c["exclusion"]}))
        .collect::<Vec<_>>();
    ensure!(
        json!(coverage) == witness["caches"],
        "Cache coverage or exclusions differ"
    );
    ensure!(transfer_count > 0, "Required transfer corpus is empty");
    fs::write(
        output.join("verification.json"),
        serde_json::to_vec_pretty(&json!({
            "passed":true,"objects":expected.len(),"caches":caches.len(),"transfers":transfer_count,
            "inactive_outside_section_table":inactive_outside,"catalog_seconds":seconds,
            "catalog_sha256":hex::encode(Sha256::digest(&bytes)),
            "witness_sha256":hex::encode(Sha256::digest(&witness_bytes)),
            "limits":"Source catalog intake and persisted readback only. Native damage and gameplay are separate."
        }))?,
    )?;
    Ok(())
}

//! Opt-in whole-resource package oracle, specified before resource emission.
use super::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{env, fs, path::PathBuf};

#[test]
#[ignore = "Requires PARHELION_PROGRAM_CORPUS and PARHELION_PROGRAM_OUTPUT"]
fn complete_native_program_resources() -> Result<()> {
    let corpus = PathBuf::from(env::var_os("PARHELION_PROGRAM_CORPUS").context("program corpus")?);
    let output = PathBuf::from(env::var_os("PARHELION_PROGRAM_OUTPUT").context("program output")?);
    let manifest: Value = serde_json::from_slice(&fs::read(corpus.join("golden-pairs.json"))?)?;
    ensure!(
        !manifest
            .as_array()
            .context("program resource pairs")?
            .is_empty(),
        "program corpus has no native golden pairs"
    );
    let mut rows = Vec::new();
    let mut rejected = 0;
    for pair in manifest.as_array().context("program resource pairs")? {
        let hexadecimal = |v: &Value| -> Result<u32> {
            Ok(u32::from_str_radix(v.as_str().context("program hex")?, 16)?)
        };
        let source_name = pair["source"].as_str().context("source tag")?;
        let native_name = pair["native"].as_str().context("native tag")?;
        let source = Payload(fs::read(corpus.join(format!("modern/{source_name}.bin")))?);
        let native = fs::read(corpus.join(format!("shadowkeep/{native_name}.bin")))?;
        let class = hexadecimal(&pair["class"])?;
        let tags = pair["references"]
            .as_object()
            .context("program references")?
            .iter()
            .map(|(k, v)| Ok((u32::from_str_radix(k, 16)?, hexadecimal(v)?)))
            .collect::<Result<BTreeMap<_, _>>>()?;
        let converted = emit(&source, class, &tags)?;
        ensure!(
            converted.payload.0 == native,
            "complete native resource {source_name} differs"
        );
        ensure!(
            converted.references.iter().all(|row| row.target.is_some()),
            "dependency binding missing"
        );
        let mut malformed = source.clone();
        malformed.0[0] ^= 1;
        ensure!(
            emit(&malformed, class, &tags).is_err(),
            "malformed resource size accepted"
        );
        rejected += 1;
        let first_program = pair["first_program"].as_u64().context("program offset")? as usize;
        let mut malformed = source.clone();
        malformed.0[first_program + 32..first_program + 40].copy_from_slice(&0u64.to_le_bytes());
        ensure!(
            emit(&malformed, class, &tags).is_err(),
            "invalid program inputs accepted"
        );
        rejected += 1;
        rows.push((
            source_name.to_string(),
            converted.payload.0.clone(),
            json!({
                "source":source_name,"native":native_name,"programs":converted.programs,
                "native_class":format!("{:08X}",converted.class),
                "sha256":hex::encode(Sha256::digest(&converted.payload.0)),
                "references":converted.references
            }),
        ));
    }
    let source = Payload(fs::read(corpus.join("modern/80CEA523.bin"))?);
    let pending = emit(&source, 0x808031DE, &BTreeMap::new())?;
    ensure!(
        pending.references.len() == 2 && pending.references.iter().all(|row| row.target.is_none()),
        "unconverted dependencies were silently bound"
    );
    for row in &pending.references {
        ensure!(
            pending.payload.u32(row.offset)? == u32::MAX,
            "unbound native reference is live"
        );
    }
    let mut bad = Payload(fs::read(corpus.join("modern/80CEA528.bin"))?);
    bad.0[12..16].copy_from_slice(&0f32.to_le_bytes());
    ensure!(
        emit(&bad, 0x808031D8, &BTreeMap::new()).is_err(),
        "active source-only scalar accepted"
    );
    rejected += 1;
    fs::create_dir_all(&output)?;
    for (name, bytes, _) in &rows {
        fs::write(output.join(format!("{name}.bin")), bytes)?;
    }
    fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&json!({
            "rows": rows.iter().map(|(_,_,report)|report).collect::<Vec<_>>(),
            "rejection_checks":rejected,"installable":false,"gameplay_verified":false
        }))?,
    )?;
    Ok(())
}

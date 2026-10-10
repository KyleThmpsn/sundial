//! Opt-in complete-bank oracle, specified before adding automated paths.
use anyhow::{Context, Result, ensure};
use parhelion_import::d2_mot::lower_audio_bank;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, env, fs, path::PathBuf};

fn objects(bytes: &[u8]) -> Result<BTreeMap<u32, (u8, &[u8])>> {
    let word = |at: usize| -> Result<u32> {
        Ok(u32::from_le_bytes(
            bytes.get(at..at + 4).context("bank word")?.try_into()?,
        ))
    };
    let mut at = 0;
    while at < bytes.len() {
        let size = usize::try_from(word(at + 4)?)?;
        let end = at.checked_add(8 + size).context("bank chunk overflow")?;
        ensure!(end <= bytes.len(), "bank chunk extent");
        if &bytes[at..at + 4] == b"HIRC" {
            let count = word(at + 8)?;
            at += 12;
            let mut rows = BTreeMap::new();
            for _ in 0..count {
                let kind = bytes[at];
                let len = usize::try_from(word(at + 1)?)?;
                let payload = bytes.get(at + 5..at + 5 + len).context("HIRC extent")?;
                let id = u32::from_le_bytes(payload.get(..4).context("HIRC id")?.try_into()?);
                ensure!(
                    rows.insert(id, (kind, payload)).is_none(),
                    "duplicate HIRC id"
                );
                at += 5 + len;
            }
            ensure!(at == end, "HIRC trailing data");
            return Ok(rows);
        }
        at = end;
    }
    anyhow::bail!("HIRC missing")
}

#[test]
#[ignore = "Requires PARHELION_AUDIO_PATH_CORPUS and PARHELION_AUDIO_PATH_OUTPUT"]
fn automated_paths_survive_complete_bank_conversion() -> Result<()> {
    let corpus = PathBuf::from(env::var_os("PARHELION_AUDIO_PATH_CORPUS").context("path corpus")?);
    let output = PathBuf::from(env::var_os("PARHELION_AUDIO_PATH_OUTPUT").context("path output")?);
    let manifest: Value = serde_json::from_slice(&fs::read(corpus.join("pairs.json"))?)?;
    ensure!(
        !manifest.as_array().context("path pairs")?.is_empty(),
        "audio path corpus has no native pairs"
    );
    fs::create_dir_all(&output)?;
    let mut report = Vec::new();
    let mut rejected = 0;
    for row in manifest.as_array().context("path pairs")? {
        let name = row["source"].as_str().context("source")?;
        let source = fs::read(corpus.join(format!("{name}.bin")))?;
        let native =
            fs::read(corpus.join(format!("{}.bin", row["native"].as_str().context("native")?)))?;
        let id = u32::try_from(row["object"].as_u64().context("object")?)?;
        let start = usize::try_from(row["native_start"].as_u64().context("native start")?)?;
        let len = usize::try_from(row["native_length"].as_u64().context("native length")?)?;
        let expected = native
            .get(start..start + len)
            .context("native positioning")?;
        ensure!(!expected.is_empty(), "native path control is empty");
        let converted = lower_audio_bank(&source)?;
        let rows = objects(&converted.bytes)?;
        let (kind, payload) = rows.get(&id).context("converted path owner missing")?;
        ensure!(*kind == 7, "path owner type differs");
        ensure!(
            payload
                .windows(expected.len())
                .filter(|v| *v == expected)
                .count()
                == 1,
            "native path parameters differ"
        );
        let original = objects(&source)?;
        ensure!(
            original.keys().all(|id| rows.contains_key(id)),
            "source object dropped"
        );
        for (field, value) in [
            ("vertices", 0u32),
            ("playlist_offset", u32::MAX),
            ("transition", u32::MAX),
        ] {
            let mut bad = source.clone();
            let at = usize::try_from(row[field].as_u64().context("malformed field")?)?;
            bad[at..at + 4].copy_from_slice(&value.to_le_bytes());
            ensure!(
                lower_audio_bank(&bad).is_err(),
                "malformed automated path accepted"
            );
            rejected += 1;
        }
        let file = format!("{name}.bnk");
        fs::write(output.join(&file), &converted.bytes)?;
        report.push(json!({"source":name,"native":row["native"],"file":file,
                          "sha256":hex::encode(Sha256::digest(&converted.bytes)),
                          "path_bytes":len,"source_objects":original.len(),
                          "native_objects":rows.len()}));
    }
    fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&json!({
            "rows":report,"rejection_checks":rejected,"installable":false,
            "gameplay_verified":false
        }))?,
    )?;
    Ok(())
}

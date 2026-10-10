//! Opt-in complete-owner oracle, specified before response emission.
use super::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{env, fs, path::PathBuf};

fn hex(value: &Value) -> Result<u32> {
    Ok(u32::from_str_radix(
        value.as_str().context("response hexadecimal value")?,
        16,
    )?)
}

#[test]
#[ignore = "Requires PARHELION_RESPONSE_CORPUS and PARHELION_RESPONSE_OUTPUT"]
fn complete_response_owners_and_pending_gates() -> Result<()> {
    let corpus =
        PathBuf::from(env::var_os("PARHELION_RESPONSE_CORPUS").context("response corpus")?);
    let output =
        PathBuf::from(env::var_os("PARHELION_RESPONSE_OUTPUT").context("response output")?);
    let load = |path: PathBuf| -> Result<Payload> { Ok(Payload(fs::read(path)?)) };
    let read_json =
        |path: PathBuf| -> Result<Value> { Ok(serde_json::from_slice(&fs::read(path)?)?) };
    let category = read_json(corpus.join("assembly-verification/category-correspondence.json"))?;
    let names = category["source"]["names"]
        .as_array()
        .context("response names")?
        .iter()
        .map(|name| Ok(u32::try_from(name.as_u64().context("category hash")?)?))
        .collect::<Result<Vec<_>>>()?;
    let mut categories = vec![None; names.len()];
    for (source, native) in category["mapping"]
        .as_object()
        .context("response categories")?
    {
        categories[source.parse::<usize>()?] =
            Some(u16::try_from(native.as_u64().context("category index")?)?);
    }
    let hashes = read_json(corpus.join("collision-corpus/tag64.json"))?["hashes"]
        .as_object()
        .context("response hashes")?
        .iter()
        .map(|(k, v)| Ok((u64::from_str_radix(k, 16)?, hex(v)?)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    let mut base_tags = BTreeMap::new();
    for file in fs::read_dir(corpus.join("assembly-verification/source-category/raw"))? {
        let path = file?.path();
        if path.extension().is_some_and(|ext| ext == "bin") {
            let source = u32::from_str_radix(
                path.file_stem()
                    .context("category tag")?
                    .to_str()
                    .context("category tag")?,
                16,
            )?;
            base_tags.insert(source, 0x80C70CA1);
        }
    }
    let mut resources = Resources {
        tags: base_tags.clone(),
        hashes,
        categories,
        names,
    };
    let allocation = load(corpus.join("network-corpus/shadowkeep/80C70B90.bin"))?;
    let pairs = read_json(corpus.join("collision-corpus/golden-pairs.json"))?;
    ensure!(
        !pairs.as_array().context("response owner pairs")?.is_empty(),
        "response corpus has no native golden pairs"
    );
    let mut results = Vec::new();
    let mut payloads = Vec::new();
    let mut rejected = 0;
    for pair in pairs.as_array().context("response owner pairs")? {
        let source_name = pair["source"].as_str().context("source owner")?;
        let native_name = pair["native"].as_str().context("native owner")?;
        let source = load(corpus.join(format!("collision-corpus/modern/{source_name}.bin")))?;
        let template = load(corpus.join(format!("collision-corpus/shadowkeep/{native_name}.bin")))?;
        resources.tags = base_tags.clone();
        for (source, target) in pair["tags"]
            .as_object()
            .context("response reference bindings")?
        {
            resources
                .tags
                .insert(u32::from_str_radix(source, 16)?, hex(target)?);
        }
        let native_tag = u32::from_str_radix(native_name, 16)?;
        let native = emit(
            &source,
            &template,
            &allocation,
            &resources,
            native_tag,
            0x80C70B90,
        )?;
        ensure!(
            native.owner.0 == template.0,
            "complete response owner {source_name} differs"
        );
        ensure!(
            native.gates.is_empty() && native.references.iter().all(|row| row.target.is_some()),
            "complete response owner has pending fields"
        );
        for at in [0, source.pointer(16)? + 16, source.pointer(0x68)? + 8] {
            let mut malformed = source.clone();
            malformed.0[at] ^= 1;
            ensure!(
                emit(
                    &malformed,
                    &template,
                    &allocation,
                    &resources,
                    native_tag,
                    0x80C70B90
                )
                .is_err(),
                "invalid response owner accepted"
            );
            rejected += 1;
        }
        results.push(json!({"source":source_name,"native":native_name,
            "sha256":hex::encode(Sha256::digest(&native.owner.0)),"references":native.references,
            "group_aliases":native.group_aliases}));
        payloads.push((format!("{source_name}.bin"), native.owner.0));
    }
    resources.tags = base_tags;
    resources.tags.insert(0x80C3A168, 0x81FE010D);
    let source = load(corpus.join("collision-corpus/modern/80CED9E2.bin"))?;
    let template = load(corpus.join("collision-corpus/shadowkeep/80FB547F.bin"))?;
    let pending = emit(
        &source,
        &template,
        &allocation,
        &resources,
        0x81FE0106,
        0x81FE0107,
    )?;
    ensure!(pending.gates.len() == 2, "source category gates were lost");
    ensure!(
        pending
            .references
            .iter()
            .filter(|r| r.target.is_none())
            .count()
            == 6,
        "response sound dependencies were silently bound"
    );
    for row in pending.references.iter().filter(|r| r.target.is_none()) {
        ensure!(
            pending.owner.u32(row.offset)? == u32::MAX,
            "unconverted reference is live"
        );
    }
    resources.hashes.clear();
    ensure!(
        emit(
            &source,
            &template,
            &allocation,
            &resources,
            0x81FE0106,
            0x81FE0107
        )
        .is_err(),
        "missing response hash lookup accepted"
    );
    rejected += 1;
    fs::create_dir_all(&output)?;
    for (name, payload) in payloads {
        fs::write(output.join(name), payload)?;
    }
    fs::write(output.join("pending-owner.bin"), &pending.owner.0)?;
    fs::write(output.join("pending-allocation.bin"), &pending.allocation.0)?;
    fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&json!({
            "pairs":results,"rejection_checks":rejected,"pending_gates":pending.gates,
            "pending_references":pending.references,"group_aliases":pending.group_aliases,
            "installable":false,"gameplay_verified":false
        }))?,
    )?;
    Ok(())
}

mod collision;

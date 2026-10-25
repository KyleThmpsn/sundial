//! Complete resource oracle specified before implementing selector emission.
use super::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{env, fs, path::PathBuf};

mod external;
mod factions;
mod names;
mod private;

fn hex(value: &Value) -> Result<u32> {
    Ok(u32::from_str_radix(
        value.as_str().context("selector hexadecimal value")?,
        16,
    )?)
}

#[test]
#[ignore = "Requires PARHELION_SELECTOR_CORPUS and PARHELION_SELECTOR_OUTPUT"]
fn complete_selector_resources_and_pending_dependencies() -> Result<()> {
    let root = PathBuf::from(env::var_os("PARHELION_SELECTOR_CORPUS").context("selector corpus")?);
    let output =
        PathBuf::from(env::var_os("PARHELION_SELECTOR_OUTPUT").context("selector output")?);
    let corpus = root.join("damage-corpus/selectors");
    let load = |path: PathBuf| -> Result<Payload> { Ok(Payload(fs::read(path)?)) };
    let read_json =
        |path: PathBuf| -> Result<Value> { Ok(serde_json::from_slice(&fs::read(path)?)?) };
    let correspondence =
        read_json(root.join("assembly-verification/category-correspondence.json"))?;
    let names = correspondence["source"]["names"]
        .as_array()
        .context("selector category names")?
        .iter()
        .map(|name| Ok(u32::try_from(name.as_u64().context("category name")?)?))
        .collect::<Result<Vec<_>>>()?;
    let mut categories = vec![None; names.len()];
    for (source, target) in correspondence["mapping"]
        .as_object()
        .context("category indices")?
    {
        categories[source.parse::<usize>()?] =
            Some(u16::try_from(target.as_u64().context("category index")?)?);
    }
    let mut resources = Resources {
        tags: BTreeMap::new(),
        names,
        categories,
    };
    let pairs = read_json(corpus.join("golden-pairs.json"))?;
    let mut rows = Vec::new();
    let mut files = Vec::new();
    let mut rejected = 0;
    let mut pending_dictionaries = None;
    let mut pending_categories = None;
    for pair in pairs.as_array().context("selector pairs")? {
        let source_name = pair["source"].as_str().context("source selector")?;
        let native_name = pair["native"].as_str().context("native selector")?;
        let source = load(corpus.join(format!("modern/{source_name}.bin")))?;
        let expected = load(corpus.join(format!("shadowkeep/{native_name}.bin")))?;
        resources.tags = pair["tags"]
            .as_object()
            .context("dictionary fixtures")?
            .iter()
            .map(|(key, value)| Ok((u32::from_str_radix(key, 16)?, hex(value)?)))
            .collect::<Result<BTreeMap<_, _>>>()?;
        let converted = emit(&source, &resources)?;
        ensure!(
            converted.payload.0 == expected.0,
            "complete selector {source_name} differs"
        );
        ensure!(
            converted.gates.is_empty() && converted.references.iter().all(|r| r.target.is_some()),
            "complete selector has unrepresented dependencies"
        );
        for at in [0, 8, source.pointer(24)? + 8] {
            let mut invalid = source.clone();
            invalid.0[at] ^= 0x10;
            ensure!(
                emit(&invalid, &resources).is_err(),
                "invalid selector accepted"
            );
            rejected += 1;
        }
        if pending_dictionaries.is_none() && !converted.references.is_empty() {
            let saved = std::mem::take(&mut resources.tags);
            let pending = emit(&source, &resources)?;
            ensure!(
                pending.references.iter().all(|r| r.target.is_none()),
                "dictionary silently bound"
            );
            for reference in &pending.references {
                ensure!(
                    pending.payload.u32(reference.offset)? == u32::MAX,
                    "unset dictionary is live"
                );
            }
            pending_dictionaries = Some(pending);
            resources.tags = saved;
        }
        if pending_categories.is_none() {
            let saved =
                std::mem::replace(&mut resources.categories, vec![None; resources.names.len()]);
            let pending = emit(&source, &resources)?;
            if !pending.gates.is_empty() {
                pending_categories = Some(pending);
            }
            resources.categories = saved;
        }
        rows.push(json!({"source": source_name, "native":native_name,
            "sha256":hex::encode(Sha256::digest(&converted.payload.0)),
            "references":converted.references,"group_aliases":converted.group_aliases}));
        files.push((format!("{source_name}.bin"), converted.payload.0));
    }
    let missing_dictionaries = pending_dictionaries.context("dictionary dependency case")?;
    let missing_categories = pending_categories.context("category dependency case")?;
    let source = load(corpus.join("modern/80CEBA39.bin"))?;
    let private = emit(&source, &resources)?;
    ensure!(
        private.references.is_empty() && private.gates.is_empty(),
        "unexpected selector dependency"
    );
    let pending = emit(&load(corpus.join("modern/80CEBA3A.bin"))?, &resources)?;
    ensure!(
        pending.gates.iter().any(|gate| gate.name == 0x9C6C4BA0),
        "source-only category silently bound"
    );
    ensure!(!output.exists(), "selector output already exists");
    fs::create_dir_all(&output)?;
    for (name, bytes) in files {
        fs::write(output.join(name), bytes)?;
    }
    fs::write(output.join("private-selector.bin"), &private.payload.0)?;
    fs::write(
        output.join("pending-dictionaries.bin"),
        &missing_dictionaries.payload.0,
    )?;
    fs::write(
        output.join("pending-categories.bin"),
        &missing_categories.payload.0,
    )?;
    fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&json!({
            "pairs":rows,"rejection_checks":rejected,
            "unrepresented_dictionaries":missing_dictionaries.references,
            "unrepresented_categories":missing_categories.gates,
            "private_selector_sha256":hex::encode(Sha256::digest(&private.payload.0)),
            "private_group_aliases":private.group_aliases,
            "unrepresented_dictionary_group_aliases":missing_dictionaries.group_aliases,
            "unrepresented_category_group_aliases":missing_categories.group_aliases,
            "installable":false,"gameplay_verified":false
        }))?,
    )?;
    Ok(())
}

mod fragment;

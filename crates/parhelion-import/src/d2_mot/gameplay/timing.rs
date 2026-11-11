//! The source weapon's firing and reload curves, translated onto the base's stat translator by
//! `crate::tiger::weapon::timing`.
//!
//! Modern binding names differ from Shadowkeep's, so the source translator is the one runtime
//! component whose definition has the modern table shape. Its table is selected by the source
//! pattern row's translation group, a name both versions share. Only literal source stats feed
//! literal routes, since conditional stats are not carried into the recipe. The record goes into
//! the graph as `firing_translation.timing`. The package build applies its edits to a private
//! copy of the base's translator after checking that the base still matches.
use crate::d2_mot::{
    payload::Payload,
    reader::{Reader, write_json},
};
use crate::tiger::weapon::timing::{MONUMENT_OF_TRIUMPH, Translator, translate};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{fs, path::Path};

/// The modern component owner class.
const OWNER: u32 = 0x8080_9B06;

fn load(path: &Path) -> Result<Value> {
    serde_json::from_slice(&fs::read(path).with_context(|| format!("Read {}", path.display()))?)
        .map_err(Into::into)
}

fn hex(value: &Value, what: &str) -> Result<u32> {
    u32::from_str_radix(value.as_str().with_context(|| what.to_owned())?, 16)
        .with_context(|| what.to_owned())
}

/// The runtime entity's own stat translator.
fn translator(r: &mut Reader, rig: &Value) -> Result<(u32, Payload)> {
    let mut found = Vec::new();
    for component in rig["components"]
        .as_array()
        .context("Source runtime components")?
    {
        if component["entity"] != rig["runtime_entity"] {
            continue;
        }
        let owner = hex(&component["owner"], "Source component owner")?;
        let p = r.tag(owner, Some(OWNER))?;
        if MONUMENT_OF_TRIUMPH.holds_table(&p) && !found.iter().any(|(tag, _)| *tag == owner) {
            found.push((owner, (*p).clone()));
        }
    }
    ensure!(
        found.len() == 1,
        "Source firing translator is missing or ambiguous"
    );
    found.pop().context("Source firing translator")
}

fn prepare(modern: &Path, native: &Path, source: &Path, donor: u32, work: &Path) -> Result<Value> {
    let rig = load(&source.join("rig.json"))?;
    let gameplay = load(&source.join("gameplay.json"))?;
    let mut m = Reader::discovery(modern, &work.join("source"), true)?;
    let (owner, payload) = translator(&mut m, &rig)?;
    let index = usize::try_from(
        rig["pattern_index"]
            .as_u64()
            .context("Source pattern index")?,
    )?;
    let key = crate::tiger::pattern::at(&mut m, index, true)?.translation_group;
    let stats = gameplay["stats"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|stat| stat["literal"] == true)
        .filter_map(|stat| {
            let hash = u32::try_from(stat["hash"].as_u64()?).ok()?;
            Some((hash, stat["value"].as_f64()?))
        })
        .collect::<Vec<_>>();
    let translator = Translator {
        owner,
        payload,
        key,
        dialect: &MONUMENT_OF_TRIUMPH,
    };
    let mut n = Reader::discovery(native, &work.join("native"), false)?;
    translate(&translator, &mut n, donor, &stats)
}

/// Translate the source timing for the base `donor` and record it in the graph. A refusal keeps
/// the base's own curves and records why, so the import still succeeds.
pub fn apply(
    modern: &Path,
    native: &Path,
    source: &Path,
    donor: u32,
    work: &Path,
    graph: &Path,
) -> Result<Value> {
    let record = match prepare(modern, native, source, donor, work) {
        Ok(report) => report,
        Err(error) => {
            crate::cancellation::check()?;
            if crate::cancellation::is_cancelled(&error) {
                return Err(error);
            }
            json!({"status":"unsupported","reason":format!("{error:#}"),"gameplay_verified":false})
        }
    };
    let manifest = graph.join("asset-graph.json");
    let mut document = load(&manifest)?;
    document["firing_translation"]["timing"] = record.clone();
    write_json(&manifest, &document)?;
    Ok(record)
}

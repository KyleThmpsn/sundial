use super::{Input, Request, TARGET_BUILD, plan, source};
use crate::{
    cancellation,
    d2_mot::{
        ability::source::{number, tag},
        reader::{Reader, package_stamp},
    },
    io,
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn report(value: Value) -> Result<Value> {
    let input: Input = serde_json::from_value(value.clone()).context("Subclass planning input")?;
    let plan = plan(&input)?;
    Ok(json!({
        "format": 1,
        "tool_version": env!("CARGO_PKG_VERSION"),
        "target_build": TARGET_BUILD,
        "installable": false,
        "gameplay_verified": false,
        "input_sha256": digest(&serde_json::to_vec(&value)?),
        "input": value,
        "plan": plan
    }))
}

fn publish(path: &Path, value: &Value) -> Result<()> {
    cancellation::check()?;
    ensure!(
        path.extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("json")),
        "Research artifacts must be JSON files"
    );
    ensure!(
        !path.exists(),
        "Research artifact already exists: {}",
        path.display()
    );
    let bytes = serde_json::to_vec_pretty(value)?;
    let parent = path.parent().context("Artifact parent directory")?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(&bytes)?;
    temporary.as_file().sync_all()?;
    cancellation::check()?;
    temporary
        .persist_noclobber(path)
        .map_err(|error| error.error)?;
    Ok(())
}

/// Recompute a research plan from a retained input document. Existing outputs are never replaced.
/// The input is preserved verbatim as JSON data, including unknown source trace fields.
pub fn plan_file(input: &Path, output: &Path) -> Result<()> {
    cancellation::check()?;
    let input = io::resolved(input)?;
    let output = io::resolved(output)?;
    ensure!(input != output, "Planning output cannot replace its input");
    let value = serde_json::from_slice(&fs::read(input)?)?;
    publish(&output, &report(value)?)
}

fn payload_evidence(reader: &Reader) -> Result<Vec<source::PayloadEvidence>> {
    let manifest: Value =
        serde_json::from_slice(&fs::read(reader.output.join("source-manifest.json"))?)?;
    let mut result = Vec::new();
    for (name, entry) in manifest["tags"]
        .as_object()
        .context("Source manifest tags")?
    {
        let tag = tag(name)?;
        let reference = number(&entry["reference"], "payload reference")?;
        // Derive filenames from checked tags rather than trusting retained manifest paths.
        let bytes = fs::read(reader.output.join("raw").join(format!("{tag:08X}.bin")))?;
        ensure!(
            entry["size"].as_u64() == Some(bytes.len() as u64),
            "Captured payload size changed"
        );
        result.push(source::PayloadEvidence {
            tag: format!("{tag:08X}"),
            reference,
            file_type: number(&entry["type"], "payload type")?,
            file_subtype: number(&entry["subtype"], "payload subtype")?,
            bytes: bytes.len() as u64,
            sha256: digest(&bytes),
        });
    }
    ensure!(!result.is_empty(), "Source capture read no payloads");
    Ok(result)
}

/// Capture selected source plugs from mounted modern packages and publish a replayable plan.
/// `output` must be a fresh directory outside the source installation. A failed capture can
/// leave diagnostic raw reads, but only a complete capture publishes `plan.json`.
/// This does not discover subclasses, lower ability banks, produce a recipe or install content.
pub fn inspect(packages: &Path, request: &Request, output: &Path) -> Result<PathBuf> {
    cancellation::check()?;
    request.validate()?;
    let packages = packages.canonicalize()?;
    let output = io::outside(output, packages.parent().unwrap_or(&packages))?;
    let stamp = package_stamp(&packages)?;
    fs::create_dir(&output)
        .context("Use a fresh subclass research directory with an existing parent")?;
    let mut reader = Reader::new(&packages, &output, true)?;
    let mut capture = source::capture(&mut reader, request, stamp.clone())?;
    capture.payloads = payload_evidence(&reader)?;
    ensure!(
        package_stamp(&packages)? == stamp,
        "Source packages changed during subclass capture"
    );
    let input = json!({"request": request, "capture": capture});
    let report = report(input.clone())?;
    publish(&output.join("input.json"), &input)?;
    let path = output.join("plan.json");
    publish(&path, &report)?;
    Ok(path)
}

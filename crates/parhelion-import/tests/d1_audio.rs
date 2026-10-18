//! Mounted source cues through complete media conversion and persisted graph readback.
use anyhow::{Context, Result, ensure};
use parhelion_import::{
    d1_roi::Source,
    tiger::{audio::bank::Bank, payload::Payload},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, env, fs, path::PathBuf};

fn chunks(bytes: &[u8]) -> Result<BTreeMap<&[u8], &[u8]>> {
    ensure!(
        bytes.get(..4) == Some(b"RIFF") && bytes.get(8..12) == Some(b"WAVE"),
        "audio signature"
    );
    let mut at = 12;
    let mut rows = BTreeMap::new();
    while at < bytes.len() {
        let id = bytes.get(at..at + 4).context("chunk identity")?;
        let size = u32::from_le_bytes(
            bytes
                .get(at + 4..at + 8)
                .context("chunk size")?
                .try_into()?,
        ) as usize;
        let end = at.checked_add(8 + size).context("chunk overflow")?;
        ensure!(
            rows.insert(id, bytes.get(at + 8..end).context("chunk payload")?)
                .is_none(),
            "duplicate chunk"
        );
        at = end + if end < bytes.len() { size & 1 } else { 0 };
    }
    ensure!(at == bytes.len(), "chunk alignment");
    Ok(rows)
}

#[test]
#[ignore = "Requires configured source/native packages, PARHELION_D1_AUDIO_CUES and a fresh PARHELION_D1_AUDIO_OUTPUT"]
fn source_cues_preserve_media_duration_speakers_and_dependency_closure() -> Result<()> {
    let source =
        PathBuf::from(env::var_os("PARHELION_D1_ROI_PACKAGES").context("source packages")?);
    let native =
        PathBuf::from(env::var_os("PARHELION_NATIVE_PACKAGES").context("native packages")?);
    let output = PathBuf::from(env::var_os("PARHELION_D1_AUDIO_OUTPUT").context("audio output")?);
    ensure!(!output.exists(), "audio output must be fresh");
    let cues = env::var("PARHELION_D1_AUDIO_CUES")?
        .split(',')
        .map(|s| u32::from_str_radix(s.trim().trim_start_matches("0x"), 16))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    ensure!(!cues.is_empty(), "required source cue set is empty");
    let source = Source::open(&source, |_, _| {})?;
    let report: Value =
        serde_json::from_slice(&fs::read(source.prepare_audio(&cues, &native, &output)?)?)?;
    let nodes = report["nodes"].as_array().context("audio graph")?;
    let symbols = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| {
            Ok((
                n["symbol"].as_str().context("symbol")?.to_owned(),
                0x81E00000 + u32::try_from(i)?,
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    ensure!(symbols.len() == nodes.len(), "duplicate audio symbol");
    let mut evidence = Vec::new();
    let mut banks = Vec::new();
    for node in nodes {
        if let Some(bank) = node.get("audio_bank") {
            banks.push(serde_json::from_value::<Bank>(bank.clone())?);
        }
        if node["storage"] != "audio_media" {
            continue;
        }
        let tag = u32::try_from(node["source_media_tag"].as_u64().context("source medium")?)?;
        let original = source.read(tag)?;
        let input = chunks(&original.0)?;
        let bytes = fs::read(output.join(node["file"].as_str().context("media file")?))?;
        let decoded = chunks(&bytes)?;
        let before = input.get(&b"fmt "[..]).context("source format")?;
        let after = decoded.get(&b"fmt "[..]).context("native format")?;
        ensure!(
            after[..2] == 0xFFFEu16.to_le_bytes() && before[2..8] == after[2..8],
            "native media codec, channels or rate differs"
        );
        let frames = u32::from_le_bytes(before[24..28].try_into()?) as usize;
        let channels = usize::from(u16::from_le_bytes(after[2..4].try_into()?));
        ensure!(
            before[20..24] == after[20..24] && decoded[&b"data"[..]].len() == frames * channels * 2,
            "native duration or speakers changed"
        );
        evidence.push(json!({"source_tag":tag,"source_sha256":hex::encode(Sha256::digest(&original.0)),
            "file":node["file"],"sha256":hex::encode(Sha256::digest(&bytes)),"frames":frames,"channels":channels}));
    }
    ensure!(
        !evidence.is_empty() && !banks.is_empty(),
        "required media or banks are empty"
    );
    let mut cue_evidence = Vec::new();
    for node in nodes {
        let bytes = fs::read(output.join(node["file"].as_str().context("node file")?))?;
        for patch in node["patches"].as_array().context("node relocations")? {
            let at = usize::try_from(patch["offset"].as_u64().context("relocation offset")?)?;
            symbols
                .get(patch["symbol"].as_str().context("relocation target")?)
                .context("unresolved audio dependency")?;
            ensure!(
                bytes.get(at..at + 4) == Some(&u32::MAX.to_le_bytes()[..]),
                "relocation placeholder"
            );
        }
        if node["audio_events"].is_array() {
            let p = Payload(bytes);
            let tag = u32::try_from(
                node["source_cue_tag"]
                    .as_u64()
                    .context("source cue identity")?,
            )?;
            let original = source.read(tag)?;
            ensure!(
                p.u32(8)? == original.u32(8)?
                    && p.u32(12)? == original.u32(12)?
                    && p.u64(0x18)? == original.u64(0x38)?,
                "source cue event or media slot loss"
            );
            cue_evidence.push(
                json!({"symbol":node["symbol"],"primary":p.u32(8)?,"secondary":p.u32(12)?,
                "media_count":p.u64(0x18)?,"relocations":node["patches"]}),
            );
        }
    }
    ensure!(cue_evidence.len() == cues.len(), "source cue loss");
    fs::write(
        output.join("readback.json"),
        serde_json::to_vec_pretty(&json!({
            "builds":source.report().builds,"source_stamp":source.report().stamp,"media":evidence,
            "cues":cue_evidence,"package_streaming_verified":false,"gameplay_verified":false
        }))?,
    )?;
    Ok(())
}

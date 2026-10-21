//! Compiled mission intake checked against an independent cache-byte witness.
use anyhow::{Context, Result, ensure};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{env, fs, path::PathBuf, time::Instant};

fn configured(name: &str) -> Result<PathBuf> {
    env::var_os(name)
        .map(PathBuf::from)
        .with_context(|| format!("Set {name}"))
}

#[test]
#[ignore = "Requires configured Reach mission audio witness, banks, decoder and fresh output"]
fn mission_recordings_preserve_cues_and_pcm_extents() -> Result<()> {
    use parhelion_import::halo_reach::level::mission::{AudioOptions, export_with_audio};
    let maps = configured("PARHELION_REACH_MAPS")?;
    let witness_bytes = fs::read(configured("PARHELION_REACH_MISSION_AUDIO_WITNESS")?)?;
    let witness: Value = serde_json::from_slice(&witness_bytes)?;
    let corpus = witness["maps"]
        .as_array()
        .filter(|v| !v.is_empty())
        .context("Required mission audio corpus is empty")?;
    let options = AudioOptions {
        banks: serde_json::from_str(&env::var("PARHELION_REACH_AUDIO_BANKS")?)?,
        decoder: configured("PARHELION_REACH_AUDIO_DECODER")?,
    };
    let output = configured("PARHELION_REACH_MISSION_AUDIO_OUTPUT")?;
    ensure!(!output.exists(), "Use a fresh mission audio output");
    fs::create_dir_all(&output)?;
    let started = Instant::now();
    let mut receipts = Vec::new();
    let mut total = 0;
    for expected in corpus {
        let name = expected["cache"].as_str().context("Cache name")?;
        ensure!(
            PathBuf::from(name).components().count() == 1,
            "Cache filename"
        );
        let source = maps.join(name);
        ensure!(
            hex::encode(Sha256::digest(fs::read(&source)?)) == expected["source_sha256"],
            "Audio witness source differs"
        );
        let folder = output.join(name);
        export_with_audio(&source, &folder, &options)?;
        let program: Value = serde_json::from_slice(&fs::read(folder.join("mission.json"))?)?;
        let cues = &program["audio"];
        let mut tags = cues["nodes"]
            .as_object()
            .context("Audio nodes")?
            .values()
            .map(|v| v["tag"]["datum"].as_u64().context("Audio tag"))
            .collect::<Result<Vec<_>>>()?;
        tags.sort_unstable();
        ensure!(
            serde_json::to_value(tags)? == expected["tags"],
            "{name}: audio dependency closure differs from typed source schemas"
        );
        for (identity, ids) in expected["sounds"].as_object().context("Witness sounds")? {
            let identity = identity.parse::<u32>()?;
            let sound = &cues["nodes"][format!("{identity:08X}")]["data"];
            let actual = sound["pitch_ranges"]
                .as_array()
                .context("Pitch ranges")?
                .iter()
                .flat_map(|v| v["variants"].as_array().into_iter().flatten())
                .map(|v| v["fsb_info"].clone())
                .collect::<Vec<_>>();
            ensure!(
                serde_json::to_value(actual)? == *ids,
                "{name}: cue recording identity differs"
            );
        }
        let audio: Value = serde_json::from_slice(&fs::read(folder.join("audio/audio.json"))?)?;
        let recordings = audio["samples"].as_array().context("Decoded recordings")?;
        let expected_samples = expected["samples"]
            .as_object()
            .context("FSB witness samples")?;
        ensure!(
            recordings.len() == expected_samples.len(),
            "{name}: missing recordings"
        );
        let mut files = Vec::new();
        for sample in recordings {
            let identity = sample["id"].as_u64().context("Recording identity")?;
            let expected = expected_samples
                .get(&identity.to_string())
                .context("Unexpected recording")?;
            let filename = sample["file"].as_str().context("Recording file")?;
            ensure!(
                PathBuf::from(filename).components().count() == 1,
                "Recording filename"
            );
            let bytes = fs::read(folder.join("audio").join(filename))?;
            let (channels, rate, frames) = wav_extent(&bytes)?;
            ensure!(
                channels == expected["channels"]
                    && rate == expected["sample_rate"]
                    && frames == expected["frames"],
                "{name}: decoded recording extent differs"
            );
            files.push(serde_json::json!({"id":identity,"file":filename,
                "sha256":hex::encode(Sha256::digest(&bytes)),
                "channels":channels,"sample_rate":rate,"frames":frames}));
        }
        total += recordings.len();
        receipts.push(
            serde_json::json!({"cache":name,"source_sha256":expected["source_sha256"],
            "recordings":files,"audio_manifest_sha256":hex::encode(Sha256::digest(
                fs::read(folder.join("audio/audio.json"))?))}),
        );
    }
    ensure!(
        total > 0,
        "Configured mission corpus contains no recordings"
    );
    fs::write(
        output.join("readback.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "passed":true,"maps":receipts,"recordings":total,
            "witness_sha256":hex::encode(Sha256::digest(&witness_bytes)),
            "seconds":started.elapsed().as_secs_f64(),
            "limits":["Persisted source cues and PCM files. Mission-triggered target playback is unsupported."]
        }))?,
    )?;
    Ok(())
}

/// Independent RIFF extent readback, compared with the witness's original FSB sample header.
fn wav_extent(bytes: &[u8]) -> Result<(Value, Value, Value)> {
    ensure!(
        bytes.get(..4) == Some(b"RIFF") && bytes.get(8..12) == Some(b"WAVE"),
        "Recording is not a WAV"
    );
    let mut at = 12;
    let mut format = None;
    let mut data = None;
    while at + 8 <= bytes.len() {
        let length = u32::from_le_bytes(bytes[at + 4..at + 8].try_into()?) as usize;
        let chunk = bytes
            .get(at + 8..at + 8 + length)
            .context("Short WAV chunk")?;
        match &bytes[at..at + 4] {
            b"fmt " => format = Some(chunk),
            b"data" => data = Some(chunk),
            _ => {}
        }
        at += 8 + length + (length & 1);
    }
    let format = format.filter(|v| v.len() >= 16).context("WAV format")?;
    let channels = u16::from_le_bytes(format[2..4].try_into()?);
    let rate = u32::from_le_bytes(format[4..8].try_into()?);
    let align = usize::from(u16::from_le_bytes(format[12..14].try_into()?));
    let data = data.context("WAV samples")?;
    ensure!(align > 0 && data.len() % align == 0, "WAV frame alignment");
    Ok((
        Value::from(channels),
        Value::from(rate),
        Value::from(data.len() / align),
    ))
}

#[test]
#[ignore = "Requires PARHELION_REACH_MAPS, PARHELION_REACH_MISSION_WITNESS and PARHELION_REACH_MISSION_OUTPUT"]
fn compiled_missions_preserve_source_graphs_and_reject_broken_links() -> Result<()> {
    let maps = configured("PARHELION_REACH_MAPS")?;
    let witness_bytes = fs::read(configured("PARHELION_REACH_MISSION_WITNESS")?)?;
    let witness: Value = serde_json::from_slice(&witness_bytes)?;
    let corpus = witness["maps"]
        .as_array()
        .filter(|rows| !rows.is_empty())
        .context("Required mission corpus is empty")?;
    let output = configured("PARHELION_REACH_MISSION_OUTPUT")?;
    ensure!(!output.exists(), "Use a fresh mission artifact directory");
    fs::create_dir_all(&output)?;
    let started = Instant::now();
    let mut receipts = Vec::new();
    let mut scripts = 0;
    for expected in corpus {
        let name = expected["cache"].as_str().context("Cache name")?;
        ensure!(
            PathBuf::from(name).components().count() == 1,
            "Witness cache must be a filename"
        );
        let source = maps.join(name);
        let source_bytes = fs::read(&source)?;
        ensure!(
            hex::encode(Sha256::digest(&source_bytes)) == expected["sha256"],
            "Witness source differs for {name}"
        );
        let folder = output.join(name);
        parhelion_import::halo_reach::level::mission::export(&source, &folder)?;
        let bytes = fs::read(folder.join("mission.json"))?;
        let actual: Value = serde_json::from_slice(&bytes)?;
        for field in [
            "source_build",
            "scenario",
            "scripts",
            "globals",
            "nodes",
            "tag_references",
            "bindings",
        ] {
            ensure!(
                actual[field] == expected[field],
                "{name}: decoded {field} differs from independent source bytes"
            );
        }
        ensure!(
            actual["analysis"]["scripts"] == expected["dependencies"],
            "{name}: script and global dependencies differ"
        );
        ensure!(
            actual["analysis"]["operations"] == expected["operations"],
            "{name}: operation names differ from retained source function names"
        );
        ensure!(
            actual["execution"] == "unsupported",
            "Decoding must not claim mission execution"
        );
        scripts += actual["scripts"].as_array().context("Scripts")?.len();
        // Broken references are exercised in a temporary copy of the real source cache.
        let temporary = tempfile::tempdir()?;
        let altered = temporary.path().join(name);
        for mutation in expected["mutations"].as_array().context("Mutation cases")? {
            let mut bytes = source_bytes.clone();
            let offset = mutation["offset"].as_u64().context("Mutation offset")? as usize;
            let value = mutation["value"].as_u64().context("Mutation value")? as u32;
            bytes
                .get_mut(offset..offset + 4)
                .context("Mutation extent")?
                .copy_from_slice(&value.to_le_bytes());
            fs::write(&altered, &bytes)?;
            let error = parhelion_import::halo_reach::level::mission::read(&altered)
                .err()
                .context("Malformed mission was accepted")?;
            let message = format!("{error:#}");
            ensure!(
                message.contains(mutation["error"].as_str().context("Expected error")?),
                "{name}: unexpected rejection: {message}"
            );
        }
        receipts.push(serde_json::json!({
            "cache": name, "source_sha256": expected["sha256"],
            "mission_sha256": hex::encode(Sha256::digest(&bytes)),
            "scripts": actual["scripts"].as_array().map(Vec::len),
            "mutations": expected["mutations"],
        }));
    }
    ensure!(scripts > 0, "Configured corpus has no compiled scripts");
    fs::write(
        output.join("readback.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "passed": true, "maps": receipts, "scripts": scripts,
            "witness_sha256": hex::encode(Sha256::digest(&witness_bytes)),
            "seconds": started.elapsed().as_secs_f64(),
            "limits": ["Source decoding and dependency analysis. No target mission execution."]
        }))?,
    )?;
    Ok(())
}

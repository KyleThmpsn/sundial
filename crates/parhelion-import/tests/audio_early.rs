//! Configured Wwise 89 banks through conversion, private allocation and persisted event routing.
//! The corpus records source events and actions using an independent Wwise parser.
use anyhow::{Context, Result, ensure};
use parhelion_import::tiger::audio::bank::{self, Namespace};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::PathBuf,
};

fn word(bytes: &[u8], at: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        bytes.get(at..at + 4).context("bank word")?.try_into()?,
    ))
}

fn hierarchy(bytes: &[u8], version: u32) -> Result<BTreeMap<u32, (u8, &[u8])>> {
    let mut at = 0;
    let mut header = false;
    let mut result = None;
    while at < bytes.len() {
        let end = at
            .checked_add(8 + word(bytes, at + 4)? as usize)
            .context("chunk overflow")?;
        let body = bytes.get(at + 8..end).context("chunk bounds")?;
        match &bytes[at..at + 4] {
            b"BKHD" => {
                ensure!(!header && word(body, 0)? == version, "bank version differs");
                header = true;
            }
            b"HIRC" => {
                let count = word(body, 0)?;
                let mut offset = 4;
                let mut rows = BTreeMap::new();
                for _ in 0..count {
                    let kind = *body.get(offset).context("HIRC type")?;
                    let size = word(body, offset + 1)? as usize;
                    let row = body
                        .get(offset + 5..offset + 5 + size)
                        .context("HIRC extent")?;
                    ensure!(
                        rows.insert(word(row, 0)?, (kind, row)).is_none(),
                        "duplicate object"
                    );
                    offset += 5 + size;
                }
                ensure!(
                    offset == body.len() && result.replace(rows).is_none(),
                    "HIRC envelope"
                );
            }
            _ => {}
        }
        at = end;
    }
    ensure!(header, "native header absent");
    result.context("native hierarchy absent")
}

fn id(value: &Value) -> Result<u32> {
    u32::from_str_radix(value.as_str().context("hex identity")?, 16).map_err(Into::into)
}

#[test]
#[ignore = "Requires PARHELION_EARLY_AUDIO_CORPUS, PARHELION_EARLY_AUDIO_OUTPUT and SUNDIAL_TEST_REVISION"]
fn early_banks_preserve_event_actions_and_private_streams() -> Result<()> {
    let corpus =
        PathBuf::from(env::var_os("PARHELION_EARLY_AUDIO_CORPUS").context("audio corpus")?);
    let output =
        PathBuf::from(env::var_os("PARHELION_EARLY_AUDIO_OUTPUT").context("audio output")?);
    ensure!(!output.exists(), "audio output must be fresh");
    let manifest: Value = serde_json::from_slice(&fs::read(corpus.join("corpus.json"))?)?;
    let rows = manifest["banks"].as_array().context("corpus banks")?;
    ensure!(!rows.is_empty(), "required audio corpus is empty");
    let mut inputs = Vec::new();
    let mut banks = Vec::new();
    for row in rows {
        let file = row["file"].as_str().context("bank input")?;
        let bytes = fs::read(corpus.join(file))?;
        ensure!(
            hex::encode(Sha256::digest(&bytes))
                == row["sha256"].as_str().context("source digest")?,
            "source bank changed"
        );
        let bank = bank::upgrade(&bytes).with_context(|| file.to_owned())?;
        // A stream cut short must fail before it can be persisted as a usable bank.
        ensure!(
            bank::upgrade(&bytes[..bytes.len() - 1]).is_err(),
            "truncated bank accepted"
        );
        inputs.push(bytes);
        banks.push(bank);
    }
    let namespace = Namespace::new("early-audio-e2e", &banks)?;
    let mut media = BTreeMap::new();
    let mut sizes = BTreeMap::new();
    for bank in &banks {
        for (_, source) in bank.media_fields() {
            let next = 0x81E00000 + u32::try_from(media.len())?;
            media.entry(source).or_insert(next);
            sizes.insert(source, 4096);
        }
    }
    fs::create_dir_all(&output)?;
    let mut artifacts = Vec::new();
    let mut events = 0;
    let mut actions = 0;
    let mut sounds = 0;
    let mut generators = 0;
    for ((row, bank), original) in rows.iter().zip(&banks).zip(&inputs) {
        let bytes = namespace.instantiate(bank, &media, &sizes)?;
        let file = format!("{:08X}.bnk", bank.bank_id);
        fs::write(output.join(&file), &bytes)?;
        let persisted = fs::read(output.join(&file))?;
        let objects = hierarchy(&persisted, 113)?;
        let source_objects = hierarchy(original, 89)?;
        ensure!(
            objects.len() == source_objects.len(),
            "converted object loss"
        );
        for (&source_id, (kind, _)) in &source_objects {
            ensure!(
                objects
                    .get(&namespace.object(source_id)?)
                    .is_some_and(|(actual, _)| actual == kind),
                "Source object disappeared or changed kind"
            );
        }
        for event in row["events"]
            .as_array()
            .context("independent event oracle")?
        {
            let mapped = namespace.object(id(&event["id"])?)?;
            let (kind, payload) = objects.get(&mapped).context("persisted event")?;
            ensure!(*kind == 4, "event changed kind");
            let expected = event["actions"].as_array().context("event actions")?;
            ensure!(
                word(payload, 4)? as usize == expected.len()
                    && payload.len() == 8 + expected.len() * 4,
                "event action extent"
            );
            for (index, action) in expected.iter().enumerate() {
                let target = namespace.object(id(action)?)?;
                ensure!(
                    word(payload, 8 + index * 4)? == target
                        && objects.get(&target).is_some_and(|(kind, _)| *kind == 3),
                    "event action routing changed"
                );
            }
            events += 1;
        }
        for action in row["actions"]
            .as_array()
            .context("independent action oracle")?
        {
            let mapped = namespace.object(id(&action["id"])?)?;
            let (kind, payload) = objects.get(&mapped).context("persisted action")?;
            let source_target = id(&action["target"])?;
            let target = namespace.object(source_target).unwrap_or(source_target);
            ensure!(
                *kind == 3
                    && u16::from_le_bytes(payload[4..6].try_into()?)
                        == action["kind"].as_u64().context("action kind")? as u16
                    && word(payload, 6)? == target,
                "action target or kind changed"
            );
            actions += 1;
        }
        for sound in row["sounds"]
            .as_array()
            .context("independent media oracle")?
        {
            let mapped = namespace.object(id(&sound["id"])?)?;
            let medium = media
                .get(&id(&sound["media"])?)
                .context("medium allocation")?;
            let (kind, payload) = objects.get(&mapped).context("persisted source")?;
            ensure!(
                *kind == 2
                    && word(payload, 4)? == 0x00010001
                    && payload[8] == 2
                    && word(payload, 9)? == *medium
                    && word(payload, 13)? == 4096,
                "native PCM streaming source differs"
            );
            sounds += 1;
        }
        let actual_generators = source_objects
            .iter()
            .filter(|(_, (kind, bytes))| *kind == 2 && word(bytes, 4).ok() == Some(0x00650002))
            .map(|(&id, _)| id)
            .collect::<BTreeSet<_>>();
        let mut observed = BTreeSet::new();
        for generator in row["generators"].as_array().into_iter().flatten() {
            let source_id = id(&generator["id"])?;
            ensure!(observed.insert(source_id), "Repeated generator oracle");
            let source_preset = id(&generator["preset"])?;
            let preset = namespace.object(source_preset)?;
            let (kind, payload) = objects
                .get(&namespace.object(source_id)?)
                .context("Persisted generator")?;
            ensure!(
                *kind == 2
                    && word(payload, 4)? == 0x00650002
                    && payload.get(8) == Some(&0)
                    && word(payload, 9)? == preset
                    && word(payload, 13)? == 0
                    && payload.get(17) == Some(&0)
                    && word(payload, 18)? == 0,
                "Generated source lost its private preset or acquired media"
            );
            let (kind, payload) = objects.get(&preset).context("Persisted source preset")?;
            let expected = hex::decode(
                generator["parameters"]
                    .as_str()
                    .context("Independent generator parameters")?,
            )?;
            ensure!(
                expected.len() == 12
                    && *kind == 19
                    && payload.len() == 29
                    && word(payload, 4)? == 0x00650002
                    && word(payload, 8)? == 12
                    && payload.get(12..24) == Some(expected.as_slice())
                    && payload.get(24..29) == Some(&[0; 5][..]),
                "Generated source preset changed parameters or state"
            );
            generators += 1;
        }
        ensure!(
            observed == actual_generators,
            "Independent oracle does not cover every generated source"
        );
        artifacts.push(
            json!({"source":row,"file":file,"sha256":hex::encode(Sha256::digest(&persisted))}),
        );
    }
    ensure!(
        events > 0 && actions > 0 && sounds > 0,
        "required audio behavior corpus is empty"
    );
    fs::write(
        output.join("readback.json"),
        serde_json::to_vec_pretty(&json!({
            "source_build":manifest["source_build"],"target_build":manifest["target_build"],
            "banks":artifacts,"events":events,"actions":actions,"sounds":sounds,"generators":generators,
            "revision":env::var("SUNDIAL_TEST_REVISION").context("source revision")?,
            "test_binary_sha256":hex::encode(Sha256::digest(fs::read(env::current_exe()?)?)),
            "namespace":namespace,"media":media,"gameplay_verified":false
        }))?,
    )?;
    Ok(())
}

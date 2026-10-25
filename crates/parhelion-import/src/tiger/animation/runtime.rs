//! A native runtime entity's own clip bank: reading its named clips and linking converted clips
//! onto a private copy of its lookup, bank and bank consumers. Source importers convert their
//! clips to the native layout and hand them here.
use crate::tiger::{payload::Payload, reader::Reader};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path};

/// The native animation lookup component class.
pub const NATIVE_LOOKUP: &str = "8080344B";

fn hex(value: &Value) -> Result<u32> {
    Ok(u32::from_str_radix(
        value.as_str().context("Runtime animation tag")?,
        16,
    )?)
}

/// The runtime entity's unique animation lookup of `class` and its runtime skeleton's bones.
pub fn lookup(rig: &Value, class: &str) -> Result<(u32, Vec<Value>)> {
    let entity = rig["runtime_entity"]
        .as_str()
        .context("Runtime animation entity")?;
    let components = rig["components"]
        .as_array()
        .context("Runtime animation components")?;
    let lookups = components
        .iter()
        .filter(|c| c["entity"] == entity && c["class"] == class)
        .collect::<Vec<_>>();
    ensure!(
        lookups.len() == 1,
        "Runtime has no unique animation lookup on its entity"
    );
    let skeletons = rig["skeletons"]
        .as_array()
        .context("Runtime skeletons")?
        .iter()
        .filter(|s| s["entity"] == entity)
        .collect::<Vec<_>>();
    ensure!(skeletons.len() == 1, "Runtime has no unique skeleton");
    Ok((
        hex(&lookups[0]["owner"])?,
        skeletons[0]["bones"]
            .as_array()
            .context("Runtime bones")?
            .clone(),
    ))
}

/// A native lookup owner's bank and its rows as clip tags with the names they carry.
pub fn bank(r: &mut Reader, owner: u32) -> Result<(u32, Vec<(u32, u32)>)> {
    let p = r.tag(owner, Some(0x80809C36))?;
    let bank_tag = p.u32(p.pointer(24)? + 0x90)?;
    let bank = r.tag(bank_tag, Some(0x808036F6))?;
    let mut clips = Vec::new();
    for row in bank.array(8, 4, Some(0x80808F48))? {
        let tag = bank.u32(row)?;
        let clip = r.tag(tag, Some(0x80808F49))?;
        ensure!(
            clip.0.len() >= 0x190 && clip.u64(0)? == clip.0.len() as u64,
            "animation clip payload size differs"
        );
        clips.push((tag, clip.u32(0x120)?));
    }
    Ok((bank_tag, clips))
}

/// The clips of a native runtime's own bank, with the trigger names they carry.
pub fn clips(r: &mut Reader, rig: &Value) -> Result<Vec<(u32, u32)>> {
    let (owner, _) = lookup(rig, NATIVE_LOOKUP)?;
    Ok(bank(r, owner)?.1)
}

/// Link converted clips onto the native runtime's own bank and report the graph's
/// `equipment_animation` section. Each converted row names the native clip it replaces and the
/// graph file holding its native payload. Every clip paired with a native one, converted or
/// rejected, is left out of the source-only list.
pub fn link(
    nr: &mut Reader,
    native_rig: &Value,
    source_clips: &[(u32, u32)],
    (converted, rejected, with_events): (Vec<Value>, Vec<Value>, usize),
    graph: &Path,
) -> Result<Value> {
    let (native_owner, _) = lookup(native_rig, NATIVE_LOOKUP)?;
    let entity = hex(&native_rig["runtime_entity"])?;
    let (bank, native_clips) = bank(nr, native_owner)?;
    let paired = converted
        .iter()
        .chain(&rejected)
        .map(|clip| {
            clip["source"]
                .as_u64()
                .and_then(|tag| u32::try_from(tag).ok())
                .context("Paired clip source")
        })
        .collect::<Result<Vec<_>>>()?;
    let mut files = BTreeMap::new();
    for (key, tag, class) in [
        ("entity", entity, 0x80809C0F),
        ("lookup_owner", native_owner, 0x80809C36),
        ("bank", bank, 0x808036F6),
    ] {
        let file = format!("animation/equipment-{key}.bin");
        fs::write(graph.join(&file), &nr.tag(tag, Some(class))?.0)?;
        files.insert(key.to_owned(), file);
    }
    let mut converted_bank = nr.tag(bank, Some(0x808036F6))?.as_ref().clone();
    for row in converted_bank.array(0x68, 32, Some(0x80809002))? {
        let tag = native_clips
            .get(usize::from(converted_bank.u16(row + 24)?))
            .context("Equipment descriptor clip index")?
            .0;
        let Some(clip) = converted
            .iter()
            .find(|clip| clip["native"].as_u64() == Some(u64::from(tag)))
        else {
            continue;
        };
        ensure!(
            converted_bank.bytes::<16>(row)? == [0; 16],
            "Equipment animation descriptor has inline tracks"
        );
        let payload = Payload(fs::read(
            graph.join(clip["file"].as_str().context("Equipment clip file")?),
        )?);
        let frames = payload.u16(0x13C)?;
        ensure!(frames > 0, "Equipment clip has no frames");
        converted_bank.0[row + 20..row + 24]
            .copy_from_slice(&(f32::from(frames - 1) / 30.0).to_le_bytes());
    }
    let converted_file = "animation/equipment-bank-converted.bin";
    fs::write(graph.join(converted_file), &converted_bank.0)?;
    files.insert("converted_bank".into(), converted_file.into());
    let mut consumers = Vec::new();
    for component in native_rig["components"]
        .as_array()
        .context("Native equipment components")?
    {
        if hex(&component["entity"])? != entity
            || !matches!(component["class"].as_str(), Some("80803640" | "808036CF"))
        {
            continue;
        }
        let owner = hex(&component["owner"])?;
        let payload = nr.tag(owner, Some(0x80809C36))?;
        let field = super::bank_field(&payload)?;
        ensure!(
            payload.u32(field)? == bank,
            "Equipment animation consumer names a different bank"
        );
        let key = format!("consumer_{}", consumers.len());
        let file = format!("animation/equipment-consumer-{owner:08X}.bin");
        fs::write(graph.join(&file), &payload.0)?;
        files.insert(key.clone(), file);
        consumers.push(json!({"owner":owner,"file_key":key}));
    }
    ensure!(
        !consumers.is_empty(),
        "Equipment animation has no supported clip-bank consumer"
    );
    let source_only = source_clips
        .iter()
        .filter(|(tag, _)| !paired.contains(tag))
        .map(|(tag, name)| json!({"source":tag,"name":name}))
        .collect::<Vec<_>>();
    Ok(
        json!({"status":"linked","runtime_entity":entity,"lookup_owner":native_owner,"bank":bank,
        "files":files,"bank_consumers":consumers,"clips":converted,"unconverted":rejected,
        "source_only":source_only,"clips_with_events":with_events,
        "controller":"native equipment states","gameplay_verified":false}),
    )
}

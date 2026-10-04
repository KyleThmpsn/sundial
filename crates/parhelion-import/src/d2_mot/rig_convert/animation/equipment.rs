//! Lower named equipment clips onto the native runtime's validated track slots.
//! Equipment lookups live on the runtime entity, unlike weapon first-person attachments.
use super::{clips, first_person};
use crate::d2_mot::{payload::Payload, reader::Reader};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path};

fn hex(value: &Value) -> Result<u32> {
    Ok(u32::from_str_radix(
        value.as_str().context("Equipment tag")?,
        16,
    )?)
}

fn lookup(rig: &Value, modern: bool) -> Result<(u32, Vec<Value>)> {
    let entity = rig["runtime_entity"]
        .as_str()
        .context("Equipment runtime entity")?;
    let components = rig["components"]
        .as_array()
        .context("Equipment components")?;
    let lookups = components
        .iter()
        .filter(|c| {
            c["entity"] == entity && c["class"] == if modern { "808025F8" } else { "8080344B" }
        })
        .collect::<Vec<_>>();
    ensure!(
        lookups.len() == 1,
        "Equipment has no unique animation lookup on its runtime entity"
    );
    let skeletons = rig["skeletons"]
        .as_array()
        .context("Equipment skeletons")?
        .iter()
        .filter(|s| s["entity"] == entity)
        .collect::<Vec<_>>();
    ensure!(
        skeletons.len() == 1,
        "Equipment has no unique runtime skeleton"
    );
    Ok((
        hex(&lookups[0]["owner"])?,
        skeletons[0]["bones"]
            .as_array()
            .context("Equipment bones")?
            .clone(),
    ))
}

/// The clips of a runtime's own animation bank, with the trigger names they carry.
pub fn runtime_clips(r: &mut Reader, rig: &Value, modern: bool) -> Result<Vec<(u32, u32)>> {
    let (owner, _) = lookup(rig, modern)?;
    Ok(first_person::bank(r, owner, modern)?.1)
}

pub fn prepare(
    modern: &Path,
    native: &Path,
    source_rig: &Value,
    native_rig: &Value,
    work: &Path,
    graph: &Path,
) -> Result<Value> {
    let (source_owner, source_bones) = lookup(source_rig, true)?;
    let (native_owner, native_bones) = lookup(native_rig, false)?;
    first_person::bone_map(&source_bones, &native_bones)?;
    let entity = hex(&native_rig["runtime_entity"])?;
    let mut sr = Reader::new(modern, &work.join("source"), true)?;
    let mut nr = Reader::new(native, &work.join("native"), false)?;
    let (_, source_clips) = first_person::bank(&mut sr, source_owner, true)?;
    let (bank, native_clips) = first_person::bank(&mut nr, native_owner, false)?;
    let pairs = first_person::clip_pairs(&mut sr, &mut nr, &native_clips, &source_clips)?;
    ensure!(
        !pairs.is_empty(),
        "Source and native equipment have no unambiguous named clips in common"
    );
    let slots = clips::slots::Table::derive(pairs.iter().map(|p| (p.3.as_ref(), p.4.as_ref())))?;
    let events = first_person::events(&mut sr, &mut nr, None, &pairs, &[], &native_clips)?;
    fs::create_dir_all(graph.join("animation"))?;
    let (converted, rejected, with_events) =
        first_person::convert_pairs(&pairs, &events, &slots, None, graph)?;
    ensure!(
        !converted.is_empty(),
        "No equipment clip could be converted: {rejected:?}"
    );
    let paired = pairs.iter().map(|p| p.1).collect::<Vec<_>>();
    let result = link(
        &mut nr,
        native_rig,
        (entity, native_owner, bank),
        &native_clips,
        &source_clips,
        &paired,
        (converted, rejected, with_events),
        graph,
    )?;
    sr.finish()?;
    nr.finish()?;
    Ok(result)
}

/// Link explicitly paired clips onto a runtime whose skeleton is the source's own, as a
/// source-owned rig installs it. Each native clip listed takes the converted source clip paired
/// with it under the native clip's trigger name. The clip keeps its source track slots, since
/// the skeleton it plays on is the source's.
pub fn prepare_paired(
    modern: &Path,
    native: &Path,
    source_rig: &Value,
    native_rig: &Value,
    work: &Path,
    graph: &Path,
    pairs: &[(u32, u32)],
) -> Result<Value> {
    let (source_owner, _) = lookup(source_rig, true)?;
    let (native_owner, _) = lookup(native_rig, false)?;
    let entity = hex(&native_rig["runtime_entity"])?;
    let mut sr = Reader::new(modern, &work.join("source"), true)?;
    let mut nr = Reader::new(native, &work.join("native"), false)?;
    let (_, source_clips) = first_person::bank(&mut sr, source_owner, true)?;
    let (bank, native_clips) = first_person::bank(&mut nr, native_owner, false)?;
    fs::create_dir_all(graph.join("animation"))?;
    let mut converted = Vec::new();
    for &(native_tag, source_tag) in pairs {
        let name = native_clips
            .iter()
            .find(|(tag, _)| *tag == native_tag)
            .context("Paired native clip is not in the runtime bank")?
            .1;
        ensure!(
            source_clips.iter().any(|(tag, _)| *tag == source_tag),
            "Paired source clip is not in the source runtime bank"
        );
        let source = sr.tag(source_tag, Some(0x80808BE0))?;
        let (mut payload, report) = clips::convert(&source.0)?;
        // The native runtime triggers its clips by this name.
        payload.0[0x120..0x124].copy_from_slice(&name.to_le_bytes());
        let file = format!("animation/runtime-clip-{native_tag:08X}.bin");
        fs::write(graph.join(&file), &payload.0)?;
        converted.push(
            json!({"native":native_tag,"source":source_tag,"name":name,"file":file,
            "streams":report["streams"],"events":report["events"]}),
        );
    }
    let paired = pairs.iter().map(|p| p.1).collect::<Vec<_>>();
    let result = link(
        &mut nr,
        native_rig,
        (entity, native_owner, bank),
        &native_clips,
        &source_clips,
        &paired,
        (converted, Vec::new(), 0),
        graph,
    )?;
    sr.finish()?;
    nr.finish()?;
    Ok(result)
}

/// Write the native runtime's lookup, bank and bank consumers, and the bank with each converted
/// clip's duration, and report the link.
#[allow(clippy::too_many_arguments)]
fn link(
    nr: &mut Reader,
    native_rig: &Value,
    (entity, native_owner, bank): (u32, u32, u32),
    native_clips: &[(u32, u32)],
    source_clips: &[(u32, u32)],
    paired: &[u32],
    (converted, rejected, with_events): (Vec<Value>, Vec<Value>, usize),
    graph: &Path,
) -> Result<Value> {
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
        let field = first_person::consumers::bank_field(&payload)?;
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

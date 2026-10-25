//! Lower named equipment clips onto the native runtime's validated track slots.
//! Equipment lookups live on the runtime entity, unlike weapon first-person attachments.
use super::{clips, first_person};
use crate::d2_mot::reader::Reader;
use crate::tiger::animation::runtime;
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{fs, path::Path};

fn lookup(rig: &Value, modern: bool) -> Result<(u32, Vec<Value>)> {
    runtime::lookup(
        rig,
        if modern {
            "808025F8"
        } else {
            runtime::NATIVE_LOOKUP
        },
    )
}

/// The clips of a runtime's own animation bank, with the trigger names they carry.
pub fn runtime_clips(r: &mut Reader, rig: &Value, modern: bool) -> Result<Vec<(u32, u32)>> {
    if !modern {
        return runtime::clips(r, rig);
    }
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
    let mut sr = Reader::new(modern, &work.join("source"), true)?;
    let mut nr = Reader::new(native, &work.join("native"), false)?;
    let (_, source_clips) = first_person::bank(&mut sr, source_owner, true)?;
    let (_, native_clips) = runtime::bank(&mut nr, native_owner)?;
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
    let result = runtime::link(
        &mut nr,
        native_rig,
        &source_clips,
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
    let mut sr = Reader::new(modern, &work.join("source"), true)?;
    let mut nr = Reader::new(native, &work.join("native"), false)?;
    let (_, source_clips) = first_person::bank(&mut sr, source_owner, true)?;
    let (_, native_clips) = runtime::bank(&mut nr, native_owner)?;
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
    let result = runtime::link(
        &mut nr,
        native_rig,
        &source_clips,
        (converted, Vec::new(), 0),
        graph,
    )?;
    sr.finish()?;
    nr.finish()?;
    Ok(result)
}

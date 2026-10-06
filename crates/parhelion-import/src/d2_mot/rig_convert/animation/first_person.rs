//! Substitute converted source first-person clips into private copies of the
//! native donor's first-person animation chain, matched by clip name.
//!
//! The native runtime entity reaches its first-person entity through the
//! attachment table owner. That entity's lookup owner names a clip bank whose
//! rows are clip tags. Every link is a plain tag word, so package authoring can
//! copy the chain and rewrite each link. Source state tables are lowered through
//! named parameter masks and converted clip ordinals. Unsupported states retain
//! an explicit native fallback. The bank keeps its row order. Shared skeleton
//! bones must retain their parents. Clip slots are validated independently of
//! the geometry skeleton's bone order.
use super::clips;
mod bank;
pub mod consumers;
pub mod controls;
pub mod dispatch;
pub mod markers;
pub mod motion;
pub(crate) mod poses;
pub mod profile;
mod states;
use crate::d2_mot::{
    payload::Payload,
    reader::{Reader, write_json},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

const NATIVE_LOOKUP: &str = "8080344B";
const SOURCE_LOOKUP: &str = "808025F8";
const NATIVE_ATTACHMENTS: &str = "80804221";

/// The first-person entity of a rig report with its skeleton and lookup owner.
struct FirstPerson {
    entity: u32,
    bones: Vec<Value>,
    lookup_owner: u32,
}

fn hex(v: &Value) -> Result<u32> {
    Ok(u32::from_str_radix(v.as_str().context("tag text")?, 16)?)
}

fn first_person(rig: &Value, lookup_class: &str) -> Result<Option<FirstPerson>> {
    let runtime = rig["runtime_entity"].as_str().context("runtime entity")?;
    let components = rig["components"].as_array().context("rig components")?;
    let mut entities = components
        .iter()
        .filter_map(|c| c["entity"].as_str())
        .filter(|e| *e != runtime)
        .collect::<Vec<_>>();
    entities.sort_unstable();
    entities.dedup();
    let entity = match entities.as_slice() {
        [] => return Ok(None),
        [one] => *one,
        _ => anyhow::bail!("more than one attached animation entity"),
    };
    let owners = components
        .iter()
        .filter(|c| c["entity"] == entity)
        .collect::<Vec<_>>();
    let skeletons = rig["skeletons"]
        .as_array()
        .context("rig skeletons")?
        .iter()
        .filter(|s| owners.iter().any(|c| c["owner"] == s["owner"]))
        .collect::<Vec<_>>();
    ensure!(
        skeletons.len() == 1,
        "first-person entity skeleton missing or ambiguous"
    );
    let lookups = owners
        .iter()
        .filter(|c| c["class"] == lookup_class)
        .collect::<Vec<_>>();
    ensure!(
        lookups.len() == 1,
        "first-person animation lookup missing or ambiguous"
    );
    Ok(Some(FirstPerson {
        entity: u32::from_str_radix(entity, 16)?,
        bones: skeletons[0]["bones"]
            .as_array()
            .context("skeleton bones")?
            .clone(),
        lookup_owner: hex(&lookups[0]["owner"])?,
    }))
}

/// Map each source bone onto the native bone of the same name.
///
/// This checks the shared hierarchy, not clip track order. Clips address slots
/// in the controller's animation rig, not this geometry skeleton's bone indices.
/// A bank can contain several clip rig sizes and be shared by entities whose
/// weapon attachment bones are ordered differently. `slots::Table` separately
/// validates the correspondence using the actual name-paired clip poses.
/// Every mapped bone must keep its parent relationship, names must be unique on
/// both sides, and two source bones may never land on one native bone. Bones
/// with no counterpart map to `u16::MAX`. Slot alignment independently rejects
/// dropping animated tracks.
pub(super) fn bone_map(source: &[Value], native: &[Value]) -> Result<Vec<u16>> {
    for (label, bones) in [("source", source), ("native", native)] {
        ensure!(
            bones
                .iter()
                .enumerate()
                .all(|(i, b)| b["index"].as_u64() == Some(i as u64)),
            "{label} first-person skeleton indices are not dense"
        );
        let mut names = bones.iter().map(|b| &b["name_hash"]).collect::<Vec<_>>();
        names.sort_by_key(|v| v.as_str().unwrap_or_default());
        let total = names.len();
        names.dedup();
        ensure!(
            names.len() == total,
            "{label} first-person bone names repeat"
        );
    }
    let mut map = Vec::with_capacity(source.len());
    for bone in source {
        map.push(
            native
                .iter()
                .position(|n| n["name_hash"] == bone["name_hash"])
                .and_then(|i| u16::try_from(i).ok())
                .unwrap_or(u16::MAX),
        );
    }
    for (index, bone) in source.iter().enumerate() {
        if map[index] == u16::MAX {
            continue;
        }
        let parent = bone["parent"].as_i64().context("source bone parent")?;
        let expected = if parent == -1 {
            -1
        } else {
            let mapped = *map
                .get(usize::try_from(parent)?)
                .context("source bone parent index")?;
            ensure!(
                mapped != u16::MAX,
                "mapped bone {index} has an unmapped parent"
            );
            i64::from(mapped)
        };
        ensure!(
            native[map[index] as usize]["parent"].as_i64() == Some(expected),
            "bone {index} changes parent between the first-person skeletons"
        );
    }
    Ok(map)
}

/// Read a lookup owner's bank rows as clip tags with the clip names they carry.
pub(super) fn bank(r: &mut Reader, owner: u32, modern: bool) -> Result<(u32, Vec<(u32, u32)>)> {
    let p = r.tag(owner, Some(if modern { 0x80809B06 } else { 0x80809C36 }))?;
    let bank_tag = p.u32(p.pointer(24)? + if modern { 0xA8 } else { 0x90 })?;
    let bank = r.tag(bank_tag, Some(if modern { 0x8080289F } else { 0x808036F6 }))?;
    let mut clips = Vec::new();
    for row in bank.array(
        8,
        if modern { 16 } else { 4 },
        Some(if modern { 0x80808BDF } else { 0x80808F48 }),
    )? {
        let tag = if modern {
            r.ref64(&bank, row)?
        } else {
            bank.u32(row)?
        };
        let clip = r.tag(tag, Some(if modern { 0x80808BE0 } else { 0x80808F49 }))?;
        ensure!(
            clip.0.len() >= 0x190 && clip.u64(0)? == clip.0.len() as u64,
            "animation clip payload size differs"
        );
        clips.push((tag, clip.u32(0x120)?));
    }
    Ok((bank_tag, clips))
}

/// Pair native and source assets that share a name unique on both sides.
/// Bank rows may alias the same asset. Those are not ambiguous names.
/// Returns `(native, source, name)` in native bank order.
fn matched(native: &[(u32, u32)], source: &[(u32, u32)]) -> Vec<(u32, u32, u32)> {
    let count = |clips: &[(u32, u32)]| {
        let mut by_name: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
        for (tag, name) in clips {
            by_name.entry(*name).or_default().insert(*tag);
        }
        by_name
    };
    let source_names = count(source);
    let native_names = count(native);
    let mut seen = BTreeSet::new();
    native
        .iter()
        .filter(|(tag, _)| seen.insert(*tag))
        .filter_map(
            |(tag, name)| match (native_names.get(name), source_names.get(name)) {
                (Some(n), Some(s)) if n.len() == 1 && s.len() == 1 => {
                    Some((*tag, *s.first().unwrap(), *name))
                }
                _ => None,
            },
        )
        .collect()
}

fn word_occurrences(payload: &[u8], tag: u32) -> usize {
    payload
        .chunks_exact(4)
        .filter(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]) == tag)
        .count()
}

/// A native clip and the source clip sharing its name: native tag, source tag,
/// name hash, source payload and native payload.
pub(super) type ClipPair = (u32, u32, u32, Arc<Payload>, Arc<Payload>);

/// An extra source clip's native template tag, source template tag, playback
/// mode, converted payload and conversion report.
type ExtraClip = (u32, u32, u16, Payload, Value);

/// Read-only inputs for adding source clips without a same-name native counterpart.
struct Extras<'a> {
    calibration: Option<&'a Value>,
    graph: &'a Path,
    source_clips: &'a [(u32, u32)],
    native_clips: &'a [(u32, u32)],
    converted: &'a [Value],
    pairs: &'a [ClipPair],
    calibration_pairs: &'a [ClipPair],
    modes: &'a BTreeMap<u32, u16>,
    event_library: &'a clips::EventLibrary,
    slots: &'a clips::slots::Table,
}

/// Write the native first-person chain payloads under the animation directory.
fn chain_files(
    out: &Path,
    attachments: &Payload,
    entity: &Payload,
    lookup: &Payload,
    bank_payload: &Payload,
) -> Result<BTreeMap<String, String>> {
    let mut files = BTreeMap::<String, String>::new();
    for (key, name, payload) in [
        (
            "attachment_owner",
            "first-person-attachment-owner.bin",
            &attachments.0,
        ),
        ("entity", "first-person-entity.bin", &entity.0),
        ("lookup_owner", "first-person-lookup-owner.bin", &lookup.0),
        ("bank", "first-person-bank.bin", &bank_payload.0),
    ] {
        fs::write(out.join(name), payload)?;
        files.insert(key.to_owned(), format!("animation/{name}"));
    }
    Ok(files)
}

/// Pair every name-matched native clip with its source clip payloads.
pub(super) fn clip_pairs(
    sr: &mut Reader,
    nr: &mut Reader,
    native: &[(u32, u32)],
    source: &[(u32, u32)],
) -> Result<Vec<ClipPair>> {
    let mut pairs = Vec::new();
    for (native_tag, source_tag, name) in matched(native, source) {
        let source_clip = sr.tag(source_tag, Some(0x80808BE0))?;
        let counterpart = nr.tag(native_tag, Some(0x80808F49))?;
        pairs.push((native_tag, source_tag, name, source_clip, counterpart));
    }
    Ok(pairs)
}

/// Equal-name clip pairs from the calibration export's bank and its explicit
/// clip references.
fn calibration_clips(
    sr: &mut Reader,
    nr: &mut Reader,
    native_clips: &[(u32, u32)],
    calibration: Option<&Value>,
) -> Result<Vec<ClipPair>> {
    let mut calibration_pairs = Vec::new();
    if let Some(reference) = calibration {
        let fp =
            first_person(reference, SOURCE_LOOKUP)?.context("calibration first-person entity")?;
        let (_, clips) = bank(sr, fp.lookup_owner, true)?;
        for (native_tag, source_tag, name) in matched(native_clips, &clips) {
            calibration_pairs.push((
                native_tag,
                source_tag,
                name,
                sr.tag(source_tag, Some(0x80808BE0))?,
                nr.tag(native_tag, Some(0x80808F49))?,
            ));
        }
        // An authored rig can need a clip shape absent from the format carrier's
        // bank. Explicit equal-name reference pairs calibrate that format only.
        // They are not installed as poses or substituted for source animations.
        for pair in reference["clip_calibration"]
            .as_array()
            .into_iter()
            .flatten()
        {
            let source_tag = hex(&pair["source"])?;
            let native_tag = hex(&pair["native"])?;
            let modern = sr.tag(source_tag, Some(0x80808BE0))?;
            let old = nr.tag(native_tag, Some(0x80808F49))?;
            ensure!(
                modern.u32(0x120)? == old.u32(0x120)? && modern.u16(0x142)? == old.u16(0x13E)?,
                "clip calibration reference changes its name or bone count"
            );
            calibration_pairs.push((native_tag, source_tag, modern.u32(0x120)?, modern, old));
        }
        ensure!(
            !calibration_pairs.is_empty(),
            "no native counterparts establish calibration slots"
        );
    }
    Ok(calibration_pairs)
}

/// The event library from every paired clip, the explicit event calibration
/// pairs and the native bank.
pub(super) fn events(
    sr: &mut Reader,
    nr: &mut Reader,
    calibration: Option<&Value>,
    pairs: &[ClipPair],
    calibration_pairs: &[ClipPair],
    native_clips: &[(u32, u32)],
) -> Result<clips::EventLibrary> {
    let mut event_pairs = Vec::new();
    if let Some(reference) = calibration {
        for pair in reference["event_calibration"]
            .as_array()
            .into_iter()
            .flatten()
        {
            let modern = sr.tag(hex(&pair["source"])?, Some(0x80808BE0))?;
            let old = nr.tag(hex(&pair["native"])?, Some(0x80808F49))?;
            ensure!(
                modern.u32(0x120)? == old.u32(0x120)?,
                "event calibration clips have different names"
            );
            event_pairs.push((modern, old));
        }
    }
    let mut event_library = clips::EventLibrary::derive(
        pairs
            .iter()
            .chain(calibration_pairs)
            .map(|p| (p.3.as_ref(), p.4.as_ref()))
            .chain(event_pairs.iter().map(|(s, n)| (s.as_ref(), n.as_ref()))),
    );
    for &(tag, _) in native_clips {
        event_library.add_native(nr.tag(tag, Some(0x80808F49))?.as_ref());
    }
    Ok(event_library)
}

/// Convert every paired source clip onto its native counterpart's slots. Returns
/// the converted and unconverted reports with the count of clips carrying events.
pub(super) fn convert_pairs(
    pairs: &[ClipPair],
    event_library: &clips::EventLibrary,
    slots: &clips::slots::Table,
    calibration: Option<&Value>,
    graph: &Path,
) -> Result<(Vec<Value>, Vec<Value>, usize)> {
    let mut converted = Vec::new();
    let mut unconverted = Vec::new();
    let mut with_events = 0;
    for (native_tag, source_tag, name, source_clip, counterpart) in pairs {
        let (native_tag, source_tag, name) = (*native_tag, *source_tag, *name);
        // Event-bearing clips ride on the native counterpart's event block, with the
        // source timing when both clips carry the same events.
        let result = clips::convert(&source_clip.0).or_else(|error| {
            if !format!("{error:#}").contains("event records") {
                return Err(error);
            }
            clips::convert_with_event_library(&source_clip.0, event_library).or_else(
                |source_error| {
                    if calibration.is_some() {
                        return Err(source_error);
                    }
                    clips::convert_with_events(&source_clip.0, &counterpart.0)
                        .with_context(|| "event carry")
                },
            )
        });
        let result = result.and_then(|(mut payload, report)| {
            slots.apply(&mut payload)?;
            Ok((payload, report))
        });
        match result {
            Ok((payload, report)) => {
                let file = format!("animation/clip-{native_tag:08X}.bin");
                fs::write(graph.join(&file), &payload.0)?;
                if report["event_count"].as_u64().unwrap_or(0) > 0 {
                    with_events += 1;
                }
                converted.push(json!({"native":native_tag,"source":source_tag,"name":name,"file":file,"streams":report["streams"],"events":report["events"]}));
            }
            Err(error) => unconverted.push(
                json!({"native":native_tag,"source":source_tag,"name":name,"reason":format!("{error:#}")}),
            ),
        }
    }
    Ok((converted, unconverted, with_events))
}

/// Every resolved holding pose must have converted.
fn require_pose_clips(
    pose_bindings: &[Value],
    converted: &[Value],
    unconverted: &[Value],
) -> Result<()> {
    for binding in pose_bindings {
        ensure!(
            converted
                .iter()
                .any(|clip| clip["native"] == binding["native"]
                    && clip["source"] == binding["source"]),
            "source holding pose did not convert: {:?}",
            unconverted
                .iter()
                .find(|clip| clip["native"] == binding["native"])
        );
    }
    Ok(())
}

/// Index converted clips by source tag with their frame counts and playback modes.
fn available_clips(
    converted: &[Value],
    modes: &BTreeMap<u32, u16>,
    graph: &Path,
) -> Result<BTreeMap<u32, bank::Clip>> {
    let mut available = BTreeMap::new();
    for row in converted {
        let tag = u32::try_from(row["native"].as_u64().context("converted clip")?)?;
        let Some(&mode) = modes.get(&tag) else {
            continue;
        };
        let payload = Payload(fs::read(
            graph.join(row["file"].as_str().context("clip file")?),
        )?);
        available.insert(
            u32::try_from(row["source"].as_u64().context("source clip")?)?,
            bank::Clip {
                tag,
                frames: payload.u16(0x13C)?,
                mode,
            },
        );
    }
    Ok(available)
}

/// Collect the source bank's descriptor clips and the playback modes their named
/// states establish.
fn clip_modes(
    sr: &Reader,
    source_bank: &Payload,
    state_modes: &BTreeMap<u32, u16>,
    descriptor_clips: &mut BTreeSet<u32>,
) -> Result<BTreeMap<u32, BTreeSet<u16>>> {
    let mut clip_modes = BTreeMap::<u32, BTreeSet<u16>>::new();
    for (index, row) in source_bank
        .array(0x58, 48, Some(0x80808BDE))?
        .into_iter()
        .enumerate()
    {
        if source_bank.bytes::<16>(row)? == [0; 16] && source_bank.f32(row + 20)? == 1.0 {
            let tag = sr.ref64(source_bank, row + 24)?;
            descriptor_clips.insert(tag);
            if let Some(mode) = state_modes.get(&(index as u32)) {
                clip_modes.entry(tag).or_default().insert(*mode);
            }
        }
    }
    Ok(clip_modes)
}

/// Try each native template for one extra source clip in bank order. Returns the
/// first conversion and the last rejection reason.
fn extra_clip(
    extras: &Extras<'_>,
    source_clip: &Payload,
    action_mode: Option<u16>,
    static_pose: bool,
    event_free: bool,
) -> Result<(Option<ExtraClip>, Option<String>)> {
    let mut result = None;
    let mut rejection = None;
    for (native_tag, template_tag, _, template, counterpart) in
        extras.pairs.iter().chain(extras.calibration_pairs)
    {
        let Some(mode) = action_mode.or_else(|| extras.modes.get(native_tag).copied()) else {
            continue;
        };
        if !static_pose && template.u16(0x142)? != source_clip.u16(0x142)? {
            continue;
        }
        let attempt = if event_free {
            if action_mode.is_none()
                && (template.u16(0x140)? != 2
                    || counterpart.u16(0x13C)? != 2
                    || template.u64(0x160)? != 0
                    || mode != 1)
            {
                continue;
            }
            clips::convert(&source_clip.0)
        } else {
            clips::convert_with_event_library(&source_clip.0, extras.event_library).or_else(
                |error| {
                    if extras.calibration.is_some() {
                        return Err(error);
                    }
                    clips::convert_with_event_subset(&source_clip.0, template, counterpart)
                },
            )
        }
        .and_then(|(mut payload, report)| {
            extras.slots.apply(&mut payload)?;
            Ok((payload, report))
        });
        match attempt {
            Ok((payload, report)) => {
                result = Some((*native_tag, *template_tag, mode, payload, report));
                break;
            }
            Err(error) => rejection = Some(format!("{error:#}")),
        }
    }
    Ok((result, rejection))
}

/// Add source clips that have no same-name native counterpart through the best
/// native template, keeping every rejection reason.
fn extra_clips(
    sr: &mut Reader,
    extras: &Extras<'_>,
    descriptor_clips: &BTreeSet<u32>,
    clip_modes: &BTreeMap<u32, BTreeSet<u16>>,
    files: &mut BTreeMap<String, String>,
    available: &mut BTreeMap<u32, bank::Clip>,
) -> Result<(Vec<Value>, Vec<Value>)> {
    let mut extra_clips = Vec::new();
    let mut extra_rejected = Vec::new();
    let mut visited = BTreeSet::new();
    for &(source_tag, name) in extras.source_clips {
        if (extras.calibration.is_none() && !descriptor_clips.contains(&source_tag))
            || !visited.insert(source_tag)
            || extras
                .converted
                .iter()
                .any(|r| r["source"].as_u64() == Some(u64::from(source_tag)))
        {
            continue;
        }
        let source_clip = sr.tag(source_tag, Some(0x80808BE0))?;
        // Playback mode comes from a unanimous same-name native action, or the
        // original event template. Event-free dynamic clips need that action evidence.
        let action_mode = clip_modes
            .get(&source_tag)
            .filter(|modes| modes.len() == 1)
            .and_then(|modes| modes.first())
            .copied();
        let static_pose = extras.calibration.is_some()
            && source_clip.u16(0x140)? == 2
            && [0xD8, 0xE8, 0xF8, 0x108]
                .into_iter()
                .all(|at| source_clip.u64(at).ok() == Some(0));
        let event_free = source_clip.u64(0x160)? == 0;
        if event_free && !static_pose && action_mode.is_none() {
            continue;
        }
        let (result, rejection) =
            extra_clip(extras, &source_clip, action_mode, static_pose, event_free)?;
        if let Some((template, source_template, mode, payload, report)) = result {
            ensure!(
                !extras.native_clips.iter().any(|c| c.0 == source_tag),
                "extra clip placeholder collides with a native tag"
            );
            let file = format!("animation/source-clip-{source_tag:08X}.bin");
            fs::write(extras.graph.join(&file), &payload.0)?;
            files.insert(format!("extra_clip_{source_tag:08X}"), file.clone());
            available.insert(
                source_tag,
                bank::Clip {
                    tag: source_tag,
                    frames: payload.u16(0x13C)?,
                    mode,
                },
            );
            extra_clips.push(json!({"source":source_tag,"name":name,"template":template,"source_template":source_template,"mode":mode,"mode_evidence":if action_mode.is_some() {"named_native_state"} else {"native_template"},"file":file,"events":report["events"]}));
        } else {
            extra_rejected.push(json!({"source":source_tag,"name":name,"reason":rejection.unwrap_or_else(|| "no compatible playback mode and clip slot mapping".into())}));
        }
    }
    Ok((extra_clips, extra_rejected))
}

/// Prepare the first-person clip substitution for one import. The result is
/// the graph's `animation` section: `first_person` is present only when the
/// chain was linked, otherwise `reason` says why the native animation stays.
pub fn prepare(
    modern_packages: &Path,
    native_packages: &Path,
    source_rig: &Value,
    native_rig: &Value,
    work: &Path,
    graph: &Path,
) -> Result<Value> {
    prepare_with_rig(
        modern_packages,
        native_packages,
        source_rig,
        native_rig,
        work,
        graph,
        None,
    )
}

/// An explicit calibration export enables a private source rig with its own weapon bones.
#[allow(clippy::too_many_arguments)]
pub fn prepare_with_rig(
    modern_packages: &Path,
    native_packages: &Path,
    source_rig: &Value,
    native_rig: &Value,
    work: &Path,
    graph: &Path,
    calibration: Option<&Value>,
) -> Result<Value> {
    let unavailable = |reason: String| json!({"first_person_status":"native","reason":reason,"gameplay_verified":false});
    let (Some(source), Some(native)) = (
        first_person(source_rig, SOURCE_LOOKUP)?,
        first_person(native_rig, NATIVE_LOOKUP)?,
    ) else {
        return Ok(unavailable(
            "source or native rig has no first-person entity".into(),
        ));
    };
    if let Some(reference) = calibration {
        super::super::owned::validate(source_rig, native_rig)?;
        super::super::owned::validate(reference, native_rig)?;
    }
    let map = match bone_map(
        &source.bones,
        if calibration.is_some() {
            &source.bones
        } else {
            &native.bones
        },
    ) {
        Ok(map) => map,
        Err(error) => {
            return Ok(unavailable(format!(
                "first-person skeletons cannot be mapped: {error:#}"
            )));
        }
    };
    let retargeted = map
        .iter()
        .enumerate()
        .filter(|(i, m)| **m != u16::MAX && usize::from(**m) != *i)
        .count();
    let unmapped = map.iter().filter(|m| **m == u16::MAX).count();
    let runtime_entity = hex(&native_rig["runtime_entity"])?;
    let attachment_owners = native_rig["components"]
        .as_array()
        .context("native components")?
        .iter()
        .filter(|c| c["class"] == NATIVE_ATTACHMENTS && c["entity"] == native_rig["runtime_entity"])
        .map(|c| hex(&c["owner"]))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        attachment_owners.len() == 1,
        "native first-person attachment owner missing or ambiguous"
    );
    let attachment_owner = attachment_owners[0];

    let mut sr = Reader::new(modern_packages, &work.join("source"), true)?;
    let (source_bank_tag, source_clips) = bank(&mut sr, source.lookup_owner, true)?;
    let source_bank = sr.tag(source_bank_tag, Some(0x8080289F))?;
    let mut nr = Reader::new(native_packages, &work.join("native"), false)?;
    let (bank_tag, native_clips) = bank(&mut nr, native.lookup_owner, false)?;
    let attachments = nr.tag(attachment_owner, Some(0x80809C36))?;
    let entity = nr.tag(native.entity, Some(0x80809C0F))?;
    let lookup = nr.tag(native.lookup_owner, Some(0x80809C36))?;
    let bank_payload = nr.tag(bank_tag, Some(0x808036F6))?;
    let mut attachment_profile = match profile::prepare(
        &mut sr,
        &mut nr,
        source_rig,
        native_rig,
        &attachments,
        &lookup,
    ) {
        Ok(patch) => json!({"status":"source","patch":patch}),
        Err(error) => json!({"status":"native","reason":format!("{error:#}")}),
    };
    ensure!(
        word_occurrences(&attachments.0, native.entity) > 0
            && word_occurrences(&entity.0, native.lookup_owner) > 0
            && word_occurrences(&lookup.0, bank_tag) > 0,
        "native first-person chain links are not plain tag words"
    );

    let out = graph.join("animation");
    fs::create_dir_all(&out)?;
    let mut rigs = if calibration.is_some() {
        super::super::owned::prepare(source_rig, native_rig, &mut sr, &mut nr, graph)?
    } else {
        json!([])
    };
    let mut files = chain_files(&out, &attachments, &entity, &lookup, &bank_payload)?;
    let bank_consumers = consumers::prepare(
        &mut nr,
        native_rig,
        native.entity,
        bank_tag,
        graph,
        &mut files,
    )?;
    // Clip tracks address slots, not bones, and the two engines order their slots
    // differently. Derive the correspondence from every name-paired clip before converting
    // any of them; a donor whose clips do not align cleanly keeps its native clips.
    let mut pairs = clip_pairs(&mut sr, &mut nr, &native_clips, &source_clips)?;
    let calibration_pairs = calibration_clips(&mut sr, &mut nr, &native_clips, calibration)?;
    let slots = match clips::slots::Table::derive(
        pairs
            .iter()
            .chain(&calibration_pairs)
            .map(|p| (p.3.as_ref(), p.4.as_ref())),
    ) {
        Ok(slots) => slots,
        Err(error) => {
            sr.finish()?;
            nr.finish()?;
            return Ok(unavailable(format!(
                "clip track slots cannot be aligned: {error:#}"
            )));
        }
    };
    let alignment = slots
        .shapes()
        .iter()
        .map(|(bones, shape)| {
            json!({"bones":bones,"modern_slots":shape.map.len(),"native_slots":shape.native_slots,"skipped_modern_slots":shape.skipped()})
        })
        .collect::<Vec<_>>();
    if let Some(reference) = calibration {
        let skeletons = rigs.as_array().context("source rig owners")?.clone();
        for skeleton in skeletons {
            let bones = usize::try_from(
                skeleton["conversion"]["bones"]
                    .as_u64()
                    .context("source rig bone count")?,
            )?;
            let first_person = skeleton["first_person"]
                .as_bool()
                .context("source rig target")?;
            let controls = controls::prepare(
                &mut sr,
                &mut nr,
                source_rig,
                native_rig,
                reference,
                &slots,
                bones,
                first_person,
                graph,
            )?;
            rigs.as_array_mut()
                .context("source rig owners")?
                .push(controls);
        }
        if let Some(markers) = markers::prepare(&mut sr, &mut nr, source_rig, native_rig, graph)? {
            rigs.as_array_mut()
                .context("source rig owners")?
                .push(markers);
        }
    }
    let slot_markers = slots
        .slot_markers()
        .iter()
        .map(|hash| format!("{hash:08X}"))
        .collect::<Vec<_>>();
    let event_library = events(
        &mut sr,
        &mut nr,
        calibration,
        &pairs,
        &calibration_pairs,
        &native_clips,
    )?;
    let mut pose_bindings = Vec::new();
    let mut pose_layers = Value::Null;
    if calibration.is_some() {
        let profile =
            profile::requested_with_category(&mut sr, source_rig, native_rig, &attachments, true)?;
        let (mut bindings, layers) = poses::bindings(
            &mut sr,
            &mut nr,
            source_rig,
            native_rig,
            &source_clips,
            &native_clips,
            &profile,
            graph,
        )?;
        bindings.extend(motion::bindings(
            &mut sr,
            &mut nr,
            source_rig,
            native_rig,
            &source_clips,
            &native_clips,
        )?);
        pose_layers = layers;
        for (native_tag, source_tag, layer) in bindings {
            let source_clip = sr.tag(source_tag, Some(0x80808BE0))?;
            let counterpart = nr.tag(native_tag, Some(0x80808F49))?;
            // Calibration comes only from equal-name clips above. Deliberately
            // different holding poses must never influence slot derivation.
            pairs.retain(|p| p.0 != native_tag);
            pairs.push((
                native_tag,
                source_tag,
                source_clip.u32(0x120)?,
                source_clip,
                counterpart,
            ));
            pose_bindings.push(json!({"native":native_tag,"source":source_tag,"layer":layer}));
        }
        ensure!(
            !pose_bindings.is_empty(),
            "source-owned rig has no resolved source holding pose"
        );
    }
    let (converted, unconverted, with_events) =
        convert_pairs(&pairs, &event_library, &slots, calibration, graph)?;
    require_pose_clips(&pose_bindings, &converted, &unconverted)?;
    let unique_native = native_clips
        .iter()
        .map(|(tag, _)| *tag)
        .collect::<BTreeSet<_>>()
        .len();
    let unmatched = unique_native - converted.len() - unconverted.len();
    let modes = bank::modes(&bank_payload, &native_clips)?;
    let mut available = available_clips(&converted, &modes, graph)?;
    let state_modes = if calibration.is_some() {
        let source_lookup = sr.tag(source.lookup_owner, Some(0x80809B06))?;
        let source_at = source_lookup.pointer(24)?;
        let native_at = lookup.pointer(24)?;
        let source_parameters = sr.tag(source_lookup.u32(source_at + 0xAC)?, Some(0x80808AB6))?;
        let native_parameters = nr.tag(lookup.u32(native_at + 0x94)?, Some(0x80808EE1))?;
        let source_states = sr.tag(source_lookup.u32(source_at + 0xB4)?, Some(0x80802612))?;
        let native_states = nr.tag(lookup.u32(native_at + 0x9C)?, Some(0x80803465))?;
        states::playback_modes(
            &source_parameters,
            &native_parameters,
            &source_states,
            &native_states,
            &bank_payload,
        )?
    } else {
        BTreeMap::new()
    };
    let mut descriptor_clips = BTreeSet::new();
    let clip_modes = clip_modes(&sr, &source_bank, &state_modes, &mut descriptor_clips)?;
    let extras = Extras {
        calibration,
        graph,
        source_clips: &source_clips,
        native_clips: &native_clips,
        converted: &converted,
        pairs: &pairs,
        calibration_pairs: &calibration_pairs,
        modes: &modes,
        event_library: &event_library,
        slots: &slots,
    };
    let (extra_clips, extra_rejected) = extra_clips(
        &mut sr,
        &extras,
        &descriptor_clips,
        &clip_modes,
        &mut files,
        &mut available,
    )?;
    let (converted_bank, descriptor_map, bank_report) = bank::convert(
        &mut sr,
        &source_bank,
        &bank_payload,
        &native_clips,
        &available,
    )?;
    pose_layers = dispatch::prepare(
        &mut sr,
        &mut nr,
        source_rig,
        native_rig,
        &source_bank,
        &bank_payload,
        &converted_bank,
        &descriptor_map,
        graph,
        pose_layers,
    )?;
    fs::write(
        out.join("first-person-bank-converted.bin"),
        &converted_bank.0,
    )?;
    files.insert(
        "converted_bank".into(),
        "animation/first-person-bank-converted.bin".into(),
    );
    let state_conversion = (|| -> Result<Value> {
        let source_lookup = sr.tag(source.lookup_owner, Some(0x80809B06))?;
        let source_at = source_lookup.pointer(24)?;
        let native_at = lookup.pointer(24)?;
        let source_parameters = sr.tag(source_lookup.u32(source_at + 0xAC)?, Some(0x80808AB6))?;
        let source_states = sr.tag(source_lookup.u32(source_at + 0xB4)?, Some(0x80802612))?;
        let parameters_tag = lookup.u32(native_at + 0x94)?;
        let states_tag = lookup.u32(native_at + 0x9C)?;
        let native_parameters = nr.tag(parameters_tag, Some(0x80808EE1))?;
        let native_states = nr.tag(states_tag, Some(0x80803465))?;
        let profile = profile::requested_with_category(
            &mut sr,
            source_rig,
            native_rig,
            &attachments,
            calibration.is_some(),
        )?;
        let converted = if calibration.is_some() {
            states::convert_for_profile(
                &source_parameters,
                &native_parameters,
                &source_states,
                &native_states,
                &descriptor_map,
                &[profile.category, profile.after, profile.before],
                Some(profile.after),
                Some((profile.after, profile.before)),
            )?
        } else {
            states::convert(
                &source_parameters,
                &native_parameters,
                &source_states,
                &native_states,
                &descriptor_map,
                &[profile.category, profile.after, profile.before],
            )?
        };
        profile::recognized(
            &crate::d2_mot::payload::Payload(converted.parameters.clone()),
            &profile,
        )?;
        for (key, name, bytes) in [
            (
                "parameters_template",
                "first-person-parameters-template.bin",
                &native_parameters.0,
            ),
            (
                "states_template",
                "first-person-states-template.bin",
                &native_states.0,
            ),
            (
                "parameters",
                "first-person-parameters.bin",
                &converted.parameters,
            ),
            ("states", "first-person-states.bin", &converted.states),
        ] {
            fs::write(out.join(name), bytes)?;
            files.insert(key.to_owned(), format!("animation/{name}"));
        }
        attachment_profile = json!({"status":"source","patch":profile});
        Ok(
            json!({"status":"source_states","parameters":parameters_tag,"states":states_tag,"source_states":converted.source_states,"native_fallback_states":converted.fallback_states,"profile_choices":converted.profile_choices}),
        )
    })();
    let state_conversion = match state_conversion {
        Ok(report) => report,
        Err(error) => {
            ensure!(
                calibration.is_none(),
                "source-owned rig needs converted state selectors: {error:#}"
            );
            json!({"status":"native","reason":format!("{error:#}")})
        }
    };
    sr.finish()?;
    nr.finish()?;
    let section = json!({
        "first_person_status": if converted.is_empty() { "native" } else { "linked" },
        "runtime_entity": runtime_entity,
        "first_person": {
            "attachment_owner": attachment_owner,
            "entity": native.entity,
            "lookup_owner": native.lookup_owner,
            "bank": bank_tag,
            "bank_consumers": bank_consumers,
            "rigs": rigs,
            "rig_calibration": calibration.map(|r| json!({"item":r["item_tag"],"runtime_entity":r["runtime_entity"]})),
            "descriptor_conversion": bank_report,
            "extra_clips": extra_clips,
            "extra_unconverted": extra_rejected,
            "skeleton_bones": if calibration.is_some() { source.bones.len() } else { native.bones.len() },
            "native_clips": native_clips.len(),
            "unique_native_clips": unique_native,
            "source_clips": source_clips.len(),
            "unmatched_native_clips": unmatched,
            "clips_with_carried_events": with_events,
            "retargeted_bones": retargeted,
            "unmapped_source_bones": unmapped,
            "slot_alignment": alignment,
            "slot_markers": slot_markers,
            "pose_bindings": pose_bindings,
            "pose_layers": pose_layers,
            "files": files,
            "attachment_profile": attachment_profile,
            "state_conversion": state_conversion,
            "clips": converted,
            "unconverted": unconverted,
        },
        "gameplay_verified": false,
    });
    write_json(&out.join("animation.json"), &section)?;
    Ok(section)
}

/// Where a prepared graph keeps its animation payloads.
pub fn directory(graph: &Path) -> PathBuf {
    graph.join("animation")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bone(index: u64, name: &str, parent: i64) -> Value {
        json!({"index":index,"name_hash":name,"parent":parent})
    }

    #[test]
    fn bones_map_by_name_and_reject_changed_parents() {
        let a = vec![
            bone(0, "root", -1),
            bone(1, "hand", 0),
            bone(2, "finger", 1),
        ];
        assert_eq!(bone_map(&a, &a).unwrap(), vec![0, 1, 2]);

        // Geometry bone order is independent of the controller's clip slot order.
        let mut flat = a.clone();
        flat[2] = bone(2, "finger", 0);
        let swapped = vec![
            bone(0, "root", -1),
            bone(1, "finger", 0),
            bone(2, "hand", 0),
        ];
        assert_eq!(bone_map(&flat, &swapped).unwrap(), vec![0, 2, 1]);

        // A source bone the native rig lacks maps to the sentinel.
        let extra = vec![
            bone(0, "root", -1),
            bone(1, "hand", 0),
            bone(2, "spare", 0),
            bone(3, "finger", 1),
        ];
        assert_eq!(bone_map(&extra, &a).unwrap(), vec![0, 1, u16::MAX, 2]);

        // A bone that changes parent is refused rather than silently remapped.
        let reparented = vec![
            bone(0, "root", -1),
            bone(1, "hand", 0),
            bone(2, "finger", 0),
        ];
        assert!(bone_map(&a, &reparented).is_err());

        // Repeated names on either side make the mapping ambiguous.
        let repeated = vec![bone(0, "root", -1), bone(1, "hand", 0), bone(2, "hand", 1)];
        assert!(bone_map(&repeated, &a).is_err());
        assert!(bone_map(&a, &repeated).is_err());
    }

    #[test]
    fn clips_pair_only_through_names_unique_on_both_sides() {
        let native = [
            (0x80C0_0001, 10),
            (0x80C0_0002, 20),
            (0x80C0_0003, 30),
            (0x80C0_0004, 30),
        ];
        let source = [
            (0x80A0_0009, 20),
            (0x80A0_0008, 10),
            (0x80A0_0007, 40),
            (0x80A0_0006, 20),
        ];
        // Name 20 repeats in the source and name 30 repeats in the native bank.
        assert_eq!(
            matched(&native, &source),
            vec![(0x80C0_0001, 0x80A0_0008, 10)]
        );
        assert!(matched(&native, &[]).is_empty());
        // Repeated bank slots for one asset must all use its converted payload.
        let aliases = [(0x80C0_0001, 10), (0x80C0_0001, 10)];
        let source_aliases = [(0x80A0_0008, 10), (0x80A0_0008, 10)];
        assert_eq!(
            matched(&aliases, &source_aliases),
            vec![(0x80C0_0001, 0x80A0_0008, 10)]
        );
    }

    #[test]
    fn first_person_entity_is_the_attached_entity_with_one_skeleton_and_lookup() {
        let rig = json!({
            "runtime_entity": "80C69D8C",
            "components": [
                {"entity":"80C69D8C","owner":"80C239CC","class":"80808546"},
                {"entity":"80C69D8C","owner":"80C69D2D","class":"8080344B"},
                {"entity":"80C69D88","owner":"80C69D33","class":"80808546"},
                {"entity":"80C69D88","owner":"81525320","class":"8080344B"}
            ],
            "skeletons": [
                {"owner":"80C239CC","bones":[bone(0,"root",-1)]},
                {"owner":"80C69D33","bones":[bone(0,"root",-1),bone(1,"hand",0)]}
            ]
        });
        let fp = first_person(&rig, NATIVE_LOOKUP).unwrap().unwrap();
        assert_eq!(
            (fp.entity, fp.lookup_owner, fp.bones.len()),
            (0x80C69D88, 0x81525320, 2)
        );
        let mut weapon_only = rig.clone();
        weapon_only["components"] =
            json!([{"entity":"80C69D8C","owner":"80C239CC","class":"80808546"}]);
        assert!(first_person(&weapon_only, NATIVE_LOOKUP).unwrap().is_none());
        let mut no_lookup = rig.clone();
        no_lookup["components"][3]["class"] = json!("80803640");
        assert!(first_person(&no_lookup, NATIVE_LOOKUP).is_err());
    }
}

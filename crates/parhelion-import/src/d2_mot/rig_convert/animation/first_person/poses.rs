//! Resolve profile-selected base poses through the controller's named layers.
//!
//! Action descriptors do not address these clips. The controller keeps native
//! bank ordinals, so replace the clip in that binding with the selected source
//! pose instead of merely appending an unreachable source clip to the bank.
use super::*;
use crate::d2_mot::payload::Payload;

struct Choice {
    tag: u32,
    entry: usize,
    driver: [u8; 16],
    profile_selected: bool,
}

fn static_pose(p: &Payload, frames: usize) -> Result<bool> {
    Ok(p.u16(frames)? == 2
        && p.u64(0x160)? == 0
        && [0xD8, 0xE8, 0xF8, 0x108]
            .into_iter()
            .all(|at| p.u64(at).ok() == Some(0)))
}

pub(crate) fn controller(
    reader: &mut Reader,
    rig: &Value,
    modern: bool,
) -> Result<(u32, u32, Payload)> {
    let class = if modern { "8080263C" } else { "80803640" };
    let owners = rig["components"]
        .as_array()
        .context("pose controller components")?
        .iter()
        .filter(|c| c["entity"] != rig["runtime_entity"] && c["class"] == class)
        .collect::<Vec<_>>();
    ensure!(owners.len() == 1, "pose controller is missing or ambiguous");
    let owner_tag = hex(&owners[0]["owner"])?;
    let owner = reader.tag(owner_tag, None)?;
    let field = owner.pointer(24)? + if modern { 0x14C } else { 0x11C };
    let layer = owner.u32(field)?;
    Ok((
        owner_tag,
        layer,
        (*reader.tag(layer, Some(if modern { 0x8080269C } else { 0x808034EB }))?).clone(),
    ))
}

/// Rank a choice's conditions against the profile: 2 for the profile itself and
/// 1 for its category. The outer `None` marks a condition outside the attachment
/// profile hierarchy. The inner `None` marks a choice the profile does not select.
fn rank(
    table: &Payload,
    conditions: &[usize],
    profile: &profile::Patch,
) -> Result<Option<Option<u32>>> {
    let mut rank = 0;
    let mut matches = true;
    for &condition in conditions {
        // Builtin 3 is the attachment profile hierarchy. Other inputs
        // vary during play and cannot be specialized at import time.
        if table.u32(condition)? != 3 {
            return Ok(None);
        }
        let name = table.u32(condition + 4)?;
        if name == profile.after {
            rank = rank.max(2);
        } else if name == profile.category {
            rank = rank.max(1);
        } else {
            matches = false;
        }
    }
    Ok(Some(matches.then_some(rank)))
}

fn choices(
    table: &Payload,
    row: usize,
    modern: bool,
    clips: &[(u32, u32)],
    profile: &profile::Patch,
) -> Result<Option<Vec<Choice>>> {
    let mut result = Vec::new();
    for choice in table.array(row, 16, Some(if modern { 0x80802903 } else { 0x8080372E }))? {
        let mut selected: Option<(u32, Choice)> = None;
        for at in table.array(
            choice,
            if modern { 80 } else { 64 },
            Some(if modern { 0x80802905 } else { 0x80803730 }),
        )? {
            let conditions = table.array(
                at + if modern { 0 } else { 16 },
                8,
                Some(if modern { 0x8080290B } else { 0x80803736 }),
            )?;
            let Some(rank) = rank(table, &conditions, profile)? else {
                return Ok(None);
            };
            let Some(rank) = rank else {
                continue;
            };
            let weighted = table.array(
                at + if modern { 16 } else { 0 },
                8,
                Some(if modern { 0x8080290C } else { 0x80803737 }),
            )?;
            if weighted.len() != 1 || table.f32(weighted[0] + 4)? != 1.0 {
                return Ok(None);
            }
            let tag = clips
                .get(table.u32(weighted[0])? as usize)
                .context("pose layer clip is outside the bank")?
                .0;
            let value = Choice {
                tag,
                entry: at,
                driver: table.bytes::<16>(at + 32)?,
                profile_selected: rank == 2,
            };
            match &selected {
                Some((old_rank, old)) if *old_rank == rank => {
                    ensure!(old.tag == tag, "pose profile selects different clips");
                }
                Some((old_rank, _)) if *old_rank > rank => {}
                _ => selected = Some((rank, value)),
            }
        }
        let Some((_, value)) = selected else {
            return Ok(None);
        };
        result.push(value);
    }
    Ok(Some(result))
}

/// Native pose clip tags with the source clip tag and layer name that replace
/// each of them, and the pose layer report.
type Bindings = (Vec<(u32, u32, u32)>, Value);

/// Static source poses selected by the authored attachment profile. These do
/// not need new controller operations or a speculative cross-family rig map.
#[allow(clippy::too_many_arguments)]
pub(super) fn bindings(
    sr: &mut Reader,
    nr: &mut Reader,
    source_rig: &Value,
    native_rig: &Value,
    source_clips: &[(u32, u32)],
    native_clips: &[(u32, u32)],
    profile: &profile::Patch,
    graph: &Path,
) -> Result<Bindings> {
    let (_, _, source) = controller(sr, source_rig, true)?;
    let (owner, tag, native) = controller(nr, native_rig, false)?;
    let mut output = native.clone();
    let mut controls = Vec::new();
    let mut groups = BTreeMap::new();
    for row in native.array(8, 24, Some(0x80803724))? {
        ensure!(
            groups.insert(native.u32(row + 16)?, row).is_none(),
            "duplicate native pose layer"
        );
    }
    let mut result = BTreeMap::new();
    for row in source.array(8, 32, Some(0x808028F9))? {
        let name = source.u32(row + 16)?;
        let Some(&native_row) = groups.get(&name) else {
            continue;
        };
        let Some(from) = choices(&source, row, true, source_clips, profile)? else {
            continue;
        };
        let mut has_pose = false;
        for choice in from.iter().filter(|c| c.profile_selected) {
            let clip = sr.tag(choice.tag, Some(0x80808BE0))?;
            has_pose |= static_pose(&clip, 0x140)?;
        }
        if !has_pose {
            continue;
        }
        // The native carrier's conditions belong to its original profile.
        let mut original = profile.clone();
        original.after = profile.before;
        original.category = profile.previous_category.unwrap_or(profile.category);
        let Some(mut to) = choices(&native, native_row, false, native_clips, &original)? else {
            continue;
        };
        if from.len() != to.len() {
            // Some native controllers sample one base clip through two drivers.
            // Both bindings still address the same bank slot. Collapse that
            // alias only, leaving repeated layers with equal drivers intact.
            let mut index = 1;
            while index < to.len() {
                if to[index - 1].tag == to[index].tag && to[index - 1].driver != to[index].driver {
                    // Keep the native controller's choice ordinals. Its extra
                    // alias has no source counterpart and must contribute zero.
                    let entry = to[index].entry;
                    let constant_zero = [1u32, 0, 0xFFFF, 0];
                    for (i, word) in constant_zero.into_iter().enumerate() {
                        output.0[entry + 32 + i * 4..entry + 36 + i * 4]
                            .copy_from_slice(&word.to_le_bytes());
                    }
                    controls.push(json!({"entry":entry,"disabled_alias":true}));
                    to.remove(index);
                } else {
                    index += 1;
                }
            }
        }
        ensure!(
            from.len() == to.len(),
            "profile-selected pose layer {name:08X} changes its binding shape"
        );
        for (from, to) in from.iter().zip(&to) {
            if !from.profile_selected {
                continue;
            }
            let source_clip = sr.tag(from.tag, Some(0x80808BE0))?;
            let native_clip = nr.tag(to.tag, Some(0x80808F49))?;
            if !static_pose(&source_clip, 0x140)? || !static_pose(&native_clip, 0x13C)? {
                continue;
            }
            // The profile's static base pose has a constant playback driver.
            // Retaining the carrier's variable driver changes the pose even
            // when its bank slot points at the correct source clip.
            ensure!(
                source.u32(from.entry + 32)? == 1
                    && source.f32(from.entry + 36)? == 1.0
                    && source.u32(from.entry + 40)? == 0xFFFF
                    && source.u32(from.entry + 44)? == 0
                    && source.f32(from.entry + 48)? == 1.0
                    && source.u16(from.entry + 52)? == 0
                    && source.u32(from.entry + 54)? == 0xFFFF
                    && source.u16(from.entry + 58)? == 0
                    && source.u32(from.entry + 60)? == 1
                    && native.u32(to.entry + 56)? == 0xFFFF
                    && native.u32(to.entry + 60)? == 0x01000000,
                "profile pose needs unsupported playback controls"
            );
            output.0[to.entry + 32..to.entry + 52]
                .copy_from_slice(&source.0[from.entry + 32..from.entry + 52]);
            controls
                .push(json!({"entry":to.entry,"source_entry":from.entry,"constant_driver":1.0}));
            if let Some((previous, _)) = result.insert(to.tag, (from.tag, name)) {
                ensure!(
                    previous == from.tag,
                    "native pose alias selects different source poses"
                );
            }
        }
    }
    let file = "animation/first-person-pose-layers.bin";
    let template_file = "animation/first-person-pose-layers-template.bin";
    fs::write(graph.join(file), &output.0)?;
    fs::write(graph.join(template_file), &native.0)?;
    Ok((
        result
            .into_iter()
            .map(|(native, (source, layer))| (native, source, layer))
            .collect(),
        json!({"owner":owner,"tag":tag,"file":file,"template_file":template_file,"controls":controls}),
    ))
}

//! Compile the selected source controller into a linked native first-person graph section.
use super::*;

mod emission;
mod pairing;
mod plan;
mod translation;

/// The graph's `animation` section and the private equipment palette its geometry skins to.
pub(in crate::marathon) struct Compiled {
    pub section: Value,
    pub palette: Vec<u32>,
}

/// Native action names whose source alias reads the reload ammunition builtin, with the source
/// value that native request corresponds to. Native requests the empty and rounds-remaining
/// reloads by name, while the source names both and branches on `empty` and `partial`.
pub(super) const RELOAD: u8 = 18;
pub(super) const RELOADS: [(&str, &str); 2] =
    [("reload_empty", "empty"), ("reload_full", "partial")];
/// Movement actions take no weapon-local clip by length. Their clips move the arms, and a
/// weapon clip of matching length is a coincidence: one examined shotgun's 13-frame
/// `sprint_enter` matched its 14-frame fire clip and racked the pump on every sprint.
const MOVEMENT: [&str; 10] = [
    "sprint_enter",
    "sprint_loop",
    "sprint_exit",
    "jump",
    "fall",
    "land",
    "crouch_slide",
    "throw_grenade",
    "melee",
    "iron_sight_jump",
];
/// Builtins the source weapon publishes: the animation profile and the optic type.
pub(super) const PROFILE: u8 = 3;
const OPTIC: u8 = 9;

type Owner = (u32, std::sync::Arc<Payload>, usize);

fn one(
    owners: &[(u32, std::sync::Arc<Payload>, usize, u32)],
    class: u32,
    what: &str,
) -> Result<Owner> {
    let selected = owners
        .iter()
        .filter(|o| o.3 == class)
        .map(|o| (o.0, o.1.clone(), o.2))
        .collect::<Vec<_>>();
    ensure!(selected.len() == 1, "Source {what} is absent or ambiguous");
    Ok(selected.into_iter().next().unwrap())
}

/// How the arm rig's weapon section covers the weapon skeleton. The arm rig roots its weapon
/// section at the weapon root. Weapon bones below the root take weapon-local motion where a
/// weapon-local descriptor accompanies the arm clip. Otherwise a bone the arm rig lacks keeps its
/// rest place under its weapon parent, and a bone the arm rig parents elsewhere is placed from
/// its arm object transform, which does not depend on the parent.
fn weapon_section(arms: &source::Rig, weapon: &source::Rig) -> Result<Value> {
    let handle = arms.role("handle")?;
    ensure!(
        arms.names[handle] == weapon.names[0],
        "Source arm weapon root differs from the weapon skeleton"
    );
    let mut without_channel = Vec::new();
    let mut other_parent = Vec::new();
    for (bone, name) in weapon.names.iter().enumerate().skip(1) {
        let parent = weapon.parents[bone].context("Source weapon bone has no parent")?;
        match arms.bone(*name) {
            None => without_channel.push(format!("{name:08X}")),
            Some(arm) if arms.parents[arm].map(|p| arms.names[p]) != Some(weapon.names[parent]) => {
                other_parent.push(format!("{name:08X}"));
            }
            Some(_) => {}
        }
    }
    Ok(json!({"without_arm_channel":without_channel,"other_arm_parent":other_parent}))
}

/// Actions whose translation failure refuses the whole controller. Other actions without a
/// complete translation keep the carrier's native state and are reported.
const REQUIRED: [&str; 8] = [
    "idle",
    "ready",
    "put_away",
    "fire",
    "iron_sight_idle",
    "iron_sight_fire",
    "reload_full",
    "reload_empty",
];

/// Set one weight track of a uniform scalar weight codec `80808F7C` to the constant zero. A
/// selector whose top two bits are 2 reads its track's bias as a constant, and ranges and biases
/// are track-indexed, so the codec keeps its size and every other track.
fn silence(envelope: &mut Payload, field: usize, track: usize) -> Result<()> {
    if envelope.u64(field)? == 0 {
        return Ok(());
    }
    let base = envelope.pointer(field)?;
    ensure!(
        envelope.u32(base - 4)? == 0x80808F7C && envelope.u16(base)? == 2,
        "Constraint weight at {field:X} is not a uniform scalar codec"
    );
    let selectors = envelope.array(base + 0x10, 2, Some(0x80808F82))?;
    let ranges = envelope.array(base + 0x30, 4, Some(0x8080000F))?;
    let biases = envelope.array(base + 0x40, 4, Some(0x8080000F))?;
    let tracks = usize::from(envelope.u16(base + 2)?);
    ensure!(
        track < tracks
            && selectors.len() == tracks
            && ranges.len() == tracks
            && biases.len() == tracks,
        "Constraint weight tracks do not cover the bindings"
    );
    put(
        &mut envelope.0,
        selectors[track],
        &(0x8000 | u16::try_from(track)?).to_le_bytes(),
    )?;
    put(&mut envelope.0, ranges[track], &0f32.to_le_bytes())?;
    put(&mut envelope.0, biases[track], &0f32.to_le_bytes())?;
    Ok(())
}

/// Constraint bindings address control slots. Remap them by bone name onto the native rig. Only
/// the shape stock first-person clips use is admitted: mode 3 with a target name the carrier's
/// runtime markers or the source weapon placement publish. Returns the target names used and the
/// rows disabled.
///
/// Every examined stock first-person row is mode 3 and no stock clip gives one slot pair two
/// targets. A source row with another mode names a target domain the native target cache never
/// fills for a weapon. Such a row keeps its place but takes a constant zero weight and the mode
/// 3 target of another row on the same slots, so the limb follows the clip's own motion while
/// the source would have pulled it to the unresolved target.
fn bindings(
    envelope: &mut Payload,
    slots: &[Option<usize>],
    targets: &BTreeSet<u32>,
) -> Result<(BTreeSet<u32>, Vec<Value>)> {
    let mut used = BTreeSet::new();
    let mut disabled = Vec::new();
    if envelope.u64(0x30)? == 0 {
        return Ok((used, disabled));
    }
    let base = envelope.pointer(0x30)?;
    ensure!(
        envelope.u32(base - 4)? == 0x80808F59,
        "Constraint binding container differs"
    );
    let rows = envelope.array(base, 64, Some(0x80808F5C))?;
    let mut translated = BTreeMap::<(u16, u16), u32>::new();
    let mut pending = Vec::new();
    for (index, &row) in rows.iter().enumerate() {
        for at in [row, row + 2] {
            let slot = usize::from(envelope.u16(at)?);
            let native = slots
                .get(slot)
                .copied()
                .flatten()
                .context("Constraint binding names a source control without a native slot")?;
            put(&mut envelope.0, at, &u16::try_from(native)?.to_le_bytes())?;
        }
        ensure!(
            envelope.u16(row + 16)? == u16::MAX && envelope.u8(row + 19)? == 0,
            "Constraint binding names a bone target"
        );
        let target = envelope.u32(row + 20)?;
        let pair = (envelope.u16(row)?, envelope.u16(row + 2)?);
        if envelope.u8(row + 18)? != 3 {
            pending.push((index, row, pair, envelope.u8(row + 18)?, target));
            continue;
        }
        ensure!(
            targets.contains(&target),
            "Constraint target {target:08X} is neither a native runtime marker nor a source placement"
        );
        translated.entry(pair).or_insert(target);
        used.insert(target);
    }
    for (index, row, pair, mode, target) in pending {
        let substitute = *translated.get(&pair).with_context(|| {
            format!("Constraint target mode {mode} has no mode 3 row on the same slots")
        })?;
        silence(envelope, 0x38, index)?;
        silence(envelope, 0x40, index)?;
        put(&mut envelope.0, row + 18, &[3])?;
        put(&mut envelope.0, row + 20, &substitute.to_le_bytes())?;
        disabled.push(
            json!({"row":index,"source_mode":mode,"source_target":format!("{target:08X}"),
            "substitute_target":format!("{substitute:08X}")}),
        );
    }
    Ok((used, disabled))
}

struct Authored {
    payload: Payload,
    frames: u16,
    composition: u8,
    targets: BTreeSet<u32>,
    report: Value,
}

/// A lowered clip's frame count, control count and the leading controls whose groups match the
/// rig. The mixer stops at the first group that differs from the rig.
fn layout(candidate: &Payload, rig: &source::Rig) -> Result<(u16, usize, usize)> {
    let frames = candidate.u16(0x13C)?;
    let slots = usize::from(candidate.u16(0x140)?);
    let groups = candidate
        .array(0x170, 8, Some(0x8080907F))?
        .into_iter()
        .map(|at| Ok((candidate.u32(at)?, candidate.u32(at + 4)? as usize)))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        groups.iter().map(|g| g.1).sum::<usize>() == slots,
        "Source clip groups do not cover its controls"
    );
    let common = groups
        .iter()
        .zip(&rig.groups)
        .take_while(|(a, b)| a == b)
        .map(|(g, _)| g.1)
        .sum::<usize>();
    ensure!(
        common > 0,
        "Source clip shares no control group with its rig"
    );
    Ok((frames, slots, common))
}

/// Weapon-local motion: per frame, each weapon bone's pose in the weapon root frame, in the
/// order of [`Overlay::bones`].
pub(super) struct Mechanism {
    name: u32,
    pub frames: Vec<Vec<Pose>>,
}

/// The weapon bones below the weapon root, as native control slots with their weapon bone, the
/// bind correction of each, and the weapon-local motion they take.
struct Overlay<'a> {
    bones: &'a [(usize, usize)],
    corrections: &'a [Pose],
    mechanism: &'a Mechanism,
    native_handle: usize,
}

/// Sample one absolute weapon-local clip on the weapon rig.
pub(super) fn mechanism(
    source: &mut Reader,
    local: &controller::Controller,
    descriptor: u32,
    weapon: &source::Rig,
    bones: &[(usize, usize)],
) -> Result<Mechanism> {
    let entry = &local.descriptors[descriptor as usize];
    let ordinal = entry.clip.context("Weapon-local descriptor is unbound")?;
    ensure!(
        entry.rate == 1.,
        "Weapon-local descriptor playback rate is untranslated"
    );
    let original = source.tag(local.clips[ordinal], Some(0x8080AE01))?;
    // Only the poses are sampled. Weapon-local events are not carried.
    let (candidate, _) = clip::lower(&original, Some(&clip::Vocabulary::new()))?;
    ensure!(
        candidate.u8(0x181)? == 0,
        "Weapon-local clip is not absolute"
    );
    let (frames, slots, common) = layout(&candidate, weapon)?;
    ensure!(frames > 0, "Weapon-local clip has no frames");
    ensure!(
        common == weapon.controls.len(),
        "Weapon-local clip addresses controls outside the weapon rig"
    );
    let defaults = (0..slots)
        .map(|slot| {
            if slot < common {
                weapon.defaults[slot]
            } else {
                Pose::IDENTITY.0
            }
        })
        .collect::<Vec<_>>();
    let mut sampled = Vec::with_capacity(usize::from(frames));
    for frame in 0..usize::from(frames) {
        let values = native_clip::sample(&candidate, &defaults, frame)?;
        let values = (0..weapon.controls.len())
            .map(|slot| {
                Pose::checked(if slot < common {
                    values[slot]
                } else {
                    weapon.defaults[slot]
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let objects = weapon.objects(&values)?;
        sampled.push(
            bones
                .iter()
                .map(|&(_, bone)| objects[0].inverse().then(objects[bone]))
                .collect(),
        );
    }
    Ok(Mechanism {
        name: entry.name,
        frames: sampled,
    })
}

/// One frame of a clip as control values on a rig, and whether the clip is additive. Channels the
/// clip omits take the rig's defaults when it is absolute and identity when it is additive. A
/// frame past the end selects the last.
pub(super) fn values(
    source: &mut Reader,
    clip: u32,
    rig: &source::Rig,
    frame: usize,
) -> Result<(Vec<Pose>, bool)> {
    let original = source.tag(clip, Some(0x8080AE01))?;
    let (candidate, _) = clip::lower(&original, Some(&clip::Vocabulary::new()))?;
    let additive = candidate.u8(0x181)? != 0;
    let (frames, slots, common) = layout(&candidate, rig)?;
    ensure!(frames > 0, "Source clip has no frames");
    ensure!(
        common == rig.controls.len(),
        "Source clip addresses controls outside its rig"
    );
    let defaults = (0..slots)
        .map(|slot| {
            if slot < common && !additive {
                rig.defaults[slot]
            } else {
                Pose::IDENTITY.0
            }
        })
        .collect::<Vec<_>>();
    let sampled = native_clip::sample(&candidate, &defaults, frame.min(usize::from(frames) - 1))?;
    let values = (0..rig.controls.len())
        .map(|slot| Pose::checked(sampled[slot]))
        .collect::<Result<Vec<_>>>()?;
    Ok((values, additive))
}

/// Whether a lowered source clip omitted cues of the kinds carrier cues stand in for, value-2
/// named events, resource references and sequence selectors, and kept none of its own.
fn omitted_cues(lowering: &Value) -> bool {
    lowering["event_count"] == 0
        && lowering["omitted_events"].as_array().is_some_and(|events| {
            events.iter().any(|e| {
                matches!(e["type"].as_u64(), Some(2 | 3)) || (e["type"] == 5 && e["value"] == 2)
            })
        })
}

/// The weapon-local descriptor a named weapon state selects, or `None` when the weapon lookup
/// has no state of that name. A weighted choice among several descriptors is refused.
fn weapon_state(
    local: &controller::Controller,
    state: u32,
    inputs: &BTreeMap<u8, u32>,
) -> Result<Option<u32>> {
    if !local.names.contains_key(&state) {
        return Ok(None);
    }
    match local.route(state, inputs)?.as_slice() {
        [(descriptor, _)] => Ok(Some(*descriptor)),
        route => anyhow::bail!(
            "Weapon-local state {state:08X} chooses among {} descriptors",
            route.len()
        ),
    }
}

/// The weapon-local descriptor named like an arm action, when exactly one bound descriptor at
/// rate 1 carries that name. One examined weapon names its fire descriptor `fire_1`, the name of
/// the native and source action.
fn weapon_named(local: &controller::Controller, timed: &[Timed], action: u32) -> Option<u32> {
    let mut named = timed
        .iter()
        .filter(|t| local.descriptors[t.descriptor as usize].name == action);
    let first = named.next()?;
    named.next().is_none().then_some(first.descriptor)
}

/// A bound weapon-local descriptor at rate 1, with its clip ordinal, frame count and whether its
/// visible parts end where they start.
struct Timed {
    descriptor: u32,
    ordinal: usize,
    frames: u16,
    closes: bool,
}

/// Whether every visible weapon part ends within 10 centimetres of its first-frame place in the
/// weapon root frame. An empty reload starts with the bolt or slide locked back and ends with it
/// closed, a few centimetres and up to 54 degrees on examined rifles, so rotation and small moves
/// do not count. A hidden part (relative scale under 0.5) at either end shows nothing there.
fn closes(mechanism: &Mechanism) -> bool {
    let (Some(first), Some(last)) = (mechanism.frames.first(), mechanism.frames.last()) else {
        return true;
    };
    first.iter().zip(last).all(|(a, b)| {
        a.0[7] < 0.5
            || b.0[7] < 0.5
            || (0..3)
                .map(|i| (a.0[4 + i] - b.0[4 + i]).powi(2))
                .sum::<f32>()
                .sqrt()
                < 0.1
    })
}

/// The weapon-local descriptor whose clip length matches an arm clip of `frames`. No source
/// record selects these descriptors for an arm action, so the pairing rests on timing: the
/// examined reload and fire clips of both rigs agree within one frame. Only clips whose parts end
/// where they start qualify, because the arm action returns to rest. One examined 15-frame clip
/// slides the magazine 0.6 metres away and leaves it there, which a fire must not play.
///
/// The nearest length wins. A tie between different clips pairs nothing, except for the two
/// reloads: across eight examined weapons whose empty and rounds-remaining reloads pair by length,
/// the empty reload's weapon clip always precedes the other in the weapon lookup, so a tie of two
/// clips gives the empty reload the earlier one and the other reload the later one.
fn paired(timed: &[Timed], frames: u16, reload: Option<usize>) -> Option<u32> {
    let eligible = timed.iter().filter(|t| t.closes).collect::<Vec<_>>();
    let nearest = eligible.iter().map(|t| t.frames.abs_diff(frames)).min()?;
    if nearest > 1 {
        return None;
    }
    let mut chosen = eligible
        .into_iter()
        .filter(|t| t.frames.abs_diff(frames) == nearest)
        .collect::<Vec<_>>();
    chosen.sort_by_key(|t| t.descriptor);
    chosen.dedup_by_key(|t| t.ordinal);
    match (chosen.as_slice(), reload) {
        ([only], _) => Some(only.descriptor),
        ([first, second], Some(order)) => Some([first, second][order].descriptor),
        _ => None,
    }
}

/// Sample a lowered source clip on the source rig and author it on the native controls.
fn author(
    candidate: &Payload,
    arms: &source::Rig,
    retarget: &retarget::Retarget,
    native: &control::Rig,
    controls: &Payload,
    targets: &BTreeSet<u32>,
    overlay: Option<&Overlay>,
) -> Result<Authored> {
    let composition = candidate.u8(0x181)?;
    ensure!(
        matches!(composition, 0 | 1),
        "Source clip composition {composition} is untranslated"
    );
    let (frames, slots, common) = layout(candidate, arms)?;
    let identity = Pose::IDENTITY.0;
    let base = |slot: usize| {
        if composition == 0 {
            arms.defaults[slot]
        } else {
            identity
        }
    };
    let defaults = (0..slots)
        .map(|slot| if slot < common { base(slot) } else { identity })
        .collect::<Vec<_>>();
    let mut tracks = vec![Vec::with_capacity(usize::from(frames)); native.controls.len()];
    let mut grip_drift = 0f32;
    let mut part_error = 0f32;
    let handle = arms.role("handle")?;
    let grip = arms.role("right_grip")?;
    for frame in 0..usize::from(frames) {
        let sampled = native_clip::sample(candidate, &defaults, frame)?;
        let values = (0..arms.controls.len())
            .map(|slot| {
                Pose::checked(if slot < common {
                    sampled[slot]
                } else {
                    base(slot)
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let mut locals = if composition == 0 {
            retarget.absolute(&values)?
        } else {
            retarget.additive(&values)?
        };
        if composition == 0 {
            // Native FK readback of the weapon against the source, before encoding.
            let source = arms.objects(&values)?;
            let objects = native.objects(&locals)?;
            let index = |name: u32| native.names.iter().position(|n| *n == name);
            if let (Some(h), Some(g)) = (index(arms.names[handle]), index(arms.names[grip])) {
                let want = source[grip].inverse().then(source[handle]);
                let have = objects[g].inverse().then(objects[h]);
                grip_drift = grip_drift.max(distance(&want, &have));
                for (bone, name) in arms.names.iter().enumerate() {
                    if arms.parents[bone] == Some(handle)
                        && let Some(n) = index(*name)
                    {
                        let want = source[handle].inverse().then(source[bone]);
                        let have = objects[h].inverse().then(objects[n]);
                        // Shared bones carry a bind correction, so compare motion from rest.
                        let rest_s = arms.bind[handle].inverse().then(arms.bind[bone]);
                        let rest_n = native.bind[h].inverse().then(native.bind[n]);
                        let moved_s = rest_s.inverse().then(want);
                        let moved_n = rest_n.inverse().then(have);
                        part_error = part_error.max(distance(&moved_s, &moved_n));
                    }
                }
            }
        }
        // The weapon model follows its own lookup in the source, while the arm clip places the
        // weapon root. Weapon bones below the root take the weapon-local pose, holding its last
        // frame, relative to the native weapon root. Additive clips leave them unchanged. The
        // readback above measures the arm translation before this replacement.
        if let (0, Some(overlay)) = (composition, overlay) {
            let frames = &overlay.mechanism.frames;
            let poses = &frames[frame.min(frames.len() - 1)];
            let mut objects = native.objects(&locals)?;
            for ((&(slot, _), relative), correction) in
                overlay.bones.iter().zip(poses).zip(overlay.corrections)
            {
                let target = objects[overlay.native_handle]
                    .then(*relative)
                    .then(*correction);
                let local = Pose::checked(native.local(slot, &objects, target)?.0)?;
                objects[native.controls[slot].bone] = native.compose(slot, &objects, local);
                locals[slot] = local.0;
            }
        }
        for (slot, local) in locals.into_iter().enumerate() {
            tracks[slot].push(local);
        }
    }
    let mut envelope = candidate.clone();
    let (used, disabled) = bindings(&mut envelope, &retarget.slots, targets)?;
    let payload = if composition == 0 {
        native_clip::replace_for(&envelope, &tracks, native.bind.len(), controls)?
    } else {
        native_clip::replace_deltas_for(&envelope, &tracks, native.bind.len(), controls)?
    };
    let weapon_local = overlay
        .filter(|_| composition == 0)
        .map(|o| format!("{:08X}", o.mechanism.name));
    Ok(Authored {
        payload,
        frames,
        composition,
        report: json!({"frames":frames,"composition":composition,"source_controls":slots,
            "shared_leading_controls":common,
            "constraint_targets":used.iter().map(|t| format!("{t:08X}")).collect::<Vec<_>>(),
            "disabled_constraints":disabled,
            "weapon_local_descriptor":weapon_local,
            "weapon_grip_drift_metres":grip_drift,"mechanism_error_metres":part_error}),
        targets: used,
    })
}

fn distance(a: &Pose, b: &Pose) -> f32 {
    (0..3)
        .map(|i| (a.0[4 + i] - b.0[4 + i]).powi(2))
        .sum::<f32>()
        .sqrt()
}

/// The optic type the arm dictionary selects on. It is published only when exactly one
/// attached component declares one.
fn declared_optic(
    source: &mut Reader,
    weapon: &Payload,
    wbase: usize,
    controller: &controller::Controller,
) -> Result<Option<u32>> {
    let mut optics = BTreeSet::new();
    for row in weapon.array(wbase + 0x3D8, 32, None)? {
        let entity = source.ref64(weapon, row + 16)?;
        if [0, u32::MAX].contains(&entity) {
            continue;
        }
        for (_, owner, base, class) in components(source, entity)? {
            if class == 0x808041DA {
                optics.insert(owner.u32(base + 0x14C)?);
            }
        }
    }
    let optics = optics.into_iter().collect::<Vec<_>>();
    Ok(match optics.as_slice() {
        [optic] if controller.declares(OPTIC, *optic) => Some(*optic),
        _ => None,
    })
}

/// Absolute source control values of a lowered clip's first frame. Controls past the shared
/// groups keep the rig defaults.
fn first_frame(candidate: &Payload, arms: &source::Rig) -> Result<Vec<Pose>> {
    let (_, slots, common) = layout(candidate, arms)?;
    let defaults = (0..slots)
        .map(|s| {
            if s < common {
                arms.defaults[s]
            } else {
                Pose::IDENTITY.0
            }
        })
        .collect::<Vec<_>>();
    let sampled = native_clip::sample(candidate, &defaults, 0)?;
    (0..arms.controls.len())
        .map(|s| {
            Pose::checked(if s < common {
                sampled[s]
            } else {
                arms.defaults[s]
            })
        })
        .collect()
}

/// The source grip relative to the utility role on the first frame of the first absolute clip a
/// named source action routes, or `None` when the source names no such action.
#[allow(clippy::too_many_arguments)]
fn source_grip(
    source: &mut Reader,
    controller: &controller::Controller,
    fixed: &BTreeMap<u8, u32>,
    arms: &source::Rig,
    carrier: &native::Carrier,
    retarget: &retarget::Retarget,
    action: &str,
) -> Result<Option<[f32; 3]>> {
    let name = source::fnv(action);
    if !controller.names.contains_key(&name) {
        return Ok(None);
    }
    for (descriptor, _) in controller.route(name, fixed)? {
        let Some(ordinal) = controller.descriptors[descriptor as usize].clip else {
            continue;
        };
        let original = source.tag(controller.clips[ordinal], Some(0x8080AE01))?;
        let (candidate, _) = clip::lower(&original, Some(&carrier.vocabulary))?;
        if candidate.u8(0x181)? == 0 {
            return Ok(Some(retarget.grip(&first_frame(&candidate, arms)?)?));
        }
    }
    Ok(None)
}

/// Native framing: the view reference offset that puts the source idle grip where the carrier's
/// idle holds its grip, both relative to the utility role. Both rigs rest the utility at eye
/// height, and the client places the camera there regardless of the utility, so the offset moves
/// the hands and weapon in view. The source aimed idle sets where the lateral and vertical parts
/// fade out.
#[allow(clippy::too_many_arguments)]
fn frame(
    source: &mut Reader,
    native: &mut Reader,
    controller: &controller::Controller,
    fixed: &BTreeMap<u8, u32>,
    arms: &source::Rig,
    carrier: &native::Carrier,
    retarget: &mut retarget::Retarget,
    donors: &cue::Donors,
) -> Result<Value> {
    let names = (
        arms.names[arms.role("utility")?],
        arms.names[arms.role("right_grip")?],
    );
    let held = source_grip(source, controller, fixed, arms, carrier, retarget, "idle")?;
    let aimed = source_grip(
        source,
        controller,
        fixed,
        arms,
        carrier,
        retarget,
        "iron_sight_idle",
    )?;
    let carried = donors.idle_grip(native, names.0, names.1)?;
    let (Some(held), Some(carried)) = (held, carried) else {
        return Ok(
            json!({"status":"source","reason":"The source or the carrier has no absolute idle"}),
        );
    };
    let offset = std::array::from_fn(|i| carried[i] - held[i]);
    let axis = |p: [f32; 3]| p[1].hypot(p[2]);
    let framing = retarget::Framing {
        offset,
        idle: axis(held),
        aimed: aimed.map_or(0., axis),
    };
    retarget.frame(framing);
    Ok(
        json!({"status":"native","source_idle_grip":held,"carrier_idle_grip":carried,
        "offset":offset,"source_aimed_grip":aimed,"idle_axis_distance":framing.idle,
        "aimed_axis_distance":framing.aimed}),
    )
}

/// Compiles the source weapon of `parts` onto the carrier of `item_tag`: plans the routes, pairs
/// weapon-local motion, translates the clips with the carrier's states for every action that does
/// not translate, writes the converted and native files into `graph` and records the receipt.
pub(in crate::marathon) fn compile(
    source: &mut Reader,
    native: &mut Reader,
    parts: &[Value],
    item_tag: u32,
    graph: &Path,
) -> Result<Compiled> {
    let sources = plan::Sources::read(source, native, parts, item_tag, graph)?;
    let mut retarget = retarget::Retarget::new(&sources.arms, &sources.native_rig)?;
    let mut donors = cue::Donors::new(&sources.carrier, sources.weapon_rig.names[0])?;
    let framing = frame(
        source,
        native,
        &sources.controller,
        &sources.fixed,
        &sources.arms,
        &sources.carrier,
        &mut retarget,
        &donors,
    )?;
    let mut routes = plan::Routes::read(&sources, graph)?;
    let local = pairing::WeaponLocal::pair(source, &sources, &routes)?;
    let mut translation =
        translation::Translation::convert(source, &sources, &mut routes, &local, &retarget)?;
    let clips = emission::Clips::write(
        native,
        graph,
        &sources,
        &routes,
        &local,
        &mut translation,
        &mut donors,
    )?;
    let tables = emission::Tables::write(native, graph, &sources, &routes, &translation, &clips)?;
    let section = emission::Receipt {
        sources: &sources,
        routes: &routes,
        local: &local,
        translation: &translation,
        clips: &clips,
        tables: &tables,
        retarget: &retarget,
        framing,
    }
    .section();
    Ok(Compiled {
        section,
        palette: sources.extension.palette,
    })
}

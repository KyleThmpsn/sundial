//! Compile the selected source controller into a linked native first-person graph section.
use super::*;

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
struct Mechanism {
    name: u32,
    frames: Vec<Vec<Pose>>,
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
fn mechanism(
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

#[allow(clippy::cognitive_complexity)]
pub(in crate::marathon) fn compile(
    source: &mut Reader,
    native: &mut Reader,
    parts: &[Value],
    item_tag: u32,
    graph: &Path,
) -> Result<Compiled> {
    let root = hash(
        parts.first().context("Empty source animation assembly")?,
        "entity",
    )?;
    let owners = components(source, root)?;
    let (_, weapon, wbase) = one(&owners, 0x80804A14, "weapon definition")?;
    let hold = weapon.u32(wbase + 0x310)?;
    let profile = weapon.u32(wbase + 0x314)?;
    let selection_tag = source.ref64(&weapon, wbase + 0x338)?;
    let selection = source.tag(selection_tag, Some(0x808025AA))?;
    let arms_entity = source.ref64(&selection, 0x10)?;
    let alternatives = selection.u64(0x20)?;
    let (_, weapon_skeleton, _) = one(&owners, source::SKELETON, "weapon skeleton")?;
    let (_, weapon_controls, _) = one(&owners, source::CONTROLS, "weapon controls")?;
    let (_, placement, _) = one(&owners, 0x80809F76, "weapon placement")?;
    let weapon_rig = source::Rig::read(&weapon_skeleton, &weapon_controls)?;
    let arm_owners = components(source, arms_entity)?;
    let (arm_skeleton_tag, arm_skeleton, _) = one(&arm_owners, source::SKELETON, "arm skeleton")?;
    let (arm_controls_tag, arm_controls, _) = one(&arm_owners, source::CONTROLS, "arm controls")?;
    let (lookup_tag, lookup, _) = one(&arm_owners, controller::LOOKUP, "arm action lookup")?;
    let arms = source::Rig::read(&arm_skeleton, &arm_controls)?;
    let coverage = weapon_section(&arms, &weapon_rig)?;
    let controller = controller::Controller::read(source, &lookup)?;
    ensure!(
        controller.declares(PROFILE, profile) && controller.declares(PROFILE, hold),
        "Source animation profile is absent from the arm dictionary"
    );
    let mut fixed = BTreeMap::from([(PROFILE, profile)]);
    if let Some(optic) = declared_optic(source, &weapon, wbase, &controller)? {
        fixed.insert(OPTIC, optic);
    }
    let carrier = native::Carrier::read(native, item_tag)?;
    let extension = native::extend(&carrier, &weapon_rig, graph)?;
    let native_rig = control::Rig::read(&extension.skeleton, &extension.controls)?;
    let mut retarget = retarget::Retarget::new(&arms, &native_rig)?;
    let mut donors = cue::Donors::new(&carrier, weapon_rig.names[0])?;
    let framing = frame(
        source,
        native,
        &controller,
        &fixed,
        &arms,
        &carrier,
        &mut retarget,
        &donors,
    )?;
    let reloads = RELOADS
        .iter()
        .map(|(action, value)| (source::fnv(action), source::fnv(value)))
        .collect::<BTreeMap<_, _>>();
    let mut routes = BTreeMap::new();
    let mut absent = Vec::new();
    let carrier_actions = carrier.actions()?;
    for &action in &carrier_actions {
        if !controller.names.contains_key(&action) {
            absent.push(format!("{action:08X}"));
            continue;
        }
        let mut inputs = fixed.clone();
        if let Some(&value) = reloads.get(&action) {
            ensure!(
                controller.declares(RELOAD, value),
                "Source reload state value is absent from the arm dictionary"
            );
            inputs.insert(RELOAD, value);
        }
        routes.insert(action, controller.route(action, &inputs)?);
    }
    ensure!(!routes.is_empty(), "No native action has a source route");
    let folder = graph.join("animation");
    fs::create_dir_all(&folder)?;
    let carrier_markers = crate::tiger::markers::read_native(&carrier.markers.1)?
        .into_iter()
        .map(|m| m.name)
        .collect::<BTreeSet<_>>();
    let mut targets = native::placement_names(&placement)?;
    targets.extend(&carrier_markers);
    // Weapon-local motion. The weapon root's own lookup animates the weapon model's bones. The
    // arm clip places the weapon root, and each absolute arm clip takes one weapon-local
    // descriptor for the bones below it: the weapon state named like the native action, else the
    // descriptor so named, else the choice another action made for the same arm descriptor by
    // either rule, else the closing descriptor whose clip length matches the arm clip, else the
    // weapon's idle state.
    let native_handle = native_rig
        .names
        .iter()
        .position(|n| *n == weapon_rig.names[0])
        .context("Native rig lacks the source weapon root")?;
    let mut mechanism_bones = Vec::new();
    for (bone, name) in weapon_rig.names.iter().enumerate().skip(1) {
        let native_bone = native_rig
            .names
            .iter()
            .position(|n| n == name)
            .with_context(|| format!("Native rig lacks source weapon bone {name:08X}"))?;
        let slot = native_rig
            .controls
            .iter()
            .position(|c| c.bone == native_bone)
            .with_context(|| format!("Native weapon bone {name:08X} has no control"))?;
        mechanism_bones.push((slot, bone));
    }
    mechanism_bones.sort_unstable();
    // Weapon geometry is skinned in source positions. A bone that shares its name with the
    // carrier keeps the carrier bind, so it takes the bind difference, as the arm retarget's
    // weapon placement does. Appended bones carry their source bind and take none.
    let mechanism_corrections = mechanism_bones
        .iter()
        .map(|&(slot, bone)| {
            let source_rest = weapon_rig.bind[0].inverse().then(weapon_rig.bind[bone]);
            let native_rest = native_rig.bind[native_handle]
                .inverse()
                .then(native_rig.bind[native_rig.controls[slot].bone]);
            source_rest.inverse().then(native_rest)
        })
        .collect::<Vec<_>>();
    let local_lookups = owners
        .iter()
        .filter(|o| o.3 == controller::LOOKUP)
        .collect::<Vec<_>>();
    let mut weapon_local = json!({"lookup":Value::Null,"bones":mechanism_bones.len(),"notes":[]});
    let local = match local_lookups.as_slice() {
        _ if mechanism_bones.is_empty() => None,
        [owner] => {
            weapon_local["lookup"] = json!(owner.0);
            match controller::Controller::read(source, &owner.1) {
                Ok(local) => Some(local),
                Err(error) => {
                    weapon_local["notes"] = json!([format!("Weapon lookup: {error:#}")]);
                    None
                }
            }
        }
        [] => None,
        _ => anyhow::bail!("Source weapon has several action lookups"),
    };
    // The weapon-local descriptor for each routed absolute arm clip, by action and source
    // descriptor, and how it was chosen.
    let mut overlays = BTreeMap::<(u32, u32), u32>::new();
    let mut pairings = Vec::new();
    let mut mechanisms = BTreeMap::<u32, Mechanism>::new();
    let mut notes = Vec::new();
    if let Some(local) = &local {
        let mut timed = Vec::new();
        for (index, entry) in local.descriptors.iter().enumerate() {
            let Some(ordinal) = entry.clip else {
                continue;
            };
            if entry.rate != 1. {
                continue;
            }
            let clip = source.tag(local.clips[ordinal], Some(0x8080AE01))?;
            let descriptor = u32::try_from(index)?;
            // A clip the weapon rig cannot sample is left out, as choosing it would fail.
            let Ok(sampled) = mechanism(source, local, descriptor, &weapon_rig, &mechanism_bones)
            else {
                continue;
            };
            timed.push(Timed {
                descriptor,
                ordinal,
                frames: clip.u16(0x140)?,
                closes: closes(&sampled),
            });
            mechanisms.insert(descriptor, sampled);
        }
        let idle = source::fnv("idle");
        let movement = MOVEMENT
            .iter()
            .map(|a| source::fnv(a))
            .collect::<BTreeSet<_>>();
        let reload_order = |action: u32| {
            [source::fnv("reload_empty"), source::fnv("reload_full")]
                .iter()
                .position(|r| *r == action)
        };
        // Actions that route one arm descriptor share its best-supported weapon descriptor.
        let rank = |how: &str| {
            ["state", "name", "length", "idle"]
                .iter()
                .position(|h| *h == how)
        };
        let mut shared = BTreeMap::<u32, (u32, &'static str)>::new();
        for (&action, route) in &routes {
            let mut inputs = BTreeMap::new();
            if local.declares(PROFILE, profile) {
                inputs.insert(PROFILE, profile);
            }
            for &(descriptor, _) in route {
                let found = if let Ok(Some(d)) = weapon_state(local, action, &inputs) {
                    Some((d, "state"))
                } else {
                    weapon_named(local, &timed, action).map(|d| (d, "name"))
                };
                if let Some(found) = found
                    && shared
                        .get(&descriptor)
                        .is_none_or(|s| rank(found.1) < rank(s.1))
                {
                    shared.insert(descriptor, found);
                }
            }
        }
        for (&action, route) in &routes {
            let mut inputs = BTreeMap::new();
            if local.declares(PROFILE, profile) {
                inputs.insert(PROFILE, profile);
            }
            if let Some(&value) = reloads.get(&action)
                && local.declares(RELOAD, value)
            {
                inputs.insert(RELOAD, value);
            }
            for &(descriptor, _) in route {
                let Some(ordinal) = controller.descriptors[descriptor as usize].clip else {
                    continue;
                };
                let arm = source.tag(controller.clips[ordinal], Some(0x8080AE01))?;
                if arm.u8(0x181)? != 0 {
                    continue;
                }
                let frames = arm.u16(0x140)?;
                let chosen = (|| -> Result<Option<(u32, &'static str)>> {
                    if let Some(d) = weapon_state(local, action, &inputs)? {
                        return Ok(Some((d, "state")));
                    }
                    if let Some(d) = weapon_named(local, &timed, action) {
                        return Ok(Some((d, "name")));
                    }
                    if let Some(&(d, _)) = shared.get(&descriptor) {
                        return Ok(Some((d, "shared")));
                    }
                    if !movement.contains(&action)
                        && let Some(d) = paired(&timed, frames, reload_order(action))
                    {
                        return Ok(Some((d, "length")));
                    }
                    Ok(weapon_state(local, idle, &inputs)?.map(|d| (d, "idle")))
                })()
                .and_then(|chosen| {
                    let Some((local_descriptor, how)) = chosen else {
                        return Ok(None);
                    };
                    if let std::collections::btree_map::Entry::Vacant(slot) =
                        mechanisms.entry(local_descriptor)
                    {
                        slot.insert(mechanism(
                            source,
                            local,
                            local_descriptor,
                            &weapon_rig,
                            &mechanism_bones,
                        )?);
                    }
                    Ok(Some((local_descriptor, how)))
                });
                match chosen {
                    Ok(Some((local_descriptor, how))) => {
                        overlays.insert((action, descriptor), local_descriptor);
                        pairings.push((action, json!({"action":format!("{action:08X}"),"source_descriptor":descriptor,
                            "arm_frames":frames,"weapon_descriptor":format!("{:08X}", mechanisms[&local_descriptor].name),
                            "weapon_frames":mechanisms[&local_descriptor].frames.len(),"chosen_by":how})));
                    }
                    Ok(None) => {}
                    Err(error) => notes.push(format!("{action:08X}: {error:#}")),
                }
            }
        }
        let reached = overlays.values().collect::<BTreeSet<_>>();
        weapon_local["unreached_descriptors"] = json!(
            local
                .descriptors
                .iter()
                .enumerate()
                .filter(|(i, d)| d.clip.is_some() && !reached.contains(&(*i as u32)))
                .map(|(_, d)| format!("{:08X}", d.name))
                .collect::<Vec<_>>()
        );
        weapon_local["notes"] = json!(notes);
    }
    // Translate every clip the routes reach, keeping each failure with its clip. An absolute
    // clip is authored once for each weapon-local descriptor that accompanies it.
    type Key = (usize, Option<u32>);
    let mut keys = BTreeMap::<(u32, u32), Key>::new();
    let mut authored = BTreeMap::<Key, std::result::Result<(Authored, Value), String>>::new();
    for (&action, route) in &routes {
        for &(descriptor, _) in route {
            let entry = &controller.descriptors[descriptor as usize];
            let Some(ordinal) = entry.clip else {
                continue;
            };
            let tag = controller.clips[ordinal];
            let original = source.tag(tag, Some(0x8080AE01))?;
            let overlay = overlays.get(&(action, descriptor)).copied();
            let key = (ordinal, overlay);
            keys.insert((action, descriptor), key);
            if authored.contains_key(&key) {
                continue;
            }
            let overlay = overlay.map(|descriptor| Overlay {
                bones: &mechanism_bones,
                corrections: &mechanism_corrections,
                mechanism: &mechanisms[&descriptor],
                native_handle,
            });
            let attempt = clip::lower(&original, Some(&carrier.vocabulary)).and_then(|(candidate, lowered)| {
                let result = author(
                    &candidate,
                    &arms,
                    &retarget,
                    &native_rig,
                    &extension.controls,
                    &targets,
                    overlay.as_ref(),
                )?;
                Ok((result, json!({"source":tag,"name":format!("{:08X}",original.u32(0x120)?),"lowering":lowered})))
            });
            authored.insert(
                key,
                attempt.map_err(|error| format!("Source clip {tag:08X}: {error:#}")),
            );
        }
    }
    // An action keeps a route only when every choice translated.
    let required = REQUIRED
        .iter()
        .map(|a| source::fnv(a))
        .collect::<BTreeSet<_>>();
    let mut unsupported = Vec::new();
    routes.retain(|action, route| {
        let reason = route.iter().find_map(|&(descriptor, _)| {
            let entry = &controller.descriptors[descriptor as usize];
            match entry.clip {
                None => Some("Selects an unbound source descriptor".to_owned()),
                Some(_) if entry.rate != 1. => {
                    Some("Source descriptor playback rate is untranslated".to_owned())
                }
                Some(_) => authored[&keys[&(*action, descriptor)]]
                    .as_ref()
                    .err()
                    .cloned(),
            }
        });
        if let Some(reason) = reason {
            unsupported.push((*action, reason));
            false
        } else {
            true
        }
    });
    for (action, reason) in &unsupported {
        ensure!(
            !required.contains(action),
            "Required action {action:08X} has no complete translation: {reason}"
        );
    }
    // A source descriptor becomes one native descriptor for each weapon-local descriptor that
    // accompanies it, numbered after the carrier's descriptors.
    let first = carrier.descriptors()?;
    let mut descriptors = BTreeMap::<(u32, Option<u32>), u32>::new();
    let mut converted = BTreeMap::<Key, (usize, u16, u8)>::new();
    let mut clips = Vec::new();
    let mut rows = Vec::new();
    let mut extra = Vec::new();
    let mut clip_reports = Vec::new();
    let mut used_targets = BTreeSet::new();
    for (&action, route) in &routes {
        for &(descriptor, _) in route {
            let key = keys[&(action, descriptor)];
            if descriptors.contains_key(&(descriptor, key.1)) {
                continue;
            }
            let entry = &controller.descriptors[descriptor as usize];
            let (index, frames, composition) = if let Some(&found) = converted.get(&key) {
                found
            } else {
                let tag = controller.clips[key.0];
                let Ok((result, mut report)) = authored.remove(&key).context("Routed clip")? else {
                    anyhow::bail!("Routed clip {tag:08X} failed translation");
                };
                let file = match key.1 {
                    Some(local) => format!(
                        "animation/source-clip-{tag:08X}-{:08X}.bin",
                        mechanisms[&local].name
                    ),
                    None => format!("animation/source-clip-{tag:08X}.bin"),
                };
                // The linker replaces each placeholder word in the bank. A second variant of
                // one source clip takes a placeholder derived from its weapon-local descriptor.
                let placeholder = if clips.contains(&tag) {
                    let local = key.1.map_or(0, |local| mechanisms[&local].name);
                    source::fnv(&format!("{tag:08X}-{local:08X}"))
                } else {
                    tag
                };
                // Source cues native audio cannot resolve give way to the carrier's cues for the
                // same action, timed by the weapon mechanism this clip carries.
                let mut payload = result.payload;
                if omitted_cues(&report["lowering"]) {
                    let actions = keys
                        .iter()
                        .filter(|(pair, k)| **k == key && routes.contains_key(&pair.0))
                        .map(|(pair, _)| pair.0)
                        .collect::<BTreeSet<_>>();
                    let timing = cue::authored(&payload, &native_rig, native_handle)?;
                    report["cues"] = match donors.choose(
                        native,
                        &actions,
                        result.frames,
                        result.composition,
                    )? {
                        Some(donor) => {
                            let map = cue::retime(donor.timing, timing);
                            json!({"carrier_clip":format!("{:08X}",donor.tag),
                                "carrier_timing":donor.timing.report(),"source_timing":timing.report(),
                                "records":clip::carry_cues(&mut payload, &donor.payload, &map)?})
                        }
                        None => {
                            json!({"carrier_clip":null,"reason":"No carrier clip of these actions carries cues"})
                        }
                    };
                }
                fs::write(graph.join(&file), &payload.0)?;
                extra.push(
                    json!({"source":placeholder,"template":carrier.clip_template,"file":file}),
                );
                used_targets.extend(&result.targets);
                report["file"] = json!(file);
                report["placeholder"] = json!(placeholder);
                report["authoring"] = result.report;
                clip_reports.push(report);
                let found = (clips.len(), result.frames, result.composition);
                clips.push(placeholder);
                converted.insert(key, found);
                found
            };
            descriptors.insert((descriptor, key.1), u32::try_from(first + rows.len())?);
            rows.push(native::Row {
                source: descriptor,
                name: entry.name,
                clip: index,
                duration: (f32::from(frames.max(1)) - 1.) / 30.,
                // Stock first-person descriptors play absolute clips in mode 1 and additive in 2.
                mode: if composition == 0 { 1 } else { 2 },
            });
        }
    }
    let native_descriptor =
        |action: u32, descriptor: u32| descriptors[&(descriptor, keys[&(action, descriptor)].1)];
    let native_routes = routes
        .iter()
        .map(|(action, route)| {
            (
                *action,
                route
                    .iter()
                    .map(|(descriptor, weight)| (native_descriptor(*action, *descriptor), *weight))
                    .collect(),
            )
        })
        .collect::<native::Routes>();
    let bank = native::bank(&carrier, &clips, &rows)?;
    // Source supplemental layers are untranslated, and source loops such as `sprint_loop` are
    // held poses that rely on them. Each absolute translated descriptor therefore takes the
    // carrier's layer route for the actions that select it, the additive sway and locomotion the
    // carrier plays with its own clip for that action. Actions that disagree, additive rows and
    // actions the carrier routes to no layer leave the descriptor without one.
    let mut selecting = BTreeMap::<u32, BTreeSet<u32>>::new();
    for (&action, route) in &native_routes {
        for &(descriptor, _) in route {
            selecting.entry(descriptor).or_default().insert(action);
        }
    }
    let mut dispatch = Vec::with_capacity(rows.len());
    let mut layer_routes = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        let ordinal = u32::try_from(first + index)?;
        let mut found = BTreeSet::new();
        if row.mode == 1 {
            for &action in selecting.get(&ordinal).into_iter().flatten() {
                found.insert(native::carrier_layer_route(&carrier, native, action)?);
            }
        }
        let entry = match found.into_iter().collect::<Vec<_>>().as_slice() {
            [Some(entry)] => *entry,
            _ => u16::MAX,
        };
        if entry != u16::MAX {
            layer_routes.push(json!({"descriptor":ordinal,"layer_route":entry}));
        }
        dispatch.push(entry);
    }
    let layers = native::layers(&carrier, &dispatch)?;
    let states = native::states(&carrier, &native_routes)?;
    // Actions the source leaves untranslated keep the carrier's state.
    let fallback = absent
        .iter()
        .cloned()
        .chain(
            unsupported
                .iter()
                .map(|(action, _)| format!("{action:08X}")),
        )
        .collect::<BTreeSet<_>>();
    // Hand targets the carrier does not publish are appended from the source placement.
    let appended = used_targets
        .difference(&carrier_markers)
        .copied()
        .collect::<BTreeSet<_>>();
    let markers = native::markers(&carrier, &placement, &weapon_rig, &appended, graph)?;
    let mut files = BTreeMap::new();
    let mut write = |key: &str, bytes: &[u8]| -> Result<()> {
        let file = format!("animation/first-person-{key}.bin");
        fs::write(graph.join(&file), bytes)?;
        files.insert(key.replace('-', "_"), file);
        Ok(())
    };
    write(
        "attachment-owner",
        &native.tag(carrier.attachment, Some(0x80809C36))?.0,
    )?;
    write("entity", &native.tag(carrier.entity, Some(0x80809C0F))?.0)?;
    write("lookup-owner", &carrier.lookup.1.0)?;
    write("bank", &carrier.bank.1.0)?;
    write("converted-bank", &bank.0)?;
    write("parameters-template", &carrier.parameters.1.0)?;
    write("parameters", &carrier.parameters.1.0)?;
    write("states-template", &carrier.states.1.0)?;
    write("states", &states.0)?;
    write("pose-layers-template", &carrier.layers.1.0)?;
    write("pose-layers", &layers.0)?;
    let mut consumers = Vec::new();
    for (index, (tag, payload)) in carrier.consumers.iter().enumerate() {
        let key = format!("consumer-{index}");
        write(&key, &payload.0)?;
        consumers.push(json!({"owner":tag,"file_key":key.replace('-', "_")}));
    }
    let mut rigs = extension.owners.clone();
    rigs.push(markers);
    // Pinned payloads use the same keys the revision recorder derives from these rows.
    for (field, entries) in [("rigs", &rigs), ("extra_clips", &extra)] {
        for (index, row) in entries.iter().enumerate() {
            files.insert(
                format!("{field}_{index}"),
                row["file"].as_str().context("Animation file")?.to_owned(),
            );
            if let Some(template) = row.get("template_file").and_then(Value::as_str) {
                files.insert(
                    format!("{field}_{index}_template_file"),
                    template.to_owned(),
                );
            }
        }
    }
    files.insert(
        "pose_layers_file".into(),
        "animation/first-person-pose-layers.bin".into(),
    );
    files.insert(
        "pose_layers_template_file".into(),
        "animation/first-person-pose-layers-template.bin".into(),
    );
    let descriptor_rows = rows
        .iter()
        .map(|r| json!({"source":r.source,"name":format!("{:08X}",r.name),"clip":clips[r.clip],"duration":r.duration,"mode":r.mode}))
        .collect::<Vec<_>>();
    let section = json!({
        "first_person_status": "linked",
        "runtime_entity": carrier.runtime,
        "first_person": {
            "controller": "marathon_source",
            "attachment_owner": carrier.attachment,
            "entity": carrier.entity,
            "lookup_owner": carrier.lookup.0,
            "bank": carrier.bank.0,
            "files": files,
            "clips": [],
            "extra_clips": extra,
            "bank_consumers": consumers,
            "pose_layers": {"owner":carrier.pose.0,"tag":carrier.layers.0,
                "file":"animation/first-person-pose-layers.bin","template_file":"animation/first-person-pose-layers-template.bin"},
            "state_conversion": {"status":"source_states","parameters":carrier.parameters.0,"states":carrier.states.0,
                "first_translated_descriptor":first,"native_fallback_states":fallback},
            "rig_extension": true,
            "rigs": rigs,
            "source": {
                "weapon_definition_profile": format!("{profile:08X}"),
                "hold_class": format!("{hold:08X}"),
                "selection": selection_tag,
                "default_arm_entity": arms_entity,
                "runner_alternatives": alternatives,
                "arm_skeleton": arm_skeleton_tag,
                "arm_controls": arm_controls_tag,
                "action_lookup": lookup_tag,
                "bank": controller.bank_tag,
                "fixed_builtins": fixed.iter().map(|(b, v)| (b.to_string(), format!("{v:08X}"))).collect::<BTreeMap<_, _>>(),
                "control_extensions": arms.extensions,
            },
            "routes": routes.iter().map(|(action, route)| (format!("{action:08X}"), route.iter().map(|(d, w)| json!({"source_descriptor":d,"native_descriptor":native_descriptor(*action, *d),"weight":w})).collect::<Vec<_>>())).collect::<BTreeMap<_, _>>(),
            "absent_native_actions": absent,
            // A shell-fed carrier requests reload_enter, a shell insert and chamber_1, so a source
            // weapon that reloads by magazine never plays its reload on it.
            "unrequested_source_reloads": RELOADS.iter().map(|(action, _)| source::fnv(action))
                .filter(|action| controller.names.contains_key(action) && !carrier_actions.contains(action))
                .map(|action| format!("{action:08X}")).collect::<Vec<_>>(),
            "unsupported_actions": unsupported.iter().map(|(action, reason)| json!({"action":format!("{action:08X}"),"reason":reason})).collect::<Vec<_>>(),
            "descriptors": descriptor_rows,
            "source_clips": clip_reports,
            "retarget": retarget.report,
            "framing": framing,
            "carrier_layer_routes": layer_routes,
            "weapon_section": extension.report,
            "arm_coverage": coverage,
            "weapon_local": weapon_local,
            // Pairings of actions left out of the state table are not reported.
            "weapon_local_pairings": pairings.iter().filter(|(action, _)| routes.contains_key(action)).map(|(_, row)| row).collect::<Vec<_>>(),
            "retained_native": [
                "First-person entity, skeleton prefix, control definitions and limb solver",
                "Bank clips and descriptors at their native ordinals, before the translated rows",
                "States, parameter dictionary, pose layers and clips of the actions listed under state_conversion.native_fallback_states",
                "Lookup look table at +EC and its shared selector at +F0, which address native clips by ordinal",
                "Bank blend table at +84 and layer names at +58. The source bank names its own blend table, which is untranslated",
                "Pose controller numeric parameters at +110, read by the native pose layers"
            ],
            "untranslated_source": [
                "Supplemental pose layers and their numeric parameter programs. Absolute translated descriptors take the carrier's layer route for their actions, listed under carrier_layer_routes",
                "Weapon-local descriptors no named state, clip length or idle state reaches, listed under weapon_local. Length pairings rest on timing, not a source selection record",
                "Attachment-local controllers and weapon-local clip events",
                "Hand targets on moving weapon bones, which are appended at their rest place",
                "Constraint rows with a target mode other than 3, which are disabled and listed per clip",
                "Audio, effect and sequence-selector clip events, recorded per clip. A clip that omitted them carries the carrier's cues for its action, listed per clip under cues"
            ],
        },
        "gameplay_verified": false,
    });
    Ok(Compiled {
        section,
        palette: extension.palette,
    })
}

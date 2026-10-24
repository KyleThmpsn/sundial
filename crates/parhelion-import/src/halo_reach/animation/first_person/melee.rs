//! Source strike timing in the native weapon-melee event envelope.
use super::*;
use crate::{presentation::put, tiger::animation::control::Rig};

pub(super) const STATES: [u32; 2] = [0xFDCA54D9, 0xFDCA54DB];
const PUNCH: u32 = 0x80C12945;

pub(super) fn defaults(
    source: &source::Source,
    root: &Path,
    native: &mut Reader,
    carrier: &carrier::Carrier,
) -> Result<Vec<String>> {
    let mut names = Vec::new();
    for name in ["first_person:melee_strike_1", "first_person:melee_strike_2"] {
        if source.supports(root, name)? {
            timing(source, name)?;
            names.push(name.to_owned());
        }
    }
    if names.is_empty() {
        return Ok(names);
    }
    let states = carrier
        .states
        .array(8, 8, Some(0x8080342E))?
        .into_iter()
        .map(|row| carrier.states.u32(row))
        .collect::<Result<BTreeSet<_>>>()?;
    if !STATES.iter().all(|state| states.contains(state)) {
        return Ok(Vec::new());
    }
    let clips = carrier.bank.array(8, 4, Some(0x80808F48))?;
    for state in STATES {
        for descriptor in animation::state::descriptors(&carrier.states, state)? {
            let row = *carrier
                .descriptors
                .get(descriptor)
                .context("Native strike descriptor")?;
            if carrier.bank.u16(row + 26)? != 1 {
                return Ok(Vec::new());
            }
            let slot = usize::from(carrier.bank.u16(row + 24)?);
            let clip = native.tag(
                carrier
                    .bank
                    .u32(*clips.get(slot).context("Native strike clip")?)?,
                Some(0x80808F49),
            )?;
            // Validate the existing attack envelope before selecting source motion.
            let (frames, primary, sound_frame, _) = timing(source, &names[0])?;
            envelope(&clip, frames, primary, sound_frame, 1)?;
        }
    }
    Ok(names)
}

pub(super) struct Strike {
    pub state: u32,
    pub descriptor: usize,
    pub template: u32,
    pub marker: u32,
    pub payload: Payload,
    pub hands: Vec<usize>,
    pub report: Value,
}

fn selected_clip<'a>(source: &'a source::Source, name: &str) -> Result<&'a Value> {
    let clips = source.graph["clips"]
        .as_array()
        .context("Source strike clips")?;
    let found = clips
        .iter()
        .filter(|clip| clip["name"] == name)
        .collect::<Vec<_>>();
    let [clip] = found[..] else {
        anyhow::bail!("Source strike {name} is absent or ambiguous");
    };
    let shared = clip["shared"]
        .as_array()
        .context("Source strike variants")?;
    let [shared] = shared.as_slice() else {
        anyhow::bail!("Source strike {name} requires an explicit variant");
    };
    Ok(shared)
}

fn swing(
    source: &source::Source,
    shared: &Value,
    name: &str,
    frames: usize,
) -> Result<(u16, Value)> {
    let sounds = shared["sound_events"]
        .as_array()
        .context("Source strike sounds")?;
    let [sound] = sounds.as_slice() else {
        anyhow::bail!("Source strike {name} requires one explicit swing sound");
    };
    let sound_frame = u16::try_from(
        sound["frame"]
            .as_u64()
            .context("Source strike sound frame")?,
    )?;
    ensure!(
        usize::from(sound_frame) < frames && sound["marker"] == "",
        "Unsupported source strike sound attachment"
    );
    let sound_index = usize::try_from(
        sound["sound_index"]
            .as_u64()
            .context("Source strike sound index")?,
    )?;
    let sound = source.graph["sounds"]
        .as_array()
        .context("Source animation sound table")?
        .get(sound_index)
        .context("Source strike sound is outside its table")?
        .clone();
    ensure!(sound["group"] == "snd!", "Source strike sound is absent");
    Ok((sound_frame, sound))
}

fn timing(source: &source::Source, name: &str) -> Result<(usize, u16, u16, Value)> {
    let shared = selected_clip(source, name)?;
    let frames = shared["frames"]
        .as_u64()
        .context("Source strike frame count")? as usize;
    ensure!(
        (2..=3600).contains(&frames),
        "Invalid source strike duration"
    );
    let primary = shared["frame_events"]
        .as_array()
        .context("Source strike frame events")?
        .iter()
        .filter(|event| event["kind"] == 1)
        .collect::<Vec<_>>();
    let [primary] = primary[..] else {
        anyhow::bail!("Source strike {name} has no unique primary keyframe");
    };
    let primary = u16::try_from(
        primary["frame"]
            .as_u64()
            .context("Source strike keyframe")?,
    )?;
    ensure!(
        primary > 0 && usize::from(primary) < frames,
        "Source strike keyframe is outside its motion"
    );
    let (sound_frame, sound) = swing(source, shared, name, frames)?;
    Ok((frames, primary, sound_frame, sound))
}

/// Keep the two qualified sequence events and one source swing cue. The native
/// class-arm cue at frame zero does not belong to the source weapon strike.
fn envelope(
    clip: &Payload,
    frames: usize,
    primary: u16,
    sound_frame: u16,
    marker: u32,
) -> Result<(Payload, usize, Value)> {
    let mut sequences = BTreeMap::new();
    let mut sound = None;
    let mut omitted = Vec::new();
    for row in clip.array(0x160, 8, Some(0x8080902B))? {
        let event = clip.pointer(row)?;
        match clip.u32(event.checked_sub(4).context("Native strike event")?)? {
            0x8080903A => {
                let kind = clip.u32(event + 4)?;
                ensure!(
                    clip.u16(event + 2)? == 2
                        && matches!(kind, 1 | 14)
                        && sequences.insert(kind, event).is_none(),
                    "Unsupported native strike sequence envelope"
                );
            }
            0x80809033 => {
                ensure!(
                    clip.u16(event + 2)? == 3 && clip.u32(event + 4)? == 2,
                    "Unsupported native strike sound event"
                );
                if clip.u32(event + 24)? == PUNCH {
                    ensure!(
                        sound.replace(event).is_none(),
                        "Native strike has repeated punch sounds"
                    );
                } else {
                    ensure!(clip.u16(event)? == 0, "Unknown native strike sound timing");
                    omitted.push(clip.u32(event + 24)?);
                }
            }
            other => anyhow::bail!("Unsupported native strike event {other:08X}"),
        }
    }
    ensure!(
        sequences.len() == 2 && omitted.len() == 1,
        "Incomplete native strike event envelope"
    );
    let contact = sequences[&1];
    let lead = sequences[&14];
    let native_primary = clip.u16(contact)?;
    let native_lead = clip.u16(lead)?;
    ensure!(
        native_primary > 0 && native_lead < native_primary,
        "Unsupported native strike keyframes"
    );
    let sound = sound.context("Native strike punch sound is absent")?;
    ensure!(
        clip.u16(sound)? == native_primary,
        "Native strike punch and sequence disagree"
    );
    let mut out = clip.clone();
    put(&mut out.0, 0x120, &marker.to_le_bytes())?;
    put(&mut out.0, 0x13C, &u16::try_from(frames)?.to_le_bytes())?;
    put(&mut out.0, contact, &primary.to_le_bytes())?;
    let lead_frame =
        ((f32::from(native_lead) / f32::from(native_primary)) * f32::from(primary)).round() as u16;
    put(&mut out.0, lead, &lead_frame.to_le_bytes())?;
    put(&mut out.0, sound, &sound_frame.to_le_bytes())?;
    let mut events = vec![(lead_frame, lead), (primary, contact), (sound_frame, sound)];
    events.sort_by_key(|&(frame, at)| (frame, at));
    append(&mut out.0, 0x160, 0x8080902B, &vec![0; events.len() * 8], 8)?;
    let mut sound_index = None;
    for (index, (field, (_, target))) in out
        .array(0x160, 8, Some(0x8080902B))?
        .into_iter()
        .zip(events)
        .enumerate()
    {
        put(
            &mut out.0,
            field,
            &(target as i64 - field as i64).to_le_bytes(),
        )?;
        if target == sound {
            sound_index = Some(index);
        }
    }
    let size = out.0.len() as u64;
    put(&mut out.0, 0, &size.to_le_bytes())?;
    Ok((
        out,
        sound_index.context("Translated strike sound is absent")?,
        json!({
            "native_primary_frame":native_primary,"source_primary_frame":primary,
            "native_lead_frame":native_lead,"translated_lead_frame":lead_frame,
            "source_sound_frame":sound_frame,"omitted_native_cues":omitted
        }),
    ))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn prepare(
    source: &source::Source,
    source_root: &Path,
    native: &mut Reader,
    bank: &Payload,
    states: &Payload,
    rig: &Rig,
    controls: &Payload,
    calibration: &motion::Calibration,
    names: &[String],
    source_audio: bool,
) -> Result<Vec<Strike>> {
    ensure!(
        !names.is_empty()
            && names.len() <= 8
            && names.iter().collect::<BTreeSet<_>>().len() == names.len(),
        "Source melee choices are empty or repeated"
    );
    let descriptors = bank.array(0x68, 32, Some(0x80809002))?;
    let clips = bank.array(8, 4, Some(0x80808F48))?;
    let mut strikes = Vec::new();
    for state in STATES {
        for descriptor in animation::state::descriptors(states, state)? {
            let row = *descriptors
                .get(descriptor)
                .context("Native strike descriptor")?;
            ensure!(
                bank.u16(row + 26)? == 1,
                "Native strike requires absolute playback"
            );
            let template = bank.u32(
                *clips
                    .get(usize::from(bank.u16(row + 24)?))
                    .context("Native strike clip")?,
            )?;
            let clip = native.tag(template, Some(0x80808F49))?;
            for name in names {
                let (frames, primary, sound_frame, source_sound) = timing(source, name)?;
                let marker = hash(&format!("reach/melee/{state:08X}/{descriptor}/{name}"));
                let playback_frame = if source_audio { sound_frame } else { primary };
                let (envelope, sound_index, mut timing) =
                    envelope(&clip, frames, primary, playback_frame, marker)?;
                timing["source_sound_frame"] = json!(sound_frame);
                timing["translated_sound_frame"] = json!(playback_frame);
                let source_frames = source.clip(source_root, name, frames)?;
                let translated = calibration.translate(&source_frames, rig)?;
                let mut payload = animation::clip::replace_for(
                    &envelope,
                    &translated.poses,
                    rig.bind.len(),
                    controls,
                )?;
                let released_constraints = constraint::release(&mut payload, rig)
                    .with_context(|| format!("Source strike {name}, native clip {template:08X}"))?;
                strikes.push(Strike { state, descriptor, template, marker, payload,
                    hands:translated.hand_slots,
                    report:json!({"state":format!("{state:08X}"),"native_descriptor":descriptor,
                        "source_clip":name,"source_sound":source_sound,"frames":frames,
                        "clip_name":format!("{marker:08X}"),"sound_event_index":sound_index,
                        "source_audio":source_audio,"source_sound_frame":sound_frame,
                        "native_sound":format!("{PUNCH:08X}"),"timing":timing,
                        "released_hand_constraints":released_constraints,
                        "maximum_shoulder_reach_adjustment_metres":translated.shoulder_reach_adjustment}) });
            }
        }
    }
    Ok(strikes)
}

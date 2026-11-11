//! Weapon parts the source shows by magazine level, as a native direct-phase layer over every
//! translated action.
//!
//! A Reach first-person graph's ammunition replacement clip scales weapon parts frame by frame,
//! with frame zero for an empty magazine and the last frame for a full one. The Needler names it
//! `first_person:ammunition` and the Needle Rifle `first_person:ammunition_needles`. The
//! native arms entity's channel bank already inherits ammunition, so one more pose controller
//! input reads it, a ramp parameter divides it by the magazine size, and an appended layer plays
//! the parts' scale as an additive clip at that phase.
use super::{hash, source::Source};
use crate::{
    halo_reach::{
        Scene,
        animation::{codec::Motion, playback},
    },
    presentation::{append, put},
    tiger::{
        animation::{self, controller, layer},
        channel::inherited,
        payload::Payload,
        reader::Reader,
    },
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{fs, path::Path, sync::Arc};

/// The first-person ammunition state, alone or with a suffix naming the parts it shows.
const SOURCE: &str = "first_person:ammunition";

fn ammunition_clip(name: &str) -> bool {
    name.strip_prefix(SOURCE)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('_'))
}
/// Current magazine ammunition, as weapon presentation banks inherit it.
const AMMUNITION: u32 = 0x32cc5402;
const IDENTITY: [f32; 8] = [0., 0., 0., 1., 0., 0., 0., 1.];

/// The translated additive clip and the bank ordinal it is about to take.
pub(super) struct Clip {
    pub marker: u32,
    pub template: u32,
    pub payload: Payload,
    pub ordinal: usize,
    pub report: Value,
}

type Outcome<T> = std::result::Result<T, &'static str>;

/// The first stock additive clip a direct-phase option plays, with the most frames, for the
/// translated clip's envelope. It has no events or auxiliary tracks.
pub(super) fn template(
    native: &mut Reader,
    bank: &Payload,
    layers: &Payload,
) -> Result<Option<u32>> {
    let clips = bank.array(8, 4, Some(0x80808f48))?;
    let mut best = None;
    for row in layers.array(8, 24, Some(0x80803724))? {
        for choice in layers.array(row, 16, Some(0x8080372e))? {
            for option in layers.array(choice, 64, Some(0x80803730))? {
                if layers.0[option + 63] != 2 {
                    continue;
                }
                for weight in layers.array(option, 12, Some(0x80803737))? {
                    let ordinal = usize::try_from(layers.u32(weight)?)?;
                    let tag = bank.u32(*clips.get(ordinal).context("Phase clip ordinal")?)?;
                    let clip = native.tag(tag, Some(0x80808f49))?;
                    let frames = clip.u16(0x13c)?;
                    if clip.u8(0x181)? == 1
                        && clip.u64(0x108)? == 0
                        && clip.u64(0x160)? == 0
                        && best.is_none_or(|(most, _)| frames > most)
                    {
                        best = Some((frames, tag));
                    }
                }
            }
        }
    }
    Ok(best.map(|(_, tag)| tag))
}

/// The source parts' scale per frame, by native control, when the source clip scales only
/// translated weapon parts.
fn scales(source: &Source, root: &Path, parts: &[Value]) -> Result<Outcome<AmmunitionClip>> {
    let graph = &source.graph;
    let clips = graph["clips"]
        .as_array()
        .context("Source clips")?
        .iter()
        .filter(|clip| clip["name"].as_str().is_some_and(ammunition_clip))
        .collect::<Vec<_>>();
    let [clip] = clips.as_slice() else {
        return Ok(Err("The source graph has no single ammunition clip"));
    };
    let variants = clip["shared"]
        .as_array()
        .context("Source ammunition variants")?;
    let [shared] = variants.as_slice() else {
        return Ok(Err("The source ammunition clip has several variants"));
    };
    // Type 3 replaces only the parts it animates.
    if shared["animation_type"] != 3 {
        return Ok(Err("The source ammunition clip is not a replacement"));
    }
    let member = playback::member(graph, shared)?;
    let file = member["motion"]["file"]
        .as_str()
        .context("Source ammunition motion")?;
    let motion: Motion = serde_json::from_slice(&fs::read(root.join(file))?)?;
    let skeleton = graph["skeleton"].as_array().context("Source skeleton")?;
    ensure!(
        motion.frames >= 2 && motion.tracks.len() == skeleton.len(),
        "Source ammunition motion dimensions differ"
    );
    let mut result = Vec::new();
    for (track, bone) in motion.tracks.iter().zip(skeleton) {
        if track.rotation.is_empty() && track.translation.is_empty() && track.scale.is_empty() {
            continue;
        }
        if !track.rotation.is_empty() || !track.translation.is_empty() {
            return Ok(Err(
                "The source ammunition clip moves parts as well as scaling them",
            ));
        }
        let name = bone["name"].as_str().context("Source bone name")?;
        let Some(part) = parts
            .iter()
            .find(|part| part["source"] == name && part["status"] == "translated")
        else {
            return Ok(Err(
                "The source ammunition clip scales a part without a native control",
            ));
        };
        let control = usize::try_from(part["control"].as_u64().context("Part control")?)?;
        ensure!(
            track.scale.len() == motion.frames || track.scale.len() == 1,
            "Incomplete source ammunition scale"
        );
        ensure!(
            track.scale.iter().all(|s| s.is_finite() && *s > 0.),
            "Invalid source ammunition scale"
        );
        result.push((control, track.scale.clone()));
    }
    if result.is_empty() {
        return Ok(Err("The source ammunition clip scales nothing"));
    }
    let name = clip["name"]
        .as_str()
        .context("Source clip name")?
        .to_owned();
    Ok(Ok((name, result)))
}

/// The loaded magazine size of the source weapon's single barrel, when it draws one.
pub(super) fn capacity(scene: &Scene) -> Option<f32> {
    let gameplay = &scene.object.gameplay;
    let Some([barrel]) = gameplay["barrels"].as_array().map(Vec::as_slice) else {
        return None;
    };
    let index = usize::try_from(barrel["magazine"].as_i64()?).ok()?;
    let loaded = gameplay["magazines"][index]["loaded"].as_i64()?;
    (1..=500).contains(&loaded).then_some(loaded as f32)
}

/// Linear samples of `track` at `frames` evenly spaced times.
fn resample(track: &[f32], frames: usize) -> Vec<f32> {
    (0..frames)
        .map(|frame| {
            let time = frame as f32 * (track.len() - 1) as f32 / (frames - 1) as f32;
            let low = time.floor() as usize;
            let high = (low + 1).min(track.len() - 1);
            track[low] + (track[high] - track[low]) * (time - low as f32)
        })
        .collect()
}

/// Translates the source ammunition clip into an additive native clip on `ordinal`, or
/// returns why it has none.
#[allow(clippy::too_many_arguments)]
pub(super) fn clip(
    native: &mut Reader,
    source: &Source,
    root: &Path,
    bank: &Payload,
    layers: &Payload,
    rig: &animation::control::Rig,
    controls: &Payload,
    parts: &[Value],
    ordinal: usize,
) -> Result<Outcome<Clip>> {
    let (name, scales) = match scales(source, root, parts)? {
        Ok(scales) => scales,
        Err(reason) => return Ok(Err(reason)),
    };
    let Some(template) = template(native, bank, layers)? else {
        return Ok(Err("The native arms have no additive direct-phase clip"));
    };
    let envelope = native.tag(template, Some(0x80808f49))?;
    let frames = usize::from(envelope.u16(0x13c)?);
    ensure!(frames >= 2, "Native phase clip is too short");
    // Additive composition multiplies scale, so each part's delta is its source scale.
    let mut deltas = vec![vec![IDENTITY; frames]; rig.controls.len()];
    let mut report = Vec::new();
    for (control, track) in &scales {
        let delta = deltas
            .get_mut(*control)
            .context("Ammunition part control is outside the rig")?;
        for (pose, scale) in delta.iter_mut().zip(resample(track, frames)) {
            pose[7] = scale;
        }
        report.push(json!({"control":control,"source_frames":track.len()}));
    }
    let payload =
        animation::clip::replace_deltas_for(&envelope, &deltas, rig.bind.len(), controls)?;
    Ok(Ok(Clip {
        marker: hash("reach/arms/ammunition"),
        template,
        payload,
        ordinal,
        report: json!({"source":name,"template":format!("{template:08X}"),"frames":frames,"parts":report}),
    }))
}

/// The converted owners, tables and entity that carry the ammunition layer.
pub(super) struct Wiring {
    pub layers: Payload,
    pub controller: Payload,
    pub controller_allocation: (u32, Payload),
    pub implementation: (u32, Payload),
    pub implementation_allocation: (u32, Payload),
    pub library: (u32, Payload),
    pub entity: Payload,
    pub report: Value,
}

/// Connection rows of the native `80809BC9` arrays at entity `+20` and `+30`.
fn connections(entity: &Payload) -> Result<Vec<usize>> {
    let mut rows = entity.array(0x20, 72, Some(0x80809bc9))?;
    rows.extend(entity.array(0x30, 72, Some(0x80809bc9))?);
    Ok(rows)
}

/// Adds the ammunition input, phase parameters, layer and buffers to the first-person arms.
/// `capacity` is the translated magazine size.
pub(super) fn wire(
    native: &mut Reader,
    clip: &Clip,
    layers: &Payload,
    consumers: &[(u32, Arc<Payload>)],
    (controller_tag, entity_tag): (u32, u32),
    capacity: f32,
) -> Result<Outcome<Wiring>> {
    ensure!(
        capacity.is_finite() && capacity >= 1.,
        "Invalid ammunition phase capacity"
    );
    let wiring = phase(
        native,
        clip,
        layers,
        consumers,
        (controller_tag, entity_tag),
        &Input {
            channel: AMMUNITION,
            names: [
                hash("reach/ammunition"),
                hash("reach/ammunition_phase"),
                hash("reach/arms/ammunition"),
            ],
            range: [0., capacity],
        },
    )?;
    Ok(wiring.map(|mut wiring| {
        wiring.report["capacity"] = json!(capacity);
        wiring.report["limits"] = json!([
            "The phase divides the ammunition channel by the imported magazine size, so a perk or mod that changes the magazine does not reach it.",
            "The parts scale through an additive layer after every action, including reload, so they follow the magazine count as the reload credits it.",
            "That the inherited ammunition is the current magazine count and the layer's in-game composition remain gameplay checks."
        ]);
        wiring
    }))
}

/// The arms channel a phase layer reads, the names of its two parameters and its layer, and the
/// channel range that maps onto the clip from its first frame to its last.
pub(super) struct Input {
    pub channel: u32,
    pub names: [u32; 3],
    pub range: [f32; 2],
}

/// Adds a ramp parameter over `input.range` and a direct-phase layer playing `clip` at it, with
/// their buffers, to the stock first-person arms. The ramp reads the controller input that
/// already carries `input.channel`, as stock arms do for aim, or one more input reading it from
/// the arms' inherited values.
pub(super) fn phase(
    native: &mut Reader,
    clip: &Clip,
    layers: &Payload,
    consumers: &[(u32, Arc<Payload>)],
    (controller_tag, entity_tag): (u32, u32),
    input: &Input,
) -> Result<Outcome<Wiring>> {
    ensure!(
        input.range.iter().all(|v| v.is_finite()) && input.range[1] > input.range[0],
        "Invalid phase input range"
    );
    let find = |class: u32| -> Result<(u32, Payload)> {
        let found = consumers
            .iter()
            .filter(|(_, p)| p.pointer(16).and_then(|at| p.u32(at - 4)).ok() == Some(class))
            .collect::<Vec<_>>();
        let [(tag, payload)] = found.as_slice() else {
            anyhow::bail!("Native arms animation owner {class:08X} is absent or ambiguous")
        };
        Ok((*tag, (**payload).clone()))
    };
    let (tag, original) = find(0x8080348f)?;
    ensure!(tag == controller_tag, "Native pose controller differs");
    let existing = controller::input(&original, input.channel)?;
    let entity = native.tag(entity_tag, Some(0x80809c0f))?;
    // Every existing controller input reads one channel bank, the arms' inherited values.
    let rows = connections(&entity)?;
    let inputs = rows
        .iter()
        .copied()
        .filter(|&row| {
            entity.u32(row + 8).ok() == Some(controller_tag)
                && entity.u32(row + 12).ok() == Some(0x80809789)
        })
        .collect::<Vec<_>>();
    let first = *inputs
        .first()
        .context("Native pose inputs have no source")?;
    let provider = entity.bytes::<32>(first + 32)?;
    ensure!(
        inputs
            .iter()
            .all(|&row| entity.bytes::<32>(row + 32).ok() == Some(provider)),
        "Native pose inputs read several providers"
    );
    let bank = native.tag(entity.u32(first + 40)?, Some(0x80809c36))?;
    let Some(channel) = inherited::declaration(&bank, input.channel)? else {
        return Ok(Err(
            "The native arms bank does not inherit the phase channel",
        ));
    };
    let added_inputs = if existing.is_some() {
        Vec::new()
    } else {
        vec![input.channel]
    };
    let (library_tag, _) = controller::resources(&original)?;
    let library = native.tag(library_tag, None)?;
    let parameters_before = library.array(8, 24, Some(0x80803650))?.len();
    let allocation_tag = original.u32(0x44)?;
    let allocation = native.tag(allocation_tag, Some(0x80809bbb))?;
    let grown = controller::grow(
        &original,
        &allocation,
        &controller::Growth {
            inputs: &added_inputs,
            parameters_before,
            parameters: 2,
            layers: 1,
        },
    )?;
    let (ordinal, declaration) = match (existing, grown.added.first()) {
        (Some(ordinal), _) => (ordinal, None),
        (None, Some(&(ordinal, declaration))) => (ordinal, Some(declaration)),
        (None, None) => anyhow::bail!("Pose controller input was not added"),
    };
    let (library_payload, phase) = controller::ramp(
        &library,
        [input.names[0], input.names[1]],
        input.channel,
        ordinal,
        input.range,
    )?;
    let frames = clip.payload.u16(0x13c)?;
    let (layers, index) = layer::append_phase(
        layers,
        &layer::Phase {
            name: input.names[2],
            parameter: 0x10000 | u32::try_from(phase)?,
            clip: u32::try_from(clip.ordinal)?,
            size: animation::clip::size(&clip.payload)?,
            // Descriptor durations span the first to the last frame at 30 frames a second.
            duration: f32::from(frames - 1) / 30.,
        },
    )?;
    ensure!(
        index == grown.layers_before,
        "Appended pose layer and its state differ"
    );
    let (implementation_tag, implementation) = find(0x808036e2)?;
    let implementation_allocation_tag = implementation.u32(0x44)?;
    let (implementation, implementation_allocation) = controller::outputs(
        &implementation,
        implementation_tag,
        &*native.tag(implementation_allocation_tag, Some(0x80809bbb))?,
        grown.layers_before,
        1,
    )?;
    let mut converted = (*entity).clone();
    for row in connections(&entity)? {
        if entity.u32(row + 8)? != controller_tag || entity.u32(row + 12)? != 0x80809789 {
            continue;
        }
        let offset = usize::try_from(entity.u64(row + 16)?)?;
        let (_, moved) = grown
            .definitions
            .iter()
            .find(|(old, _)| *old == offset)
            .context("Pose input connection is outside its definitions")?;
        put(&mut converted.0, row + 16, &(*moved as u64).to_le_bytes())?;
    }
    if let Some(declaration) = declaration {
        let old = converted.array(0x20, 72, Some(0x80809bc9))?;
        let mut data = old
            .iter()
            .flat_map(|&at| converted.0[at..at + 72].to_vec())
            .collect::<Vec<_>>();
        let mut row = converted.bytes::<72>(first)?;
        row[16..24].copy_from_slice(&(declaration as u64).to_le_bytes());
        row[64..68].copy_from_slice(&channel.to_le_bytes());
        data.extend(row);
        // The rows move to an array of their own. The old ones are cleared, so no owner tag
        // stays behind in bytes the build's owner retargeting no longer reaches.
        for &at in &old {
            converted.0[at..at + 72].fill(0);
        }
        append(&mut converted.0, 0x20, 0x80809bc9, &data, 72)?;
    }
    Ok(Ok(Wiring {
        layers,
        controller: grown.controller,
        controller_allocation: (allocation_tag, grown.allocation),
        implementation: (implementation_tag, implementation),
        implementation_allocation: (implementation_allocation_tag, implementation_allocation),
        library: (library_tag, library_payload),
        entity: converted,
        report: json!({"status":"translated","clip":clip.report,"channel":format!("{:08X}", input.channel),
            "provider":format!("{:08X}", entity.u32(first + 40)?),"declaration":channel,"input":ordinal,"input_added":existing.is_none(),
            "phase_parameter":phase,"range":input.range,"layer":index,
            "gameplay_verified":false}),
    }))
}

type AmmunitionClip = (String, Vec<(usize, Vec<f32>)>);

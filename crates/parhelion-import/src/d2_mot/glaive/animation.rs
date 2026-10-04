//! The glaive's first-person edits on the glaive base.
//!
//! Every glaive plays the same source first-person set, so its conversion onto the base is the
//! same for every glaive, and these edits, made by hand for The Enigma, apply to all of them:
//!
//! - Hip fire: the source fire clip joins the bank with its source descriptor, and every choice
//!   of the native fire state plays it. The clip has the holding pose baked in, so its route is
//!   the one its source route lowers to, the two look layers.
//! - Holding pose: the one native pose layer no state reaches becomes the source holding layer,
//!   with the generic overlay's entry, and every descriptor whose source route plays the
//!   holding or generic layer plays its native counterpart.
//! - Melee: three source swings convert with the events native clips support, keep the bones of
//!   the swing the importer converted itself, are trimmed to 28 frames (the native busy
//!   deadline is the clip length less two frames), and the two light attack states pick among
//!   them.
//! - Sprint: the sprint loop's sway layer plays the source sway clip, its two arm poses play the
//!   source poses, and the converted sprint descriptor takes the route that plays all three.
//! - Ready: after every class melee the client raises the weapon through the ready state, so
//!   ready plays one idle frame instead of the equip clip. A swap therefore also shows no equip
//!   flourish. This follows the pose refresh, which finds each descriptor's source by its clip.
//!
//! The clips and descriptors are found by the source identities every glaive shares, and each
//! edit checks the structure it expects before it changes anything.
use crate::d2_mot::{
    payload::Payload,
    reader::{Reader, write_json},
    rig_convert::{
        animation::{
            clips,
            first_person::{dispatch, poses},
        },
        write_array,
    },
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
    sync::Arc,
};

const FIRE_CLIP: u32 = 0x80A8_A407;
/// The fire clip's frame-zero selector event has no native counterpart.
const FIRE_OMITTED_EVENTS: [usize; 1] = [0];
const FIRE_STATE: u32 = 0x9FAC_79C9;
const SPRINT_SWAY: u32 = 0x80A8_A3F0;
const SPRINT_LAYER: u32 = 0x4D46_7CC1;
const SPRINT_DESCRIPTOR: u32 = 0xF932_09E9;
/// The sprint loop's family 1 pose layers, their choice and the source pose each plays.
const SPRINT_POSES: [(u32, usize, u32); 2] =
    [(0xA6A1_214F, 2, 0x80A8_A5F4), (0x292F_EA2C, 4, 0x80A8_A5F5)];
/// The swings and the source events each keeps. The third swing's clip is already converted
/// and is the event template the other two follow.
const SWINGS: [(u32, [usize; 3]); 3] = [
    (0x80A8_A3F5, [0, 2, 3]),
    (0x80A8_A441, [0, 2, 3]),
    (0x80A8_A43E, [0, 1, 2]),
];
const SWING_FRAMES: u16 = 28;
/// The light attack states, which pick a swing by cumulative weight.
const SWING_STATES: [u32; 2] = [0xFDCA_54D9, 0xFDCA_54DB];
const READY_STATE: u32 = 0xDCA2_827A;
const IDLE_STATE: u32 = 0x6FB7_60FF;
const EQUIP_CLIP: u32 = 0x80A8_A40D;
const IDLE_CLIP: u32 = 0x80A8_A3C2;
/// The generic weapon overlay layer, present in both versions.
const GENERIC_LAYER: u32 = 0xFC23_AA50;

// Converted bank and state table layouts.
const CLIP_SLOTS: u32 = 0x8080_8F48;
const CLIP_AUXILIARY: u32 = 0x8080_0007;
const DESCRIPTORS: u32 = 0x8080_9002;
const STATE_NAMES: u32 = 0x8080_342E;
const STATE_NODES: u32 = 0x8080_342F;
const WEIGHTED_DESCRIPTORS: u32 = 0x8080_3439;
// Pose layer table layouts.
const LAYER_CHOICES: u32 = 0x8080_372E;
const LAYER_ENTRIES: u32 = 0x8080_3730;
const LAYER_CLIPS: u32 = 0x8080_3737;
const ROUTES: u32 = 0x8080_34EF;
const ROUTE_EDGES: u32 = 0x8080_34F1;
const DISPATCH: u32 = 0x8080_0006;
const FRAMES: usize = 0x13C;
const FRAME: f32 = 1.0 / 30.0;

pub(super) struct Inputs<'a> {
    pub modern: &'a Path,
    pub native: &'a Path,
    pub graph: &'a Path,
    pub work: &'a Path,
    pub source_rig: &'a Value,
    pub native_rig: &'a Value,
    pub calibration: &'a Value,
}

fn hex(value: &Value) -> Result<u32> {
    Ok(u32::from_str_radix(
        value.as_str().context("tag text")?.trim_start_matches("0x"),
        16,
    )?)
}

/// A clip's length in seconds at 30 frames a second, rounded once from double precision as the
/// importer's descriptors are.
fn duration(frames: u16) -> f32 {
    (f64::from(frames.saturating_sub(1)) / 30.0) as f32
}

fn put<const N: usize>(data: &mut [u8], at: usize, bytes: [u8; N]) -> Result<()> {
    data.get_mut(at..at + N)
        .context("write outside payload")?
        .copy_from_slice(&bytes);
    Ok(())
}

/// Appends a replacement array for the descriptor at `field` and returns its first row.
fn append(
    data: &mut Vec<u8>,
    field: usize,
    class: u32,
    count: usize,
    rows: &[u8],
) -> Result<usize> {
    write_array(data, field, class, count, rows)?;
    let size = data.len() as u64;
    put(data, 0, size.to_le_bytes())?;
    Ok(Payload(data.clone()).pointer(field + 8)? + 16)
}

fn size_word(data: &mut [u8]) -> Result<()> {
    let size = data.len() as u64;
    put(data, 0, size.to_le_bytes())
}

/// A converted clip with the frame count it plays.
struct Converted {
    payload: Payload,
    report: Value,
}

/// The slot table and event library every conversion here uses, gathered as the importer
/// gathers them: name-matched pairs, the calibration pairs and the calibration bank's pairs.
struct Conversion {
    slots: clips::slots::Table,
    library: clips::EventLibrary,
    native_bones: u16,
}

fn conversion(
    sr: &mut Reader,
    nr: &mut Reader,
    fp: &Value,
    graph: &Path,
    calibration: &Value,
) -> Result<Conversion> {
    let mut pairs: Vec<(Arc<Payload>, Arc<Payload>)> = Vec::new();
    for row in fp["clips"].as_array().context("converted clips")? {
        let source = sr.tag(
            u32::try_from(row["source"].as_u64().context("source clip")?)?,
            Some(0x8080_8BE0),
        )?;
        let native = nr.tag(
            u32::try_from(row["native"].as_u64().context("native clip")?)?,
            Some(0x8080_8F49),
        )?;
        if source.u32(0x120)? == native.u32(0x120)? {
            pairs.push((source, native));
        }
    }
    for row in calibration["clip_calibration"]
        .as_array()
        .context("clip calibration")?
    {
        let source = sr.tag(hex(&row["source"])?, Some(0x8080_8BE0))?;
        let native = nr.tag(hex(&row["native"])?, Some(0x8080_8F49))?;
        ensure!(
            source.u32(0x120)? == native.u32(0x120)?,
            "calibration clip identity differs"
        );
        pairs.push((source, native));
    }
    let lookup = calibration["components"]
        .as_array()
        .context("calibration components")?
        .iter()
        .find(|row| row["class"] == "808025F8" && row["entity"] != calibration["runtime_entity"])
        .context("calibration lookup owner")?;
    let lookup = sr.tag(hex(&lookup["owner"])?, Some(0x8080_9B06))?;
    let bank = sr.tag(lookup.u32(lookup.pointer(24)? + 0xA8)?, Some(0x8080_289F))?;
    let native_bank = Payload(fs::read(
        graph.join(fp["files"]["bank"].as_str().context("native bank file")?),
    )?);
    let mut names = BTreeMap::new();
    for at in native_bank.array(8, 4, Some(CLIP_SLOTS))? {
        let clip = nr.tag(native_bank.u32(at)?, Some(0x8080_8F49))?;
        names.insert(clip.u32(0x120)?, clip);
    }
    for at in bank.array(8, 16, Some(0x8080_8BDF))? {
        let clip = sr.tag(sr.ref64(&bank, at)?, Some(0x8080_8BE0))?;
        if let Some(native) = names.get(&clip.u32(0x120)?) {
            pairs.push((clip, native.clone()));
        }
    }
    let native_bones = names
        .values()
        .next()
        .context("native bank has no clips")?
        .u16(0x13E)?;
    Ok(Conversion {
        slots: clips::slots::Table::derive(pairs.iter().map(|(s, n)| (s.as_ref(), n.as_ref())))?,
        library: clips::EventLibrary::derive(pairs.iter().map(|(s, n)| (s.as_ref(), n.as_ref()))),
        native_bones,
    })
}

/// Converts a source clip keeping the listed events, or every event but the omitted ones, and
/// aligns its tracks to the base's slots. A swing keeps the bones of the swing the importer
/// converted itself, which it plays beside.
fn convert(
    sr: &mut Reader,
    with: &Conversion,
    library: &clips::EventLibrary,
    tag: u32,
    keep: Option<(&[usize], u16)>,
    omit: &[usize],
) -> Result<Converted> {
    let swing_bones = keep.map(|(_, bones)| bones);
    let keep = keep.map(|(keep, _)| keep);
    let source = sr.tag(tag, Some(0x8080_8BE0))?;
    let events = usize::try_from(source.u64(0x160)?)?;
    let retained = match keep {
        Some(keep) => keep.to_vec(),
        None => (0..events).filter(|i| !omit.contains(i)).collect(),
    };
    ensure!(
        retained.iter().all(|i| *i < events) && omit.iter().all(|i| *i < events),
        "source clip {tag:08X} lacks an event the glaive keeps"
    );
    let (mut payload, report) = if events == 0 {
        clips::convert(&source.0)?
    } else {
        clips::convert_with_selected_events(&source.0, &retained, library)?
    };
    match swing_bones {
        Some(bones) => with.slots.apply_prefix(&mut payload, bones).map(drop),
        None => with
            .slots
            .apply(&mut payload)
            .or_else(|_| with.slots.apply_prefix(&mut payload, with.native_bones))
            .map(drop),
    }
    .with_context(|| format!("source clip {tag:08X} slot alignment"))?;
    Ok(Converted { payload, report })
}

fn source_name(sr: &mut Reader, tag: u32) -> Result<u32> {
    sr.tag(tag, Some(0x8080_8BE0))?.u32(0x120)
}

/// The name of the one plain source descriptor that plays `clip`.
fn source_descriptor(sr: &mut Reader, bank: &Payload, clip: u32) -> Result<u32> {
    let mut names = BTreeSet::new();
    for row in bank.array(0x58, 48, Some(0x8080_8BDE))? {
        if sr.ref64(bank, row + 24)? == clip
            && bank.bytes::<16>(row)? == [0; 16]
            && bank.f32(row + 20)? == 1.0
        {
            names.insert(bank.u32(row + 16)?);
        }
    }
    let [name] = names.into_iter().collect::<Vec<_>>()[..] else {
        anyhow::bail!("source clip {clip:08X} has no single plain descriptor");
    };
    Ok(name)
}

// Trim: keep a clip's first frames byte for byte.

const UNIFORM: u32 = 0x8080_8F71;
const SCALAR: u32 = 0x8080_8F7C;
const CURVE: u32 = 0x8080_8F78;
const STATIC: u32 = 0x8080_8F6F;
const CONSTANT: u32 = 0x8080_8F59;
const SAMPLES: u32 = 0x8080_000A;
const FLOATS: u32 = 0x8080_000F;
const SCALAR_INDICES: u32 = 0x8080_8F82;
const CURVE_WORDS: u32 = 0x8080_0006;
const CURVE_BYTES: u32 = 0x8080_0009;
const SEQUENCE_EVENT: u32 = 0x8080_903A;

fn set_count(data: &mut [u8], clip: &Payload, field: usize, count: usize) -> Result<()> {
    let header = clip.pointer(field + 8)?;
    put(data, field, (count as u64).to_le_bytes())?;
    put(data, header, (count as u64).to_le_bytes())
}

/// Each event's frame and class, with the event list's own count check.
fn clip_events(clip: &Payload) -> Result<Vec<(u16, u32)>> {
    clip.array(0x160, 8, None)?
        .into_iter()
        .map(|row| {
            let at = clip.pointer(row)?;
            Ok((clip.u16(at)?, clip.u32(at - 4)?))
        })
        .collect()
}

fn trim_uniform(data: &mut [u8], clip: &Payload, s: usize, frames: usize, n: usize) -> Result<()> {
    let (encoding, scale, rotation, position) = (
        clip.u16(s)?,
        clip.u16(s + 2)?,
        clip.u16(s + 4)?,
        clip.u16(s + 6)?,
    );
    ensure!(encoding == 2, "uniform stream encoding {encoding}");
    ensure!(
        clip.u32(s + 0x10)? as usize == frames,
        "uniform stream frames differ from the clip's"
    );
    let widths = std::iter::repeat_n(1, usize::from(scale))
        .chain(std::iter::repeat_n(4, usize::from(rotation)))
        .chain(std::iter::repeat_n(3, usize::from(position)))
        .collect::<Vec<usize>>();
    let channels = widths.iter().sum::<usize>();
    let samples = clip.array(s + 0x18, 2, Some(SAMPLES))?;
    ensure!(
        samples.len() == channels * frames,
        "uniform stream samples differ from its channels"
    );
    for field in [0x28, 0x38] {
        ensure!(
            clip.array(s + field, 4, Some(FLOATS))?.len() == channels,
            "uniform stream quantization differs"
        );
    }
    let first = *samples.first().context("uniform stream has no samples")?;
    let old = clip.0[first..first + samples.len() * 2].to_vec();
    let mut kept = Vec::with_capacity(channels * n * 2);
    let mut offset = 0;
    for width in widths {
        kept.extend_from_slice(&old[offset..offset + n * width * 2]);
        offset += frames * width * 2;
    }
    data[first..first + kept.len()].copy_from_slice(&kept);
    data[first + kept.len()..first + old.len()].fill(0);
    set_count(data, clip, s + 0x18, channels * n)?;
    put(data, s + 0x10, (n as u32).to_le_bytes())
}

fn trim_scalar(data: &mut [u8], clip: &Payload, s: usize, frames: usize, n: usize) -> Result<()> {
    let (encoding, tracks) = (clip.u16(s)?, usize::from(clip.u16(s + 2)?));
    ensure!(encoding == 2, "scalar stream encoding {encoding}");
    ensure!(
        clip.u32(s + 12)? as usize == frames,
        "scalar stream frames differ from the clip's"
    );
    let indices = clip.array(s + 0x10, 2, Some(SCALAR_INDICES))?;
    ensure!(indices.len() == tracks, "scalar stream index count differs");
    let mut dense = 0;
    for (track, at) in indices.iter().enumerate() {
        let value = clip.u16(*at)?;
        ensure!(
            usize::from(value & 0x3FFF) == track && matches!(value >> 14, 1 | 2),
            "scalar stream has explicit track indices"
        );
        dense += usize::from(value >> 14 == 1);
    }
    for field in [0x30, 0x40] {
        ensure!(
            clip.array(s + field, 4, Some(FLOATS))?.len() == tracks,
            "scalar stream quantization differs"
        );
    }
    let samples = clip.array(s + 0x20, 2, Some(SAMPLES))?;
    ensure!(
        samples.len() == dense * frames,
        "scalar stream samples differ from its tracks"
    );
    if let Some(&first) = samples.first() {
        let old = clip.0[first..first + samples.len() * 2].to_vec();
        let kept = (0..dense)
            .flat_map(|track| old[track * frames * 2..track * frames * 2 + n * 2].to_vec())
            .collect::<Vec<_>>();
        data[first..first + kept.len()].copy_from_slice(&kept);
        data[first + kept.len()..first + old.len()].fill(0);
        set_count(data, clip, s + 0x20, dense * n)?;
    }
    put(data, s + 12, (n as u32).to_le_bytes())
}

/// The variable curve codec samples by time with explicit offsets, so it keeps its data, but
/// every track must still reach the new last frame.
fn check_curve(clip: &Payload, s: usize, n: usize) -> Result<()> {
    let tracks = usize::from(clip.u16(s + 2)?);
    ensure!(
        clip.array(s + 0x10, 2, Some(CURVE_WORDS))?.is_empty(),
        "curve stream has dense samples"
    );
    let indices = clip.array(s + 0x50, 2, Some(CURVE_WORDS))?;
    ensure!(
        indices.len() == tracks + 1,
        "curve stream index count differs"
    );
    let sparse = clip.array(s + 0x20, 2, Some(CURVE_WORDS))?;
    let deltas = clip.array(s + 0x30, 1, Some(CURVE_BYTES))?;
    let index = |i: usize| -> Result<i16> { clip.i16(indices[i]) };
    ensure!(
        usize::try_from(index(tracks)?).ok() == Some(sparse.len()),
        "curve stream terminator differs"
    );
    for track in 0..tracks {
        let first = index(track)?;
        ensure!(first > 0, "curve stream track {track} is dense");
        let at = sparse[usize::try_from(first - 1)?];
        let (offset, segments) = (
            usize::try_from(clip.i16(at)?)?,
            usize::try_from(clip.i16(at + 4)?)?,
        );
        ensure!(
            segments > 0 && offset + segments <= deltas.len(),
            "curve stream segments overflow"
        );
        let span = (0..segments)
            .map(|k| clip.u8(deltas[offset + k]).map(usize::from))
            .sum::<Result<usize>>()?;
        ensure!(
            span + 1 >= n,
            "curve stream track {track} ends before the new last frame"
        );
    }
    Ok(())
}

/// Keeps a converted clip's first `n` frames. Its contact events and every other event must
/// fall before them.
fn trim(clip: &Payload, n: u16) -> Result<Payload> {
    let frames = clip.u16(FRAMES)?;
    ensure!(n < frames, "clip has {frames} frames, not more than {n}");
    let events = clip_events(clip)?;
    let contact = events
        .iter()
        .filter(|(_, class)| *class == SEQUENCE_EVENT)
        .map(|(frame, _)| *frame)
        .max();
    ensure!(
        contact.is_some_and(|frame| frame < n),
        "swing contact is not before frame {n}"
    );
    ensure!(
        events.iter().all(|(frame, _)| *frame < n),
        "swing has an event at or past frame {n}"
    );
    let (frames, n_frames) = (usize::from(frames), usize::from(n));
    let mut data = clip.0.clone();
    for field in (0x10..0x68).step_by(8) {
        if clip.u64(field)? == 0 {
            continue;
        }
        let s = clip.pointer(field)?;
        match clip.u32(s - 4)? {
            UNIFORM => trim_uniform(&mut data, clip, s, frames, n_frames)?,
            SCALAR => trim_scalar(&mut data, clip, s, frames, n_frames)?,
            CURVE => check_curve(clip, s, n_frames)?,
            STATIC | CONSTANT => {}
            class => anyhow::bail!("swing stream at {field:#x} is codec {class:08X}"),
        }
    }
    put(&mut data, FRAMES, n.to_le_bytes())?;
    let trimmed = Payload(data);
    ensure!(
        clip_events(&trimmed)? == events,
        "trimming changed the swing's events"
    );
    Ok(trimmed)
}

// The converted bank: clip slots, descriptors and pose layer names.

struct Bank {
    data: Vec<u8>,
    clips: Vec<u32>,
    auxiliary: Vec<u32>,
    descriptors: Vec<[u8; 32]>,
}

impl Bank {
    fn read(data: Vec<u8>) -> Result<Self> {
        let p = Payload(data);
        let clips = p
            .array(8, 4, Some(CLIP_SLOTS))?
            .into_iter()
            .map(|at| p.u32(at))
            .collect::<Result<Vec<_>>>()?;
        let auxiliary = p
            .array(24, 4, Some(CLIP_AUXILIARY))?
            .into_iter()
            .map(|at| p.u32(at))
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            auxiliary.len() == clips.len(),
            "bank auxiliary count differs"
        );
        let descriptors = p
            .array(0x68, 32, Some(DESCRIPTORS))?
            .into_iter()
            .map(|at| p.bytes::<32>(at))
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            data: p.0,
            clips,
            auxiliary,
            descriptors,
        })
    }

    fn name(row: &[u8; 32]) -> u32 {
        u32::from_le_bytes(row[16..20].try_into().unwrap_or_default())
    }

    fn slot(row: &[u8; 32]) -> usize {
        usize::from(u16::from_le_bytes([row[24], row[25]]))
    }

    fn mode(row: &[u8; 32]) -> u16 {
        u16::from_le_bytes([row[26], row[27]])
    }

    fn clip_of(&self, row: &[u8; 32]) -> Option<u32> {
        self.clips.get(Self::slot(row)).copied()
    }

    /// Descriptor indices that play `clip`.
    fn playing(&self, clip: u32) -> Vec<usize> {
        (0..self.descriptors.len())
            .filter(|&i| self.clip_of(&self.descriptors[i]) == Some(clip))
            .collect()
    }

    fn set_duration(&mut self, index: usize, seconds: f32) {
        self.descriptors[index][20..24].copy_from_slice(&seconds.to_le_bytes());
    }

    fn slot_of(&mut self, clip: u32) -> usize {
        if let Some(slot) = self.clips.iter().position(|c| *c == clip) {
            return slot;
        }
        self.clips.push(clip);
        self.auxiliary.push(u32::MAX);
        self.clips.len() - 1
    }

    /// Appends a descriptor that plays `clip` in playback mode 1, as converted clips do.
    fn add(&mut self, name: u32, clip: u32, frames: u16) -> Result<usize> {
        let slot = u16::try_from(self.slot_of(clip))?;
        let mut row = [0; 32];
        row[16..20].copy_from_slice(&name.to_le_bytes());
        row[20..24].copy_from_slice(&duration(frames).to_le_bytes());
        row[24..26].copy_from_slice(&slot.to_le_bytes());
        row[26..28].copy_from_slice(&1u16.to_le_bytes());
        self.descriptors.push(row);
        Ok(self.descriptors.len() - 1)
    }

    fn write(mut self) -> Result<Vec<u8>> {
        let words = |values: &[u32]| {
            values
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>()
        };
        let (clips, auxiliary) = (words(&self.clips), words(&self.auxiliary));
        write_array(&mut self.data, 8, CLIP_SLOTS, self.clips.len(), &clips)?;
        write_array(
            &mut self.data,
            24,
            CLIP_AUXILIARY,
            self.auxiliary.len(),
            &auxiliary,
        )?;
        let rows = self.descriptors.concat();
        write_array(
            &mut self.data,
            0x68,
            DESCRIPTORS,
            self.descriptors.len(),
            &rows,
        )?;
        size_word(&mut self.data)?;
        Ok(self.data)
    }
}

// The state table: names, nodes and their weighted descriptor choices.

/// Every node's weighted descriptor lists, by node.
fn state_lists(states: &Payload) -> Result<Vec<Vec<usize>>> {
    let mut lists = Vec::new();
    for node in states.array(24, 16, Some(STATE_NODES))? {
        let at = states.pointer(node + 8)?;
        lists.push(if states.u64(node)? == 1 {
            vec![at]
        } else {
            states.array(at, 16, None)?
        });
    }
    Ok(lists)
}

fn state_nodes(states: &Payload) -> Result<BTreeMap<u32, usize>> {
    states
        .array(8, 8, Some(STATE_NAMES))?
        .into_iter()
        .map(|at| Ok((states.u32(at)?, usize::try_from(states.u32(at + 4)?)?)))
        .collect()
}

fn weighted(states: &Payload, list: usize) -> Result<Vec<(u32, f32)>> {
    states
        .array(list, 8, Some(WEIGHTED_DESCRIPTORS))?
        .into_iter()
        .map(|at| Ok((states.u32(at)?, states.f32(at + 4)?)))
        .collect()
}

// The pose layer table.

type Route = [Vec<(u16, u16)>; 2];

struct Layers {
    data: Vec<u8>,
}

impl Layers {
    fn p(&self) -> Payload {
        Payload(self.data.clone())
    }

    fn layers(&self, family: usize) -> Result<Vec<usize>> {
        self.p().array(8 + family * 16, 24, None)
    }

    fn named(&self, family: usize, name: u32) -> Result<(usize, usize)> {
        let p = self.p();
        let rows = self.layers(family)?;
        let found = rows
            .iter()
            .enumerate()
            .filter(|(_, row)| p.u32(**row + 16).ok() == Some(name))
            .map(|(index, row)| (index, *row))
            .collect::<Vec<_>>();
        let [one] = found[..] else {
            anyhow::bail!("pose layer {name:08X} is missing or repeated");
        };
        Ok(one)
    }

    fn dispatch(&self) -> Result<Vec<u16>> {
        let p = self.p();
        p.array(40, 2, Some(DISPATCH))?
            .into_iter()
            .map(|at| p.u16(at))
            .collect()
    }

    fn routes(&self) -> Result<Vec<Route>> {
        let p = self.p();
        p.array(56, 32, Some(ROUTES))?
            .into_iter()
            .map(|row| {
                let edges = |family: usize| -> Result<Vec<(u16, u16)>> {
                    p.array(row + family * 16, 4, None)?
                        .into_iter()
                        .map(|at| Ok((p.u16(at)?, p.u16(at + 2)?)))
                        .collect()
                };
                Ok([edges(0)?, edges(1)?])
            })
            .collect()
    }

    fn write_routes(&mut self, routes: &[Route], dispatch: &[u16]) -> Result<()> {
        let first = append(
            &mut self.data,
            56,
            ROUTES,
            routes.len(),
            &vec![0; routes.len() * 32],
        )?;
        for (index, route) in routes.iter().enumerate() {
            for (family, edges) in route.iter().enumerate() {
                let bytes = edges
                    .iter()
                    .flat_map(|(choice, layer)| {
                        choice.to_le_bytes().into_iter().chain(layer.to_le_bytes())
                    })
                    .collect::<Vec<_>>();
                write_array(
                    &mut self.data,
                    first + index * 32 + family * 16,
                    ROUTE_EDGES,
                    edges.len(),
                    &bytes,
                )?;
            }
        }
        let bytes = dispatch
            .iter()
            .flat_map(|i| i.to_le_bytes())
            .collect::<Vec<_>>();
        write_array(&mut self.data, 40, DISPATCH, dispatch.len(), &bytes)?;
        size_word(&mut self.data)
    }
}

/// Records what the edits changed, for the graph.
#[derive(Default)]
struct Record(serde_json::Map<String, Value>);

/// Ready plays one idle frame instead of the equip clip: the equip descriptors only the ready
/// state plays take the idle clip and last one frame. It follows the pose refresh, which finds
/// each descriptor's source by the clip it plays.
fn ready_idle(bank: &mut Bank, states: &Payload) -> Result<Value> {
    let nodes = state_nodes(states)?;
    let lists = state_lists(states)?;
    let node = |name: u32| {
        nodes
            .get(&name)
            .copied()
            .with_context(|| format!("state {name:08X} is missing"))
    };
    let descriptors_of = |index: usize| -> Result<BTreeSet<u32>> {
        let mut out = BTreeSet::new();
        for &list in &lists[index] {
            out.extend(weighted(states, list)?.into_iter().map(|(d, _)| d));
        }
        Ok(out)
    };
    let ready = node(READY_STATE)?;
    let equip = descriptors_of(ready)?
        .into_iter()
        .filter(|d| bank.clip_of(&bank.descriptors[*d as usize]) == Some(EQUIP_CLIP))
        .collect::<Vec<_>>();
    ensure!(!equip.is_empty(), "the ready state plays no equip clip");
    for index in (0..lists.len()).filter(|index| *index != ready) {
        ensure!(
            descriptors_of(index)?.iter().all(|d| !equip.contains(d)),
            "another state plays the equip clip"
        );
    }
    let idle = descriptors_of(node(IDLE_STATE)?)?
        .into_iter()
        .find(|d| bank.clip_of(&bank.descriptors[*d as usize]) == Some(IDLE_CLIP))
        .context("the idle state does not play the idle clip")?;
    let idle_row = bank.descriptors[idle as usize];
    for &d in &equip {
        let row = &mut bank.descriptors[d as usize];
        ensure!(
            Bank::mode(row) == Bank::mode(&idle_row),
            "ready and idle play differently"
        );
        row[24..26].copy_from_slice(&idle_row[24..26]);
        row[20..24].copy_from_slice(&FRAME.to_le_bytes());
    }
    Ok(json!({"descriptors":equip,"idle_descriptor":idle}))
}

pub(super) fn apply(inputs: &Inputs) -> Result<Value> {
    let graph_path = inputs.graph.join("asset-graph.json");
    let mut graph: Value = serde_json::from_slice(&fs::read(&graph_path)?)?;
    ensure!(
        graph["animation"]["first_person_status"] == "linked",
        "the glaive's first-person animation did not link"
    );
    ensure!(
        graph["animation"]["first_person"]
            .get("glaive_edits")
            .is_none(),
        "the glaive's first-person edits are already made"
    );
    let mut sr = Reader::discovery(inputs.modern, &inputs.work.join("source"), true)?;
    let mut nr = Reader::discovery(inputs.native, &inputs.work.join("native"), false)?;
    let fp = graph["animation"]["first_person"].clone();
    let file = |key: &str| -> Result<std::path::PathBuf> {
        Ok(inputs.graph.join(
            fp["files"][key]
                .as_str()
                .with_context(|| format!("first-person {key} file"))?,
        ))
    };
    let mut record = Record::default();

    // Clips: hip fire, sprint sway and poses, and the swings.
    let with = conversion(&mut sr, &mut nr, &fp, inputs.graph, inputs.calibration)?;
    let fire = convert(
        &mut sr,
        &with,
        &with.library,
        FIRE_CLIP,
        None,
        &FIRE_OMITTED_EVENTS,
    )?;
    let sway = convert(&mut sr, &with, &with.library, SPRINT_SWAY, None, &[])?;
    let mut poses = Vec::new();
    for (_, _, clip) in SPRINT_POSES {
        poses.push(convert(&mut sr, &with, &with.library, clip, None, &[])?);
    }
    // The swings follow the importer's own conversion of the third swing.
    let template_tag = SWINGS[2].0;
    let template = fp["extra_clips"]
        .as_array()
        .context("extra clips")?
        .iter()
        .find(|row| row["source"] == template_tag)
        .cloned()
        .with_context(|| format!("the importer did not convert swing {template_tag:08X}"))?;
    let template_payload =
        Payload(fs::read(inputs.graph.join(
            template["file"].as_str().context("swing template file")?,
        ))?);
    let source_template = sr.tag(template_tag, Some(0x8080_8BE0))?;
    let swing_library =
        clips::EventLibrary::derive([(source_template.as_ref(), &template_payload)]);
    let swing_bones = template_payload.u16(0x13E)?;
    let mut swings = Vec::new();
    for (clip, keep) in SWINGS {
        let converted = convert(
            &mut sr,
            &with,
            &swing_library,
            clip,
            Some((&keep, swing_bones)),
            &[],
        )?;
        let trimmed =
            trim(&converted.payload, SWING_FRAMES).with_context(|| format!("swing {clip:08X}"))?;
        swings.push(Converted {
            payload: trimmed,
            report: converted.report,
        });
    }

    // Bank: the swings and hip fire join it, the ready descriptors play one idle frame.
    let mut bank = Bank::read(fs::read(file("converted_bank")?)?)?;
    let native_count = fp["descriptor_conversion"]["native_descriptors"]
        .as_u64()
        .context("native descriptor count")? as usize;
    let source_bank_tag = {
        let lookup = inputs.source_rig["components"]
            .as_array()
            .context("source components")?
            .iter()
            .find(|row| {
                row["class"] == "808025F8" && row["entity"] != inputs.source_rig["runtime_entity"]
            })
            .context("source lookup owner")?;
        let owner = sr.tag(hex(&lookup["owner"])?, Some(0x8080_9B06))?;
        owner.u32(owner.pointer(24)? + 0xA8)?
    };
    let source_bank = (*sr.tag(source_bank_tag, Some(0x8080_289F))?).clone();
    let mut swing_descriptors = Vec::new();
    for ((clip, _), converted) in SWINGS.iter().zip(&swings) {
        let name = source_descriptor(&mut sr, &source_bank, *clip)?;
        let frames = converted.payload.u16(FRAMES)?;
        let mut playing = bank.playing(*clip);
        if playing.is_empty() {
            playing.push(bank.add(name, *clip, frames)?);
        }
        for &index in &playing {
            ensure!(
                Bank::name(&bank.descriptors[index]) == name,
                "swing {clip:08X} plays outside its source descriptor"
            );
            bank.set_duration(index, duration(frames));
        }
        swing_descriptors.push(playing[0]);
    }
    ensure!(
        bank.playing(FIRE_CLIP).is_empty(),
        "the hip fire clip is already in the bank"
    );
    let fire_name = source_descriptor(&mut sr, &source_bank, FIRE_CLIP)?;
    let fire_descriptor = bank.add(fire_name, FIRE_CLIP, fire.payload.u16(FRAMES)?)?;

    // States: hip fire on every fire choice, the swings on the light attacks.
    let states_path = file("states")?;
    let mut states = fs::read(&states_path)?;
    let state_edits = update_states(&mut states, fire_descriptor, &swing_descriptors)?;
    record.0.insert("fire".into(), state_edits.fire);

    // Holding pose: the one layer no state reaches becomes the source holding layer.
    let mut layers = Layers {
        data: fs::read(
            inputs.graph.join(
                fp["pose_layers"]["file"]
                    .as_str()
                    .context("pose layers file")?,
            ),
        )?,
    };
    record.0.insert(
        "holding".into(),
        holding_routes(
            inputs,
            &mut sr,
            &fp,
            &state_edits.active,
            &mut bank,
            &mut layers,
        )?,
    );

    // Sprint: the sway layer plays the source sway clip for its own length.
    let (_, sprint_row) = layers.named(0, SPRINT_LAYER)?;
    let lp = layers.p();
    let sprint_choice = *lp
        .array(sprint_row, 16, None)?
        .first()
        .context("sprint layer choice")?;
    let sprint_entry = *lp
        .array(sprint_choice, 64, None)?
        .first()
        .context("sprint layer entry")?;
    let [sway_slot] = lp.array(sprint_entry, 8, Some(LAYER_CLIPS))?[..] else {
        anyhow::bail!("the sprint layer plays several clips");
    };
    let sway_carrier = *bank
        .clips
        .get(lp.u32(sway_slot)? as usize)
        .context("sprint layer clip slot")?;
    put(
        &mut layers.data,
        sprint_entry + 52,
        duration(sway.payload.u16(FRAMES)?).to_le_bytes(),
    )?;
    // The sprint loop's arm poses: each carrier clip plays only in its own layer choice.
    let uses = layer_clip_uses(&layers, &bank)?;
    let described = bank
        .descriptors
        .iter()
        .filter_map(|row| bank.clip_of(row))
        .collect::<BTreeSet<_>>();
    let substituted = fp["clips"]
        .as_array()
        .context("converted clips")?
        .iter()
        .filter_map(|row| row["native"].as_u64())
        .collect::<BTreeSet<_>>();
    let mut carriers = Vec::new();
    for (layer, choice, _) in SPRINT_POSES {
        let found = uses
            .iter()
            .filter(|(_, places)| places.as_slice() == [(1, layer, choice)])
            .map(|(clip, _)| *clip)
            .collect::<Vec<_>>();
        let [carrier] = found[..] else {
            anyhow::bail!("sprint pose {layer:08X} choice {choice} has no clip of its own");
        };
        ensure!(
            !described.contains(&carrier),
            "sprint pose clip {carrier:08X} is also a descriptor's"
        );
        carriers.push(carrier);
    }
    for carrier in carriers.iter().chain([&sway_carrier]) {
        ensure!(
            !substituted.contains(&u64::from(*carrier)),
            "native clip {carrier:08X} already plays a source clip"
        );
    }

    // Commit the bank, states and layers, then let the importer route the new descriptors.
    let total = bank.descriptors.len();
    let clip_count = bank.clips.len();
    fs::write(file("converted_bank")?, bank.write()?)?;
    fs::write(&states_path, &states)?;
    fs::write(
        inputs.graph.join(
            fp["pose_layers"]["file"]
                .as_str()
                .context("pose layers file")?,
        ),
        &layers.data,
    )?;
    let fp = graph["animation"]["first_person"]
        .as_object_mut()
        .context("first-person section")?;
    let mut clips_written = Vec::new();
    for ((clip, _), converted) in SWINGS
        .iter()
        .zip(&swings)
        .chain([(&(FIRE_CLIP, [0; 3]), &fire)])
    {
        let name = format!("animation/source-clip-{clip:08X}.bin");
        fs::write(inputs.graph.join(&name), &converted.payload.0)?;
        let extra = fp
            .get_mut("extra_clips")
            .and_then(Value::as_array_mut)
            .context("extra clips")?;
        let events = converted.report["events"].clone();
        if let Some(existing) = extra.iter_mut().find(|row| row["source"] == *clip) {
            existing["events"] = events;
        } else {
            let mut row = template.clone();
            row["source"] = json!(clip);
            row["name"] = json!(source_name(&mut sr, *clip)?);
            row["file"] = json!(name);
            row["events"] = events;
            row["mode_evidence"] = json!("glaive kit");
            extra.push(row);
        }
        fp["files"][format!("extra_clip_{clip:08X}")] = json!(name);
        clips_written.push(*clip);
    }
    if let Some(rows) = fp
        .get_mut("extra_unconverted")
        .and_then(Value::as_array_mut)
    {
        rows.retain(|row| {
            !row["source"]
                .as_u64()
                .is_some_and(|s| clips_written.contains(&(s as u32)))
        });
    }
    let substitutions = carriers
        .iter()
        .zip(SPRINT_POSES.iter().zip(&poses))
        .map(|(carrier, ((layer, choice, clip), converted))| {
            (
                *carrier,
                *clip,
                converted,
                format!("sprint pose {layer:08X}#{choice}"),
            )
        })
        .chain([(sway_carrier, SPRINT_SWAY, &sway, "sprint sway".to_owned())])
        .collect::<Vec<_>>();
    for (carrier, clip, converted, layer) in substitutions {
        let name = format!("animation/clip-{carrier:08X}.bin");
        fs::write(inputs.graph.join(&name), &converted.payload.0)?;
        fp.get_mut("clips")
            .and_then(Value::as_array_mut)
            .context("converted clips")?
            .push(json!({"events":null,"file":name,"name":source_name(&mut sr, clip)?,"native":carrier,"source":clip,"motion_layer":layer}));
    }
    fp["descriptor_conversion"]["descriptors"] = json!(total);
    fp["descriptor_conversion"]["clips"] = json!(clip_count);
    fp["descriptor_conversion"]["added_descriptors"] = json!(total - native_count);
    ensure!(
        dispatch::refresh(
            &mut sr,
            &mut nr,
            inputs.source_rig,
            inputs.native_rig,
            inputs.graph,
            &mut graph
        )?,
        "the glaive's pose dispatch did not refresh"
    );

    // Sprint takes the native sprint descriptor's route.
    let fp = &graph["animation"]["first_person"];
    let mut layers = Layers {
        data: fs::read(
            inputs.graph.join(
                fp["pose_layers"]["file"]
                    .as_str()
                    .context("pose layers file")?,
            ),
        )?,
    };
    let mut bank = Bank::read(fs::read(file("converted_bank")?)?)?;
    let mut dispatch_indices = layers.dispatch()?;
    // The native sprint loop's route plays the sway layer and both sprint poses.
    let sway_layer = u16::try_from(layers.named(0, SPRINT_LAYER)?.0)?;
    let mut pose_edges = Vec::new();
    for (layer, choice, _) in SPRINT_POSES {
        pose_edges.push((
            u16::try_from(choice)?,
            u16::try_from(layers.named(1, layer)?.0)?,
        ));
    }
    let loops = layers
        .routes()?
        .iter()
        .enumerate()
        .filter(|(_, route)| {
            route[0].iter().any(|(_, layer)| *layer == sway_layer)
                && pose_edges.iter().all(|edge| route[1].contains(edge))
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let [sprint_route] = loops[..] else {
        anyhow::bail!("the base has {} sprint loop routes, not one", loops.len());
    };
    let sprint_route = u16::try_from(sprint_route)?;
    let mut sprints = Vec::new();
    for (i, dispatch) in dispatch_indices
        .iter_mut()
        .enumerate()
        .take(bank.descriptors.len())
        .skip(native_count)
    {
        if Bank::name(&bank.descriptors[i]) == SPRINT_DESCRIPTOR && *dispatch == u16::MAX {
            *dispatch = sprint_route;
            sprints.push(i);
        }
    }
    ensure!(
        !sprints.is_empty(),
        "the converted sprint descriptor already has a route"
    );
    let routes = layers.routes()?;
    layers.write_routes(&routes, &dispatch_indices)?;
    fs::write(
        inputs.graph.join(
            fp["pose_layers"]["file"]
                .as_str()
                .context("pose layers file")?,
        ),
        &layers.data,
    )?;
    record.0.insert(
        "sprint".into(),
        json!({"descriptors":sprints,"route":sprint_route,"layer_clip":format!("{sway_carrier:08X}"),
            "pose_clips":carriers.iter().map(|c| format!("{c:08X}")).collect::<Vec<_>>()}),
    );
    let ready = ready_idle(&mut bank, &Payload(fs::read(&states_path)?))?;
    fs::write(file("converted_bank")?, bank.write()?)?;
    record.0.insert("ready_idle".into(), ready);
    record.0.insert(
        "swings".into(),
        json!({"descriptors":swing_descriptors,"frames":SWING_FRAMES,"states":SWING_STATES.iter().map(|s| format!("{s:08X}")).collect::<Vec<_>>()}),
    );
    record.0.insert("gameplay_verified".into(), json!(false));
    let edits = Value::Object(record.0);
    graph["animation"]["first_person"]["glaive_edits"] = edits.clone();
    write_json(&graph_path, &graph)?;
    sr.finish()?;
    nr.finish()?;
    Ok(edits)
}

fn holding_routes(
    inputs: &Inputs,
    sr: &mut Reader,
    fp: &Value,
    active: &BTreeSet<u32>,
    bank: &mut Bank,
    layers: &mut Layers,
) -> Result<Value> {
    let profile = u32::try_from(
        fp["attachment_profile"]["patch"]["after"]
            .as_u64()
            .context("attachment profile")?,
    )?;
    let (_, _, source_layers) = poses::controller(sr, inputs.source_rig, true)?;
    let mut dispatch_indices = layers.dispatch()?;
    let mut routes = layers.routes()?;
    let used = active
        .iter()
        .filter_map(|d| dispatch_indices.get(*d as usize).copied())
        .filter(|route| *route != u16::MAX)
        .filter_map(|route| routes.get(usize::from(route)))
        .flat_map(|route| route[0].iter().map(|(_, layer)| usize::from(*layer)))
        .collect::<BTreeSet<_>>();
    let source_rows = source_layers.array(8, 32, None)?;
    let source_names = source_rows
        .iter()
        .map(|row| source_layers.u32(row + 16))
        .collect::<Result<BTreeSet<_>>>()?;
    let lp = layers.p();
    let family0 = layers.layers(0)?;
    let free = family0
        .iter()
        .enumerate()
        .filter(|(index, row)| {
            !used.contains(index)
                && lp.u64(**row).ok() == Some(1)
                && lp
                    .u32(**row + 16)
                    .is_ok_and(|name| !source_names.contains(&name))
        })
        .map(|(index, row)| (index, *row))
        .collect::<Vec<_>>();
    let [(target, target_row)] = free[..] else {
        anyhow::bail!(
            "the base has {} unreachable pose layers, not one",
            free.len()
        );
    };
    let old_name = lp.u32(target_row + 16)?;
    let (generic, generic_row) = layers.named(0, GENERIC_LAYER)?;
    let generic_choice = *lp
        .array(generic_row, 16, None)?
        .first()
        .context("generic layer has no choice")?;
    let [generic_entry] = lp.array(generic_choice, 64, None)?[..] else {
        anyhow::bail!("the generic layer's first choice is not one entry");
    };
    ensure!(
        lp.array(generic_entry + 16, 8, None)?.is_empty(),
        "the generic layer's entry has conditions"
    );
    let generic_clips = lp.array(generic_entry, 8, Some(LAYER_CLIPS))?;
    ensure!(
        generic_clips.len() == 1,
        "the generic layer's entry plays several clips"
    );
    let mut fields = [0; 16];
    fields[0..4].copy_from_slice(&1u32.to_le_bytes());
    fields[4..8].copy_from_slice(&1.0f32.to_le_bytes());
    fields[8..12].copy_from_slice(&0xFFFFu32.to_le_bytes());
    ensure!(
        lp.bytes::<16>(generic_entry + 32)? == fields,
        "the generic layer's entry fields differ"
    );
    let mut entry = vec![0; 32];
    entry.extend_from_slice(&lp.0[generic_entry + 32..generic_entry + 64]);
    let clip_rows = lp.0[generic_clips[0]..generic_clips[0] + 8].to_vec();
    let choice = append(&mut layers.data, target_row, LAYER_CHOICES, 1, &[0; 16])?;
    let entry_row = append(&mut layers.data, choice, LAYER_ENTRIES, 1, &entry)?;
    append(&mut layers.data, entry_row, LAYER_CLIPS, 1, &clip_rows)?;
    put(&mut layers.data, target_row + 16, profile.to_le_bytes())?;
    let ordinal = usize::from(layers.p().u16(target_row + 20)?);
    let bank_names = Payload(bank.data.clone()).array(0x58, 4, None)?;
    let name_at = *bank_names
        .get(ordinal)
        .context("pose layer ordinal outside the bank's names")?;
    ensure!(
        Payload(bank.data.clone()).u32(name_at)? == old_name,
        "the bank names the unreachable layer differently"
    );
    put(&mut bank.data, name_at, profile.to_le_bytes())?;
    let target16 = u16::try_from(target)?;
    let generic16 = u16::try_from(generic)?;
    for route in &mut routes {
        route[0].retain(|(_, layer)| *layer != target16);
    }
    // Source routes reach the holding and generic layers through these aliases.
    let named_source = |name: u32| -> Result<u16> {
        let index = source_rows
            .iter()
            .position(|row| source_layers.u32(row + 16).ok() == Some(name))
            .with_context(|| format!("source pose layer {name:08X} is missing"))?;
        Ok(u16::try_from(index)?)
    };
    let (sg, sp) = (named_source(GENERIC_LAYER)?, named_source(profile)?);
    let aliases = BTreeMap::from([
        ((0, sg), (0, generic16)),
        ((1, sg), (1, generic16)),
        ((0, sp), (0, target16)),
    ]);
    let source_dispatch = source_layers
        .array(40, 2, None)?
        .into_iter()
        .map(|at| source_layers.u16(at))
        .collect::<Result<Vec<_>>>()?;
    let source_routes = source_layers.array(56, 32, None)?;
    let mapping = dispatch_mapping(fp)?;
    let mut holding = Vec::new();
    for (from, to) in mapping {
        let Some(&route) = source_dispatch
            .get(from as usize)
            .filter(|r| **r != u16::MAX)
        else {
            continue;
        };
        let row = *source_routes
            .get(usize::from(route))
            .context("source route")?;
        let edges = source_layers
            .array(row, 4, None)?
            .into_iter()
            .map(|at| Ok((source_layers.u16(at)?, source_layers.u16(at + 2)?)))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .filter_map(|edge| aliases.get(&edge).copied())
            .collect::<Vec<_>>();
        if edges.is_empty() {
            continue;
        }
        let route: Route = [edges, Vec::new()];
        let index = match routes.iter().position(|r| *r == route) {
            Some(index) => index,
            None => {
                routes.push(route);
                routes.len() - 1
            }
        };
        *dispatch_indices
            .get_mut(to as usize)
            .context("dispatch does not cover the bank")? = u16::try_from(index)?;
        holding.push(json!({"source":from,"native":to,"route":index}));
    }
    layers.write_routes(&routes, &dispatch_indices)?;
    Ok(
        json!({"replaced_unreachable_layer":format!("{old_name:08X}"),"source_layer":format!("{profile:08X}"),
            "native_layer":target,"generic_layer":generic,"routes":holding}),
    )
}

fn dispatch_mapping(fp: &Value) -> Result<Vec<(u32, u32)>> {
    let mut mapping = Vec::<(u32, u32)>::new();
    for label in [
        "source_absent",
        "translated",
        "unsupported_supplemental_routes",
    ] {
        for row in fp["pose_layers"]["dispatch"][label]
            .as_array()
            .into_iter()
            .flatten()
        {
            let from = u32::try_from(row["source"].as_u64().context("dispatch source")?)?;
            let to = u32::try_from(row["native"].as_u64().context("dispatch native")?)?;
            match mapping.iter_mut().find(|(source, _)| *source == from) {
                Some(existing) => existing.1 = to,
                None => mapping.push((from, to)),
            }
        }
    }
    Ok(mapping)
}

struct StateEdits {
    active: BTreeSet<u32>,
    fire: Value,
}

fn update_states(
    states: &mut Vec<u8>,
    fire_descriptor: usize,
    swing_descriptors: &[usize],
) -> Result<StateEdits> {
    let nodes = state_nodes(&Payload(states.clone()))?;
    let lists = state_lists(&Payload(states.clone()))?;
    let node = |name: u32| {
        nodes
            .get(&name)
            .copied()
            .with_context(|| format!("state {name:08X} is missing"))
    };
    // Every descriptor a state plays before these edits, which decides the unreachable layer.
    let original = Payload(states.clone());
    let mut active = BTreeSet::new();
    for node_lists in &lists {
        for &list in node_lists {
            active.extend(weighted(&original, list)?.into_iter().map(|(d, _)| d));
        }
    }
    let fire_node = node(FIRE_STATE)?;
    let mut replaced = Vec::new();
    for &list in &lists[fire_node] {
        for at in Payload(states.clone()).array(list, 8, Some(WEIGHTED_DESCRIPTORS))? {
            replaced.push(Payload(states.clone()).u32(at)?);
            put(states, at, u32::try_from(fire_descriptor)?.to_le_bytes())?;
        }
    }
    ensure!(!replaced.is_empty(), "the fire state plays nothing");
    let fire = json!({"descriptor":fire_descriptor,"replaced":replaced});
    let cutoffs = swing_descriptors
        .iter()
        .enumerate()
        .flat_map(|(i, d)| {
            (*d as u32)
                .to_le_bytes()
                .into_iter()
                .chain(((i + 1) as f32 / swing_descriptors.len() as f32).to_le_bytes())
        })
        .collect::<Vec<_>>();
    let mut done = BTreeSet::new();
    for state in SWING_STATES {
        let index = node(state)?;
        let p = Payload(states.clone());
        let node_row = p.array(24, 16, Some(STATE_NODES))?[index];
        ensure!(
            p.u64(node_row)? == 1,
            "swing state {state:08X} is not a single choice"
        );
        if done.insert(index) {
            let list = p.pointer(node_row + 8)?;
            ensure!(
                weighted(&p, list)?.len() == 1,
                "swing state {state:08X} already varies"
            );
            append(
                states,
                list,
                WEIGHTED_DESCRIPTORS,
                swing_descriptors.len(),
                &cutoffs,
            )?;
        }
    }
    size_word(states)?;

    Ok(StateEdits { active, fire })
}

type LayerClipUses = BTreeMap<u32, Vec<(usize, u32, usize)>>;

fn layer_clip_uses(layers: &Layers, bank: &Bank) -> Result<LayerClipUses> {
    let lp = layers.p();
    let mut uses: LayerClipUses = BTreeMap::new();
    for family in 0..2 {
        for row in layers.layers(family)? {
            for (index, choice) in lp.array(row, 16, None)?.into_iter().enumerate() {
                for entry in lp.array(choice, 64, None)? {
                    for at in lp.array(entry, 8, Some(LAYER_CLIPS))? {
                        let clip = *bank
                            .clips
                            .get(lp.u32(at)? as usize)
                            .context("layer clip slot")?;
                        uses.entry(clip)
                            .or_default()
                            .push((family, lp.u32(row + 16)?, index));
                    }
                }
            }
        }
    }
    Ok(uses)
}

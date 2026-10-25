//! Rise of Iron clips on the native clip layout, and a weapon runtime's own clips linked by name.
//!
//! A Rise of Iron clip `808005A1` holds the native clip `80808F49` fields behind a header 0x20
//! bytes shorter, with byte-wide track maps. Its codec streams keep their layout and element
//! classes, so the data after the header moves as one block and only the header, the track maps,
//! the codec classes and the array allocation markers change. Supported scalar and four-lane
//! channels retain their sample data. Direct cue events receive private graph references.
use super::*;
use crate::tiger::{animation::runtime, payload::write_array};
pub mod ability;
mod channel;
pub(super) mod character;
mod constraint;
mod controls;
mod event;
mod first_person;

const SOURCE_CLIP: u32 = 0x8080_05A1;
const SOURCE_BANK: u32 = 0x8080_2C0E;
const SOURCE_BANK_ROW: u32 = 0x8080_408D;
const SOURCE_LOOKUP: u32 = 0x8080_20BB;
const SOURCE_OWNER: u32 = 0x8080_0861;
const SOURCE_ARRAY_MARKER: u32 = 0x8080_0184;
const NATIVE_ARRAY_MARKER: u32 = 0x8080_9FBD;
const SOURCE_HEADER: usize = 0x170;
const NATIVE_HEADER: usize = 0x190;
const SHIFT: usize = NATIVE_HEADER - SOURCE_HEADER;
const TRACK_MAPS: usize = 7;
/// Element classes the codec arrays share in both formats: u16, f32, i16 and u8 rows.
const ELEMENTS: [u32; 4] = [0x8080_000A, 0x8080_000F, 0x8080_0006, 0x8080_0009];

/// Transform codecs: source class, native class, encoding discriminator and array fields.
const CODECS: [(u32, u32, u16, &[usize]); 3] = [
    (0x8080_04A9, 0x8080_8F6F, 3, &[0x38]),
    (0x8080_05E6, 0x8080_8F71, 2, &[0x18, 0x28, 0x38]),
    (
        0x8080_0432,
        0x8080_8F72,
        1,
        &[0x10, 0x20, 0x30, 0x40, 0x50, 0x60, 0x70],
    ),
];

fn put(target: &mut [u8], at: usize, bytes: &[u8]) {
    target[at..at + bytes.len()].copy_from_slice(bytes);
}

/// Point the native field `to` at the moved target of the source field `from`. A target inside
/// the source header has no place in the moved block.
fn relocate(source: &Payload, native: &mut [u8], from: usize, to: usize) -> Result<usize> {
    let target = source.pointer(from)?;
    ensure!(
        (SOURCE_HEADER..source.0.len()).contains(&target),
        "Source clip field {from:X} points into its header"
    );
    let moved = target + SHIFT;
    put(native, to, &(moved as i64 - to as i64).to_le_bytes());
    Ok(target)
}

/// Move an array field, relabeling its allocation marker and optionally its row class.
fn move_array(
    source: &Payload,
    native: &mut [u8],
    from: usize,
    to: usize,
    classes: (u32, u32),
) -> Result<()> {
    let count = source.u64(from)?;
    if count == 0 {
        ensure!(
            source.u64(from + 8)? == 0,
            "Empty source clip array has a pointer"
        );
        return Ok(());
    }
    source.array_range(from, 1, Some(classes.0))?;
    put(native, to, &count.to_le_bytes());
    let header = relocate(source, native, from + 8, to + 8)?;
    ensure!(
        header >= 4 && source.u32(header - 4)? == SOURCE_ARRAY_MARKER,
        "Source clip array at {from:X} has another allocation marker"
    );
    put(
        native,
        header + SHIFT - 4,
        &NATIVE_ARRAY_MARKER.to_le_bytes(),
    );
    put(native, header + SHIFT + 8, &classes.1.to_le_bytes());
    Ok(())
}

/// Convert one transform codec stream in place and return its native class.
fn codec(
    source: &Payload,
    native: &mut [u8],
    at: usize,
    to: usize,
    counts: [usize; 3],
) -> Result<Value> {
    let start = relocate(source, native, at, to)?;
    ensure!(start >= 4, "Source clip codec has no class word");
    let class = source.u32(start - 4)?;
    let &(_, target, encoding, fields) = CODECS
        .iter()
        .find(|codec| codec.0 == class)
        .with_context(|| format!("Source clip codec {class:08X} at {at:X} has no converter"))?;
    ensure!(
        source.u16(start)? == encoding,
        "Source clip codec {class:08X} encoding differs"
    );
    for (group, &count) in counts.iter().enumerate() {
        ensure!(
            usize::from(source.u16(start + 2 + group * 2)?) == count,
            "Source clip codec track counts differ from its track maps"
        );
    }
    put(native, start + SHIFT - 4, &target.to_le_bytes());
    for &field in fields {
        let field = start + field;
        if source.u64(field)? == 0 {
            continue;
        }
        let header = source.pointer(field + 8)?;
        ensure!(
            header >= 4 && source.u32(header - 4)? == SOURCE_ARRAY_MARKER,
            "Source clip codec array has another allocation marker"
        );
        let element = source.u32(header + 8)?;
        ensure!(
            ELEMENTS.contains(&element),
            "Source clip codec array class {element:08X} differs"
        );
        source.array_range(field, 1, Some(element))?;
        put(
            native,
            header + SHIFT - 4,
            &NATIVE_ARRAY_MARKER.to_le_bytes(),
        );
    }
    Ok(
        json!({"field":at,"source_class":format!("{class:08X}"),"native_class":format!("{target:08X}"),
        "encoded_tracks_preserved":true}),
    )
}

/// Convert one Rise of Iron clip to the native clip layout.
fn convert_bound(
    source: &Payload,
    binding: Option<(&Payload, &[u16])>,
) -> Result<(Payload, Value)> {
    convert_events(source, binding, None)
}

fn convert_events(
    source: &Payload,
    binding: Option<(&Payload, &[u16])>,
    events: Option<&event::Library>,
) -> Result<(Payload, Value)> {
    let length = source.0.len();
    ensure!(
        length > SOURCE_HEADER && source.u64(0)? == length as u64,
        "Source clip size differs"
    );
    ensure!(source.u64(8)? == 0, "Source clip has a stream extension");
    for at in (0x30..0x50).step_by(8) {
        ensure!(
            source.u64(at)? == 0 || (binding.is_some() && at != 0x48),
            "Source clip constraint or blend binding at {at:X} needs translation"
        );
    }
    ensure!(
        (source.u32(0x120)? == 0 && source.u32(0x124)? == 0) || binding.is_some(),
        "Source clip constraint codec sizes need translation"
    );
    ensure!(
        source.u16(0x12E)? == 0 && source.u32(0x168)? == 0 && source.u32(0x16C)? == 0,
        "Source clip header words differ"
    );
    let frames = source.u16(0x128)?;
    let bones = source.u8(0x12A)?;
    let slots = source.u8(0x12B)?;
    ensure!(
        frames > 0 && slots > 0 && slots <= bones,
        "Source clip counts differ"
    );
    let mut native = vec![0; NATIVE_HEADER];
    native.extend_from_slice(&source.0[SOURCE_HEADER..]);
    let events = match events {
        Some(library) => library.convert(source, &mut native, frames)?,
        None => event::convert(source, &mut native, frames)?,
    };
    // Fields from 0x60 up to the track maps, and from the last map to the counts, move by 0x10.
    native[0x70..0xA8].copy_from_slice(&source.0[0x60..0x98]);
    native[0x118..0x130].copy_from_slice(&source.0[0x108..0x120]);
    move_array(source, &mut native, 0x80, 0x90, (0x8080_0070, 0x8080_0070))?;
    move_array(
        source,
        &mut native,
        0x130,
        0x150,
        (0x8080_0070, 0x8080_0070),
    )?;
    move_array(
        source,
        &mut native,
        0x150,
        0x170,
        (0x8080_0380, 0x8080_907F),
    )?;
    put(&mut native, 0x13C, &frames.to_le_bytes());
    put(&mut native, 0x13E, &u16::from(bones).to_le_bytes());
    put(&mut native, 0x140, &u16::from(slots).to_le_bytes());
    put(&mut native, 0x142, &source.0[0x12C..0x12E]);
    put(&mut native, 0x180, &source.u32(0x164)?.to_le_bytes());
    let mut maps = Vec::with_capacity(TRACK_MAPS);
    for index in 0..TRACK_MAPS {
        let at = 0x98 + index * 0x10;
        let rows = source.array_range(at, 1, Some(0x8080_0009))?;
        let tracks = source.0[rows]
            .iter()
            .map(|&slot| u16::from(slot))
            .collect::<Vec<_>>();
        ensure!(
            tracks.iter().all(|&slot| slot < u16::from(slots))
                && tracks.iter().collect::<BTreeSet<_>>().len() == tracks.len(),
            "Source clip track map {index} addresses an invalid slot"
        );
        maps.push(tracks);
    }
    ensure!(maps[6].is_empty(), "Source clip has an unknown track group");
    // The byte-wide maps stay in the moved block unreferenced. Their markers follow the format.
    for index in 0..TRACK_MAPS {
        let at = 0x98 + index * 0x10;
        if source.u64(at)? != 0 {
            let header = source.pointer(at + 8)?;
            ensure!(
                header >= SOURCE_HEADER + 4 && source.u32(header - 4)? == SOURCE_ARRAY_MARKER,
                "Source clip track map {index} has another allocation marker"
            );
            put(
                &mut native,
                header + SHIFT - 4,
                &NATIVE_ARRAY_MARKER.to_le_bytes(),
            );
        }
    }
    let mut streams = Vec::new();
    for (at, groups) in [(0x10, 0..3), (0x18, 3..6)] {
        let counts = [
            maps[groups.start].len(),
            maps[groups.start + 1].len(),
            maps[groups.start + 2].len(),
        ];
        if source.u64(at)? == 0 {
            ensure!(counts == [0; 3], "Source clip tracks have no codec");
            continue;
        }
        streams.push(codec(source, &mut native, at, at, counts)?);
    }
    ensure!(!streams.is_empty(), "Source clip contains no tracks");
    streams.extend(channel::convert(source, &mut native, usize::from(frames))?);
    if source.u64(0x30)? != 0 {
        let (template, bones) =
            binding.context("Source constraints need a native binding witness")?;
        streams.extend(constraint::convert(source, &mut native, template, bones)?);
    } else {
        ensure!(
            source.u64(0x38)? == 0
                && source.u64(0x40)? == 0
                && source.u32(0x120)? == 0
                && source.u32(0x124)? == 0,
            "Source constraint streams have no bindings"
        );
    }
    if source.u64(0x58)? != 0 {
        let at = source.pointer(0x58)?;
        let counts = [
            source.u16(at + 2)?,
            source.u16(at + 4)?,
            source.u16(at + 6)?,
        ];
        ensure!(
            counts.iter().all(|v| *v <= u16::from(bones)),
            "Source auxiliary transforms exceed the palette"
        );
        if matches!(source.u16(at)?, 2 | 3) {
            ensure!(
                source.u16(at + 0x10)? == frames,
                "Source auxiliary transforms have another frame count"
            );
        }
        streams.push(codec(
            source,
            &mut native,
            0x58,
            0x60,
            counts.map(usize::from),
        )?);
    }
    for (index, tracks) in maps.iter().enumerate() {
        if tracks.is_empty() {
            continue;
        }
        let bytes = tracks
            .iter()
            .flat_map(|slot| slot.to_le_bytes())
            .collect::<Vec<_>>();
        write_array(
            &mut native,
            0xA8 + index * 0x10,
            0x8080_000A,
            tracks.len(),
            &bytes,
        )?;
    }
    let size = native.len() as u64;
    put(&mut native, 0, &size.to_le_bytes());
    let native = Payload(native);
    for index in 0..TRACK_MAPS {
        native.array_range(0xA8 + index * 0x10, 2, Some(0x8080_000A))?;
    }
    let report = json!({"source_class":format!("{SOURCE_CLIP:08X}"),"native_class":"80808F49",
        "name_hash":format!("{:08X}", source.u32(0x110)?),"frames":frames,"bones":bones,"slots":slots,
        "streams":streams,"dropped_source_word":format!("{:08X}", source.u32(0x160)?),
        "event_count":events["events"].as_array().map_or(0, Vec::len),
        "audio":events,"runtime_ready":false});
    Ok((native, report))
}

/// The clips of the source runtime entity's own clip bank, with the names they carry.
fn runtime_bank(source: &Source, runtime: &Value) -> Result<Vec<(u32, u32)>> {
    let entity = u32::from_str_radix(
        runtime["runtime_entity"]
            .as_str()
            .context("Source runtime entity")?,
        16,
    )?;
    let lookups = runtime["components"]
        .as_array()
        .context("Source runtime components")?
        .iter()
        .filter(|c| {
            c["entity"].as_u64() == Some(u64::from(entity))
                && c["class"].as_u64() == Some(u64::from(SOURCE_LOOKUP))
        })
        .map(|c| c["owner"].as_u64().context("Source animation lookup"))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        lookups.len() == 1,
        "Source runtime has no unique animation lookup"
    );
    let owner = u32::try_from(lookups[0])?;
    ensure!(
        source.reference(owner)? == SOURCE_OWNER,
        "Source animation lookup is not a component owner"
    );
    let lookup = source.read(owner)?;
    let resource = lookup.pointer(24)?;
    ensure!(
        resource >= 4 && lookup.u32(resource - 4)? == SOURCE_LOOKUP,
        "Source animation lookup layout differs"
    );
    let bank_tag = lookup.u32(resource + 0xA8)?;
    ensure!(
        source.reference(bank_tag)? == SOURCE_BANK,
        "Source animation lookup names no clip bank"
    );
    let bank = source.read(bank_tag)?;
    let mut clips = Vec::new();
    for row in bank.array(8, 4, Some(SOURCE_BANK_ROW))? {
        let tag = bank.u32(row)?;
        ensure!(
            source.reference(tag)? == SOURCE_CLIP,
            "Source clip bank row is not a clip"
        );
        clips.push((tag, source.read(tag)?.u32(0x110)?));
    }
    Ok(clips)
}

/// Pair clips by a name unique in both banks. Rows may repeat one clip.
fn pairs(native: &[(u32, u32)], source: &[(u32, u32)]) -> Vec<(u32, u32, u32)> {
    let unique = |clips: &[(u32, u32)], name: u32| {
        let tags = clips
            .iter()
            .filter(|(_, n)| *n == name)
            .map(|(tag, _)| *tag)
            .collect::<BTreeSet<_>>();
        (tags.len() == 1).then(|| *tags.first().unwrap())
    };
    let mut seen = BTreeSet::new();
    native
        .iter()
        .filter(|(tag, _)| seen.insert(*tag))
        .filter_map(|&(tag, name)| {
            Some((unique(native, name)?, unique(source, name)?, name)).filter(|p| p.0 == tag)
        })
        .collect()
}

/// Convert the source runtime's own clips that share a name with a native runtime clip, and link
/// them onto a private copy of the native runtime's clip bank. The result is the graph's
/// `equipment_animation` section.
pub(super) fn prepare(
    source: &Source,
    runtime: &Value,
    native: &mut Reader,
    native_rig: &Value,
    graph: &Path,
) -> Result<Value> {
    let mapping = controls::Mapping::read(source, runtime, native, native_rig, false)?;
    let source_clips = runtime_bank(source, runtime)?;
    let native_clips = runtime::clips(native, native_rig)?;
    let pairs = pairs(&native_clips, &source_clips);
    fs::create_dir_all(graph.join("animation"))?;
    let mut converted = Vec::new();
    let mut rejected = Vec::new();
    for (native_tag, source_tag, name) in pairs {
        let result = (|| {
            let original = source.read(source_tag)?;
            let template = native.tag(native_tag, Some(0x80808F49))?;
            let (payload, mut report) =
                convert_bound(&original, Some((&template, &mapping.bone_map)))?;
            let (payload, slots) = mapping.apply(&original, &payload)?;
            report["control_mapping"] = slots;
            for cue in report["audio"]["cues"]
                .as_array()
                .context("Clip audio cues")?
            {
                let cue = u32::try_from(cue.as_u64().context("Clip audio cue")?)?;
                ensure!(
                    source.reference(cue)? == 0x8080080A,
                    "Source event resource is not an audio cue"
                );
            }
            Ok::<_, anyhow::Error>((payload, report))
        })();
        match result {
            Ok((payload, report)) => {
                let file = format!("animation/runtime-clip-{native_tag:08X}.bin");
                fs::write(graph.join(&file), &payload.0)?;
                converted.push(json!({"native":native_tag,"source":source_tag,"name":name,
                    "file":file,"streams":report["streams"],"events":report["audio"]["events"],
                    "patches":report["audio"]["patches"],"report":report}));
            }
            Err(error) => rejected.push(
                json!({"native":native_tag,"source":source_tag,"name":name,"reason":format!("{error:#}")}),
            ),
        }
    }
    let cues = converted
        .iter()
        .flat_map(|clip| {
            clip["report"]["audio"]["cues"]
                .as_array()
                .into_iter()
                .flatten()
        })
        .map(|cue| Ok(u32::try_from(cue.as_u64().context("Clip audio cue")?)?))
        .collect::<Result<BTreeSet<_>>>()?;
    let mut audio = None;
    if !cues.is_empty() {
        match crate::d1_roi::audio::prepare(source, native, &graph.join("audio-assets"), &cues) {
            Ok(mut assets) => {
                for node in assets["nodes"].as_array_mut().context("Clip audio nodes")? {
                    node["file"] = json!(format!(
                        "audio-assets/{}",
                        node["file"].as_str().context("Clip audio file")?
                    ));
                }
                assets["installable"] = json!(true);
                audio = Some(assets);
            }
            Err(error) => {
                converted.retain(|clip| {
                    if clip["patches"].as_array().is_some_and(|v| !v.is_empty()) {
                        rejected.push(json!({"native":clip["native"],"source":clip["source"],"name":clip["name"],
                            "reason":format!("Source clip audio closure: {error:#}")}));
                        false
                    } else { true }
                });
            }
        }
    }
    if converted.is_empty() {
        return Ok(json!({"status":"native","reason":if rejected.is_empty() {
            "No source runtime clip shares a name with a native runtime clip".to_owned()
        } else {
            format!("No named source runtime clip converted: {rejected:?}")
        },"gameplay_verified":false}));
    }
    let with_events = converted
        .iter()
        .filter(|clip| clip["events"].as_array().is_some_and(|v| !v.is_empty()))
        .count();
    let mut result = runtime::link(
        native,
        native_rig,
        &source_clips,
        (converted, rejected, with_events),
        graph,
    )?;
    if let Some(audio) = audio {
        result["audio_assets"] = audio;
    }
    Ok(result)
}

pub(super) fn prepare_first_person(
    source: &Source,
    runtime: &Value,
    native: &mut Reader,
    native_rig: &Value,
    graph: &Path,
) -> Result<Value> {
    first_person::prepare(source, runtime, native, native_rig, graph)
}

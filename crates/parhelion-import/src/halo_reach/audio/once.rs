//! Source one-shot PCM on a private cue with a shared transient-object voice limit.
use crate::{
    presentation::{Graph, put},
    tiger::{
        audio::bank::{Attenuation, Bank},
        reader::Reader,
    },
};
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::path::Path;

/// A source preemption fade has no one-shot counterpart. A caller either refuses such a sound
/// or states that the fade is omitted, as swing recordings do.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::halo_reach) enum Preemption {
    Refuse,
    Omit,
}

/// The empty FNV-1 name. As a cue's second event at `+0C` it selects the one-shot submission,
/// which keeps no playback handle, so the event's cleanup has nothing to stop.
const NO_SECOND_EVENT: u32 = 0x811c_9dc5;

pub(in crate::halo_reach) fn cue(
    native: &mut Reader,
    graph: &mut Graph,
    sound: &Value,
    template: u32,
    bus: Option<u32>,
    prefix: &str,
) -> Result<Value> {
    cue_with(
        native,
        graph,
        sound,
        template,
        bus,
        prefix,
        Preemption::Refuse,
        false,
    )
}

#[allow(clippy::too_many_arguments)]
pub(in crate::halo_reach) fn cue_with(
    native: &mut Reader,
    graph: &mut Graph,
    sound: &Value,
    template: u32,
    bus: Option<u32>,
    prefix: &str,
    preemption: Preemption,
    shared: bool,
) -> Result<Value> {
    let recordings = graph
        .root
        .parent()
        .context("One-shot source directory")?
        .join("source/audio");
    write(
        native,
        graph,
        sound,
        (template, bus),
        prefix,
        preemption,
        shared.then_some(prefix),
        u32::MAX,
        (&recordings, false),
    )
}

/// A one-shot cue as [`cue`] makes, for a graph whose source recordings are in `recordings`. Its
/// bank takes the linked namespace range `range` names, so cues of several graph sections can
/// share an item's namespace. The cue's second event is empty, as in stock one-shot cues, so a
/// sound event that retires as soon as it plays, such as one on a contact host, does not stop
/// the sound. The cue is positioned where it plays and falls off with distance through a private
/// copy of the attenuation the template cue's hierarchy gives its output bus, as [`attenuation`]
/// finds it. A template without one leaves the cue nonspatial and says so.
#[allow(clippy::too_many_arguments)]
pub(in crate::halo_reach) fn detached(
    native: &mut Reader,
    graph: &mut Graph,
    sound: &Value,
    template: u32,
    bus: Option<u32>,
    prefix: &str,
    range: &str,
    recordings: &Path,
) -> Result<Value> {
    write(
        native,
        graph,
        sound,
        (template, bus),
        prefix,
        Preemption::Refuse,
        Some(range),
        NO_SECOND_EVENT,
        (recordings, true),
    )
}

/// A native node's output bus, its parent, whether it overrides its parent's positioning, and the
/// positioning bytes and attenuation of a 3D spatial override (`D9`), from version 113 node
/// parameters at `node`: three effect bytes, which must be empty, the bus, the parent, a flag
/// byte, the property bundle (a count, one id byte and one value word each), the ranged bundle (a
/// count, one id byte and two words each) and the positioning flags. Bit 0 of the flags overrides
/// the parent.
struct Placement {
    bus: u32,
    parent: u32,
    overrides: bool,
    spatial: Option<([u8; 2], u32)>,
}

fn placement(object: &[u8], node: usize) -> Result<Option<Placement>> {
    if object.get(node..node + 3) != Some(&[0, 0, 0]) {
        return Ok(None);
    }
    let byte = |at: usize| {
        object
            .get(at)
            .copied()
            .context("Native node parameters end early")
    };
    let mut at = node + 12;
    let properties = usize::from(byte(at)?);
    at += 1 + properties * 5;
    let ranged = usize::from(byte(at)?);
    at += 1 + ranged * 9;
    let bits = byte(at)?;
    Ok(Some(Placement {
        bus: super::word(object, node + 3)?,
        parent: super::word(object, node + 7)?,
        overrides: bits & 1 != 0,
        spatial: if bits == 0xd9 {
            Some(([bits, byte(at + 1)?], super::word(object, at + 2)?))
        } else {
            None
        },
    }))
}

/// The attenuation that native version 113 `bank` gives its event `event`'s sound on `bus`: the
/// first play action (`0403`) whose target routes to `bus`, positioned by its nearest node that
/// overrides positioning, when that node is 3D spatial with an attenuation. Each node's parent
/// supplies the bus and positioning it inherits. The stock contact cue `80BF59B7` plays three
/// layers, and the first on bus `2CE93FB5` takes attenuation `0942ACBA` from its actor-mixer.
fn attenuation(bank: &[u8], event: u32, bus: u32) -> Result<(u32, Attenuation)> {
    let objects = super::bank_objects(bank)?;
    let (kind, body) = objects
        .get(&event)
        .context("Native one-shot event is absent from its bank")?;
    ensure!(*kind == 4, "Native one-shot event has another type");
    for index in 0..usize::try_from(super::word(body, 4)?)? {
        let Some((3, action)) = objects.get(&super::word(body, 8 + index * 4)?) else {
            continue;
        };
        if action.get(4..6) != Some(&[3, 4]) {
            continue;
        }
        let mut target = super::word(action, 6)?;
        let (mut routed, mut positioned) = (None, None);
        for _ in 0..64 {
            let Some((kind, object)) = objects.get(&target) else {
                break;
            };
            let node = match kind {
                2 => 18,
                5 | 6 | 7 | 9 => 4,
                _ => break,
            };
            let Some(placed) = placement(object, node)? else {
                break;
            };
            if routed.is_none() && placed.bus != 0 {
                routed = Some(placed.bus);
            }
            if positioned.is_none() && placed.overrides {
                positioned = Some(placed.spatial);
            }
            if routed.is_some() && positioned.is_some() || placed.parent == 0 {
                break;
            }
            target = placed.parent;
        }
        if routed != Some(bus) {
            continue;
        }
        let Some(Some((positioning, id))) = positioned else {
            bail!("The native sound on bus {bus:08X} is not 3D spatial");
        };
        let Some((14, object)) = objects.get(&id) else {
            bail!("Native attenuation {id:08X} is absent from its bank");
        };
        return Ok((id, Attenuation::new(positioning, object[4..].to_vec())?));
    }
    bail!("No native play action of the one-shot event routes to bus {bus:08X}")
}

/// The sample identity of each source variation and its recording from `recordings` as native
/// media.
fn variations(variants: &[Value], recordings: &Path) -> Result<Vec<(u32, Vec<u8>)>> {
    let mut result: Vec<(u32, Vec<u8>)> = Vec::new();
    for variant in variants {
        let id = u32::try_from(
            variant["fsb_info"]
                .as_u64()
                .context("Source one-shot sample")?,
        )?;
        ensure!(
            result.iter().all(|(seen, _)| *seen != id),
            "Source one-shot variations repeat a sample"
        );
        let data = crate::tiger::audio::transcode::normalize_pcm_wem(&std::fs::read(
            recordings.join(format!("{id:08X}.wav")),
        )?)?;
        result.push((id, data));
    }
    Ok(result)
}

/// Every check and recording read happens before the first graph node is added, so a sound that
/// does not convert leaves the graph unchanged.
#[allow(clippy::too_many_arguments)]
fn write(
    native: &mut Reader,
    graph: &mut Graph,
    sound: &Value,
    (template, bus): (u32, Option<u32>),
    prefix: &str,
    preemption: Preemption,
    range: Option<&str>,
    second: u32,
    (recordings, spatial): (&Path, bool),
) -> Result<Value> {
    ensure!(
        !prefix.is_empty()
            && prefix
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_')),
        "Invalid one-shot graph namespace"
    );
    let bank_symbol = format!("{prefix}-bank");
    let cue_symbol = format!("{prefix}-cue");
    let original = native.tag(template, Some(0x80809802))?;
    let bank_tag = original.u32(20)?;
    let bank = native.tag(bank_tag, None)?;
    let bus = super::output_bus(&bank.0, bus, original.u32(8)?)?;
    super::verify_bus(&bank.0, bus)?;
    let attenuation = spatial.then(|| attenuation(&bank.0, original.u32(8)?, bus));
    let source = &sound["data"];
    ensure!(
        source["promotion_index"] == -1 && source["custom_playback_index"] == -1,
        "Source one-shot sound requires source promotion or custom playback"
    );
    let ranges = source["pitch_ranges"]
        .as_array()
        .context("Source one-shot pitch ranges")?;
    ensure!(
        ranges.len() == 1,
        "Source one-shot sound requires range switching"
    );
    let variants = ranges[0]["variants"]
        .as_array()
        .context("Source one-shot variations")?;
    ensure!(
        !variants.is_empty(),
        "Source one-shot sound has no variations"
    );
    let limits = &source["playback_limits"];
    ensure!(
        (preemption == Preemption::Omit || limits["preemption_ms"] == 0)
            && limits["maximum_per_object_per_class"] == 0,
        "Source one-shot sound requires timed or class preemption"
    );
    let limit = u16::try_from(
        limits["maximum_per_tag"]
            .as_u64()
            .filter(|v| (1..=16).contains(v))
            .context("Source one-shot global voice limit")?,
    )?;
    let gain = source["playback"]["gain_db"]
        .as_f64()
        .context("Source one-shot gain")? as f32;
    let medium = original.u32(
        *original
            .array(0x18, 4, Some(0x80800014))?
            .first()
            .context("Native one-shot medium")?,
    )?;
    ensure!(
        original.0.len() >= 0x50 && original.pointer(0x20)? == 0x40,
        "Source one-shot cue envelope differs"
    );
    let recorded = variations(variants, recordings)?;
    let ids = recorded.iter().map(|(id, _)| *id).collect::<Vec<_>>();
    let (mut bank, attenuated) = match &attenuation {
        Some(Ok((id, attenuation))) => (
            Bank::one_shot_global_spatial(&ids, bus, gain, limit, attenuation)?,
            json!({"status":"translated","native":format!("{id:08X}"),
                "limits":["The cue falls off with distance by the native contact attenuation, not the source sound's own distance bounds."]}),
        ),
        Some(Err(error)) => (
            Bank::one_shot_global(&ids, bus, gain, limit)?,
            json!({"status":"omitted","reason":format!("{error:#}")}),
        ),
        None => (Bank::one_shot_global(&ids, bus, gain, limit)?, Value::Null),
    };
    // Generated banks all use bank 1 and objects from 1. Cues that share the graph's linked
    // namespace take separate 8192-wide ranges named by their range name.
    let event = if let Some(range) = range {
        let base = 0x1000_0000 | (crate::tiger::audio::bank::hash(range) & 0x0FFF_E000);
        bank = bank.rebased(base)?;
        base + 1
    } else {
        1
    };
    let mut media = Vec::new();
    for (id, data) in &recorded {
        let symbol = format!("{prefix}-media-{id:08X}");
        graph.add(&symbol, medium, data, None, Vec::new())?;
        graph.nodes.last_mut().unwrap()["storage"] = json!("audio_media");
        media.push(json!({"id":id,"symbol":symbol}));
    }
    graph.add(&bank_symbol, bank_tag, &bank.bytes, None, Vec::new())?;
    let node = graph.nodes.last_mut().unwrap();
    node["storage"] = json!("audio_bank");
    node["audio_bank"] = serde_json::to_value(&bank)?;
    node["audio_media"] = json!(media);
    let mut data = original.0[..0x50].to_vec();
    data.resize(0x50 + ids.len() * 4, 0xff);
    let size = data.len() as u64;
    put(&mut data, 0, &size.to_le_bytes())?;
    put(&mut data, 8, &event.to_le_bytes())?;
    put(&mut data, 12, &second.to_le_bytes())?;
    put(&mut data, 20, &u32::MAX.to_le_bytes())?;
    for at in [0x18, 0x40] {
        put(&mut data, at, &(ids.len() as u64).to_le_bytes())?;
    }
    let mut patches = vec![json!({"offset":20,"symbol":bank_symbol})];
    for (index, id) in ids.iter().enumerate() {
        patches.push(json!({"offset":0x50+index*4,"symbol":format!("{prefix}-media-{id:08X}")}));
    }
    graph.add(&cue_symbol, template, &data, None, patches)?;
    graph.nodes.last_mut().unwrap()["audio_events"] = json!([{"offset":8,"id":event}]);
    let playback = if attenuated["status"] == "translated" {
        "Playback at fixed pitch retains source recordings and base gain, positioned where it plays. Source distance bounds, environment tails, pitch modulation, skip fractions, randomized gain and per-object limits are not reproduced."
    } else {
        "Nonspatial playback at fixed pitch retains source recordings and base gain. Source attenuation, environment tails, pitch modulation, skip fractions, randomized gain and per-object limits are not reproduced."
    };
    Ok(
        json!({"source":sound["tag"],"variations":ids,"gain_db":gain,"global_voice_limit":limit,"output_bus":bus,"attenuation":attenuated,"source_playback":source["playback"],"source_playback_limits":limits,"limits":[playback]}),
    )
}

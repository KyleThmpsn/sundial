//! Source swing recordings on translated character actions.
//!
//! Reach sounds a weapon's swing from the wielder's first-person action. The Spartan third-person
//! weapon actions carry no sound events. A translated `{mode}:{class}:{role}` action therefore takes
//! the sound events of `first_person:{role}` in the character's own first-person graph, which lives
//! under the third-person model's folder in `fp`. Each recording becomes a private one-shot cue,
//! and the clip names it through a Wwise record shaped like the stock human-bank swing records.
//!
//! The donor's presentation sequences and Wwise records on that clip are removed. On the
//! inspected sword donor, each sequence holds the donor's swing cues and a trail particle system.
//! Named, sequence-kind and motion events stay at their native frames.
use super::Options;
use crate::{
    halo_reach::audio::once,
    presentation::{Graph, append, put},
    tiger::{payload::Payload, reader::Reader},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path};

const LIST: u32 = 0x8080902B;
const RESOURCE: u32 = 0x80809033;
/// The stock third-person sword swing cue, `sword_weak_1.wwise_event`, which human-bank sword
/// clips play. Its bank supplies the output bus and its streamed medium the media template.
const TEMPLATE: u32 = 0x80B4F754;
/// The locator word of every stock human-bank Wwise record, the FNV-1 basis of an empty name.
const NO_LOCATOR: u32 = 0x811C9DC5;
const RECORD: usize = 32;

/// One source sound event of a first-person action.
struct Swing {
    frame: u64,
    datum: u64,
}

fn shared<'a>(graph: &'a Value, name: &str) -> Option<&'a Value> {
    let clips = graph["clips"].as_array()?;
    let found = clips
        .iter()
        .filter(|clip| clip["name"] == name)
        .collect::<Vec<_>>();
    let [clip] = found[..] else { return None };
    match clip["shared"].as_array()?.as_slice() {
        [shared] => Some(shared),
        _ => None,
    }
}

fn frames(shared: &Value) -> Result<u64> {
    shared["frames"]
        .as_u64()
        .filter(|frames| (2..=3600).contains(frames))
        .context("Source swing clip duration")
}

/// The first-person event, placed on the third-person action by duration, then on the native
/// clip by that action's own motion fit.
fn fit(
    frame: u64,
    first_person: u64,
    third_person: u64,
    native: u64,
    timing: &Value,
) -> Result<u16> {
    let at = frame as f64 * (third_person - 1) as f64 / (first_person - 1) as f64;
    let last = (native - 1) as f64;
    let mapped = if timing["mode"] == "contact" {
        let source = timing["source_frame"]
            .as_f64()
            .context("Source contact frame")?;
        let contact = timing["native_frame"]
            .as_f64()
            .context("Native contact frame")?;
        if at <= source {
            at * contact / source
        } else {
            contact + (at - source) * (last - contact) / ((third_person - 1) as f64 - source)
        }
    } else {
        at * last / (third_person - 1) as f64
    };
    Ok(u16::try_from((mapped.round() as u64).min(native - 1))?)
}

fn allocate(bytes: &mut Vec<u8>, class: u32, size: usize) -> Result<usize> {
    let at = (bytes.len() + 4 + 7) & !7;
    bytes.resize(at + size, 0);
    put(bytes, at - 4, &class.to_le_bytes())?;
    Ok(at)
}

/// Replace the donor presentation records and add one Wwise record per cue. Returns the clip,
/// the offset of each record's cue word in cue order and the removed resource paths.
fn rebuild(clip: &Payload, cues: &[(u16, &str)]) -> Result<(Payload, Vec<usize>, Vec<String>)> {
    let mut out = clip.clone();
    let mut list = Vec::new();
    let mut removed = Vec::new();
    for (order, row) in clip.array(0x160, 8, Some(LIST))?.into_iter().enumerate() {
        let at = clip.pointer(row)?;
        let class = clip.u32(at.checked_sub(4).context("Native clip event class")?)?;
        if class == RESOURCE {
            let start = clip.pointer(at + 16)?;
            let end = clip.0[start..]
                .iter()
                .position(|b| *b == 0)
                .context("Native clip resource path")?;
            let path = String::from_utf8_lossy(&clip.0[start..start + end]).into_owned();
            if path.ends_with("sequence.tft") || path.ends_with(".wwise_event") {
                removed.push(path);
                continue;
            }
        }
        list.push((clip.u16(at)?, order, at));
    }
    let mut words = Vec::new();
    for (index, (frame, path)) in cues.iter().enumerate() {
        let at = allocate(&mut out.0, RESOURCE, RECORD)?;
        put(
            &mut out.0,
            at,
            &((3u32 << 16) | u32::from(*frame)).to_le_bytes(),
        )?;
        put(&mut out.0, at + 8, &NO_LOCATOR.to_le_bytes())?;
        put(&mut out.0, at + 24, &TEMPLATE.to_le_bytes())?;
        let text = out.0.len();
        out.0.extend_from_slice(path.as_bytes());
        out.0.push(0);
        put(
            &mut out.0,
            at + 16,
            &(text as i64 - (at + 16) as i64).to_le_bytes(),
        )?;
        list.push((*frame, usize::MAX - cues.len() + index, at));
        words.push(at + 24);
    }
    list.sort_by_key(|&(frame, order, _)| (frame, order));
    append(
        &mut out.0,
        0x160,
        u64::from(LIST),
        &vec![0; list.len() * 8],
        8,
    )?;
    let rows = out.array(0x160, 8, Some(LIST))?;
    ensure!(rows.len() == list.len(), "Rebuilt clip event list differs");
    for (&row, &(_, _, at)) in rows.iter().zip(&list) {
        put(&mut out.0, row, &(at as i64 - row as i64).to_le_bytes())?;
    }
    for &word in &words {
        ensure!(
            out.u32(word)? == TEMPLATE && out.u32(word - 28)? == RESOURCE,
            "Swing record did not relocate cleanly"
        );
    }
    Ok((out, words, removed))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn prepare(
    native: &mut Reader,
    options: &Options,
    third_person: &Value,
    actions: &[Value],
    converted: &mut [Value],
    graph: &mut Value,
    root: &Path,
    private: u32,
) -> Result<Value> {
    let source_root = root.parent().context("Swing import root")?.join("source");
    let folder = options
        .model
        .rsplit_once('\\')
        .map(|(folder, _)| format!("{folder}\\fp\\"))
        .context("Character model path has no folder")?;
    let animations: Value =
        serde_json::from_slice(&fs::read(source_root.join("animations.json"))?)?;
    let candidates = animations
        .as_array()
        .context("Resolved source animations")?
        .iter()
        .filter(|jmad| {
            jmad["tag"]["path"]
                .as_str()
                .is_some_and(|path| path.starts_with(&folder))
        })
        .collect::<Vec<_>>();
    let [first_person] = candidates[..] else {
        return Ok(json!({"status":"native","reason":format!(
            "{} first-person graphs under {folder}, one is required", candidates.len())}));
    };
    let behavior: Value = serde_json::from_slice(&fs::read(source_root.join("behavior.json"))?)?;
    let sounds = first_person["sounds"]
        .as_array()
        .context("First-person sound table")?;
    let mut nodes = Graph::new(
        root.to_owned(),
        graph["nodes"].as_array().cloned().context("Graph nodes")?,
    );
    let mut cues = BTreeMap::<u64, Value>::new();
    let mut symbols = Vec::new();
    let mut linked = Vec::new();
    let mut omitted = Vec::new();
    for action in actions {
        let Some(name) = action["clip"]
            .as_str()
            .or_else(|| action["source"].as_str())
        else {
            continue;
        };
        let parts = name.splitn(3, ':').collect::<Vec<_>>();
        let [_, _, role] = parts[..] else { continue };
        let counterpart = format!("first_person:{role}");
        let Some(fp) = shared(first_person, &counterpart) else {
            continue;
        };
        let mut swings = Vec::new();
        for event in fp["sound_events"]
            .as_array()
            .context("Source sound events")?
        {
            let sound = event["sound_index"]
                .as_u64()
                .and_then(|index| sounds.get(usize::try_from(index).ok()?))
                .filter(|sound| sound["group"] == "snd!");
            match (
                sound.and_then(|sound| sound["datum"].as_u64()),
                event["marker"].as_str(),
            ) {
                (Some(datum), Some("")) => swings.push(Swing {
                    frame: event["frame"].as_u64().context("Source sound frame")?,
                    datum,
                }),
                _ => omitted.push(
                    json!({"action":name,"source_clip":counterpart,"event":event,
                    "reason":"Source sound event names no sound or attaches to a marker"}),
                ),
            }
        }
        if swings.is_empty() {
            continue;
        }
        let fp_frames = frames(fp)?;
        let tp_frames = frames(shared(third_person, name).context("Third-person source action")?)?;
        for swing in &swings {
            ensure!(
                swing.frame < fp_frames,
                "Source sound event lies outside its clip"
            );
            if let std::collections::btree_map::Entry::Vacant(entry) = cues.entry(swing.datum) {
                let prefix = format!("character-swing-{:08X}", swing.datum);
                let node = &behavior["nodes"][format!("{:08X}", swing.datum)];
                ensure!(
                    node["tag"]["group"] == "snd!",
                    "Source swing sound is absent from the behavior receipt"
                );
                let mut report = once::cue_with(
                    native,
                    &mut nodes,
                    node,
                    TEMPLATE,
                    None,
                    &prefix,
                    once::Preemption::Omit,
                    true,
                )?;
                report["cue_symbol"] = json!(format!("{prefix}-cue"));
                report["omitted_preemption_ms"] =
                    node["data"]["playback_limits"]["preemption_ms"].clone();
                symbols.push(format!("{prefix}-cue"));
                symbols.push(format!("{prefix}-bank"));
                symbols.extend(
                    report["variations"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_u64)
                        .map(|id| format!("{prefix}-media-{id:08X}")),
                );
                entry.insert(report);
            }
        }
        for (descriptor, ordinal) in action["clips"]
            .as_object()
            .context("Translated action clips")?
        {
            let ordinal = usize::try_from(ordinal.as_u64().context("Translated clip ordinal")?)?;
            let clip = converted
                .get_mut(ordinal)
                .context("Translated clip is absent")?;
            let file = clip["file"]
                .as_str()
                .context("Translated clip file")?
                .to_owned();
            let payload = Payload(fs::read(root.join(&file))?);
            let native_frames = u64::from(payload.u16(0x13C)?);
            ensure!(native_frames >= 2, "Translated clip has no motion interval");
            let mut placed = Vec::new();
            for (index, swing) in swings.iter().enumerate() {
                let frame = fit(
                    swing.frame,
                    fp_frames,
                    tp_frames,
                    native_frames,
                    &clip["timing"],
                )?;
                let path = format!(
                    "content\\audio\\wwise_events\\characters\\weapon_animations\\sword\\reach_{private:08x}_{ordinal}_{index}.wwise_event"
                );
                placed.push((frame, path));
            }
            let (rebuilt, words, removed) = rebuild(
                &payload,
                &placed
                    .iter()
                    .map(|(frame, path)| (*frame, path.as_str()))
                    .collect::<Vec<_>>(),
            )?;
            fs::write(root.join(&file), &rebuilt.0)?;
            let rows = swings
                .iter()
                .zip(&placed)
                .zip(&words)
                .map(|((swing, (frame, _)), word)| {
                    json!({"offset":word,"symbol":cues[&swing.datum]["cue_symbol"],
                        "source_frame":swing.frame,"native_frame":frame})
                })
                .collect::<Vec<_>>();
            clip["cues"] = json!(rows);
            linked.push(
                json!({"action":name,"descriptor":descriptor,"source_clip":counterpart,
                "source_frames":fp_frames,"native_frames":native_frames,"cues":rows,
                "removed_native_resources":removed}),
            );
        }
    }
    graph["nodes"] = json!(nodes.nodes);
    let status = if linked.is_empty() {
        "native"
    } else {
        "linked"
    };
    Ok(
        json!({"status":status,"first_person_graph":first_person["tag"],"template_cue":format!("{TEMPLATE:08X}"),
        "sounds":cues.into_values().collect::<Vec<_>>(),"symbols":symbols,"clips":linked,"omitted":omitted,
        "fitting":"First-person frames scale to the third-person action's duration, then follow its contact or linear fit",
        "limits":["Each source sound event becomes a nonspatial private one-shot cue. Source preemption fades, attenuation and per-object limits are not translated.",
            "Removing the donor sequences also removes their trail particles.",
            "In-game playback of the authored cues is unverified."]}),
    )
}

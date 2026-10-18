//! Source recordings on the imported first-person clips' sound events.
//!
//! A translated action keeps its native descriptor, duration and every event
//! that is not a donor Wwise cue: named, sequence and `.sequence.tft` resource
//! events stay at their native frames. The donor's `.wwise_event` records are
//! replaced by one private one-shot cue per source sound event, placed at the
//! source frame fitted to the native duration with the arm motion's mapping.
//! The weapon melee route shares the one-shot cue builder below.
use super::*;
use crate::{
    presentation::{append, put},
    tiger::{audio::bank::Bank, payload::Payload, reader::Reader},
};

/// Native clip event list and resource record classes.
const LIST: u32 = 0x8080902B;
const RESOURCE: u32 = 0x80809033;
/// Resource records naming a Wwise event carry the donor cue tag at `+24`.
const WWISE_EVENT: &[u8] = b".wwise_event";
const RECORD: usize = 32;

fn merge(audio: &mut Value, name: &str, row: Value) -> Result<()> {
    let rows = audio[name]
        .as_array_mut()
        .context("Source audio asset list")?;
    if let Some(existing) = rows
        .iter()
        .find(|old| old["source_tag"] == row["source_tag"])
    {
        ensure!(
            existing == &row,
            "Source audio identity has conflicting assets"
        );
    } else {
        rows.push(row);
    }
    Ok(())
}

/// Convert one source sound to a private one-shot cue on the native bus.
/// Media and banks already written for the same identity are shared.
pub(in crate::halo_reach) fn cue(
    source: &Value,
    recordings: &Path,
    root: &Path,
    bus: u32,
    audio: &mut Value,
) -> Result<Value> {
    let identity = source["tag"]["datum"]
        .as_u64()
        .context("Source sound identity")?;
    let key = format!("{identity:08X}");
    let data = &source["data"];
    ensure!(
        data["promotion_index"] == -1 && data["custom_playback_index"] == -1,
        "Source sound requires custom playback translation"
    );
    let ranges = data["pitch_ranges"]
        .as_array()
        .context("Source sound pitch ranges")?;
    let [range] = ranges.as_slice() else {
        anyhow::bail!("Source sound requires one pitch range");
    };
    let variants = range["variants"]
        .as_array()
        .context("Source sound variations")?;
    ensure!(
        !variants.is_empty()
            && variants
                .iter()
                .all(|v| v["skip_fraction_encoded"] == 0 && v["gain_encoded"] == 0),
        "Source sound needs explicit variation gains or skipping"
    );
    let playback = &data["playback"];
    ensure!(
        playback["skip_fraction"] == 0.0
            && playback["gain_variance_db"] == 0.0
            && playback["pitch_bounds"] == json!([0, 0])
            && range["parameters"]["natural_pitch_cents"] == 0
            && range["parameters"]["bend_bounds_cents"] == json!([0, 0]),
        "Source sound requires randomized pitch or gain translation"
    );
    let gain = playback["gain_db"].as_f64().context("Source sound gain")?
        + playback["first_person_gain_db"]
            .as_f64()
            .context("Source first-person gain")?;
    let limits = &data["playback_limits"];
    let voice_limit = u16::try_from(
        limits["maximum_per_object_per_tag"]
            .as_u64()
            .context("Source voice limit")?,
    )?;
    ensure!(
        limits["maximum_per_object_per_class"] == 0,
        "Source sound requires a shared object-class limit"
    );
    let mut media = Vec::new();
    fs::create_dir_all(root.join("audio"))?;
    for variant in variants {
        let id = u32::try_from(
            variant["fsb_info"]
                .as_u64()
                .context("Source recording identity")?,
        )?;
        let key = format!("{id:08X}");
        let file = format!("audio/{key}.wem");
        let pcm = crate::tiger::audio::transcode::normalize_pcm_wem(&fs::read(
            recordings.join(format!("{key}.wav")),
        )?)?;
        let path = root.join(&file);
        if path.exists() {
            ensure!(
                fs::read(&path)? == pcm,
                "Source recording identity conflicts with existing audio"
            );
        } else {
            fs::write(path, pcm)?;
        }
        merge(
            audio,
            "transcoded_media",
            json!({"source_tag":key,"file":file}),
        )?;
        media.push(id);
    }
    let bank = Bank::one_shot(&media, bus, gain as f32, voice_limit)?;
    let file = format!("audio/cue-{key}.bank.json");
    fs::write(root.join(&file), serde_json::to_vec(&bank)?)?;
    merge(
        audio,
        "converted_banks",
        json!({"source_tag":key,"file":file,"version":113,"routing":"source_one_shot"}),
    )?;
    let media = media
        .iter()
        .map(|id| format!("{id:08X}"))
        .collect::<Vec<_>>();
    Ok(
        json!({"source":source["tag"],"source_playback":playback,"source_playback_limits":limits,
        "media":media,"media_ids":media,"bank":key,"event_id":"00000001","routing":"source_one_shot"}),
    )
}

/// One native clip event list row.
struct Event {
    at: usize,
    class: u32,
    kind: u32,
    /// The Wwise event path and donor cue tag of an audio record.
    audio: Option<(Vec<u8>, u32)>,
}

fn events(clip: &Payload) -> Result<Vec<Event>> {
    let mut events = Vec::new();
    for row in clip.array(0x160, 8, Some(LIST))? {
        let at = clip.pointer(row)?;
        let class = clip.u32(at.checked_sub(4).context("Native clip event class")?)?;
        let kind = clip.u32(at)?;
        let mut audio = None;
        if class == RESOURCE {
            let start = clip.pointer(at + 16)?;
            let end = clip.0[start..]
                .iter()
                .position(|b| *b == 0)
                .context("Native clip resource path")?;
            let path = clip.0[start..start + end].to_vec();
            if path.ends_with(WWISE_EVENT) {
                ensure!(
                    kind >> 16 == 3
                        && clip.u32(at + 4)? == 2
                        && clip.u32(at + 12)? == 0
                        && clip.u32(at + 28)? == 0,
                    "Unsupported native clip audio record"
                );
                audio = Some((path, clip.u32(at + 24)?));
            }
        }
        events.push(Event {
            at,
            class,
            kind,
            audio,
        });
    }
    Ok(events)
}

/// A donor audio record whose cue can model a private streamed sound.
struct Template {
    record: [u8; RECORD],
    path: Vec<u8>,
    native: Value,
}

/// Checked native cue assets, or `None` when the cue cannot model a private
/// sound: embedded media, no media, or retained empty media slots.
fn native_sound(
    native: &mut Reader,
    tag: u32,
    cache: &mut BTreeMap<u32, Option<Value>>,
) -> Result<Option<Value>> {
    if let Some(known) = cache.get(&tag) {
        return Ok(known.clone());
    }
    let assets = crate::tiger::audio::sound_assets(native, tag, false)
        .with_context(|| format!("Native clip audio record cue {tag:08X}"))?;
    let media = assets["media"].as_array().map_or(0, Vec::len);
    let sound = native.tag(tag, Some(0x80809802))?;
    let slots = if sound.0.len() == 0x30 {
        0
    } else {
        usize::try_from(sound.u64(0x40)?)?
    };
    let usable = media > 0 && media == slots && assets.get("embedded_media").is_none();
    let value = usable.then_some(assets);
    cache.insert(tag, value.clone());
    Ok(value)
}

fn template(
    native: &mut Reader,
    clip: &Payload,
    event: &Event,
    cache: &mut BTreeMap<u32, Option<Value>>,
) -> Result<Option<Template>> {
    let Some((path, tag)) = &event.audio else {
        return Ok(None);
    };
    let record = clip.bytes::<RECORD>(event.at)?;
    Ok(native_sound(native, *tag, cache)?.map(|native| Template {
        record,
        path: path.clone(),
        native,
    }))
}

/// Source frame on the native clip, following the arm motion's linear fit of
/// the source sequence to the native duration.
fn fit(sequence_frame: u64, total: u64, native: u16) -> u16 {
    if total <= 1 || native <= 1 {
        return 0;
    }
    let frame = (sequence_frame as f64 * f64::from(native - 1) / (total - 1) as f64).round();
    (frame as u16).min(native - 1)
}

fn allocate(bytes: &mut Vec<u8>, class: u32, size: usize) -> Result<usize> {
    let at = (bytes.len() + 4 + 7) & !7;
    bytes.resize(at + size, 0);
    put(bytes, at - 4, &class.to_le_bytes())?;
    Ok(at)
}

/// Replace the donor audio records with one private cue record per source
/// event, keeping every other event, and name the clip by its private marker.
/// Returns the clip and the list index of each cue in source order.
fn rebuild(
    clip: &Payload,
    marker: u32,
    cues: &[(u16, &Template)],
) -> Result<(Payload, Vec<usize>)> {
    let existing = events(clip)?;
    let mut out = clip.clone();
    let mut list = existing
        .iter()
        .enumerate()
        .filter(|(_, event)| event.audio.is_none())
        .map(|(order, event)| ((event.kind & 0xFFFF) as u16, order, event.at))
        .collect::<Vec<_>>();
    let mut added = Vec::new();
    for (index, (frame, template)) in cues.iter().enumerate() {
        let at = allocate(&mut out.0, RESOURCE, RECORD)?;
        put(&mut out.0, at, &template.record)?;
        put(
            &mut out.0,
            at,
            &((3u32 << 16) | u32::from(*frame)).to_le_bytes(),
        )?;
        let path = out.0.len();
        out.0.extend_from_slice(&template.path);
        out.0.push(0);
        put(
            &mut out.0,
            at + 16,
            &(path as i64 - (at + 16) as i64).to_le_bytes(),
        )?;
        list.push((*frame, existing.len() + index, at));
        added.push(at);
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
    put(&mut out.0, 0x120, &marker.to_le_bytes())?;
    let indices = added
        .iter()
        .map(|at| {
            list.iter()
                .position(|&(_, _, target)| target == *at)
                .context("Rebuilt cue is absent from its clip")
        })
        .collect::<Result<Vec<_>>>()?;
    // The relocated list must read back as the native shape it was built from.
    let check = events(&out)?;
    // Frame ordering is deliberate, so the retained events compare as a set.
    let kept = |found: &[Event]| {
        let mut kept = found
            .iter()
            .filter(|event| event.audio.is_none())
            .map(|event| (event.class, event.kind))
            .collect::<Vec<_>>();
        kept.sort_unstable();
        kept
    };
    ensure!(
        check.len() == list.len()
            && check.iter().filter(|event| event.audio.is_some()).count() == cues.len()
            && indices.iter().zip(cues).all(|(&index, (frame, template))| {
                let event = &check[index];
                event.kind == (3 << 16) | u32::from(*frame)
                    && event.audio.as_ref().is_some_and(|(path, tag)| {
                        *path == template.path && template.native["tag"] == format!("{tag:08X}")
                    })
            })
            && kept(&check) == kept(&existing),
        "Rebuilt clip events did not relocate cleanly"
    );
    Ok((out, indices))
}

/// One source sound event of an action, in sequence order.
struct SourceEvent {
    clip: String,
    index: usize,
    frame: u64,
    sequence_frame: u64,
    marker: String,
    sound: Option<u64>,
}

fn source_clip<'a>(jmad: &'a Value, name: &str) -> Result<&'a Value> {
    let clips = jmad["clips"]
        .as_array()
        .context("Source animation clips")?
        .iter()
        .filter(|clip| clip["name"] == name)
        .collect::<Vec<_>>();
    let [clip] = clips[..] else {
        anyhow::bail!("Source clip {name} is absent or ambiguous");
    };
    let variants = clip["shared"].as_array().context("Source clip variants")?;
    let [shared] = variants.as_slice() else {
        anyhow::bail!("Source clip {name} requires an explicit variant selection");
    };
    Ok(shared)
}

/// Source sound events of an action's clip or clip sequence, with the total
/// source frame count the native duration is fitted to.
fn source_events(jmad: &Value, names: &[String]) -> Result<(Vec<SourceEvent>, u64)> {
    let sounds = jmad["sounds"]
        .as_array()
        .context("Source animation sound table")?;
    let mut events = Vec::new();
    let mut total = 0u64;
    for name in names {
        let shared = source_clip(jmad, name)?;
        let frames = shared["frames"]
            .as_u64()
            .filter(|frames| (1..=3600).contains(frames))
            .context("Source clip duration")?;
        for (index, event) in shared["sound_events"]
            .as_array()
            .context("Source clip sound events")?
            .iter()
            .enumerate()
        {
            let frame = event["frame"].as_u64().context("Source sound frame")?;
            let sound = event["sound_index"]
                .as_u64()
                .and_then(|index| sounds.get(usize::try_from(index).ok()?))
                .filter(|sound| sound["group"] == "snd!")
                .and_then(|sound| sound["datum"].as_u64());
            events.push(SourceEvent {
                clip: name.clone(),
                index,
                frame,
                sequence_frame: total + frame,
                marker: event["marker"].as_str().unwrap_or_default().to_owned(),
                sound,
            });
        }
        total += frames;
    }
    Ok((events, total))
}

fn clip_names(source_clip: &Value) -> Result<Vec<String>> {
    match source_clip {
        Value::String(name) => Ok(vec![name.clone()]),
        Value::Array(names) => names
            .iter()
            .map(|name| {
                name.as_str()
                    .map(str::to_owned)
                    .context("Source action sequence name")
            })
            .collect(),
        _ => anyhow::bail!("Source action clip selection is invalid"),
    }
}

/// Primary firing recordings reachable from the source barrel. They belong to
/// the firing route and are never bound to a clip event.
fn firing_sounds(behavior: &Value) -> Result<BTreeSet<u64>> {
    let root = behavior["root"]["datum"]
        .as_u64()
        .context("Behavior root")?;
    let queue = behavior["edges"]
        .as_array()
        .context("Behavior links")?
        .iter()
        .filter(|e| e["owner"] == root && e["role"] == "fire")
        .map(|e| e["tag"]["datum"].as_u64().context("Fire identity"))
        .collect::<Result<Vec<_>>>()?;
    Ok(super::fire_sounds_from(behavior, queue)?
        .iter()
        .filter_map(|sound| sound["tag"]["datum"].as_u64())
        .collect())
}

/// Bind the source handling recordings of every translated first-person action
/// to that action's private clips. Firing states stay with the firing route.
/// Actions and events the route cannot serve are reported, not fatal.
#[allow(clippy::cognitive_complexity)]
pub(in crate::halo_reach) fn prepare(
    native: &mut Reader,
    behavior: &Value,
    graph: &mut Value,
    root: &Path,
) -> Result<Value> {
    let first_person = graph["animation"]["first_person"].clone();
    let actions = first_person["arm_motion"]["actions"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    if actions.is_empty() {
        return Ok(json!({"status":"source_only","reason":"No translated first-person actions"}));
    }
    if graph["audio"]["authoring_schema"] != 3 {
        return Ok(
            json!({"status":"source_only","reason":"Handling cues need the linked source firing route and its native bus","audio":graph["audio"]}),
        );
    }
    let bus = u32::try_from(
        graph["audio"]["output_bus"]
            .as_u64()
            .context("Source audio bus")?,
    )?;
    let source_graph = first_person["arm_motion"]["source_graph"]["datum"]
        .as_u64()
        .context("Source arm animation graph")?;
    let source_animations: Value = serde_json::from_slice(&fs::read(
        root.parent()
            .context("Import root")?
            .join("source/animations.json"),
    )?)?;
    let jmad = source_animations
        .as_array()
        .context("Resolved source animations")?
        .iter()
        .find(|graph| graph["tag"]["datum"].as_u64() == Some(source_graph))
        .context("Source arm animation graph is absent from the resolved animation receipt")?;
    let firing = firing_sounds(behavior)?;
    let recordings = root.parent().context("Import root")?.join("source/audio");
    // Donor audio records across every translated clip supply the record
    // layout and the native cue a private sound is modeled on.
    let mut sounds = BTreeMap::new();
    let mut payloads = BTreeMap::new();
    let mut shared = None;
    for action in &actions {
        for clip in action["clips"]
            .as_array()
            .context("Translated action clips")?
        {
            let file = clip["file"].as_str().context("Translated clip file")?;
            let payload = Payload(fs::read(root.join(file))?);
            // A clip whose donor records the route does not recognize keeps its donor events
            // and is reported when its action is reached.
            for event in events(&payload).unwrap_or_default() {
                if shared.is_none() {
                    shared = template(native, &payload, &event, &mut sounds)?;
                }
            }
            payloads.insert(file.to_owned(), payload);
        }
    }
    let mut converted = BTreeMap::<u64, Result<Value, String>>::new();
    let mut rows = Vec::new();
    let mut reports = Vec::new();
    let mut omitted = Vec::new();
    let mut selected = BTreeSet::new();
    for action in &actions {
        crate::cancellation::check()?;
        let state = action["state"].as_str().context("Action state")?;
        let names = clip_names(&action["source_clip"])?;
        selected.extend(names.iter().cloned());
        let (source, total) = source_events(jmad, &names)?;
        if state.starts_with("fire") {
            if !source.is_empty() {
                omitted.push(
                    json!({"state":state,"source_clips":names,"events":source.len(),
                    "reason":"Firing states are routed by the firing route"}),
                );
            }
            continue;
        }
        if source.is_empty() {
            reports.push(json!({"state":state,"source_clips":names,"cues":0,
                "status":"No source sound events, donor cues retained"}));
            continue;
        }
        let mut cues = Vec::new();
        for event in &source {
            let Some(datum) = event.sound else {
                omitted.push(
                    json!({"state":state,"source_clip":event.clip,"event":event.index,
                    "reason":"Source sound event names no sound"}),
                );
                continue;
            };
            if firing.contains(&datum) {
                omitted.push(
                    json!({"state":state,"source_clip":event.clip,"event":event.index,
                    "reason":"Primary firing recording is routed by the firing route"}),
                );
                continue;
            }
            ensure!(
                event.sequence_frame < total,
                "Source sound event lies outside its clip"
            );
            let result = converted.entry(datum).or_insert_with(|| {
                let node = &behavior["nodes"][format!("{datum:08X}")];
                if node["tag"]["group"] != "snd!" {
                    return Err("Source sound is absent from the behavior receipt".to_owned());
                }
                cue(node, &recordings, root, bus, &mut graph["audio"])
                    .map_err(|error| format!("{error:#}"))
            });
            match result {
                Ok(value) => cues.push((event, value.clone())),
                Err(reason) => omitted.push(
                    json!({"state":state,"source_clip":event.clip,"event":event.index,"reason":reason.as_str()}),
                ),
            }
        }
        if cues.is_empty() {
            reports.push(json!({"state":state,"source_clips":names,"cues":0,
                "status":"No source sound event could be converted, donor cues retained"}));
            continue;
        }
        let mut clips = Vec::new();
        for clip in action["clips"]
            .as_array()
            .context("Translated action clips")?
        {
            let file = clip["file"].as_str().context("Translated clip file")?;
            let marker = u32::from_str_radix(clip["marker"].as_str().context("Clip marker")?, 16)?;
            let payload = payloads.get(file).context("Translated clip payload")?;
            let frames = payload.u16(0x13C)?;
            ensure!(frames > 0, "Translated clip has no frames");
            let native_events = match events(payload) {
                Ok(events) => events,
                Err(error) => {
                    omitted.push(json!({"state":state,"descriptor":clip["descriptor"],
                        "reason":format!("Donor clip events are kept, {error:#}")}));
                    continue;
                }
            };
            let mut own = None;
            for event in native_events {
                if own.is_none() {
                    own = template(native, payload, &event, &mut sounds)?;
                }
            }
            let Some(template) = own.as_ref().or(shared.as_ref()) else {
                omitted.push(json!({"state":state,"descriptor":clip["descriptor"],
                    "reason":"No donor audio record with streamed media can model a private cue"}));
                continue;
            };
            let fitted = cues
                .iter()
                .map(|(event, _)| (fit(event.sequence_frame, total, frames), template))
                .collect::<Vec<_>>();
            let (rebuilt, indices) = rebuild(payload, marker, &fitted)?;
            fs::write(root.join(file), &rebuilt.0)?;
            for ((event, converted), (index, (frame, _))) in
                cues.iter().zip(indices.iter().zip(&fitted))
            {
                rows.push(
                    json!({"clip_name":format!("{marker:08X}"),"event_index":index,
                    "sounds":[converted],"native_sounds":[template.native],
                    "state":state,"descriptor":clip["descriptor"],"source_clip":event.clip,
                    "source_event":event.index,"source_frame":event.frame,
                    "source_sequence_frame":event.sequence_frame,"source_frames":total,
                    "native_frame":frame,"native_frames":frames,"source_marker":event.marker}),
                );
            }
            clips.push(
                json!({"descriptor":clip["descriptor"],"marker":format!("{marker:08X}"),
                "native_frames":frames,"template_cue":template.native["tag"],
                "frames":fitted.iter().map(|(frame, _)| *frame).collect::<Vec<_>>()}),
            );
        }
        reports.push(json!({"state":state,"source_clips":names,"source_frames":total,
            "cues":cues.len(),"sounds":cues.iter().map(|(event, _)| event.sound).collect::<Vec<_>>(),
            "clips":clips,"status":"linked"}));
    }
    // Source clips with sound events that no action or strike selects.
    let strikes = first_person["melee"]["strikes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|strike| strike["source_clip"].as_str().map(str::to_owned))
        .collect::<BTreeSet<_>>();
    let unmapped = jmad["clips"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|clip| {
            clip["name"]
                .as_str()
                .is_some_and(|name| !selected.contains(name) && !strikes.contains(name))
        })
        .filter(|clip| {
            clip["shared"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|shared| {
                    shared["sound_events"]
                        .as_array()
                        .is_some_and(|events| !events.is_empty())
                })
        })
        .map(|clip| clip["name"].clone())
        .collect::<Vec<_>>();
    let audio = &mut graph["audio"];
    let mut existing = match audio.get("clip_events") {
        None => Vec::new(),
        Some(value) => value.as_array().context("Clip event rows")?.clone(),
    };
    for row in &rows {
        ensure!(
            existing.iter().all(|old| {
                old["clip_name"] != row["clip_name"] || old["event_index"] != row["event_index"]
            }),
            "Handling cue repeats an existing clip event"
        );
    }
    let cues = rows.len();
    existing.extend(rows);
    audio["clip_events"] = json!(existing);
    let status = if cues > 0 { "linked" } else { "source_only" };
    Ok(json!({
        "status":status,"cues":cues,
        "source_sounds":converted.values().filter(|result| result.is_ok()).count(),
        "actions":reports,"omitted":omitted,"unmapped_source_clips":unmapped,
        "fitting":"Source sound frames follow the arm motion's linear fit of the source clip or sequence to each native clip duration",
        "retained_native_behavior":["Native action durations, named events, sequence events and resource sequences",
            "Primary firing audio through the firing route","Donor cues on actions without source sound events"],
        "limits":["Each source sound event becomes a nonspatial private one-shot cue at its fitted frame. Source preemption, attenuation, marker attachment and class limits are not translated.",
            "Donor .sequence.tft resource events remain on the native clips and can still carry donor recordings and effects.",
            "Dry fire has no established native clip-event slot and stays native.",
            "In-game playback of the authored cues is unverified."]
    }))
}

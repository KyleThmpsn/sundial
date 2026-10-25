//! The glaive's melee audio, as The Enigma was given it by hand.
//!
//! - Swings: each converted swing's punch cue, the native class melee sound, plays the glaive's
//!   own swing sound. The source melee controller's attack for the swing's state names a
//!   feedback entity, and its sound owner names the sound. Every glaive's controller has the
//!   three swing states, so an exotic with a controller of its own gets its own swing sounds.
//! - Hits: two impact cues play through a contact profile of the weapon's own, named by a key
//!   derived from its namespace, with the native contact masks they answer to. That key is also
//!   the glaive's melee attack, which Adaptive Glaive applies, so every glaive needs the profile
//!   or its melee falls back to the class melee. Each of the controller's hit descriptions names
//!   a hit owner just before its response table, and that owner names the two cues in order.
//!   Every glaive's owner, the exotics' included, names the same two.
//! - Surfaces: the per-surface cues of the response table the controller's hit descriptions
//!   name most convert, each with the surface rows it plays for. They play through the hit
//!   profile, so they follow the hits. A cue whose media stays inside its bank keeps the stock
//!   sound.
use crate::d2_mot::{
    payload::Payload,
    prepare_clip_events, prepare_sounds,
    reader::{Reader, write_json},
    sound_assets,
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

/// Each swing's clip and the melee controller state whose attack plays it.
const SWING_STATES: [(u32, u32); 3] = [
    (0x80A8_A3F5, 0x0177_964C),
    (0x80A8_A441, 0x0177_964F),
    (0x80A8_A43E, 0x0177_964E),
];
/// The native class melee punch cue the converted swings carry.
const PUNCH: u32 = 0x80C1_2945;
const CONTROLLER_DEFINITION: u32 = 0x8080_34C6;
const ATTACKS: u32 = 0x8080_20E3;
const FEEDBACK: u32 = 0x8080_9AD8;
const OWNER: u32 = 0x8080_9B06;
const CLIP_EVENTS: u32 = 0x8080_902B;
const SOUND_EVENT: u32 = 0x8080_9033;
/// The native contact masks the hit owner's first and second impact cues answer to.
const HIT_MASKS: [&str; 2] = ["00410400", "00410800"];
/// A hit description names its hit owner this far before its response table.
const HIT_OWNER_BEFORE_TABLE: usize = 0x18;
const SOUND_CLASS: u32 = 0x8080_9738;
/// The hit descriptions' response tables.
const RESPONSE_CLASS: u32 = 0x8080_873F;

pub(super) struct Inputs<'a> {
    pub modern: &'a Path,
    pub native: &'a Path,
    pub graph: &'a Path,
    pub work: &'a Path,
    pub source_rig: &'a Value,
    pub namespace: &'a str,
}

fn hex(value: &Value) -> Result<u32> {
    Ok(u32::from_str_radix(
        value.as_str().context("tag text")?.trim_start_matches("0x"),
        16,
    )?)
}

/// FNV-1 of `text`, the hash the Enigma's contact profile key was named with.
fn fnv1(text: &str) -> u32 {
    text.bytes().fold(0x811C_9DC5u32, |key, byte| {
        key.wrapping_mul(16_777_619) ^ u32::from(byte)
    })
}

/// Adds rows to `list` unless one with the same source tag is already there.
fn merge(list: &mut Value, rows: &Value) -> Result<usize> {
    if list.is_null() {
        *list = json!([]);
    }
    let list = list.as_array_mut().context("audio list")?;
    let mut added = 0;
    for row in rows.as_array().into_iter().flatten() {
        if list
            .iter()
            .all(|old| old["source_tag"] != row["source_tag"])
        {
            list.push(row.clone());
            added += 1;
        }
    }
    Ok(added)
}

/// The source melee controller among the source runtime's owners.
fn controller(sr: &mut Reader, rig: &Value) -> Result<(u32, std::sync::Arc<Payload>)> {
    let mut found = Vec::new();
    let owners = rig["components"]
        .as_array()
        .context("source components")?
        .iter()
        .map(|row| hex(&row["owner"]))
        .collect::<Result<BTreeSet<_>>>()?;
    for owner in owners {
        let payload = sr.tag(owner, None)?;
        let is_controller = payload
            .pointer(24)
            .ok()
            .filter(|at| *at >= 4)
            .is_some_and(|at| payload.u32(at - 4).ok() == Some(CONTROLLER_DEFINITION));
        if is_controller {
            found.push((owner, payload));
        }
    }
    ensure!(
        found.len() == 1,
        "the source has {} melee controllers, not one",
        found.len()
    );
    Ok(found.remove(0))
}

/// Each swing's punch cue, played by the source sound its attack's feedback names.
fn swings(
    sr: &mut Reader,
    nr: &mut Reader,
    (controller_tag, controller): &(u32, std::sync::Arc<Payload>),
    graph: &Path,
) -> Result<Vec<Value>> {
    let base = controller.pointer(24)?;
    let attacks = controller.array(base + 0x758, 0x180, Some(ATTACKS))?;
    let native_sound = sound_assets(nr, PUNCH, false)?;
    let mut rows = Vec::new();
    for (clip, state) in SWING_STATES {
        let matching = attacks
            .iter()
            .copied()
            .filter(|at| controller.u32(at + 4).ok() == Some(state))
            .collect::<Vec<_>>();
        let [attack] = matching[..] else {
            anyhow::bail!(
                "melee state {state:08X} has {} attacks, not one",
                matching.len()
            );
        };
        ensure!(
            controller.u32(attack + 8)? == state,
            "melee attack {state:08X} identity differs"
        );
        let feedback = sr.ref64(controller, attack + 0xD8)?;
        let entity = sr.tag(feedback, Some(FEEDBACK))?;
        let mut sounds = Vec::new();
        for row in entity.array(8, 12, None)? {
            let owner = entity.u32(row)?;
            let payload = sr.tag(owner, None)?;
            if payload.0.len() < 0xBC0 || sr.reference(owner)? != OWNER {
                continue;
            }
            let sound = sr.ref64(&payload, 0xBB0)?;
            if !matches!(sound, 0 | u32::MAX) {
                sounds.push(sound);
            }
        }
        let [sound] = sounds[..] else {
            anyhow::bail!(
                "swing feedback {feedback:08X} names {} sounds, not one",
                sounds.len()
            );
        };
        let source_sound = sound_assets(sr, sound, true)?;
        let converted = Payload(fs::read(
            graph.join(format!("animation/source-clip-{clip:08X}.bin")),
        )?);
        let punches = converted
            .array(0x160, 8, Some(CLIP_EVENTS))?
            .into_iter()
            .enumerate()
            .filter_map(|(index, row)| {
                let at = converted.pointer(row).ok()?;
                (converted.u32(at - 4).ok() == Some(SOUND_EVENT)
                    && converted.u32(at + 24).ok() == Some(PUNCH))
                .then_some(index)
            })
            .collect::<Vec<_>>();
        let [punch] = punches[..] else {
            anyhow::bail!("swing {clip:08X} has {} punch cues, not one", punches.len());
        };
        let name = sr.tag(clip, Some(0x8080_8BE0))?.u32(0x120)?;
        rows.push(json!({"clip_name":format!("{name:08X}"),"event_index":punch,"sounds":[source_sound],
            "native_sounds":[native_sound.clone()],"source_controller":format!("{controller_tag:08X}"),
            "source_state":format!("{state:08X}"),"source_feedback":format!("{feedback:08X}")}));
    }
    Ok(rows)
}

/// The response table the controller's hit descriptions name most, the first named on a tie.
fn response_table(sr: &mut Reader, controller: &Payload) -> Result<u32> {
    let mut named: Vec<(u32, usize)> = Vec::new();
    for at in (0..controller.0.len().saturating_sub(4)).step_by(4) {
        let tag = controller.u32(at)?;
        if tag >> 24 == 0x80 && sr.reference(tag).ok() == Some(RESPONSE_CLASS) {
            match named.iter_mut().find(|(seen, _)| *seen == tag) {
                Some((_, count)) => *count += 1,
                None => named.push((tag, 1)),
            }
        }
    }
    let most = named
        .iter()
        .map(|(_, count)| *count)
        .max()
        .context("the melee controller names no response table")?;
    Ok(named
        .into_iter()
        .find(|(_, count)| *count == most)
        .map(|(tag, _)| tag)
        .unwrap_or_default())
}

/// The hit owner the controller's hit descriptions name and its two impact cues, in row order.
fn hits(sr: &mut Reader, controller: &Payload) -> Result<(u32, [u32; 2])> {
    let mut owners = BTreeSet::new();
    for at in (HIT_OWNER_BEFORE_TABLE..controller.0.len().saturating_sub(8)).step_by(8) {
        let Ok(table) = sr.ref64(controller, at) else {
            continue;
        };
        if table >> 24 != 0x80 || sr.reference(table).ok() != Some(RESPONSE_CLASS) {
            continue;
        }
        let owner = sr.ref64(controller, at - HIT_OWNER_BEFORE_TABLE)?;
        ensure!(
            sr.reference(owner)? == OWNER,
            "hit description at {at:#x} names no hit owner"
        );
        owners.insert(owner);
    }
    let [owner] = owners.iter().copied().collect::<Vec<_>>()[..] else {
        anyhow::bail!(
            "the melee controller names {} hit owners, not one",
            owners.len()
        );
    };
    let payload = sr.tag(owner, Some(OWNER))?;
    let mut cues = Vec::new();
    for at in (0..payload.0.len().saturating_sub(8)).step_by(8) {
        let Ok(sound) = sr.ref64(&payload, at) else {
            continue;
        };
        if sound >> 24 == 0x80 && sr.reference(sound).ok() == Some(SOUND_CLASS) {
            cues.push(sound);
        }
    }
    let [first, second] = cues[..] else {
        anyhow::bail!(
            "hit owner {owner:08X} names {} impact cues, not two",
            cues.len()
        );
    };
    Ok((owner, [first, second]))
}

/// The surface response table's cues with their media, by the surface rows they play for.
type SurfaceResponses = (BTreeMap<u32, Vec<String>>, Vec<Value>);

fn responses(sr: &mut Reader, table_tag: u32) -> Result<SurfaceResponses> {
    let table = sr.tag(table_tag, Some(RESPONSE_CLASS))?;
    let groups = table.array(0x80, 24, None)?;
    ensure!(
        groups.len() == 1,
        "the surface table has {} branches",
        groups.len()
    );
    let mut by_cue: BTreeMap<u32, Vec<String>> = BTreeMap::new();
    let mut embedded = Vec::new();
    for row in table.array(groups[0] + 8, 32, None)? {
        let name = format!("{:08X}", table.u32(row)?);
        ensure!(
            table.u32(row + 4)? == 0,
            "surface row name is wider than 32 bits"
        );
        let resources = table.array(row + 8, 32, None)?;
        ensure!(resources.len() <= 1, "surface row {name} has several cues");
        let Some(&at) = resources.first() else {
            continue;
        };
        let cue = sr.ref64(&table, at)?;
        if matches!(cue, 0 | u32::MAX) {
            continue;
        }
        match sound_assets(sr, cue, true) {
            Ok(sound) if sound["media"].as_array().is_some_and(|m| !m.is_empty()) => {
                by_cue.entry(cue).or_default().push(name);
            }
            Ok(_) => embedded.push(json!({"row":name,"cue":format!("{cue:08X}")})),
            Err(error) => embedded
                .push(json!({"row":name,"cue":format!("{cue:08X}"),"error":format!("{error:#}")})),
        }
    }
    Ok((by_cue, embedded))
}

pub(super) fn apply(inputs: &Inputs) -> Result<Value> {
    let graph_path = inputs.graph.join("asset-graph.json");
    let mut graph: Value = serde_json::from_slice(&fs::read(&graph_path)?)?;
    ensure!(
        graph["audio"]["authoring_schema"] == 3,
        "glaive melee sounds need the model's source bank routing"
    );
    for key in ["clip_events", "impact_events", "response_events"] {
        ensure!(
            graph["audio"].get(key).is_none(),
            "the model already carries {key}"
        );
    }
    let mut sr = Reader::discovery(inputs.modern, &inputs.work.join("source"), true)?;
    let mut nr = Reader::discovery(inputs.native, &inputs.work.join("native"), false)?;

    // Swings, through the source's own controller.
    let controller = controller(&mut sr, inputs.source_rig)?;
    let rows = swings(&mut sr, &mut nr, &controller, inputs.graph)?;
    let swing_count = rows.len();
    prepare_clip_events(
        inputs.modern,
        inputs.native,
        inputs.graph,
        &mut graph["audio"],
        rows,
    )?;
    // Hits, through a contact profile of the weapon's own.
    let (hit_owner, cues) = hits(&mut sr, &controller.1)?;
    let report = prepare_sounds(inputs.modern, inputs.native, inputs.graph, &cues)?;
    let mut events = report["effect_events"]
        .as_array()
        .cloned()
        .context("hit sound events")?;
    ensure!(events.len() == cues.len(), "hit sounds converted unevenly");
    for (event, (sound, mask)) in events.iter_mut().zip(cues.into_iter().zip(HIT_MASKS)) {
        ensure!(
            event["sounds"][0]["tag"] == format!("{sound:08X}"),
            "hit sound {sound:08X} converted out of order"
        );
        event["mask"] = json!(mask);
    }
    let audio = &mut graph["audio"];
    merge(&mut audio["converted_banks"], &report["converted_banks"])?;
    merge(&mut audio["transcoded_media"], &report["transcoded_media"])?;
    audio["impact_events"] = json!(events);
    let key = fnv1(&format!("{}.contact-feedback", inputs.namespace));
    audio["impact_key"] = json!(format!("{key:08X}"));

    // Surfaces. A cue the converter refuses keeps its rows stock and leaves no files.
    let table = response_table(&mut sr, &controller.1)?;
    let (by_cue, mut embedded) = responses(&mut sr, table)?;
    let audio_dir = inputs.graph.join("audio");
    let mut response_events = Vec::new();
    for (cue, names) in &by_cue {
        let existing = fs::read_dir(&audio_dir)?
            .map(|entry| Ok(entry?.file_name()))
            .collect::<Result<BTreeSet<_>>>()?;
        let report = match prepare_sounds(inputs.modern, inputs.native, inputs.graph, &[*cue]) {
            Ok(report) => report,
            Err(error) => {
                for entry in fs::read_dir(&audio_dir)? {
                    let entry = entry?;
                    if !existing.contains(&entry.file_name()) {
                        fs::remove_file(entry.path())?;
                    }
                }
                for name in names {
                    embedded.push(
                        json!({"row":name,"cue":format!("{cue:08X}"),"error":format!("{error:#}")}),
                    );
                }
                continue;
            }
        };
        let tag = format!("{cue:08X}");
        let sound = report["effect_events"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|event| event["sounds"].as_array().into_iter().flatten())
            .find(|sound| sound["tag"] == tag.as_str())
            .cloned()
            .with_context(|| format!("converted cue {tag} missing"))?;
        response_events.push(json!({"rows":names,"sounds":[sound]}));
        let audio = &mut graph["audio"];
        merge(&mut audio["converted_banks"], &report["converted_banks"])?;
        merge(&mut audio["transcoded_media"], &report["transcoded_media"])?;
    }
    ensure!(!response_events.is_empty(), "no surface cue converted");
    let audio = &mut graph["audio"];
    audio["response_events"] = json!(response_events);
    audio["response_source"] =
        json!({"table":format!("{table:08X}"),"embedded_media_rows":embedded});
    write_json(&graph_path, &graph)?;
    sr.finish()?;
    nr.finish()?;
    Ok(
        json!({"swings":swing_count,"controller":format!("{:08X}", controller.0),"impact_key":format!("{key:08X}"),
        "hit_owner":format!("{hit_owner:08X}"),"hit_cues":cues.map(|cue| format!("{cue:08X}")),"surface_table":format!("{table:08X}"),"surface_cues":response_events.len(),
        "stock_surface_rows":embedded.len()}),
    )
}

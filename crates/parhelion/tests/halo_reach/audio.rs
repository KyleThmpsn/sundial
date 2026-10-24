//! Follow the emitted firing cue into its actual private events and media.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn verify_events(bank: &Payload, start: u32, stop: u32, looping: bool) -> Result<usize> {
    let mut cursor = 0;
    let mut objects = BTreeMap::new();
    while cursor < bank.0.len() {
        let size = usize::try_from(bank.u32(cursor + 4)?)?;
        let end = cursor
            .checked_add(8 + size)
            .context("Bank chunk overflow")?;
        ensure!(end <= bank.0.len(), "Truncated bank chunk");
        if bank.0.get(cursor..cursor + 4) == Some(b"HIRC") {
            let mut row = cursor + 12;
            for _ in 0..bank.u32(cursor + 8)? {
                let kind = bank.0[row];
                let length = usize::try_from(bank.u32(row + 1)?)?;
                ensure!(
                    length >= 4 && row + 5 + length <= end,
                    "Truncated audio object"
                );
                ensure!(
                    objects
                        .insert(
                            bank.u32(row + 5)?,
                            (kind, Payload(bank.0[row + 5..row + 5 + length].to_vec()))
                        )
                        .is_none(),
                    "Duplicate private audio object"
                );
                row += 5 + length;
            }
            ensure!(row == end, "Audio object extent differs");
        }
        cursor = end;
    }
    let actions = |event: u32| -> Result<(BTreeSet<u32>, BTreeSet<u32>)> {
        let (kind, payload) = objects
            .get(&event)
            .context("Cue event is absent from its bank")?;
        ensure!(*kind == 4, "Cue selects a non-event object");
        let mut plays = BTreeSet::new();
        let mut stops = BTreeSet::new();
        let count = payload.u32(4)? as usize;
        ensure!(
            count > 0 && payload.0.len() == 8 + count * 4,
            "Empty or truncated event"
        );
        for i in 0..count {
            let (kind, action) = objects
                .get(&payload.u32(8 + i * 4)?)
                .context("Event action is absent")?;
            ensure!(*kind == 3, "Event selects a non-action object");
            let target = action.u32(6)?;
            ensure!(
                objects.contains_key(&target),
                "Action escapes its private bank"
            );
            match action.u16(4)? {
                0x0403 => {
                    plays.insert(target);
                }
                0x0103 => {
                    stops.insert(target);
                }
                _ => anyhow::bail!("Unexpected translated firing action"),
            }
        }
        Ok((plays, stops))
    };
    let (start_plays, start_stops) = actions(start)?;
    ensure!(!start_plays.is_empty(), "Firing event starts no sound");
    if looping {
        ensure!(stop != start, "Loop start and stop share an event");
        let (stop_plays, stop_stops) = actions(stop)?;
        ensure!(
            start_plays.is_subset(&stop_stops),
            "Release leaves a firing layer active"
        );
        ensure!(
            start_plays.is_subset(&start_stops) && stop_plays.is_subset(&start_stops),
            "Repeated fire leaves preceding loops or tails active"
        );
    } else {
        ensure!(
            matches!(stop, 0 | u32::MAX | 0x811c9dc5),
            "One-shot cue keeps a stop event from the donor bank"
        );
    }
    Ok(start_plays.len())
}

#[allow(clippy::cognitive_complexity)]
pub(super) fn verify(
    staged: &mut Reader,
    stock: &Reader,
    item: u32,
    identity: u32,
    directory: &Path,
    graph: &Value,
) -> Result<()> {
    // The witness comes from source recordings or a separately inspected prior import.
    // Requiring a route here catches a conversion that silently keeps donor audio.
    if let Some(path) = std::env::var_os("PARHELION_REACH_AUDIO_WITNESS") {
        let witness: Value = serde_json::from_slice(&fs::read(path)?)?;
        if let Some(expected) = witness.get(format!("{identity:08X}")) {
            ensure!(
                graph["audio"]["authoring_schema"] == 3,
                "Required source firing audio was not translated for {item:08X}: {}",
                graph["audio"]
            );
            ensure!(
                graph["audio"]["routing"] == expected["routing"],
                "Required firing route changed for {item:08X}"
            );
            let media = graph["audio"]["transcoded_media"]
                .as_array()
                .context("Required firing media")?
                .iter()
                .filter_map(|row| row["source_tag"].as_str())
                .collect::<BTreeSet<_>>();
            for id in expected["media"]
                .as_array()
                .context("Audio witness media")?
            {
                ensure!(
                    media.contains(id.as_str().context("Audio witness identity")?),
                    "Required source firing recording {id} disappeared for {item:08X}"
                );
            }
        }
    }
    if graph["audio"]["authoring_schema"] == 3 {
        let routed = parhelion_import::tiger::rig::inspect_with_audio(staged, item, false)?;
        let mut sounds = routed["audio"]["unnamed_groups"]
            .as_array()
            .context("Staged firing groups")?
            .iter()
            .flat_map(|g| g["sounds"].as_array().into_iter().flatten())
            .cloned()
            .collect::<Vec<_>>();
        sounds.extend(
            routed["audio"]["named_groups"]
                .as_array()
                .into_iter()
                .flatten()
                .flat_map(|g| g["events"].as_array().into_iter().flatten())
                .flat_map(|e| e["sounds"].as_array().into_iter().flatten())
                .cloned(),
        );
        let mut clip_sounds = BTreeSet::new();
        if let Some(events) = graph["audio"]["clip_events"].as_array() {
            let wanted = events
                .iter()
                .map(|e| {
                    Ok((
                        u32::from_str_radix(e["clip_name"].as_str().context("Clip name")?, 16)?,
                        usize::try_from(e["event_index"].as_u64().context("Clip event index")?)?,
                    ))
                })
                .collect::<Result<BTreeSet<_>>>()?;
            let mut reached = BTreeSet::new();
            for component in routed["components"]
                .as_array()
                .context("Staged components")?
            {
                if component["class"] != "8080344B" {
                    continue;
                }
                let lookup = staged.tag(
                    u32::from_str_radix(component["owner"].as_str().context("Lookup owner")?, 16)?,
                    None,
                )?;
                let bank = staged.tag(lookup.u32(lookup.pointer(24)? + 0x90)?, Some(0x808036f6))?;
                for row in bank.array(8, 4, Some(0x80808f48))? {
                    let tag = bank.u32(row)?;
                    if tag == u32::MAX {
                        continue;
                    }
                    let clip = staged.tag(tag, Some(0x80808f49))?;
                    let name = clip.u32(0x120)?;
                    for &(wanted_name, index) in wanted.iter().filter(|(n, _)| *n == name) {
                        let rows = clip.array(0x160, 8, Some(0x8080902b))?;
                        let at =
                            clip.pointer(*rows.get(index).context("Staged swing event absent")?)?;
                        ensure!(
                            clip.u32(at - 4)? == 0x80809033,
                            "Swing event is not a sound"
                        );
                        let sound = clip.u32(at + 24)?;
                        ensure!(
                            stock.reference(sound).is_err(),
                            "Swing still selects stock sound"
                        );
                        if clip_sounds.insert(sound) {
                            sounds.push(parhelion_import::tiger::audio::sound_assets(
                                staged, sound, false,
                            )?);
                        }
                        reached.insert((wanted_name, index));
                    }
                }
            }
            ensure!(
                reached == wanted,
                "A source swing cue is unreachable from the staged animation bank"
            );
        }
        let mut private = Vec::new();
        for sound in &sounds {
            let bank = u32::from_str_radix(sound["bank"].as_str().context("Firing bank")?, 16)?;
            if stock.reference(bank).is_err() {
                let cue = staged.tag(
                    u32::from_str_radix(sound["tag"].as_str().context("Sound cue")?, 16)?,
                    None,
                )?;
                let payload = staged.tag(bank, None)?;
                let plays = verify_events(
                    &payload,
                    cue.u32(8)?,
                    cue.u32(12)?,
                    graph["audio"]["routing"] == "source_loop"
                        && !clip_sounds.contains(&u32::from_str_radix(
                            sound["tag"].as_str().context("Sound tag")?,
                            16,
                        )?),
                )?;
                if graph["audio"]["routing"] == "source_one_shot"
                    && !clip_sounds.contains(&u32::from_str_radix(
                        sound["tag"].as_str().context("Sound tag")?,
                        16,
                    )?)
                {
                    let events = ["matched_events", "firing_events"]
                        .into_iter()
                        .flat_map(|name| graph["audio"][name].as_array().into_iter().flatten())
                        .flat_map(|event| event["sounds"].as_array().into_iter().flatten())
                        .collect::<Vec<_>>();
                    let [source] = events.as_slice() else {
                        anyhow::bail!("Expected one combined source firing event");
                    };
                    let layers = source["layers"]
                        .as_array()
                        .context("Source firing layers")?;
                    ensure!(
                        plays == layers.len(),
                        "Firing event does not start every source layer"
                    );
                }
                private.push(sound.clone());
            }
        }
        parhelion_import::io::write_json(
            &staged.output.join(format!("audio-{item:08X}.json")),
            &json!({"sounds":private,"expected":graph["audio"],"graph":directory}),
        )?;
        for media in graph["audio"]["transcoded_media"]
            .as_array()
            .context("Imported audio media")?
        {
            let expected = fs::read(directory.join(media["file"].as_str().context("Audio file")?))?;
            let mut found = false;
            for sound in &sounds {
                for tag in sound["media"].as_array().into_iter().flatten() {
                    let tag = u32::from_str_radix(tag.as_str().context("Staged audio tag")?, 16)?;
                    if stock.reference(tag).is_ok() {
                        continue;
                    }
                    found |= staged.tag(tag, None)?.0 == expected;
                }
            }
            ensure!(found, "Staged firing route does not reach source PCM");
        }
    }
    Ok(())
}

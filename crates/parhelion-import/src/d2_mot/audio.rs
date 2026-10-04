//! Resolve per-weapon audio routing from the pattern components in each game build.
//! Preserve event identities while lowering source banks and decoding their media.
use super::payload::Payload;
use super::reader::Reader;
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{collections::BTreeSet, fs, path::Path};
use tiger_pkg::TagHash;

pub mod bank;
pub mod cue;
pub mod legacy;
pub mod modern;
pub mod transcode;

fn tag(value: u32) -> String {
    format!("{value:08X}")
}

/// An unset reference: zero, all bits set, or the FNV-1 basis of an empty name.
fn unset(value: u32) -> bool {
    matches!(value, 0 | u32::MAX | 0x811C_9DC5)
}

/// Resolve a sound's event, bank and streamed media from checked native records.
pub fn sound_assets(reader: &mut Reader, sound_tag: u32, modern: bool) -> Result<Value> {
    let sound = reader.tag(
        sound_tag,
        Some(if modern { 0x80809738 } else { 0x80809802 }),
    )?;
    let bank = sound.u32(if modern { 0x18 } else { 0x14 })?;
    let bank_entry = reader
        .manager
        .get_entry(TagHash(bank))
        .context("missing sound bank")?;
    ensure!(
        bank_entry.file_type == 26,
        "sound bank is not a raw audio asset"
    );
    // A sound without the media array keeps its media inside the bank.
    if sound.0.len() == 0x30 {
        return Ok(
            json!({"tag":tag(sound_tag),"event_id":tag(sound.u32(8)?),"bank":tag(bank),"media":[],"media_ids":[],"embedded_media":true}),
        );
    }
    let count = usize::try_from(sound.u64(0x40)?)?;
    ensure!(
        count <= 1024 && 0x50 + count * 4 <= sound.0.len(),
        "invalid sound media array"
    );
    let mut media = Vec::new();
    let mut media_ids = Vec::new();
    for index in 0..count {
        let media_tag = sound.u32(0x50 + index * 4)?;
        let entry = reader
            .manager
            .get_entry(TagHash(media_tag))
            .context("missing sound media")?;
        ensure!(
            entry.file_type == 26,
            "sound media is not a raw audio asset"
        );
        media.push(tag(media_tag));
        media_ids.push(tag(entry.reference));
    }
    Ok(
        json!({"tag":tag(sound_tag),"event_id":tag(sound.u32(8)?),"bank":tag(bank),"media":media,"media_ids":media_ids}),
    )
}

/// Pair each source event with the donor event of the same hash.
///
/// Named events are keyed by the hash the animation fires, so the donor event with that
/// hash is the one the imported clip triggers, and its sound supplies the native bank,
/// routing and switch structure the source media is fitted into. A donor sound with the
/// same event path and media IDs is recorded as identical. Source sounds whose media
/// lives inside their bank cannot be converted yet and stay with the donor.
pub(crate) fn compatibility(source: &Value, native: &Value) -> Value {
    let source_events = source["named_groups"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|group| group["events"].as_array().into_iter().flatten());
    let native_events = native["named_groups"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|group| {
            group["events"]
                .as_array()
                .into_iter()
                .flatten()
                .map(move |event| (group, event))
        })
        .collect::<Vec<_>>();
    let has_media = |sound: &Value| {
        sound["media"]
            .as_array()
            .is_some_and(|media| !media.is_empty())
    };
    let mut matched = Vec::new();
    let mut missing = Vec::new();
    let mut seen = BTreeSet::new();
    for event in source_events {
        // Several source groups can list the same event, and the first one plays.
        if !seen.insert(event["hash"].to_string()) {
            continue;
        }
        let sounds = event["sounds"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|sound| has_media(sound))
            .cloned()
            .collect::<Vec<_>>();
        let donor = native_events.iter().find(|(_, candidate)| {
            candidate["hash"] == event["hash"]
                && candidate["sounds"]
                    .as_array()
                    .is_some_and(|rows| rows.iter().any(has_media))
        });
        let mut entry = json!({"hash":event["hash"],"sounds":sounds});
        match donor {
            Some((group, candidate)) if !sounds.is_empty() => {
                let identical = sounds.iter().all(|sound| {
                    candidate["sounds"].as_array().is_some_and(|rows| {
                        rows.iter().any(|row| {
                            row["event_path"] == sound["event_path"]
                                && row["media_ids"] == sound["media_ids"]
                        })
                    })
                });
                entry["native_group"] =
                    json!({"owner":group["owner"],"parent":group["parent"],"group":group["group"]});
                entry["native_sounds"] = json!(
                    candidate["sounds"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter(|row| has_media(row))
                        .collect::<Vec<_>>()
                );
                entry["mapping"] = json!(if identical { "identical" } else { "event_hash" });
                matched.push(entry)
            }
            _ => {
                entry["sounds"] = event["sounds"].clone();
                missing.push(entry)
            }
        }
    }
    let status = if matched.is_empty() {
        "native_donor_only"
    } else if missing.is_empty() {
        "source_named_events"
    } else {
        "partial_source_named_events"
    };
    json!({"status":status,"matched_events":matched,"unmatched_events":missing,"source_media_imported":false,"gameplay_verified":false})
}

fn firing_pairs(source: &Value, native: &Value) -> Value {
    let mut pairs = Vec::new();
    for source_group in source["unnamed_groups"].as_array().into_iter().flatten() {
        for native_group in native["unnamed_groups"].as_array().into_iter().flatten() {
            let sources = source_group["sounds"]
                .as_array()
                .into_iter()
                .flatten()
                .collect::<Vec<_>>();
            let natives = native_group["sounds"]
                .as_array()
                .into_iter()
                .flatten()
                .collect::<Vec<_>>();
            let mut used = BTreeSet::new();
            for native_sound in &natives {
                // First preserve identical global cues. The sole remaining weapon cue
                // in a corresponding presentation slot can then be paired structurally.
                if let Some(source_sound) = sources.iter().find(|sound| {
                    sound["event_id"] == native_sound["event_id"]
                        && sound["root_slot"] == native_sound["root_slot"]
                }) {
                    used.insert(source_sound["tag"].to_string());
                    pairs.push(json!({"sounds":[source_sound],"native_sounds":[native_sound],"native_presentation":native_group,"mapping":"event_id"}));
                }
            }
            for native_sound in &natives {
                if pairs.iter().any(|pair| {
                    pair["native_sounds"][0]["component_owner"] == native_sound["component_owner"]
                        && pair["native_sounds"][0]["offset"] == native_sound["offset"]
                }) {
                    continue;
                }
                let Some(name) = native_sound["node_name"]
                    .as_str()
                    .filter(|name| !["00000000", "FFFFFFFF", "811C9DC5"].contains(name))
                else {
                    continue;
                };
                let candidates = sources
                    .iter()
                    .filter(|sound| {
                        (sound["node_name"] == name
                            || (native_sound["node_name"] == native_sound["event_id"]
                                && sound["node_name"] == "811C9DC5"))
                            && sound["root_slot"] == native_sound["root_slot"]
                            && sound["node_kind"] == native_sound["node_kind"]
                    })
                    .collect::<Vec<_>>();
                if !candidates.is_empty()
                    && candidates.iter().all(|sound| {
                        sound["bank"] == candidates[0]["bank"]
                            && sound["event_id"] == candidates[0]["event_id"]
                    })
                {
                    used.insert(candidates[0]["tag"].to_string());
                    pairs.push(json!({"sounds":[candidates[0]],"native_sounds":[native_sound],"native_presentation":native_group,"mapping":"effect_node_name"}));
                }
            }
            for native_sound in &natives {
                if pairs.iter().any(|pair| {
                    pair["native_sounds"][0]["component_owner"] == native_sound["component_owner"]
                        && pair["native_sounds"][0]["offset"] == native_sound["offset"]
                }) {
                    continue;
                }
                let candidates = sources
                    .iter()
                    .filter(|sound| {
                        !used.contains(&sound["tag"].to_string())
                            && sound["root_slot"] == native_sound["root_slot"]
                            && sound["component_class"] == "80808179"
                            && native_sound["component_class"] == "808084E9"
                    })
                    .collect::<Vec<_>>();
                let remaining = natives
                    .iter()
                    .filter(|sound| {
                        sound["root_slot"] == native_sound["root_slot"]
                            && sound["component_class"] == native_sound["component_class"]
                            && !pairs.iter().any(|pair| {
                                pair["native_sounds"][0]["component_owner"]
                                    == sound["component_owner"]
                                    && pair["native_sounds"][0]["offset"] == sound["offset"]
                            })
                    })
                    .count();
                if candidates.len() == 1 && remaining == 1 {
                    used.insert(candidates[0]["tag"].to_string());
                    pairs.push(json!({"sounds":[candidates[0]],"native_sounds":[native_sound],"native_presentation":native_group,"mapping":"unique_presentation_slot"}));
                }
            }
        }
    }
    json!(pairs)
}

/// Prepare named source events through the selected native attachment, including
/// its embedded default. Conversion failures remain explicit in the graph.
pub fn prepare(
    modern: &Path,
    native: &Path,
    graph: &Path,
    source_rig: &Value,
    native_rig: &Value,
) -> Value {
    let mut audio = compatibility(&source_rig["audio"], &native_rig["audio"]);
    let pairs = firing_pairs(&source_rig["audio"], &native_rig["audio"]);
    let pairs = pairs.as_array().expect("firing pairs are an array");
    let identical = |event: &&Value| {
        event["sounds"][0]["event_id"] == event["native_sounds"][0]["event_id"]
            && event["sounds"][0]["media_ids"] == event["native_sounds"][0]["media_ids"]
    };
    // Shared global cues already play the same media identities. Preserve
    // their routing and reserve private banks for changed source cues.
    audio["firing_events"] = json!(
        pairs
            .iter()
            .filter(|event| !identical(event))
            .collect::<Vec<_>>()
    );
    audio["preserved_firing_events"] = json!(pairs.iter().filter(identical).collect::<Vec<_>>());
    audio["unmatched_firing_events"] = json!(
        source_rig["audio"]["unnamed_groups"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|group| group["sounds"].as_array().into_iter().flatten())
            .filter(|sound| !pairs
                .iter()
                .any(
                    |event| event["sounds"][0]["component_owner"] == sound["component_owner"]
                        && event["sounds"][0]["offset"] == sound["offset"]
                ))
            .collect::<Vec<_>>()
    );
    audio["authoring_schema"] = json!(3);
    audio["runtime_entity"] = native_rig["runtime_entity"].clone();
    if let Err(error) = prepare_media(modern, native, graph, &mut audio) {
        audio["conversion_errors"] = json!([format!("{error:#}")]);
    }
    audio
}

/// Keep converted banks and media beside the graph, pinned by GraphReference.
/// Presentation cues require an unambiguous native trigger correspondence.
pub(crate) fn prepare_media(
    modern: &Path,
    native: &Path,
    graph: &Path,
    report: &mut Value,
) -> Result<()> {
    let mut tags = BTreeSet::new();
    let events = report["matched_events"]
        .as_array()
        .into_iter()
        .flatten()
        .chain(report["firing_events"].as_array().into_iter().flatten())
        .chain(report["clip_events"].as_array().into_iter().flatten())
        .chain(report["effect_events"].as_array().into_iter().flatten())
        .collect::<Vec<_>>();
    let mut banks = BTreeSet::new();
    for event in &events {
        for sound in event["sounds"].as_array().into_iter().flatten() {
            banks.insert(u32::from_str_radix(
                sound["bank"].as_str().context("source bank tag")?,
                16,
            )?);
            for media in sound["media"].as_array().context("source sound media")? {
                tags.insert(u32::from_str_radix(
                    media.as_str().context("audio media tag")?,
                    16,
                )?);
            }
        }
    }
    if banks.is_empty() {
        return Ok(());
    }
    let mut reader = Reader::discovery(modern, &graph.join("audio-reader"), true)?;
    let mut native_reader = Reader::discovery(native, &graph.join("audio-native-reader"), false)?;
    let source_settings = init_settings(&mut reader, 150)?;
    let native_settings = init_settings(&mut native_reader, 113)?;
    let directory = graph.join("audio");
    fs::create_dir_all(&directory)?;
    let mut converted = Vec::new();
    let mut failures = Vec::new();
    let mut bank_fallbacks = Vec::new();
    let firing_banks = report["firing_events"]
        .as_array()
        .into_iter()
        .flatten()
        .chain(report["clip_events"].as_array().into_iter().flatten())
        .chain(report["effect_events"].as_array().into_iter().flatten())
        .flat_map(|event| event["sounds"].as_array().into_iter().flatten())
        .filter_map(|sound| sound["bank"].as_str())
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    let mut converted_banks = Vec::new();
    for tag in banks {
        let result = (|| -> Result<()> {
            let source = reader.tag(tag, None)?;
            let bank = bank::lower_with_settings(&source.0, &source_settings, &native_settings)?;
            let file = format!("audio/{tag:08X}.bank.json");
            fs::write(graph.join(&file), serde_json::to_vec(&bank)?)?;
            converted_banks.push(json!({"source_tag":format!("{tag:08X}"),"file":file,"version":113,"routing":"source"}));
            Ok(())
        })();
        if let Err(error) = result {
            if firing_banks.contains(&format!("{tag:08X}")) {
                failures.push(format!("firing bank {tag:08X}: {error:#}"));
            } else {
                bank_fallbacks.push(json!({"source_tag":format!("{tag:08X}"),"routing":"source_media_native_bank","reason":format!("{error:#}")}));
            }
        }
    }
    for tag in tags {
        let result = (|| -> Result<()> {
            let entry = reader
                .manager
                .get_entry(TagHash(tag))
                .context("modern audio media entry missing")?;
            ensure!(
                entry.file_type == 26,
                "modern audio media is not a raw asset"
            );
            let source = reader.tag(tag, None)?;
            let pcm = transcode::pcm_wem(&source.0)?;
            let file = format!("audio/{tag:08X}.pcm.wem");
            fs::write(graph.join(&file), &pcm)?;
            converted.push(json!({"source_tag":format!("{tag:08X}"),"media_id":format!("{:08X}",entry.reference),"file":file,"codec":"pcm_s16le"}));
            Ok(())
        })();
        if let Err(error) = result {
            failures.push(format!("{tag:08X}: {error:#}"))
        }
    }
    report["transcoded_media"] = json!(converted);
    report["converted_banks"] = json!(converted_banks);
    report["bank_fallbacks"] = json!(bank_fallbacks);
    if !failures.is_empty() {
        report["conversion_errors"] = json!(failures)
    }
    Ok(())
}

/// Add explicitly resolved source animation sounds and rebuild the graph's
/// complete media closure. The caller supplies the verified clip event index
/// and its native sound template. Emission validates those links again.
pub fn prepare_clip_events(
    modern: &Path,
    native: &Path,
    graph: &Path,
    report: &mut Value,
    events: Vec<Value>,
) -> Result<()> {
    ensure!(
        report["authoring_schema"] == 3,
        "clip audio needs source bank routing"
    );
    report["clip_events"] = json!(events);
    report
        .as_object_mut()
        .context("audio report")?
        .remove("conversion_errors");
    prepare_media(modern, native, graph, report)?;
    ensure!(
        report.get("conversion_errors").is_none(),
        "clip audio conversion failed: {}",
        report["conversion_errors"]
    );
    Ok(())
}

/// Translate a runtime effect's complete set of sound cues without matching a
/// donor event. Keep event IDs and bank relationships for the sequence compiler.
/// The caller must link the resulting cues to translated runtime triggers.
pub fn prepare_sounds(modern: &Path, native: &Path, graph: &Path, sounds: &[u32]) -> Result<Value> {
    ensure!(!sounds.is_empty(), "effect has no sound cues");
    let mut reader = Reader::discovery(modern, &graph.join("effect-sound-reader"), true)?;
    let mut cues = Vec::new();
    for sound in sounds.iter().copied().collect::<BTreeSet<_>>() {
        cues.push(json!({"sounds":[sound_assets(&mut reader, sound, true)?]}));
    }
    let mut report = json!({"authoring_schema":3,"effect_events":cues});
    prepare_media(modern, native, graph, &mut report)?;
    ensure!(
        report.get("conversion_errors").is_none(),
        "effect audio conversion failed: {}",
        report["conversion_errors"]
    );
    ensure!(
        report["bank_fallbacks"]
            .as_array()
            .is_none_or(Vec::is_empty),
        "effect audio cannot fall back to donor banks"
    );
    Ok(report)
}

fn init_settings(reader: &mut Reader, version: u32) -> Result<bank::settings::Settings> {
    let candidates = reader
        .manager
        .get_all_by_reference(bank::hash("init"))
        .iter()
        .filter(|(_, entry)| entry.file_type == 26 && matches!(entry.file_subtype, 4 | 5))
        .map(|(tag, _)| tag.0)
        .collect::<Vec<_>>();
    let mut selected = None;
    let mut rejected = Vec::new();
    for tag in candidates {
        let bytes = reader.tag(tag, None)?;
        match bank::settings::Settings::read(&bytes.0, version) {
            Ok(settings) => {
                ensure!(
                    selected.as_ref().is_none_or(|s| s == &settings),
                    "Wwise Init {version} settings disagree at {tag:08X}"
                );
                selected = Some(settings);
            }
            Err(error) => rejected.push(format!("{tag:08X}: {error:#}")),
        }
    }
    selected.with_context(|| {
        format!(
            "no validated Wwise Init {version} settings: {}",
            rejected.join(", ")
        )
    })
}

fn sound_group(reader: &mut Reader, parent_tag: u32, modern: bool) -> Result<Value> {
    let parent = reader.tag(
        parent_tag,
        Some(if modern { 0x80806FA3 } else { 0x8080744A }),
    )?;
    let group_tag = parent.u32(if modern { 8 } else { 16 })?;
    let group = reader.tag(
        group_tag,
        Some(if modern { 0x80808C0D } else { 0x8080902D }),
    )?;
    let mut events = Vec::new();
    let mut errors = Vec::new();
    for row in group.array(8, 0x18, Some(if modern { 0x80808C0F } else { 0x8080902F }))? {
        let mut sounds = Vec::new();
        for sound in group.array(
            row + 8,
            0x28,
            Some(if modern { 0x80808C13 } else { 0x80809033 }),
        )? {
            let sound_tag = if modern {
                reader.ref64(&group, sound + 0x18)?
            } else {
                group.u32(sound + 0x18)?
            };
            if unset(sound_tag) {
                continue;
            }
            let asset = (|| -> Result<Value> {
                ensure!(
                    reader.reference(sound_tag)? == if modern { 0x80809738 } else { 0x80809802 },
                    "unexpected Wwise sound class for {sound_tag:08X}"
                );
                let at = group.pointer(sound + 0x10)?;
                let path = group
                    .0
                    .get(at..)
                    .context("sound path outside group")?
                    .split(|byte| *byte == 0)
                    .next()
                    .context("missing sound path")?;
                let mut asset = sound_assets(reader, sound_tag, modern)?;
                asset["event_path"] = json!(String::from_utf8_lossy(path).as_ref());
                Ok(asset)
            })();
            match asset {
                Ok(asset) => sounds.push(asset),
                Err(error) => errors.push(format!("{sound_tag:08X}: {error:#}")),
            }
        }
        if !sounds.is_empty() {
            events.push(json!({"hash":tag(group.u32(row)?),"sounds":sounds}));
        }
    }
    Ok(
        json!({"parent":tag(parent_tag),"group":tag(group_tag),"events":events,"sound_errors":errors}),
    )
}

fn named(reader: &mut Reader, owner_tag: u32, content: u32, modern: bool) -> Result<Vec<Value>> {
    let owner = reader.tag(
        owner_tag,
        Some(if modern { 0x80809B06 } else { 0x80809C36 }),
    )?;
    let resource = owner.pointer(24)?;
    ensure!(
        owner.u32(resource - 4)? == if modern { 0x8080356E } else { 0x80804221 },
        "unexpected named audio component"
    );
    let parent_offset = if modern { 0xD0 } else { 0x98 };
    let mut groups = Vec::new();
    for row in super::rig::attachment_rows(&owner, resource, content, modern)? {
        let parent = if modern {
            reader.ref64(&owner, row + parent_offset)?
        } else {
            owner.u32(row + parent_offset)?
        };
        if !unset(parent) {
            let mut group = sound_group(reader, parent, modern).unwrap_or_else(|error| {
                json!({"parent":tag(parent),"events":[],"sound_errors":[format!("{error:#}")]})
            });
            group["owner"] = json!(tag(owner_tag));
            groups.push(group);
        }
    }
    Ok(groups)
}

/// Record the once or loop node whose reference word sits at `offset`.
fn node_name(sound: &mut Value, owner: &Payload, offset: usize, modern: bool) -> Result<()> {
    let delta = if modern { 84 } else { 60 };
    if offset >= delta {
        let class = owner.u32(offset - delta)?;
        let kind = match (modern, class) {
            (true, 0x80806640) | (false, 0x80806B37) => Some("once"),
            (true, 0x8080663E) | (false, 0x80806B35) => Some("loop"),
            _ => None,
        };
        if let Some(kind) = kind {
            // Native nodes carry an owner word and an input
            // pointer before the name. Modern nodes inline
            // that name immediately after the class word.
            sound["node_name"] =
                json!(tag(owner.u32(offset - delta + if modern { 4 } else { 12 })?));
            sound["node_kind"] = json!(kind);
        }
    }
    Ok(())
}

fn direct_sounds(
    reader: &mut Reader,
    entities: &[String],
    modern: bool,
) -> Result<(Vec<Value>, Vec<String>)> {
    let mut seen = BTreeSet::new();
    let mut sounds = Vec::new();
    let mut errors = Vec::new();
    for entity in entities {
        let entity_tag = u32::from_str_radix(entity, 16)?;
        let rows = reader
            .tag(
                entity_tag,
                Some(if modern { 0x80809AD8 } else { 0x80809C0F }),
            )
            .and_then(|payload| {
                Ok((
                    payload.array(if modern { 8 } else { 16 }, 12, None)?,
                    payload,
                ))
            });
        let (rows, payload) = match rows {
            Ok(found) => found,
            Err(error) => {
                errors.push(format!("{entity}: {error:#}"));
                continue;
            }
        };
        for row in rows {
            let owner_tag = payload.u32(row)?;
            if unset(owner_tag) {
                continue;
            }
            let Ok(owner) = reader.tag(
                owner_tag,
                Some(if modern { 0x80809B06 } else { 0x80809C36 }),
            ) else {
                continue;
            };
            for offset in (0..owner.0.len().saturating_sub(3)).step_by(4) {
                let word = owner.u32(offset)?;
                let sound_tag = if modern && word == u32::MAX {
                    reader.ref64(&owner, offset).unwrap_or(word)
                } else {
                    word
                };
                if !seen.contains(&(owner_tag, offset))
                    && reader.reference(sound_tag).ok()
                        == Some(if modern { 0x80809738 } else { 0x80809802 })
                {
                    match sound_assets(reader, sound_tag, modern) {
                        Ok(mut sound) => {
                            sound["component_owner"] = json!(tag(owner_tag));
                            sound["offset"] = json!(offset);
                            sound["entity"] = json!(entity);
                            let resource = owner.pointer(24)?;
                            sound["component_class"] = json!(tag(owner.u32(resource - 4)?));
                            node_name(&mut sound, &owner, offset, modern)?;
                            sounds.push(sound);
                        }
                        Err(error) => errors.push(format!("{sound_tag:08X}: {error:#}")),
                    }
                    seen.insert((owner_tag, offset));
                }
            }
        }
    }
    Ok((sounds, errors))
}

/// The pattern entities in a root's six slots, with the slot each occupies.
fn root_slots(reader: &Reader, root: &Payload, modern: bool) -> Result<(Vec<String>, Vec<Value>)> {
    let mut entities = Vec::new();
    let mut slots = Vec::new();
    for slot in 0..6 {
        let at = 0x18 + slot * if modern { 0x18 } else { 0x10 };
        let entity = if modern {
            reader.ref64(root, at)?
        } else {
            root.u32(at)?
        };
        if !unset(entity)
            && reader.reference(entity).ok() == Some(if modern { 0x80809AD8 } else { 0x80809C0F })
        {
            entities.push(tag(entity));
            slots.push(json!({"slot":slot,"entity":tag(entity)}));
        }
    }
    Ok((entities, slots))
}

fn unnamed(reader: &mut Reader, owner_tag: u32, content: u32, modern: bool) -> Result<Vec<Value>> {
    let owner = reader.tag(
        owner_tag,
        Some(if modern { 0x80809B06 } else { 0x80809C36 }),
    )?;
    let resource = owner.pointer(24)?;
    ensure!(
        owner.u32(resource - 4)? == if modern { 0x80802CF4 } else { 0x80803AC9 },
        "unexpected unnamed audio component"
    );
    let (offset, stride, class) = if modern {
        (0x318, 0x2A8, 0x8080B773)
    } else {
        (0x240, 0x1C0, 0x80803ACF)
    };
    let mut groups = Vec::new();
    let variants = owner.array(resource + offset, stride, Some(class))?;
    let selected = variants
        .into_iter()
        .filter(|&row| owner.u32(row + 0x10).ok() == Some(content))
        .collect::<Vec<_>>();
    ensure!(
        selected.len() <= 1,
        "unnamed audio content variant is ambiguous"
    );
    let row = selected
        .first()
        .copied()
        .unwrap_or(resource + if modern { 0x70 } else { 0x80 });
    let parent_tag = if modern {
        reader.ref64(&owner, row + 0x120)?
    } else {
        owner.u32(row + 0xC0)?
    };
    if unset(parent_tag) {
        return Ok(groups);
    }
    let parent = reader.tag(
        parent_tag,
        Some(if modern { 0x80806FA3 } else { 0x8080744A }),
    )?;
    let root_tag = if modern {
        reader.ref64(&parent, 8)?
    } else {
        parent.u32(16)?
    };
    if unset(root_tag) {
        return Ok(groups);
    }
    let root = reader.tag(root_tag, Some(if modern { 0x80802D09 } else { 0x80803ADC }))?;
    let (entities, slots) = root_slots(reader, &root, modern)?;
    let (mut sounds, sound_errors) = direct_sounds(reader, &entities, modern)?;
    for sound in &mut sounds {
        let placement = slots
            .iter()
            .find(|slot| slot["entity"] == sound["entity"])
            .context("sound entity slot")?;
        sound["root_slot"] = placement["slot"].clone();
    }
    groups.push(json!({"owner":tag(owner_tag),"content_key":tag(content),"row":row,"parent_offset":row+if modern {0x120} else {0xC0},"parent":tag(parent_tag),"root":tag(root_tag),"slots":slots,"entities":entities,"sounds":sounds,"sound_errors":sound_errors}));
    Ok(groups)
}

pub(crate) fn inspect(
    reader: &mut Reader,
    components: &[Value],
    content: u32,
    modern: bool,
) -> Result<Value> {
    let mut named_groups = Vec::new();
    let mut unnamed_groups = Vec::new();
    for component in components {
        let class = component["class"].as_str().context("component class")?;
        let owner =
            u32::from_str_radix(component["owner"].as_str().context("component owner")?, 16)?;
        if class == if modern { "8080356E" } else { "80804221" } {
            named_groups.extend(named(reader, owner, content, modern)?);
        } else if class == if modern { "80802CF4" } else { "80803AC9" } {
            unnamed_groups.extend(unnamed(reader, owner, content, modern)?);
        }
    }
    Ok(
        json!({"content_key":tag(content),"named_groups":named_groups,"unnamed_groups":unnamed_groups,"source_media_imported":false}),
    )
}

/// Discover audio only for the chosen source and donor. Donor matching calls
/// `rig::inspect` for many candidates and must not pay this cost for each one.
pub(crate) fn add_to_rig(reader: &mut Reader, rig: &mut Value, modern: bool) -> Result<()> {
    let content = u32::from_str_radix(rig["content_key"].as_str().context("rig content key")?, 16)?;
    let components = rig["components"].as_array().context("rig components")?;
    let audio = inspect(reader, components, content, modern).unwrap_or_else(|error| {
        json!({"status":"unavailable","reason":format!("{error:#}"),"source_media_imported":false})
    });
    rig["audio"] = audio;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn events_pair_by_hash_and_record_identical_sounds() {
        let sound = json!({"event_path":"auto/ready.wwise_event","media":["80000001"],"media_ids":["12345678"]});
        let source = json!({"named_groups":[{"events":[{"hash":"AAAAAAAA","sounds":[sound]}]}]});
        let same = source.clone();
        let report = compatibility(&source, &same);
        assert_eq!(report["status"], "source_named_events");
        assert_eq!(report["matched_events"][0]["mapping"], "identical");

        let mut different = same.clone();
        different["named_groups"][0]["events"][0]["sounds"][0]["media_ids"][0] = json!("87654321");
        let report = compatibility(&source, &different);
        assert_eq!(report["matched_events"][0]["mapping"], "event_hash");

        let mut other = same.clone();
        other["named_groups"][0]["events"][0]["hash"] = json!("BBBBBBBB");
        assert_eq!(
            compatibility(&source, &other)["status"],
            "native_donor_only"
        );

        let mut embedded = source.clone();
        embedded["named_groups"][0]["events"][0]["sounds"][0]["media"] = json!([]);
        let report = compatibility(&embedded, &same);
        assert_eq!(report["status"], "native_donor_only", "{report}");
    }

    #[test]
    #[ignore = "Requires configured source and native package paths and item tags"]
    fn configured_weapon_audio_reaches_sound_media() {
        for (prefix, modern) in [("MODERN", true), ("NATIVE", false)] {
            let packages = std::env::var_os(format!("PARHELION_AUDIO_{prefix}_PACKAGES"))
                .expect("configured audio package path");
            let item = std::env::var(format!("PARHELION_AUDIO_{prefix}_ITEM"))
                .expect("configured audio item tag");
            let item = u32::from_str_radix(&item, 16).unwrap();
            let scratch = tempfile::tempdir().unwrap();
            let mut reader =
                Reader::discovery(Path::new(&packages), scratch.path(), modern).unwrap();
            let mut rig = super::super::rig::inspect(&mut reader, item, modern).unwrap();
            add_to_rig(&mut reader, &mut rig, modern).unwrap();
            let groups = rig["audio"]["named_groups"].as_array().unwrap();
            assert!(!groups.is_empty(), "{prefix} has no named audio group");
            let sounds = groups
                .iter()
                .flat_map(|group| {
                    group["events"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .flat_map(|event| event["sounds"].as_array().unwrap().iter())
                })
                .collect::<Vec<_>>();
            assert!(!sounds.is_empty(), "{prefix} has no mapped sound events");
            assert!(sounds.iter().all(|sound| {
                sound["bank"].as_str().is_some()
                    && sound["media"]
                        .as_array()
                        .is_some_and(|media| !media.is_empty())
            }));
        }
    }

    #[test]
    #[ignore = "Requires configured source and native packages and item tags"]
    fn configured_source_audio_converts_named_media() {
        let modern = std::env::var_os("PARHELION_AUDIO_MODERN_PACKAGES").expect("modern packages");
        let native = std::env::var_os("PARHELION_AUDIO_NATIVE_PACKAGES").expect("native packages");
        let source_item = u32::from_str_radix(
            &std::env::var("PARHELION_AUDIO_MODERN_ITEM").expect("modern item"),
            16,
        )
        .unwrap();
        let native_item = u32::from_str_radix(
            &std::env::var("PARHELION_AUDIO_NATIVE_ITEM").expect("native item"),
            16,
        )
        .unwrap();
        let scratch = tempfile::tempdir().unwrap();
        let mut source = Reader::discovery(Path::new(&modern), scratch.path(), true).unwrap();
        let mut native_reader =
            Reader::discovery(Path::new(&native), scratch.path(), false).unwrap();
        let source_rig =
            super::super::rig::inspect_with_audio(&mut source, source_item, true).unwrap();
        let native_rig =
            super::super::rig::inspect_with_audio(&mut native_reader, native_item, false).unwrap();
        assert!(
            source_rig["audio"]["status"].is_null(),
            "{}",
            source_rig["audio"]
        );
        assert!(
            native_rig["audio"]["status"].is_null(),
            "{}",
            native_rig["audio"]
        );
        let mut report = compatibility(&source_rig["audio"], &native_rig["audio"]);
        let output = std::env::var_os("PARHELION_AUDIO_OUTPUT").map(std::path::PathBuf::from);
        let graph = output.as_deref().unwrap_or(scratch.path());
        fs::create_dir_all(graph).unwrap();
        prepare_media(Path::new(&modern), Path::new(&native), graph, &mut report).unwrap();
        assert!(report["conversion_errors"].is_null(), "{report}");
        assert!(!report["transcoded_media"].as_array().unwrap().is_empty());
        if output.is_some() {
            super::super::reader::write_json(&graph.join("audio-report.json"), &report).unwrap();
            super::super::reader::write_json(
                &graph.join("source-audio.json"),
                &source_rig["audio"],
            )
            .unwrap();
            super::super::reader::write_json(
                &graph.join("native-audio.json"),
                &native_rig["audio"],
            )
            .unwrap();
        }
    }
}

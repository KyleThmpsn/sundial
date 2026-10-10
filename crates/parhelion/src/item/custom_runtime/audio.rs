//! Author modern PCM media into private Dawn Wwise banks and runtime sound
//! components. Current imports retain source bank routing, actions and variation
//! weights. Earlier graphs retain their explicitly recorded native bank fallback.
use super::imports::Inputs;
use super::*;
use crate::shared_tag_dependency_index::{
    dependency_entries,
    scoped::{LoadingOwner, clone_scoped_dependencies},
};
use parhelion_import::d2_mot::payload::Payload;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
#[cfg(test)]
use std::fs;

const OWNER_CLASS: u32 = 0x8080_9C36;
const PARENT_CLASS: u32 = 0x8080_744A;
const GROUP_CLASS: u32 = 0x8080_902D;
const SOUND_CLASS: u32 = 0x8080_9802;
mod clips;
mod firing;
mod impacts;
pub(in crate::item) use impacts::author as author_impacts;

pub(in crate::item) struct ImportedAudio {
    pub runtime_entity: u32,
    item: u32,
    events: Vec<Value>,
    firing: Vec<Value>,
    clips: Vec<Value>,
    impacts: Vec<Value>,
    impact_key: Option<u32>,
    /// Per-surface melee impact sounds, by the native response row each replaces.
    responses: Vec<Value>,
    media: BTreeMap<String, Vec<u8>>,
    banks: BTreeMap<String, parhelion_import::d2_mot::ConvertedAudioBank>,
}

impl ImportedAudio {
    /// Reusable gear graphs keep their source identity on disk. Audio event names must
    /// use the authored copy's identity when a recipe is renamed or duplicated.
    pub(in crate::item) fn set_item(&mut self, item: u32) {
        self.item = item;
    }

    /// Upper bounds of the payloads `author` places in the weapon's asset group: the PCM
    /// media of every event, and a bank and sound for each sound, which fit one block.
    pub(in crate::item) fn asset_bounds(&self) -> Vec<usize> {
        let mut bounds = Vec::new();
        for sound in self
            .events
            .iter()
            .chain(&self.firing)
            .chain(&self.clips)
            .chain(&self.impacts)
            .chain(&self.responses)
            .flat_map(|event| event["sounds"].as_array().into_iter().flatten())
        {
            for media in sound["media"].as_array().into_iter().flatten() {
                if let Some(pcm) = media.as_str().and_then(|tag| self.media.get(tag)) {
                    bounds.push(pcm.len());
                }
            }
            bounds.push(
                sound["bank"]
                    .as_str()
                    .and_then(|tag| self.banks.get(tag))
                    .map_or(crate::format::BLOCK_SIZE, |bank| bank.bytes.len()),
            );
            bounds.push(crate::format::BLOCK_SIZE);
        }
        if !self.impacts.is_empty() {
            bounds.push(crate::format::BLOCK_SIZE);
        }
        if !self.responses.is_empty() {
            bounds.push(crate::format::BLOCK_SIZE);
        }
        bounds
    }
}

fn hex(value: &Value, label: &str) -> AuthoringResult<u32> {
    let text = value
        .as_str()
        .ok_or_else(|| invalid(format!("Imported audio lacks {label}")))?;
    u32::from_str_radix(text, 16)
        .map_err(|_| invalid(format!("Imported audio {label} is not a tag")))
}

fn word(bytes: &[u8], at: usize) -> AuthoringResult<u32> {
    let slice = bytes
        .get(at..at + 4)
        .ok_or_else(|| invalid("Imported audio payload is truncated"))?;
    Ok(u32::from_le_bytes(slice.try_into().unwrap()))
}

fn put(bytes: &mut [u8], at: usize, value: u32) -> AuthoringResult<()> {
    let slot = bytes
        .get_mut(at..at + 4)
        .ok_or_else(|| invalid("Imported audio patch exceeds payload"))?;
    slot.copy_from_slice(&value.to_le_bytes());
    Ok(())
}

fn fnv1(name: &str) -> u32 {
    name.bytes()
        .map(|byte| byte.to_ascii_lowercase())
        .fold(0x811C_9DC5u32, |hash, byte| {
            hash.wrapping_mul(16_777_619) ^ u32::from(byte)
        })
}

pub(in crate::item) fn load(graph: &Inputs) -> AuthoringResult<Option<ImportedAudio>> {
    let graph_value = graph.value();
    let audio = &graph_value["audio"];
    if !matches!(audio["authoring_schema"].as_u64(), Some(1..=3)) {
        return Ok(None);
    }
    if audio["conversion_errors"]
        .as_array()
        .is_some_and(|errors| !errors.is_empty())
    {
        return Err(invalid(format!(
            "Imported audio conversion failed: {}",
            audio["conversion_errors"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    let events = audio["matched_events"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let firing = audio["firing_events"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let clips = audio["clip_events"].as_array().cloned().unwrap_or_default();
    let impacts = audio["impact_events"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let responses = audio["response_events"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let impact_key = if impacts.is_empty() {
        if !responses.is_empty() {
            return Err(invalid(
                "Impact responses require the private contact profile that selects them",
            ));
        }
        None
    } else {
        if audio["authoring_schema"] != 3 {
            return Err(invalid("Impact audio requires source bank routing"));
        }
        Some(hex(&audio["impact_key"], "impact property key")?)
    };
    if !clips.is_empty() && audio["authoring_schema"] != 3 {
        return Err(invalid("Clip audio requires source bank routing"));
    }
    if !clips.is_empty() && graph_value["animation"]["first_person_status"] != "linked" {
        return Err(invalid(
            "Clip audio requires linked first-person animations",
        ));
    }
    if events.is_empty() && firing.is_empty() && clips.is_empty() && impacts.is_empty() {
        return Ok(None);
    }
    let mut media = BTreeMap::new();
    let mut banks = BTreeMap::new();
    for row in audio["converted_banks"].as_array().into_iter().flatten() {
        let name = row["file"]
            .as_str()
            .ok_or_else(|| invalid("Converted bank file missing"))?;
        let path = std::path::Path::new(name);
        if path
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
        {
            return Err(invalid("Converted bank escapes its graph"));
        }
        let bytes = graph
            .read(path)
            .map_err(|error| invalid(format!("Converted bank {name}: {error}")))?;
        let bank = serde_json::from_slice(&bytes)
            .map_err(|error| invalid(format!("Converted bank {name}: {error}")))?;
        let tag = row["source_tag"]
            .as_str()
            .ok_or_else(|| invalid("Converted bank source missing"))?;
        if banks.insert(tag.to_owned(), bank).is_some() {
            return Err(invalid("Converted bank repeats"));
        }
    }
    if audio["authoring_schema"] == 3 {
        for event in firing
            .iter()
            .chain(&clips)
            .chain(&impacts)
            .chain(&responses)
        {
            for sound in event["sounds"].as_array().into_iter().flatten() {
                if sound["bank"]
                    .as_str()
                    .is_none_or(|tag| !banks.contains_key(tag))
                {
                    return Err(invalid("Source firing or clip bank is missing"));
                }
            }
        }
        for event in &events {
            for sound in event["sounds"].as_array().into_iter().flatten() {
                if sound["bank"]
                    .as_str()
                    .is_some_and(|tag| banks.contains_key(tag))
                {
                    continue;
                }
                if !audio["bank_fallbacks"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|row| {
                        row["source_tag"] == sound["bank"]
                            && row["routing"] == "source_media_native_bank"
                    })
                {
                    return Err(invalid("Source named-event bank has no recorded fallback"));
                }
            }
        }
    }
    for row in audio["transcoded_media"]
        .as_array()
        .ok_or_else(|| invalid("Imported audio has no converted media"))?
    {
        let tag = row["source_tag"]
            .as_str()
            .ok_or_else(|| invalid("Converted audio source tag missing"))?;
        let name = row["file"]
            .as_str()
            .ok_or_else(|| invalid("Converted audio file missing"))?;
        let path = std::path::Path::new(name);
        if path
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
        {
            return Err(invalid("Converted audio file escapes its graph"));
        }
        let bytes = graph
            .read(path)
            .map_err(|error| invalid(format!("Converted audio {name}: {error}")))?;
        let bytes = parhelion_import::d2_mot::normalize_pcm_wem(&bytes)
            .map_err(|error| invalid(format!("Converted audio {name}: {error:#}")))?;
        if media.insert(tag.to_owned(), bytes).is_some() {
            return Err(invalid("Converted audio media tag repeats"));
        }
    }
    let runtime_entity = hex(&audio["runtime_entity"], "runtime entity")?;
    let item = u32::try_from(
        graph_value["item_hash"]
            .as_u64()
            .ok_or_else(|| invalid("Imported audio item identity missing"))?,
    )
    .map_err(|_| invalid("Imported audio item identity is invalid"))?;
    Ok(Some(ImportedAudio {
        runtime_entity,
        item,
        events,
        firing,
        clips,
        impacts,
        impact_key,
        responses,
        media,
        banks,
    }))
}

pub(in crate::item) use clips::author as author_clip_events;
pub(in crate::item) use clips::growth as clip_event_growth;

fn list<'a>(value: &'a Value, what: &str) -> AuthoringResult<&'a Vec<Value>> {
    value
        .as_array()
        .ok_or_else(|| invalid(format!("Imported audio {what} missing")))
}

fn stock(manager: &PackageManager, tag: u32, class: u32, what: &str) -> AuthoringResult<Vec<u8>> {
    let entry = manager
        .get_entry(TagHash(tag))
        .ok_or_else(|| invalid(format!("Imported audio {what} 0x{tag:08X} is unavailable")))?;
    if entry.reference != class {
        return Err(invalid(format!(
            "Imported audio {what} 0x{tag:08X} has an unexpected class"
        )));
    }
    read_tag(manager, TagHash(tag), what)
}

fn append(
    manager: &PackageManager,
    allocator: AppendedTagAllocator,
    tags: &mut Vec<NewTagSpec>,
    template: u32,
    payload: Vec<u8>,
    storage: crate::NewTagStorageMode,
    what: &str,
) -> AuthoringResult<u32> {
    if manager.get_entry(TagHash(template)).is_none() {
        return Err(invalid(format!(
            "Imported audio {what} template 0x{template:08X} is unavailable"
        )));
    }
    let tag = allocator
        .assigned_tag(tags.len(), "Imported audio", what)?
        .0;
    tags.push(NewTagSpec {
        template_tag: TagHash(template),
        payload,
        storage,
    });
    Ok(tag)
}

fn array(bytes: &[u8], offset: usize, stride: usize, class: u32) -> AuthoringResult<Vec<usize>> {
    Payload(bytes.to_vec())
        .array(offset, stride, Some(class))
        .map_err(|error| invalid(format!("Imported audio array: {error:#}")))
}

fn sound_row(group: &[u8], event_hash: u32, native_sound: u32) -> AuthoringResult<(usize, String)> {
    let payload = Payload(group.to_vec());
    for event in array(group, 8, 0x18, 0x8080_902F)? {
        if word(group, event)? != event_hash {
            continue;
        }
        for row in array(group, event + 8, 0x28, 0x8080_9033)? {
            if word(group, row + 0x18)? != native_sound {
                continue;
            }
            let start = payload
                .pointer(row + 0x10)
                .map_err(|error| invalid(format!("Imported audio event path: {error:#}")))?;
            let path = group
                .get(start..)
                .ok_or_else(|| invalid("Imported audio path exceeds group"))?
                .split(|byte| *byte == 0)
                .next()
                .ok_or_else(|| invalid("Imported audio path missing"))?;
            return Ok((row, String::from_utf8_lossy(path).into_owned()));
        }
    }
    Err(invalid(format!(
        "Imported audio group lacks event 0x{event_hash:08X} sound 0x{native_sound:08X}"
    )))
}

fn rename_path(group: &mut Vec<u8>, row: usize, old_path: &str, name: &str) -> AuthoringResult<()> {
    let slash = old_path
        .rfind(['\\', '/'])
        .ok_or_else(|| invalid("Imported audio event path lacks a directory"))?;
    let new_path = format!("{}{}.wwise_event", &old_path[..=slash], name);
    let at = group.len();
    group.extend_from_slice(new_path.as_bytes());
    group.push(0);
    let delta = i64::try_from(at).unwrap() - i64::try_from(row + 0x10).unwrap();
    group[row + 0x10..row + 0x18].copy_from_slice(&delta.to_le_bytes());
    let len = u64::try_from(group.len()).map_err(|_| invalid("Imported audio group too large"))?;
    group[..8].copy_from_slice(&len.to_le_bytes());
    Ok(())
}

fn private_bank(
    native_bank: &[u8],
    name: &str,
    variations: &[u32],
    media_ids: &BTreeMap<u32, u32>,
    media_sizes: &BTreeMap<u32, u32>,
) -> AuthoringResult<Vec<u8>> {
    let pcm = parhelion_import::d2_mot::pcm_bank_template(native_bank)
        .map_err(|error| invalid(format!("Imported audio legacy bank: {error:#}")))?;
    let stock_bank =
        parhelion_import::d2_mot::fit_variations(&pcm, variations, |container, index| {
            fnv1(&format!("{name}_{container:08x}_v{index}"))
        })
        .map_err(|error| invalid(format!("Imported audio variations: {error:#}")))?;
    let stock_bank = stock_bank.as_slice();
    let mut bank = stock_bank.to_vec();
    let old_bank_id = word(stock_bank, 12)?;
    let new_bank_id = fnv1(name);
    let mut ids = BTreeMap::new();
    let mut cursor = 0usize;
    let mut media_seen = BTreeSet::new();
    let mut source_sizes = Vec::new();
    while cursor < stock_bank.len() {
        let size = word(stock_bank, cursor + 4)? as usize;
        let start = cursor + 8;
        let end = start
            .checked_add(size)
            .ok_or_else(|| invalid("Imported audio bank section overflow"))?;
        if end > stock_bank.len() {
            return Err(invalid("Imported audio bank section exceeds payload"));
        }
        if stock_bank.get(cursor..cursor + 4) == Some(b"HIRC") {
            let count = word(stock_bank, start)? as usize;
            let mut object = start + 4;
            for _ in 0..count {
                let kind = *stock_bank
                    .get(object)
                    .ok_or_else(|| invalid("Imported audio bank object missing"))?;
                let length = word(stock_bank, object + 1)? as usize;
                let next = object
                    .checked_add(5 + length)
                    .ok_or_else(|| invalid("Imported audio bank object overflow"))?;
                if next > end {
                    return Err(invalid("Imported audio bank object exceeds section"));
                }
                let old = word(stock_bank, object + 5)?;
                let new = if old == old_bank_id {
                    new_bank_id
                } else {
                    fnv1(&format!("{name}_{old:08x}"))
                };
                if ids.insert(old, new).is_some() {
                    return Err(invalid("Imported audio bank object ID repeats"));
                }
                if kind == 2 {
                    // Version 113 stores source ID and in-memory media size
                    // after the plugin and stream type. These banks use full
                    // external media with no prefetch or embedded data.
                    if length < 18
                        || stock_bank[object + 13] != 2
                        || stock_bank[object + 22] & !8 != 0
                    {
                        return Err(invalid(
                            "Imported audio source needs another streaming layout",
                        ));
                    }
                    let media = word(stock_bank, object + 14)?;
                    if !media_ids.contains_key(&media) {
                        return Err(invalid(format!(
                            "Imported audio bank source ID 0x{media:08X} lacks converted media"
                        )));
                    }
                    media_seen.insert(media);
                    let size = media_sizes
                        .get(&media)
                        .ok_or_else(|| invalid("Imported audio source lacks its media size"))?;
                    source_sizes.push((object + 18, *size));
                }
                object = next;
            }
            if object != end {
                return Err(invalid("Imported audio bank object count differs"));
            }
        }
        cursor = end;
    }
    if ids.get(&old_bank_id) != Some(&new_bank_id) || media_seen.len() != media_ids.len() {
        return Err(invalid(
            "Imported audio bank IDs do not cover its event and media",
        ));
    }
    let mut remap = ids;
    for (old, new) in media_ids {
        if remap.insert(*old, *new).is_some() {
            return Err(invalid("Imported audio bank object and media IDs collide"));
        }
    }
    let mut replaced = BTreeMap::<u32, usize>::new();
    for at in 0..stock_bank.len().saturating_sub(3) {
        let old = word(stock_bank, at)?;
        if let Some(new) = remap.get(&old) {
            put(&mut bank, at, *new)?;
            *replaced.entry(old).or_default() += 1;
        }
    }
    if remap.keys().any(|old| !replaced.contains_key(old)) {
        return Err(invalid(
            "Imported audio bank keeps an unlinked object or media ID",
        ));
    }
    for (offset, size) in source_sizes {
        put(&mut bank, offset, size)?;
    }
    Ok(bank)
}

#[allow(clippy::too_many_arguments)]
fn private_sound(
    manager: &PackageManager,
    audio: &ImportedAudio,
    source: &Value,
    native: &Value,
    name: &str,
    allocator: AppendedTagAllocator,
    tags: &mut Vec<NewTagSpec>,
    remap: &mut BTreeMap<u32, u32>,
) -> AuthoringResult<u32> {
    let native_tag = hex(&native["tag"], "native sound")?;
    let stock_sound = stock(manager, native_tag, SOUND_CLASS, "sound")?;
    let source_media = list(&source["media"], "source media")?;
    let source_ids = list(&source["media_ids"], "source media IDs")?;
    let native_media = list(&native["media"], "native media")?;
    let native_count = word(&stock_sound, 0x40)? as usize;
    if source_media.is_empty()
        || source_media.len() != source_ids.len()
        || native_media.len() != native_count
        || native_count == 0
        || word(&stock_sound, 0x18)? as usize != native_count
        || stock_sound.len() != 0x50 + 4 * native_count
    {
        return Err(invalid("Imported audio sound layouts differ"));
    }
    // The media array ends the sound tag, so it follows the source's variation count.
    let mut sound = stock_sound[..0x50].to_vec();
    sound.resize(0x50 + 4 * source_media.len(), 0);
    let count = source_media.len() as u64;
    let size = sound.len() as u64;
    sound[0..8].copy_from_slice(&size.to_le_bytes());
    sound[0x18..0x20].copy_from_slice(&count.to_le_bytes());
    sound[0x40..0x48].copy_from_slice(&count.to_le_bytes());
    let mut variations = Vec::new();
    let mut media_ids = BTreeMap::new();
    let mut media_sizes = BTreeMap::new();
    for (index, (source_tag, media_id)) in source_media.iter().zip(source_ids).enumerate() {
        let source_tag = source_tag
            .as_str()
            .ok_or_else(|| invalid("Source media tag missing"))?;
        let source_id = hex(media_id, "source media ID")?;
        let template = hex(&native_media[index.min(native_count - 1)], "native media")?;
        let entry = manager
            .get_entry(TagHash(template))
            .ok_or_else(|| invalid("Native media entry missing"))?;
        if entry.file_type != 26 {
            return Err(invalid("Native media has an unexpected file type"));
        }
        let private = match media_ids.get(&source_id) {
            Some(private) => *private,
            None => {
                let pcm = audio
                    .media
                    .get(source_tag)
                    .ok_or_else(|| {
                        invalid(format!("Converted source media {source_tag} is missing"))
                    })?
                    .clone();
                media_sizes.insert(
                    source_id,
                    u32::try_from(pcm.len())
                        .map_err(|_| invalid("Imported audio medium is too large"))?,
                );
                append(
                    manager,
                    allocator,
                    tags,
                    template,
                    pcm,
                    crate::NewTagStorageMode::AudioMedia,
                    "PCM media",
                )?
            }
        };
        put(&mut sound, 0x50 + index * 4, private)?;
        media_ids.insert(source_id, private);
        variations.push(source_id);
        if index < native_count {
            remap.insert(template, private);
        }
    }
    let bank_tag = hex(&native["bank"], "native bank")?;
    let bank_entry = manager
        .get_entry(TagHash(bank_tag))
        .ok_or_else(|| invalid("Native bank is missing"))?;
    if bank_entry.file_type != 26 {
        return Err(invalid("Native bank has an unexpected file type"));
    }
    let (bank, event_id) =
        if let Some(converted) = source["bank"].as_str().and_then(|tag| audio.banks.get(tag)) {
            converted
                .instantiate(
                    name,
                    hex(&source["event_id"], "source event ID")?,
                    &media_ids,
                    &media_sizes,
                )
                .map_err(|error| invalid(format!("Source audio bank: {error:#}")))?
        } else {
            let stock_bank = read_tag(manager, TagHash(bank_tag), "native audio bank")?;
            (
                private_bank(&stock_bank, name, &variations, &media_ids, &media_sizes)?,
                fnv1(name),
            )
        };
    let private_bank_tag = append(
        manager,
        allocator,
        tags,
        bank_tag,
        bank,
        crate::NewTagStorageMode::AudioBank,
        "PCM bank",
    )?;
    remap.insert(bank_tag, private_bank_tag);
    put(&mut sound, 8, event_id)?;
    put(&mut sound, 0x14, private_bank_tag)?;
    let private_sound_tag = append(
        manager,
        allocator,
        tags,
        native_tag,
        sound,
        crate::NewTagStorageMode::InheritTemplate,
        "PCM sound",
    )?;
    remap.insert(native_tag, private_sound_tag);
    Ok(private_sound_tag)
}

fn stock_companion(manager: &PackageManager, owner: u32) -> AuthoringResult<(TagHash, Vec<u8>)> {
    let mut found = None;
    for (tag, entry) in manager.get_all_by_reference(0x8080_9EF9) {
        if entry.file_type != 8 || entry.file_subtype != 0 {
            continue;
        }
        let payload = read_tag(manager, tag, "audio shared-tag companion")?;
        if payload.get(12..16) != Some(&owner.to_le_bytes()) {
            continue;
        }
        dependency_entries(&payload, tag, TagHash(owner))?;
        if found.replace((tag, payload)).is_some() {
            return Err(invalid("Audio parent has multiple shared-tag companions"));
        }
    }
    found.ok_or_else(|| invalid("Audio parent has no shared-tag companion"))
}

#[allow(clippy::too_many_arguments)]
pub(in crate::item) fn author(
    manager: &PackageManager,
    audio: &ImportedAudio,
    pattern_tag: TagHash,
    pattern: &mut [u8],
    animation_owner: Option<u32>,
    allocator: AppendedTagAllocator,
    tags: &mut Vec<NewTagSpec>,
    (assets, asset_tags): (AppendedTagAllocator, &mut Vec<NewTagSpec>),
) -> AuthoringResult<usize> {
    if pattern_tag.0 != audio.runtime_entity {
        return Err(invalid(
            "Imported audio was prepared for another runtime entity",
        ));
    }
    let mut grouped = BTreeMap::<(u32, u32, u32), Vec<&Value>>::new();
    for event in &audio.events {
        let group = &event["native_group"];
        grouped
            .entry((
                hex(&group["owner"], "audio owner")?,
                hex(&group["parent"], "audio parent")?,
                hex(&group["group"], "audio group")?,
            ))
            .or_default()
            .push(event);
    }
    let mut owners = BTreeMap::<u32, Vec<u8>>::new();
    let mut count = firing::author(manager, audio, pattern, allocator, tags, assets, asset_tags)?;
    for ((owner_tag, parent_tag, group_tag), events) in grouped {
        let group_start = tags.len();
        let asset_start = asset_tags.len();
        let mut remap = BTreeMap::new();
        let mut group = stock(manager, group_tag, GROUP_CLASS, "event group")?;
        for event in events {
            let hash = hex(&event["hash"], "event hash")?;
            let sources = event["sounds"]
                .as_array()
                .ok_or_else(|| invalid("Source event sounds missing"))?;
            let natives = event["native_sounds"]
                .as_array()
                .ok_or_else(|| invalid("Matched native sounds missing"))?;
            let mut used = BTreeSet::new();
            for (index, source) in sources.iter().enumerate() {
                // An identical donor sound wins, otherwise the donor sounds pair in order.
                let position = natives
                    .iter()
                    .position(|native| {
                        native["event_path"] == source["event_path"]
                            && native["media_ids"] == source["media_ids"]
                    })
                    .or((index < natives.len()).then_some(index));
                let Some(position) = position.filter(|position| used.insert(*position)) else {
                    continue;
                };
                let native = &natives[position];
                let native_tag = hex(&native["tag"], "native sound")?;
                let (row, old_path) = sound_row(&group, hash, native_tag)?;
                let name = format!("ph_{:08x}_{hash:08x}_{native_tag:08x}_{index}", audio.item);
                let private = private_sound(
                    manager, audio, source, native, &name, assets, asset_tags, &mut remap,
                )?;
                put(&mut group, row + 0x18, private)?;
                rename_path(&mut group, row, &old_path, &name)?;
                count += 1;
            }
        }
        let private_group = append(
            manager,
            allocator,
            tags,
            group_tag,
            group,
            crate::NewTagStorageMode::InheritTemplate,
            "event group",
        )?;
        remap.insert(group_tag, private_group);
        let mut parent = stock(manager, parent_tag, PARENT_CLASS, "audio parent")?;
        if word(&parent, 16)? != group_tag {
            return Err(invalid("Audio parent group link changed"));
        }
        put(&mut parent, 16, private_group)?;
        let private_parent = append(
            manager,
            allocator,
            tags,
            parent_tag,
            parent,
            crate::NewTagStorageMode::InheritTemplate,
            "audio parent",
        )?;
        remap.insert(parent_tag, private_parent);
        let private_companion = allocator
            .assigned_tag(tags.len(), "Imported audio", "shared-tag companion")?
            .0;
        let (stock_companion_tag, companion_template) = stock_companion(manager, parent_tag)?;
        remap.insert(stock_companion_tag.0, private_companion);
        let mut dependencies = BTreeSet::new();
        for ordinal in group_start..tags.len() {
            dependencies.insert(
                allocator
                    .assigned_tag(ordinal, "Imported audio", "dependency")?
                    .0,
            );
        }
        for ordinal in asset_start..asset_tags.len() {
            dependencies.insert(
                assets
                    .assigned_tag(ordinal, "Imported audio", "asset dependency")?
                    .0,
            );
        }
        dependencies.insert(private_companion);
        let companion = clone_scoped_dependencies(
            (
                &companion_template,
                LoadingOwner {
                    owner: TagHash(parent_tag),
                    companion: stock_companion_tag,
                },
            ),
            LoadingOwner {
                owner: TagHash(private_parent),
                companion: TagHash(private_companion),
            },
            &dependencies.into_iter().map(TagHash).collect::<Vec<_>>(),
            &[],
        )?;
        let emitted_companion = append(
            manager,
            allocator,
            tags,
            stock_companion_tag.0,
            companion,
            crate::NewTagStorageMode::InheritTemplate,
            "shared-tag companion",
        )?;
        if emitted_companion != private_companion {
            return Err(invalid("Audio shared-tag companion allocation changed"));
        }
        let owner = match owners.entry(owner_tag) {
            std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(stock(manager, owner_tag, OWNER_CLASS, "audio owner")?)
            }
        };
        let resource = Payload(owner.clone())
            .pointer(24)
            .map_err(|error| invalid(format!("Audio owner resource: {error:#}")))?;
        let rows = array(owner, resource + 0x588, 0xE8, 0x8080_3E72)?;
        let mut changes = 0;
        // The embedded default is live when the content selector has no
        // variant. Keep its routing in sync with any rows sharing the parent.
        for row in std::iter::once(resource + 0x4A0).chain(rows) {
            if word(owner, row + 0x98)? == parent_tag {
                put(owner, row + 0x98, private_parent)?;
                changes += 1;
            }
        }
        if changes == 0 {
            return Err(invalid("Audio owner has no parent link"));
        }
    }
    for (owner_tag, mut owner) in owners {
        let animated_owner = animation_owner.filter(|private| {
            allocator
                .ordinal(TagHash(*private))
                .ok()
                .and_then(|ordinal| tags.get(ordinal))
                .is_some_and(|tag| tag.template_tag.0 == owner_tag)
        });
        if let Some(private) = animated_owner {
            let ordinal = allocator.ordinal(TagHash(private))?;
            let entry = tags
                .get_mut(ordinal)
                .ok_or_else(|| invalid("Private animation owner is missing"))?;
            if entry.template_tag.0 != owner_tag {
                return Err(invalid("Private animation owner differs from audio owner"));
            }
            let old = stock(manager, owner_tag, OWNER_CLASS, "audio owner")?;
            if old.len() != owner.len() || entry.payload.len() != owner.len() {
                return Err(invalid("Private animation owner layout differs"));
            }
            for at in (0..owner.len().saturating_sub(3)).step_by(4) {
                let before = word(&old, at)?;
                let after = word(&owner, at)?;
                if before != after {
                    put(&mut entry.payload, at, after)?
                }
            }
        } else {
            let expected = allocator
                .assigned_tag(tags.len(), "Imported audio", "audio owner")?
                .0;
            retarget_weapon_component_owner_payload(&mut owner, pattern, owner_tag, expected)
                .map_err(invalid)?;
            let private = append(
                manager,
                allocator,
                tags,
                owner_tag,
                owner,
                crate::NewTagStorageMode::InheritTemplate,
                "audio owner",
            )?;
            retarget_weapon_component_owner(pattern, owner_tag, private).map_err(invalid)?;
        }
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wwise_event_name_uses_fnv1() {
        assert_eq!(fnv1("auto_suros_ready2"), 0x8517_4761);
    }

    #[test]
    #[ignore = "Requires an explicitly configured Dawn Wwise Vorbis bank"]
    fn configured_bank_can_be_privately_reidentified() {
        let path = std::env::var_os("PARHELION_AUDIO_NATIVE_BANK").expect("native bank path");
        let bank = fs::read(path).unwrap();
        let mut media_ids = BTreeMap::new();
        let mut media_sizes = BTreeMap::new();
        let mut at = 0usize;
        while at < bank.len() {
            let size = word(&bank, at + 4).unwrap() as usize;
            if bank.get(at..at + 4) == Some(b"HIRC") {
                let end = at + 8 + size;
                let mut object = at + 12;
                while object < end {
                    let length = word(&bank, object + 1).unwrap() as usize;
                    if bank[object] == 2 {
                        let id = word(&bank, object + 14).unwrap();
                        media_ids.insert(id, id ^ 0x5080_0000);
                        media_sizes.insert(id, 48044);
                    }
                    object += 5 + length;
                }
            }
            at += 8 + size;
        }
        assert!(
            !media_ids.is_empty(),
            "the configured bank has no sound objects"
        );
        let variations = media_ids.keys().copied().collect::<Vec<_>>();
        let output = private_bank(
            &bank,
            "parhelion_audio_test",
            &variations,
            &media_ids,
            &media_sizes,
        )
        .unwrap();
        assert_eq!(output.len(), bank.len());
        assert_eq!(word(&output, 12).unwrap(), 0x28BB_08CD);
        let mut at = 0;
        while at < output.len() {
            let end = at + 8 + word(&output, at + 4).unwrap() as usize;
            if output.get(at..at + 4) == Some(b"HIRC") {
                let mut object = at + 12;
                while object < end {
                    if output[object] == 2 {
                        let original_id = word(&bank, object + 14).unwrap();
                        assert_eq!(word(&output, object + 14).unwrap(), media_ids[&original_id]);
                        assert_eq!(word(&output, object + 18).unwrap(), 48044);
                        assert_eq!(word(&output, object + 9).unwrap(), 0x0001_0001);
                    }
                    object += 5 + word(&output, object + 1).unwrap() as usize;
                }
            }
            at = end;
        }
    }
}

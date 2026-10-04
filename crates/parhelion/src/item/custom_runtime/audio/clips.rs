//! Source sounds owned by particular imported animation events.
//!
//! Resolve the event inside the private clip before animation deduplication.
//! A shared stock punch cue can therefore become a weapon's own swing without
//! changing the cue used by other weapons or modifying an already cached clip.
use super::super::animation::ImportedAnimation;
use super::*;

fn event(payload: &[u8], index: usize, expected: u32) -> AuthoringResult<usize> {
    let p = Payload(payload.to_vec());
    let rows = p
        .array(0x160, 8, Some(0x8080902B))
        .map_err(|error| invalid(format!("Clip audio event list: {error:#}")))?;
    let row = *rows
        .get(index)
        .ok_or_else(|| invalid("Clip audio event is absent"))?;
    let at = p
        .pointer(row)
        .map_err(|error| invalid(format!("Clip audio event pointer: {error:#}")))?;
    if at < 4
        || word(payload, at - 4)? != 0x80809033
        || word(payload, at)? >> 16 != 3
        || word(payload, at + 4)? != 2
        || word(payload, at + 24)? != expected
    {
        return Err(invalid(
            "Clip audio event does not match its recorded sound",
        ));
    }
    Ok(at)
}

pub(in crate::item) fn growth(
    audio: &ImportedAudio,
    animation: &ImportedAnimation,
) -> AuthoringResult<usize> {
    let mut maximum = 0;
    for bytes in animation
        .clips
        .iter()
        .map(|(_, b)| b)
        .chain(animation.extra_clips.iter().map(|(_, _, b)| b))
    {
        let mut extra = 0usize;
        for row in &audio.clips {
            let name = hex(&row["clip_name"], "clip name")?;
            if word(bytes, 0x120)? != name {
                continue;
            }
            let index = row["event_index"]
                .as_u64()
                .and_then(|v| usize::try_from(v).ok())
                .ok_or_else(|| invalid("Clip audio event index is missing"))?;
            let native = list(&row["native_sounds"], "clip native sounds")?;
            if native.len() != 1 {
                return Err(invalid("Clip native sound is ambiguous"));
            }
            let at = event(bytes, index, hex(&native[0]["tag"], "clip native sound")?)?;
            let p = Payload(bytes.clone());
            let start = p
                .pointer(at + 16)
                .map_err(|e| invalid(format!("Clip path: {e:#}")))?;
            let tail = bytes
                .get(start..)
                .ok_or_else(|| invalid("Clip path is outside its payload"))?;
            let length = tail
                .iter()
                .position(|b| *b == 0)
                .ok_or_else(|| invalid("Clip path is unterminated"))?;
            // The entire old path plus the new filename bounds the appended path.
            let name = format!("ph_{:08x}_clip_{name:08x}_{index}.wwise_event", audio.item);
            extra = extra
                .checked_add(length + name.len() + 1)
                .ok_or_else(|| invalid("Clip audio reservation overflow"))?;
        }
        maximum = maximum.max(extra);
    }
    Ok(maximum)
}

#[allow(clippy::too_many_arguments)]
pub(in crate::item) fn author(
    manager: &PackageManager,
    audio: &ImportedAudio,
    animation: &mut ImportedAnimation,
    allocator: AppendedTagAllocator,
    tags: &mut Vec<NewTagSpec>,
) -> AuthoringResult<usize> {
    let mut seen = BTreeSet::new();
    for row in &audio.clips {
        let name = hex(&row["clip_name"], "clip name")?;
        let index = row["event_index"]
            .as_u64()
            .and_then(|v| usize::try_from(v).ok())
            .ok_or_else(|| invalid("Clip audio event index is missing"))?;
        if !seen.insert((name, index)) {
            return Err(invalid("Imported clip audio event repeats"));
        }
        let sources = list(&row["sounds"], "clip source sounds")?;
        let natives = list(&row["native_sounds"], "clip native sounds")?;
        if sources.len() != 1 || natives.len() != 1 {
            return Err(invalid("A clip event must resolve one source sound"));
        }
        let native = &natives[0];
        let native_tag = hex(&native["tag"], "clip native sound")?;
        let mut matches = Vec::new();
        for (ordinal, (_, bytes)) in animation.clips.iter().enumerate() {
            if word(bytes, 0x120)? == name {
                matches.push((false, ordinal, event(bytes, index, native_tag)?));
            }
        }
        for (ordinal, (_, _, bytes)) in animation.extra_clips.iter().enumerate() {
            if word(bytes, 0x120)? == name {
                matches.push((true, ordinal, event(bytes, index, native_tag)?));
            }
        }
        if matches.is_empty() {
            return Err(invalid("Imported clip audio has no converted animation"));
        }
        let sound_name = format!("ph_{:08x}_clip_{name:08x}_{index}", audio.item);
        let private = private_sound(
            manager,
            audio,
            &sources[0],
            native,
            &sound_name,
            allocator,
            tags,
            &mut BTreeMap::new(),
        )?;
        for (extra, ordinal, at) in matches {
            let bytes = if extra {
                &mut animation.extra_clips[ordinal].2
            } else {
                &mut animation.clips[ordinal].1
            };
            let p = Payload(bytes.clone());
            let start = p
                .pointer(at + 16)
                .map_err(|error| invalid(format!("Clip audio path: {error:#}")))?;
            let tail = bytes
                .get(start..)
                .ok_or_else(|| invalid("Clip audio path is outside the clip"))?;
            let end = tail
                .iter()
                .position(|b| *b == 0)
                .ok_or_else(|| invalid("Clip audio path is not terminated"))?;
            let old = std::str::from_utf8(&tail[..end])
                .map_err(|_| invalid("Clip audio path is not UTF-8"))?
                .to_owned();
            rename_path(bytes, at, &old, &sound_name)?;
            put(bytes, at + 24, private)?;
            let size = bytes.len() as u64;
            bytes[..8].copy_from_slice(&size.to_le_bytes());
        }
    }
    Ok(seen.len())
}

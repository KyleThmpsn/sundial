//! Reference locations come from native layouts or the authored v113 Wwise sound layout.
use super::*;

pub(super) fn fields(manager: &PackageManager, tag: &NewTagSpec) -> Option<Vec<(usize, u32)>> {
    match tag.storage {
        crate::NewTagStorageMode::AudioMedia => Some(Vec::new()),
        crate::NewTagStorageMode::AudioBank => audio_fields(&tag.payload),
        crate::NewTagStorageMode::InheritTemplate => {
            // A loading index also names resources through package IDs and bitmap
            // or sparse entry indices. Ordinary tag-field relocation misses them.
            // Keep exact-placement reuse, but recompile if allocation changes.
            if manager.get_entry(tag.template_tag)?.reference == 0x8080_9EF9 {
                return None;
            }
            sundial::package_authoring::declared_payload_references(
                manager,
                tag.template_tag.0,
                &tag.payload,
            )
            .ok()
        }
    }
}

fn word(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}

/// Authored banks use version 113 with fully streamed media. Only sound source IDs are
/// package tags. Event, bus, bank and other HIRC IDs remain ordinary Wwise identities.
fn audio_fields(bytes: &[u8]) -> Option<Vec<(usize, u32)>> {
    if bytes.get(..4)? != b"BKHD" || word(bytes, 8)? != 113 {
        return None;
    }
    let mut at = 0usize;
    let mut fields = Vec::new();
    let mut hirc = false;
    while at < bytes.len() {
        let name = bytes.get(at..at.checked_add(4)?)?;
        let start = at.checked_add(8)?;
        let end = start.checked_add(word(bytes, at + 4)? as usize)?;
        if end > bytes.len() {
            return None;
        }
        match name {
            b"HIRC" => {
                if hirc {
                    return None;
                }
                hirc = true;
                hirc_fields(bytes, (start, end), &mut fields)?;
            }
            // Embedded or prefetched media requires a different identity layout.
            b"DIDX" | b"DATA" if end != start => return None,
            _ => {}
        }
        at = end;
    }
    hirc.then_some(fields)
}

/// The source ID of each sound object (kind 2) in the HIRC section at `start..end`. Each sound
/// must stream its one source, and the objects must fill the section exactly.
fn hirc_fields(
    bytes: &[u8],
    (start, end): (usize, usize),
    fields: &mut Vec<(usize, u32)>,
) -> Option<()> {
    let count = word(bytes, start)? as usize;
    let mut object = start.checked_add(4)?;
    if count > end.saturating_sub(object) / 9 {
        return None;
    }
    for _ in 0..count {
        let kind = *bytes.get(object)?;
        let length = word(bytes, object + 1)? as usize;
        let next = object.checked_add(5)?.checked_add(length)?;
        if length < 4 || next > end {
            return None;
        }
        if kind == 2 {
            if length < 18 || *bytes.get(object + 13)? != 2 || bytes[object + 22] & !8 != 0 {
                return None;
            }
            fields.push((object + 14, word(bytes, object + 14)?));
        }
        object = next;
    }
    (object == end).then_some(())
}

pub(super) fn unpack(
    tag: &Tag,
    bytes: &[u8],
    moves: &BTreeMap<u32, u32>,
    exact: bool,
) -> Option<NewTagSpec> {
    let mut payload = slice(bytes, tag.span)?.to_vec();
    if !exact {
        for (offset, old) in tag.fields.as_ref()? {
            if word(&payload, *offset)? != *old {
                return None;
            }
            if let Some(new) = moves.get(old) {
                payload
                    .get_mut(*offset..offset.checked_add(4)?)?
                    .copy_from_slice(&new.to_le_bytes());
            }
        }
    }
    Some(NewTagSpec {
        template_tag: TagHash(tag.template),
        storage: match tag.storage {
            0 => crate::NewTagStorageMode::InheritTemplate,
            1 => crate::NewTagStorageMode::AudioMedia,
            2 => crate::NewTagStorageMode::AudioBank,
            _ => return None,
        },
        payload,
    })
}

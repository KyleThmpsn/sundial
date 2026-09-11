//! Typed references that must stay inside a cloned component-owner partition.

use super::*;

const EVENTS_DESCRIPTOR: usize = 0x20;
const EVENT_ROW_CLASS: u32 = 0x8080_9BC9;
const EVENT_ROW_SIZE: usize = 0x48;

pub(super) fn event_owner_fields(entity: &[u8], owner_tag: u32) -> Result<BTreeSet<usize>, String> {
    let mut fields = BTreeSet::new();
    // Empty event lists can have a null descriptor instead of an array header.
    if read_u64(entity, EVENTS_DESCRIPTOR)? == 0 {
        return Ok(fields);
    }
    let events = native_array(entity, EVENTS_DESCRIPTOR)?;
    if events.row_class != EVENT_ROW_CLASS {
        return Err(format!(
            "Weapon entity event array has unsupported class 0x{:08X}",
            events.row_class
        ));
    }
    checked_rows_end(events, EVENT_ROW_SIZE, entity.len(), "Weapon entity event")?;
    for index in 0..events.count {
        let row = events.rows + index * EVENT_ROW_SIZE;
        // Each endpoint wraps a 16-byte typed owner/absolute-offset reference.
        for offset in [row + 8, row + 0x28] {
            if read_u32(entity, offset)? == owner_tag {
                if !is_native_class(read_u32(entity, offset + 4)?) {
                    return Err("Weapon entity event endpoint has an invalid class".into());
                }
                fields.insert(offset);
            }
        }
    }
    Ok(fields)
}

pub(super) fn self_reference_owner_fields(data: &[u8], owner_tag: u32) -> BTreeSet<usize> {
    let mut fields = BTreeSet::new();
    // A tag-valued integer is insufficient. Require two complete, aligned, typed
    // references in this payload that name the owner and point back to each other.
    for offset in (0..data.len().saturating_sub(15)).step_by(8) {
        if read_u32(data, offset) != Ok(owner_tag)
            || !read_u32(data, offset + 4).is_ok_and(is_native_class)
        {
            continue;
        }
        let Ok(target) = read_u64(data, offset + 8).and_then(|v| {
            usize::try_from(v).map_err(|_| "Object reference offset is too large".to_owned())
        }) else {
            continue;
        };
        if target == offset
            || target % 8 != 0
            || target.checked_add(16).is_none_or(|end| end > data.len())
            || read_u32(data, target) != Ok(owner_tag)
            || !read_u32(data, target + 4).is_ok_and(is_native_class)
            || read_u64(data, target + 8) != Ok(offset as u64)
        {
            continue;
        }
        fields.insert(offset);
        fields.insert(target);
    }
    fields
}

fn is_native_class(value: u32) -> bool {
    value & 0xFFFF_0000 == 0x8080_0000
}

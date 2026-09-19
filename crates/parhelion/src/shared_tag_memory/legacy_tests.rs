use super::*;
use crate::tag_payload::write_i64;
const ARRAY_MARKER: u32 = 0x8080_9FBD;
const PACKAGE_GROUP_CLASS: u64 = 0x8080_9EFB;
const ENTRY_INDEX_CLASS: u64 = 0x8080_000A;
const GROUP_ROWS_OFFSET: usize = 0x50;
pub(super) fn encode_canonical_payload(
    template_payload: &[u8],
    companion_tag: TagHash,
    container_tag: TagHash,
    groups: &BTreeMap<u16, Vec<u16>>,
) -> AuthoringResult<Vec<u8>> {
    let group_count = groups.len();
    let mut payload = template_payload[..0x30].to_vec();
    write_u32(&mut payload, 0x08, u32::from(companion_tag))?;
    write_u32(&mut payload, 0x0C, u32::from(container_tag))?;
    write_u64(&mut payload, 0x10, group_count as u64)?;
    write_i64(&mut payload, 0x18, 0x28)?;
    write_u64(&mut payload, 0x20, 0)?;
    write_u64(&mut payload, 0x28, 0)?;

    payload.resize(0x3C, 0);
    push_u32(&mut payload, ARRAY_MARKER);
    push_u64(&mut payload, group_count as u64);
    push_u64(&mut payload, PACKAGE_GROUP_CLASS);
    if payload.len() != GROUP_ROWS_OFFSET {
        return Err(validation(
            "Icon companion fixed envelope did not end at its stock group-table offset",
        ));
    }

    let mut row_offsets = Vec::with_capacity(group_count);
    for (package_id, indices) in groups {
        row_offsets.push(payload.len());
        push_u64(&mut payload, u64::from(*package_id));
        payload.resize(payload.len() + 0x10, 0);
        push_u64(&mut payload, indices.len() as u64);
        push_i64(&mut payload, 0);
    }

    for ((_, indices), row) in groups.iter().zip(row_offsets) {
        let entries_start = align_up(
            payload
                .len()
                .checked_add(0x14)
                .ok_or_else(|| invalid("Icon companion tail offset overflowed"))?,
            0x10,
        )?;
        let marker_offset = entries_start - 0x14;
        payload.resize(marker_offset, 0);
        push_u32(&mut payload, ARRAY_MARKER);
        push_u64(&mut payload, indices.len() as u64);
        push_u64(&mut payload, ENTRY_INDEX_CLASS);
        if payload.len() != entries_start {
            return Err(validation(
                "Icon companion entry-index array did not satisfy stock alignment",
            ));
        }
        for index in indices {
            push_u16(&mut payload, *index);
        }
        let relative_field = row + 0x20;
        let count_target = entries_start - 0x10;
        let relative = i64::try_from(count_target)
            .and_then(|target| i64::try_from(relative_field).map(|field| target - field))
            .map_err(|_| invalid("Icon companion relative pointer overflowed"))?;
        write_i64(&mut payload, relative_field, relative)?;
    }

    let payload_len = u64::try_from(payload.len())
        .map_err(|_| invalid("Icon companion payload length overflowed"))?;
    write_u64(&mut payload, 0x00, payload_len)?;
    Ok(payload)
}

fn align_up(value: usize, alignment: usize) -> AuthoringResult<usize> {
    let mask = alignment - 1;
    value
        .checked_add(mask)
        .map(|aligned| aligned & !mask)
        .ok_or_else(|| invalid("Icon companion alignment overflowed"))
}

fn push_u16(data: &mut Vec<u8>, value: u16) {
    data.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(data: &mut Vec<u8>, value: u32) {
    data.extend_from_slice(&value.to_le_bytes());
}

fn push_u64(data: &mut Vec<u8>, value: u64) {
    data.extend_from_slice(&value.to_le_bytes());
}

fn push_i64(data: &mut Vec<u8>, value: i64) {
    data.extend_from_slice(&value.to_le_bytes());
}

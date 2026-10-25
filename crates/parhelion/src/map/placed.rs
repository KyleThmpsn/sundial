//! Checked handle, redirect and placed-record templates shared by authored map objects.
use std::collections::BTreeSet;

use crate::{
    AuthoringResult,
    error::invalid,
    tag_payload::{
        array_at, bounded_relative_target, read_i64, read_tag, read_u16, read_u32, read_u64,
        synchronize_payload_size, write_u16, write_u32,
    },
};
use sundial::package_authoring::PackageManager;
use tiger_pkg::TagHash;
const HANDLE_CLASS: u32 = 0x8080_9468;
const HANDLE_LIST: usize = 0x08;
const HANDLE_REDIRECTS: usize = 0x10;
const DESCRIPTOR_SLOT_INDEX: usize = 0x36;
fn class(manager: &PackageManager, tag: u32) -> Option<u32> {
    manager.get_entry(TagHash(tag)).map(|entry| entry.reference)
}
fn target(payload: &[u8], field: usize) -> AuthoringResult<usize> {
    if read_i64(payload, field)? == 0 {
        return Err(invalid("Placed record pointer is null"));
    }
    bounded_relative_target(payload, field, "Placed record pointer")
}
fn words(payload: &[u8], descriptor: usize) -> AuthoringResult<Vec<u32>> {
    if read_u64(payload, descriptor)? == 0 {
        return Ok(Vec::new());
    }
    let (count, _, rows, _) = array_at(payload, descriptor)?;
    (0..count)
        .map(|index| read_u32(payload, rows + index * 4))
        .collect()
}
const REDIRECT_CLASS: u32 = 0x8080_9B14;
const REDIRECT_RECORD: [usize; 2] = [0x0C, 0x10];
const PLACED_CLASS: u32 = 0x8080_9C36;
const RECORD_SECONDARY: usize = 0x18;
const DESCRIPTOR_KEY: usize = 0x30;
const DESCRIPTOR_SLOT_TYPE: usize = 0x34;
const DESCRIPTOR_NAME: usize = 0x50;
const NAME_CLASS: u32 = 0x8080_0065;
/// A handle, its redirect and its placed record, with their payloads.
#[derive(Clone)]
pub(super) struct Placed {
    pub(super) tags: [TagHash; 3],
    pub(super) payloads: [Vec<u8>; 3],
}

/// The handle `handle`'s single redirect and placed record, when it has that form.
pub(super) fn read(manager: &PackageManager, handle: u32) -> AuthoringResult<Option<Placed>> {
    if class(manager, handle) != Some(HANDLE_CLASS) {
        return Ok(None);
    }
    let handle_payload = read_tag(manager, TagHash(handle), "object handle")?;
    if read_u32(&handle_payload, HANDLE_LIST)? != u32::MAX {
        return Ok(None);
    }
    let [redirect] = words(&handle_payload, HANDLE_REDIRECTS)?[..] else {
        return Ok(None);
    };
    if class(manager, redirect) != Some(REDIRECT_CLASS) {
        return Ok(None);
    }
    let redirect_payload = read_tag(manager, TagHash(redirect), "object redirect")?;
    let record = read_u32(&redirect_payload, REDIRECT_RECORD[0])?;
    if class(manager, record) != Some(PLACED_CLASS) {
        return Ok(None);
    }
    let record_payload = read_tag(manager, TagHash(record), "placed record")?;
    Ok(Some(Placed {
        tags: [TagHash(handle), TagHash(redirect), TagHash(record)],
        payloads: [handle_payload, redirect_payload, record_payload],
    }))
}

/// The keyed descriptor of a placed record: its secondary component.
pub(super) fn keyed(record: &[u8]) -> AuthoringResult<usize> {
    target(record, RECORD_SECONDARY)
}

/// Copies of `placed`'s handle, redirect and record: the handle names `redirect`, the redirect
/// names `record`, and the record its own tag wherever it named the template's.
pub(super) fn clone_placed(
    placed: &Placed,
    redirect: TagHash,
    record: TagHash,
) -> AuthoringResult<[Vec<u8>; 3]> {
    let mut handle_payload = placed.payloads[0].clone();
    let (_, _, rows, _) = array_at(&handle_payload, HANDLE_REDIRECTS)?;
    write_u32(&mut handle_payload, rows, redirect.0)?;
    let mut redirect_payload = placed.payloads[1].clone();
    for field in REDIRECT_RECORD {
        write_u32(&mut redirect_payload, field, record.0)?;
    }
    let mut record_payload = placed.payloads[2].clone();
    for at in (0..record_payload.len().saturating_sub(3)).step_by(4) {
        if read_u32(&record_payload, at)? == placed.tags[2].0 {
            write_u32(&mut record_payload, at, record.0)?;
        }
    }
    Ok([handle_payload, redirect_payload, record_payload])
}

/// Gives a placed record's keyed descriptor the object `key` and slot `kind` and `index`.
pub(super) fn set_slot(record: &mut [u8], key: u32, kind: u16, index: u16) -> AuthoringResult<()> {
    let descriptor = keyed(record)?;
    write_u32(record, descriptor + DESCRIPTOR_KEY, key)?;
    write_u16(record, descriptor + DESCRIPTOR_SLOT_TYPE, kind)?;
    write_u16(record, descriptor + DESCRIPTOR_SLOT_INDEX, index)
}

/// A common slot definition carries its bubble at `+0x38`. The paired volume definition has
/// a different prefix and counts its volume rows there instead. Published interface bodies
/// also repeat the owner and source offset, so only the header-selected definition can match.
pub(super) fn set_bubble(
    record: &mut [u8],
    own: u32,
    bubble: u32,
    slots: &BTreeSet<(u32, u16, u16)>,
) -> AuthoringResult<()> {
    if read_i64(record, RECORD_SECONDARY)? == 0 {
        return Ok(());
    }
    let descriptor = keyed(record)?;
    let slot = (
        read_u32(record, descriptor + DESCRIPTOR_KEY)?,
        read_u16(record, descriptor + DESCRIPTOR_SLOT_TYPE)?,
        read_u16(record, descriptor + DESCRIPTOR_SLOT_INDEX)?,
    );
    if !slots.contains(&slot) {
        return Ok(());
    }
    let primary = target(record, 0x10)?;
    if primary < 4
        || descriptor < 4
        || read_u32(record, primary)? != own
        || read_u32(record, descriptor)? != own
        || read_u32(record, primary + 4)? != read_u32(record, descriptor - 4)?
        || read_u32(record, descriptor + 4)? != read_u32(record, primary - 4)?
        || read_u64(record, primary + 8)? != descriptor as u64
        || read_u64(record, descriptor + 8)? != primary as u64
    {
        return Err(invalid(format!(
            "Placed slot definition {own:08X} has inconsistent component links"
        )));
    }
    for field in [0x44, 0x48] {
        let schema = read_u32(record, descriptor + field)?;
        if schema != u32::MAX && schema >> 16 != 0x8080 {
            return Err(invalid(format!(
                "Placed slot definition {own:08X} has an invalid schema at +0x{field:X}"
            )));
        }
    }
    if read_u32(record, descriptor + 0x38)? != u32::MAX {
        write_u32(record, descriptor + 0x38, bubble)?;
    }
    Ok(())
}

/// `record` with its name, the last item of its payload, replaced by `name`.
pub(super) fn renamed(record: &[u8], name: &str) -> AuthoringResult<Vec<u8>> {
    let descriptor = keyed(record)?;
    let start = target(record, descriptor + DESCRIPTOR_NAME)?;
    if start < 12
        || read_u32(record, start - 12)? != NAME_CLASS
        || !record[start..].ends_with(&[0])
        || read_u64(record, start - 8)? != (record.len() - start) as u64
    {
        return Err(invalid("A placed record's name is not its last item"));
    }
    let mut payload = record[..start].to_vec();
    let length = name.len() as u64 + 1;
    payload[start - 8..start].copy_from_slice(&length.to_le_bytes());
    payload.extend_from_slice(name.as_bytes());
    payload.push(0);
    synchronize_payload_size(payload)
}

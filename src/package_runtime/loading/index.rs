//! Read-only decoding of native loading indexes. Native package IDs are retained verbatim.
use crate::package_payload::{i64_at, relative_offset, u16_at, u32_at, u64_at};
use std::collections::{BTreeMap, BTreeSet};

const MARKER: u32 = 0x8080_9FBD;
const GROUP_CLASS: u64 = 0x8080_9EFB;
const BITMAP_CLASS: u64 = 0x8080_000B;
const INDEX_CLASS: u64 = 0x8080_000A;
const GROUP_START: usize = 0x50;
const GROUP_SIZE: usize = 0x28;

fn invalid(message: &str) -> String {
    message.to_owned()
}

#[derive(Clone, Debug, Default)]
pub struct Group {
    pub bitmap: Vec<u32>,
    pub indices: Vec<u16>,
}

fn array(
    payload: &[u8],
    descriptor: usize,
    class: u64,
    stride: usize,
    max: usize,
) -> Result<Vec<u8>, String> {
    let count = usize::try_from(u64_at(payload, descriptor)?)
        .map_err(|_| invalid("Dependency array count overflow"))?;
    let relative = u64_at(payload, descriptor + 8)?;
    if count == 0 {
        if relative != 0 {
            return Err(invalid("Empty dependency array has a non-null pointer"));
        }
        return Ok(Vec::new());
    }
    if count > max {
        return Err(invalid(
            "Dependency array exceeds its native entry-index limit",
        ));
    }
    let header = relative_offset(descriptor, 8, i64_at(payload, descriptor + 8)?)?;
    if header < 4
        || u32_at(payload, header - 4)? != MARKER
        || u64_at(payload, header)? != count as u64
        || u64_at(payload, header + 8)? != class
    {
        return Err(invalid(
            "Dependency array marker, class, or repeated count is invalid",
        ));
    }
    let start = header
        .checked_add(16)
        .ok_or_else(|| invalid("Dependency array offset overflow"))?;
    let end = start
        .checked_add(count * stride)
        .ok_or_else(|| invalid("Dependency array size overflow"))?;
    payload
        .get(start..end)
        .map(<[u8]>::to_vec)
        .ok_or_else(|| invalid("Dependency array exits payload"))
}

pub fn decode(payload: &[u8], companion: u32, owner: u32) -> Result<BTreeMap<u16, Group>, String> {
    if payload.len() < GROUP_START
        || u64_at(payload, 0)? != payload.len() as u64
        || u32_at(payload, 8)? != companion
        || u32_at(payload, 12)? != owner
        || payload[0x20..0x3C].iter().any(|b| *b != 0)
    {
        return Err(invalid(
            "Shared dependency index identity or fixed envelope is invalid",
        ));
    }
    let rows = array(payload, 0x10, GROUP_CLASS, GROUP_SIZE, 4096)?;
    if rows.is_empty() || relative_offset(0x10, 8, i64_at(payload, 0x18)?)? != 0x40 {
        return Err(invalid(
            "Shared dependency index has no canonical package-group table",
        ));
    }
    let mut groups = BTreeMap::new();
    let mut previous = None;
    for index in 0..rows.len() / GROUP_SIZE {
        let row = GROUP_START + index * GROUP_SIZE;
        let package = u16::try_from(u64_at(payload, row)?)
            .map_err(|_| invalid("Dependency package id exceeds u16"))?;
        // Stock indexes retain groups outside the installed/header-authoring window (for
        // example 0x0E06..0x0EC0). Preserve those encoded dependencies; do not prune them.
        if package > 0x1FFF || previous.is_some_and(|p| package <= p) {
            return Err(invalid(
                "Dependency package groups are invalid or not strictly sorted",
            ));
        }
        previous = Some(package);
        let bitmap = array(payload, row + 8, BITMAP_CLASS, 4, 256)?
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().expect("word")))
            .collect::<Vec<_>>();
        let sparse = array(payload, row + 0x18, INDEX_CLASS, 2, 8192)?;
        let indices = (0..sparse.len() / 2)
            .map(|i| u16_at(&sparse, i * 2))
            .collect::<Result<Vec<_>, String>>()?;
        if indices.iter().any(|i| *i >= 8192)
            || !indices.windows(2).all(|w| w[0] < w[1])
            || indices.iter().any(|i| {
                bitmap
                    .get(*i as usize / 32)
                    .is_some_and(|word| word & (1 << (*i % 32)) != 0)
            })
        {
            return Err(invalid(
                "Dependency indices are invalid, duplicated, or unsorted",
            ));
        }
        groups.insert(package, Group { bitmap, indices });
    }
    Ok(groups)
}
pub fn entries(groups: &BTreeMap<u16, Group>) -> BTreeSet<(u16, u16)> {
    let mut entries = BTreeSet::new();
    for (&package, group) in groups {
        for (word_index, word) in group.bitmap.iter().enumerate() {
            for bit in 0..32 {
                if word & (1 << bit) != 0 {
                    entries.insert((package, (word_index * 32 + bit) as u16));
                }
            }
        }
        entries.extend(group.indices.iter().map(|&entry| (package, entry)));
    }
    entries
}

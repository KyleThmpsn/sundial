//! Little-endian access to entry bytes and typed array descriptors, as Sunrise's table readers
//! use them (`reader/layout.h`, `tables/definition_index_table.cpp`).
use crate::package_payload::bytes_at;

/// The marker in front of every typed array header.
pub(super) const ARRAY_MARKER: u32 = 0x8080_9FBD;
/// A typed array header holds a u64 count, a u32 element class and padding. Its marker sits this
/// many bytes before it and its rows follow it.
pub(super) const MARKER_BACK: usize = 4;
pub(super) const HEADER_SIZE: usize = 16;
/// Arrays larger than this are not definition tables.
const MAXIMUM_ARRAY_COUNT: u64 = 300_000;
/// Tag-like class and marker words carry this high half.
const TAG_HIGH_HALF: u32 = 0x8080;

pub(super) fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    bytes_at(bytes, at).ok().map(u16::from_le_bytes)
}
pub(super) fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    bytes_at(bytes, at).ok().map(u32::from_le_bytes)
}
pub(super) fn u64_at(bytes: &[u8], at: usize) -> Option<u64> {
    bytes_at(bytes, at).ok().map(u64::from_le_bytes)
}
pub(super) fn i64_at(bytes: &[u8], at: usize) -> Option<i64> {
    bytes_at(bytes, at).ok().map(i64::from_le_bytes)
}

/// Writes `value` at `at`. Callers have placed `at` inside `bytes`.
pub(super) fn put(bytes: &mut [u8], at: usize, value: &[u8]) {
    bytes[at..at + value.len()].copy_from_slice(value);
}

/// `value` rounded up to `unit`, a power of two.
pub(super) fn align(value: u64, unit: u64) -> u64 {
    (value + unit - 1) & !(unit - 1)
}

/// One typed array a descriptor names. `data` is 0 when it has no header.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Array {
    pub count: u64,
    pub data: usize,
    pub class: u32,
}

/// The array whose descriptor sits at `descriptor`. An empty array may have no header at all, or
/// a header that repeats its zero count.
pub(super) fn optional_array_at(bytes: &[u8], descriptor: usize) -> Option<Array> {
    let count = u64_at(bytes, descriptor)?;
    let relative = i64_at(bytes, descriptor.checked_add(8)?)?;
    if count > MAXIMUM_ARRAY_COUNT {
        return None;
    }
    if count == 0 && relative == 0 {
        return Some(Array::default());
    }
    let header = i64::try_from(descriptor + 8).ok()?.checked_add(relative)?;
    let header = usize::try_from(header).ok()?;
    if header < MARKER_BACK || header.checked_add(HEADER_SIZE)? > bytes.len() {
        return None;
    }
    let marker = u32_at(bytes, header - MARKER_BACK)?;
    let class = u32_at(bytes, header + 8)?;
    if u64_at(bytes, header)? != count
        || marker >> 16 != TAG_HIGH_HALF
        || class >> 16 != TAG_HIGH_HALF
    {
        return None;
    }
    Some(Array {
        count,
        data: header + HEADER_SIZE,
        class,
    })
}

/// Whether `[at, at + size)` lies inside `bytes`.
pub(super) fn fits(bytes: &[u8], at: u64, size: u64) -> bool {
    at <= bytes.len() as u64 && size <= bytes.len() as u64 - at
}

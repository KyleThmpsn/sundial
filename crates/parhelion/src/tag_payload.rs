//! Bounds-checked primitives for fixed-build investment tag payloads.

use crate::{AuthoringResult, error::invalid};

pub(crate) fn contains_u32_row_key(
    data: &[u8],
    rows: usize,
    count: usize,
    stride: usize,
    value: u32,
) -> AuthoringResult<bool> {
    Ok(find_u32_row_key(data, rows, count, stride, value)?.is_some())
}

pub(crate) fn find_u32_row_key(
    data: &[u8],
    rows: usize,
    count: usize,
    stride: usize,
    value: u32,
) -> AuthoringResult<Option<usize>> {
    for index in 0..count {
        let row = checked_row_offset(rows, index, stride, 0)?;
        if read_u32(data, row)? == value {
            return Ok(Some(index));
        }
    }
    Ok(None)
}

pub(crate) fn contains_u32_at_offset(
    data: &[u8],
    rows: usize,
    count: usize,
    stride: usize,
    field: usize,
    value: u32,
) -> AuthoringResult<bool> {
    for index in 0..count {
        let row = checked_row_offset(rows, index, stride, field)?;
        if read_u32(data, row)? == value {
            return Ok(true);
        }
    }
    Ok(false)
}

fn checked_row_offset(
    rows: usize,
    index: usize,
    stride: usize,
    field: usize,
) -> AuthoringResult<usize> {
    rows.checked_add(
        index
            .checked_mul(stride)
            .ok_or_else(|| invalid("Tag-payload row offset overflowed"))?,
    )
    .and_then(|row| row.checked_add(field))
    .ok_or_else(|| invalid("Tag-payload row offset overflowed"))
}

pub(crate) fn array_at(
    data: &[u8],
    descriptor: usize,
) -> AuthoringResult<(usize, usize, usize, u32)> {
    sundial::package_authoring::native_payload::native_array_at(data, descriptor).map_err(invalid)
}

pub(crate) fn set_array_count(
    data: &mut [u8],
    descriptor: usize,
    header: usize,
    count: usize,
) -> AuthoringResult<()> {
    let count = u64::try_from(count).map_err(|_| invalid("Array count is too large"))?;
    write_u64(data, descriptor, count)?;
    write_u64(data, header, count)
}

pub(crate) fn relative_target(data: &[u8], pointer: usize) -> AuthoringResult<usize> {
    sundial::package_authoring::native_payload::relative_offset(
        pointer,
        0,
        read_i64(data, pointer)?,
    )
    .map_err(invalid)
}

pub(crate) fn read_u16(data: &[u8], offset: usize) -> AuthoringResult<u16> {
    Ok(u16::from_le_bytes(read_array(data, offset)?))
}

pub(crate) fn write_relative_pointer(
    data: &mut [u8],
    pointer: usize,
    target: usize,
) -> AuthoringResult<()> {
    let relative = i64::try_from(target)
        .and_then(|target| i64::try_from(pointer).map(|pointer| target - pointer))
        .map_err(|_| invalid("Relative pointer does not fit 64 bits"))?;
    write_i64(data, pointer, relative)
}

pub(crate) fn read_u8(data: &[u8], offset: usize) -> AuthoringResult<u8> {
    Ok(read_array::<1>(data, offset)?[0])
}

pub(crate) fn read_u32(data: &[u8], offset: usize) -> AuthoringResult<u32> {
    Ok(u32::from_le_bytes(read_array(data, offset)?))
}

pub(crate) fn read_i32(data: &[u8], offset: usize) -> AuthoringResult<i32> {
    Ok(i32::from_le_bytes(read_array(data, offset)?))
}

pub(crate) fn read_u64(data: &[u8], offset: usize) -> AuthoringResult<u64> {
    Ok(u64::from_le_bytes(read_array(data, offset)?))
}

pub(crate) fn read_i64(data: &[u8], offset: usize) -> AuthoringResult<i64> {
    Ok(i64::from_le_bytes(read_array(data, offset)?))
}

pub(crate) fn read_array<const N: usize>(data: &[u8], offset: usize) -> AuthoringResult<[u8; N]> {
    sundial::package_authoring::native_payload::bytes_at(data, offset).map_err(invalid)
}

pub(crate) fn write_u16(data: &mut [u8], offset: usize, value: u16) -> AuthoringResult<()> {
    write_bytes(data, offset, &value.to_le_bytes())
}

pub(crate) fn write_u32(data: &mut [u8], offset: usize, value: u32) -> AuthoringResult<()> {
    write_bytes(data, offset, &value.to_le_bytes())
}

pub(crate) fn write_i32(data: &mut [u8], offset: usize, value: i32) -> AuthoringResult<()> {
    write_bytes(data, offset, &value.to_le_bytes())
}

pub(crate) fn write_u64(data: &mut [u8], offset: usize, value: u64) -> AuthoringResult<()> {
    write_bytes(data, offset, &value.to_le_bytes())
}

pub(crate) fn write_i64(data: &mut [u8], offset: usize, value: i64) -> AuthoringResult<()> {
    write_bytes(data, offset, &value.to_le_bytes())
}

pub(crate) fn write_localized_reference(
    data: &mut [u8],
    offset: usize,
    table_index: u32,
    string_hash: u32,
) -> AuthoringResult<()> {
    let hash_offset = offset
        .checked_add(4)
        .ok_or_else(|| invalid("Localized-reference offset overflowed"))?;
    write_u32(data, offset, table_index)?;
    write_u32(data, hash_offset, string_hash)
}

pub(crate) fn write_bytes(data: &mut [u8], offset: usize, value: &[u8]) -> AuthoringResult<()> {
    sundial::package_authoring::native_payload::write_bytes(data, offset, value).map_err(invalid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_searches_reject_offset_overflow() {
        assert!(
            contains_u32_at_offset(&[], usize::MAX, 1, 4, 1, 0)
                .unwrap_err()
                .to_string()
                .contains("row offset overflowed")
        );
    }
}

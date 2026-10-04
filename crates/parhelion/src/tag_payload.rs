//! Bounds-checked primitives for fixed-build investment tag payloads.

use sundial::package_authoring::{PackageManager, investment_schema::ITEM_INDEX_ROW_SIZE};
use tiger_pkg::TagHash;

use crate::{
    AuthoringResult,
    error::{invalid, validation},
};

/// The row size shared by the investment index tables.
const INDEX_ROW_SIZE: usize = ITEM_INDEX_ROW_SIZE;

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
    sundial::package_authoring::native_payload::native_array_at(data, descriptor)
        .map_err(|error| invalid(format!("Native array at 0x{descriptor:X}: {error}")))
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

/// The marker in front of every native array header.
const ARRAY_MARKER: u32 = 0x8080_9FBD;

/// Appends a native array to the payload and points `descriptor` at it: the marker, the row count
/// and element class, then the rows, which start on a 16-byte boundary. With no rows the
/// descriptor is cleared instead.
pub(crate) fn append_native_array(
    payload: &mut Vec<u8>,
    descriptor: usize,
    class: u32,
    count: usize,
    rows: &[u8],
) -> AuthoringResult<()> {
    if count == 0 {
        write_u64(payload, descriptor, 0)?;
        return write_i64(payload, descriptor + 8, 0);
    }
    let start = payload
        .len()
        .checked_add(0x23)
        .ok_or_else(|| invalid("Native array alignment overflowed"))?
        & !0xF;
    payload.resize(start - 0x14, 0);
    payload.extend_from_slice(&ARRAY_MARKER.to_le_bytes());
    let count = u64::try_from(count).map_err(|_| invalid("Array count is too large"))?;
    payload.extend_from_slice(&count.to_le_bytes());
    payload.extend_from_slice(&u64::from(class).to_le_bytes());
    payload.extend_from_slice(rows);
    write_u64(payload, descriptor, count)?;
    write_relative_pointer(payload, descriptor + 8, start - 16)
}

pub(crate) fn bounded_relative_target(
    data: &[u8],
    field: usize,
    description: &str,
) -> AuthoringResult<usize> {
    let relative = read_i64(data, field)?;
    let target = field.checked_add_signed(relative as isize).ok_or_else(|| {
        crate::error::input(format!("{description} relative reference overflowed"))
    })?;
    if target >= data.len() {
        return Err(crate::error::input(format!(
            "{description} relative reference points outside its payload"
        )));
    }
    Ok(target)
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

pub(crate) fn read_tag(
    manager: &PackageManager,
    tag: TagHash,
    description: &str,
) -> AuthoringResult<Vec<u8>> {
    manager
        .read_tag(tag)
        .map_err(|error| invalid(format!("Could not read {description} tag {tag}: {error}")))
}

/// Writes the payload's own length into its leading size field.
pub(crate) fn synchronize_payload_size(mut data: Vec<u8>) -> AuthoringResult<Vec<u8>> {
    let size =
        u64::try_from(data.len()).map_err(|_| invalid("Payload size does not fit 64 bits"))?;
    write_u64(&mut data, 0, size)?;
    if read_u64(&data, 0)? != size {
        return Err(validation("Serialized payload extent was not updated"));
    }
    Ok(data)
}

/// An index table whose fixed rows end the payload: its count, header and first row.
pub(crate) fn terminal_index_table_layout(
    data: &[u8],
    expected_class: u32,
    description: &str,
) -> AuthoringResult<(usize, usize, usize)> {
    let (count, header, rows, class) = array_at(data, 8)?;
    let rows_end = rows
        .checked_add(
            count
                .checked_mul(INDEX_ROW_SIZE)
                .ok_or_else(|| invalid(format!("{description} row extent overflowed")))?,
        )
        .ok_or_else(|| invalid(format!("{description} row extent overflowed")))?;
    if class != expected_class || rows_end != data.len() {
        return Err(invalid(format!(
            "{description} is not a terminal native fixed-row array"
        )));
    }
    Ok((count, header, rows))
}

/// Appends a copy of the row at `template_index` under a new hash and tag.
pub(crate) fn append_index_row(
    mut data: Vec<u8>,
    template_index: usize,
    new_hash: u32,
    new_tag: TagHash,
    expected_class: u32,
    description: &str,
) -> AuthoringResult<Vec<u8>> {
    let (count, header, rows) = terminal_index_table_layout(&data, expected_class, description)?;
    if template_index >= count {
        return Err(invalid(format!(
            "{description} donor template is outside the table"
        )));
    }
    if contains_u32_row_key(&data, rows, count, INDEX_ROW_SIZE, new_hash)? {
        return Err(invalid(format!(
            "Authored hash 0x{new_hash:08X} already exists in the {description}"
        )));
    }
    let end = data.len();
    let template = data
        [rows + template_index * INDEX_ROW_SIZE..rows + (template_index + 1) * INDEX_ROW_SIZE]
        .to_vec();
    data.extend_from_slice(&template);
    write_u32(&mut data, end, new_hash)?;
    write_u32(&mut data, end + 16, new_tag.0)?;
    set_array_count(&mut data, 8, header, count + 1)?;
    let (authored_count, _, authored_rows) =
        terminal_index_table_layout(&data, expected_class, description)?;
    if authored_count != count + 1
        || read_u32(&data, authored_rows + count * INDEX_ROW_SIZE)? != new_hash
        || read_u32(&data, authored_rows + count * INDEX_ROW_SIZE + 16)? != new_tag.0
    {
        return Err(validation(format!(
            "Authored {description} row is inconsistent"
        )));
    }
    Ok(data)
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

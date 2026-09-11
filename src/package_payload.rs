//! Bounds-checked reads and writes for native package tag payloads.

pub fn native_array_at(
    data: &[u8],
    descriptor: usize,
) -> Result<(usize, usize, usize, u32), String> {
    let count_raw = u64_at(data, descriptor)?;
    let count = usize::try_from(count_raw).map_err(|_| "Package array is too large")?;
    let pointer = descriptor
        .checked_add(8)
        .ok_or("Package array pointer overflowed")?;
    let header = relative_offset(descriptor, 8, i64_at(data, pointer)?)?;
    // Even an empty array owns a complete 16-byte header before its row data.
    bytes_at::<16>(data, header)?;
    if u64_at(data, header)? != count_raw {
        return Err("Package array count mismatch".into());
    }
    let rows = header
        .checked_add(16)
        .ok_or("Package array row offset overflowed")?;
    let class_offset = header
        .checked_add(8)
        .ok_or("Package array class offset overflowed")?;
    Ok((count, header, rows, u32_at(data, class_offset)?))
}

pub(crate) fn array_at(data: &[u8], descriptor: usize) -> Result<(usize, usize, u32), String> {
    let (count, _, rows, class) = native_array_at(data, descriptor)?;
    Ok((count, rows, class))
}

pub(crate) fn u16_at(data: &[u8], offset: usize) -> Result<u16, String> {
    Ok(u16::from_le_bytes(bytes_at(data, offset)?))
}

pub(crate) fn bool_at(data: &[u8], offset: usize) -> Result<bool, String> {
    match bytes_at::<1>(data, offset)?[0] {
        0 => Ok(false),
        1 => Ok(true),
        value => Err(format!("Invalid package boolean {value} at {offset}")),
    }
}

pub(crate) fn i32_at(data: &[u8], offset: usize) -> Result<i32, String> {
    Ok(i32::from_le_bytes(bytes_at(data, offset)?))
}

pub(crate) fn u32_at(data: &[u8], offset: usize) -> Result<u32, String> {
    Ok(u32::from_le_bytes(bytes_at(data, offset)?))
}

pub(crate) fn u64_at(data: &[u8], offset: usize) -> Result<u64, String> {
    Ok(u64::from_le_bytes(bytes_at(data, offset)?))
}

pub(crate) fn i64_at(data: &[u8], offset: usize) -> Result<i64, String> {
    Ok(i64::from_le_bytes(bytes_at(data, offset)?))
}

pub fn bytes_at<const N: usize>(data: &[u8], offset: usize) -> Result<[u8; N], String> {
    let end = offset
        .checked_add(N)
        .ok_or_else(|| format!("Package offset overflowed at {offset}"))?;
    data.get(offset..end)
        .ok_or_else(|| format!("Package data ended at {offset}"))?
        .try_into()
        .map_err(|_| format!("Invalid {N}-byte package value"))
}

pub fn write_bytes(data: &mut [u8], offset: usize, value: &[u8]) -> Result<(), String> {
    let end = offset
        .checked_add(value.len())
        .ok_or_else(|| format!("Package write overflowed at 0x{offset:X}"))?;
    data.get_mut(offset..end)
        .ok_or_else(|| format!("Package write ended at 0x{offset:X}"))?
        .copy_from_slice(value);
    Ok(())
}

pub fn relative_offset(base: usize, bias: usize, relative: i64) -> Result<usize, String> {
    let origin = base
        .checked_add(bias)
        .ok_or("Package relative pointer overflowed")?;
    if relative >= 0 {
        let relative =
            usize::try_from(relative).map_err(|_| "Package relative pointer is too large")?;
        origin
            .checked_add(relative)
            .ok_or_else(|| "Package relative pointer overflowed".into())
    } else {
        let magnitude = usize::try_from(relative.unsigned_abs())
            .map_err(|_| "Package relative pointer is too small")?;
        origin
            .checked_sub(magnitude)
            .ok_or_else(|| "Package relative pointer points before the data".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejected_payload_writes_leave_every_byte_unchanged() {
        for (offset, value) in [(usize::MAX, &[1][..]), (3, &[1, 2][..]), (5, &[][..])] {
            let mut data = [10, 20, 30, 40];
            assert!(write_bytes(&mut data, offset, value).is_err());
            assert_eq!(data, [10, 20, 30, 40]);
        }
        assert!(bytes_at::<4>(&[0; 4], usize::MAX).is_err());
    }

    #[test]
    fn payload_writes_preserve_neighbors_and_accept_an_empty_end_range() {
        let mut data = [10, 20, 30, 40];
        write_bytes(&mut data, 1, &[2, 3]).unwrap();
        write_bytes(&mut data, 4, &[]).unwrap();
        assert_eq!(data, [10, 2, 3, 40]);
    }

    #[test]
    fn package_booleans_are_strict() {
        assert_eq!(bool_at(&[0], 0), Ok(false));
        assert_eq!(bool_at(&[1], 0), Ok(true));
        assert!(bool_at(&[2], 0).is_err());
        assert!(bool_at(&[], 0).is_err());
    }

    #[test]
    fn relative_offsets_support_both_directions() {
        assert_eq!(relative_offset(8, 0, 8), Ok(16));
        assert_eq!(relative_offset(8, 0, -4), Ok(4));
        assert!(relative_offset(0, 0, -1).is_err());
    }
}

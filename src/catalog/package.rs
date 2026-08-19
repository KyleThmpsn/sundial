use std::{fs, path::Path, time::UNIX_EPOCH};

pub fn validate_install(install: &Path) -> Result<(), String> {
    if install.join("destiny2.exe").is_file()
        && install.join("packages").is_dir()
        && install.join("bin/x64/oo2core_3_win64.dll").is_file()
    {
        Ok(())
    } else {
        Err("Not a Shadowkeep install: expected destiny2.exe, packages, and bin\\x64\\oo2core_3_win64.dll".into())
    }
}

pub(super) fn install_fingerprint(install: &Path) -> Result<String, String> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(install.join("packages"))
        .map_err(|e| format!("Could not read packages: {e}"))?
    {
        let entry = entry.map_err(|e| format!("Could not inspect packages: {e}"))?;
        let path = entry.path();
        if path
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.eq_ignore_ascii_case("pkg"))
        {
            let meta = entry
                .metadata()
                .map_err(|e| format!("Could not inspect {}: {e}", path.display()))?;
            let modified = meta
                .modified()
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |duration| duration.as_secs());
            entries.push(format!(
                "{}:{}:{}",
                entry.file_name().to_string_lossy(),
                meta.len(),
                modified
            ));
        }
    }
    entries.sort();
    Ok(entries.join("|"))
}

pub(super) fn array_at(data: &[u8], descriptor: usize) -> Result<(usize, usize, u32), String> {
    let count_raw = u64_at(data, descriptor)?;
    let count = usize::try_from(count_raw).map_err(|_| "Package array is too large")?;
    let pointer = descriptor
        .checked_add(8)
        .ok_or("Package array pointer overflowed")?;
    let header = relative_offset(descriptor, 8, i64_at(data, pointer)?)?;
    if u64_at(data, header)? != count_raw {
        return Err("Package array count mismatch".into());
    }
    let rows = header
        .checked_add(16)
        .ok_or("Package array row offset overflowed")?;
    let class_offset = header
        .checked_add(8)
        .ok_or("Package array class offset overflowed")?;
    Ok((count, rows, u32_at(data, class_offset)?))
}

pub(super) fn u16_at(data: &[u8], offset: usize) -> Result<u16, String> {
    Ok(u16::from_le_bytes(bytes_at(data, offset)?))
}

pub(super) fn bool_at(data: &[u8], offset: usize) -> Result<bool, String> {
    match bytes_at::<1>(data, offset)?[0] {
        0 => Ok(false),
        1 => Ok(true),
        value => Err(format!("Invalid package boolean {value} at {offset}")),
    }
}

pub(super) fn i32_at(data: &[u8], offset: usize) -> Result<i32, String> {
    Ok(i32::from_le_bytes(bytes_at(data, offset)?))
}

pub(super) fn u32_at(data: &[u8], offset: usize) -> Result<u32, String> {
    Ok(u32::from_le_bytes(bytes_at(data, offset)?))
}

pub(super) fn u64_at(data: &[u8], offset: usize) -> Result<u64, String> {
    Ok(u64::from_le_bytes(bytes_at(data, offset)?))
}

pub(super) fn i64_at(data: &[u8], offset: usize) -> Result<i64, String> {
    Ok(i64::from_le_bytes(bytes_at(data, offset)?))
}

fn bytes_at<const N: usize>(data: &[u8], offset: usize) -> Result<[u8; N], String> {
    let end = offset
        .checked_add(N)
        .ok_or_else(|| format!("Package offset overflowed at {offset}"))?;
    data.get(offset..end)
        .ok_or_else(|| format!("Package data ended at {offset}"))?
        .try_into()
        .map_err(|_| format!("Invalid {N}-byte package value"))
}

pub(super) fn relative_offset(base: usize, bias: usize, relative: i64) -> Result<usize, String> {
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
    fn package_booleans_are_strict() {
        assert_eq!(bool_at(&[0], 0), Ok(false));
        assert_eq!(bool_at(&[1], 0), Ok(true));
        assert!(bool_at(&[2], 0).is_err());
        assert!(bool_at(&[], 0).is_err());
    }
}

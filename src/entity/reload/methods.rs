//! Reload's published interface retains concrete method dispatch independently of its class.
use super::*;
use crate::package_payload::{native_array_at, rows_fit};

fn read(manager: &PackageManager, tag: u32) -> Result<Vec<(u32, u32)>, String> {
    if manager
        .get_entry(TagHash(tag))
        .is_none_or(|entry| entry.reference != 0x8080_9C54)
    {
        return Err("Reload publisher has no native method resource".into());
    }
    let bytes = manager.read_tag(TagHash(tag)).map_err(|e| e.to_string())?;
    validate_file_size(&bytes, "Reload methods")?;
    if read_u32(&bytes, 8)? != 0x8080_3AFA || read_u32(&bytes, 12)? != 0x8080_3AF9 {
        return Err("Reload publisher has another interface".into());
    }
    let (count, _, rows, class) = native_array_at(&bytes, 16)?;
    if count != 25 || class != 0x8080_9C56 {
        return Err("Reload publisher has another method layout".into());
    }
    rows_fit(&bytes, rows, count, 24)?;
    (0..count)
        .map(|index| {
            let at = rows + index * 24;
            if read_u64(&bytes, at + 8)? != 0 || read_u64(&bytes, at + 16)? != 0 {
                return Err("Reload method has nonzero persisted linkage".into());
            }
            Ok((read_u32(&bytes, at)?, read_u32(&bytes, at + 4)?))
        })
        .collect()
}

pub(super) fn magazine(
    manager: &PackageManager,
    owner: &[u8],
    reload: WeaponComponentBinding,
) -> Result<(usize, u32), String> {
    let instance = usize::try_from(reload.resource_offset).map_err(|_| "Reload offset overflow")?;
    let definition = usize::try_from(read_u64(owner, instance + 8)?)
        .map_err(|_| "Reload definition offset overflow")?;
    // 80803843 inherits this 80803845 publisher at +B0. Its method resource at +B8
    // must change as well as the reciprocal class words, or shell overrides still execute.
    let field = definition
        .checked_add(0xB8)
        .ok_or("Reload publisher offset overflow")?;
    let source = read(manager, read_u32(owner, field)?)?;
    // Native magazine Reload's shared interface implementation. This is a method resource,
    // independent of any weapon. Admit only the checked inheritance and complete slot mapping.
    let target_tag = 0x815B_8946;
    let target = read(manager, target_tag)?;
    let mut overrides = 0;
    for ((class, method), expected) in source.into_iter().zip(target) {
        let mapped = match class {
            0x8080_3844 => (class, method),
            0x8080_3842 => {
                overrides += 1;
                let method = match method {
                    0 => 27,
                    1 => 22,
                    2 => 23,
                    3 => 18,
                    4 => 10,
                    5 => 12,
                    _ => return Err("Shell reload has an unqualified method override".into()),
                };
                (0x8080_3844, method)
            }
            _ => return Err("Reload method has an unexpected implementation".into()),
        };
        if mapped != expected {
            return Err("Magazine and shell reload method slots disagree".into());
        }
    }
    if overrides != 6 {
        return Err("Shell reload has an unexpected override set".into());
    }
    Ok((field, target_tag))
}

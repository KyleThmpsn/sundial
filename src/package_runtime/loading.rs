//! Checked reads of the native investment loading index, independent of UI caches.
use std::collections::BTreeSet;

use tiger_pkg::{PackageManager, TagHash};

use crate::package_payload::{u32_at, u64_at};

pub mod index;

const ROOT: u32 = 0x80EC_3F62;
const COMPANION: u32 = 0x80EE_8CBD;

pub(crate) fn investment(manager: &PackageManager) -> Result<BTreeSet<u32>, String> {
    let root = read(manager, ROOT, 16, 0x8080_56A6)?;
    if u32_at(&root, 0x10)? != 0x80EC_3F60 {
        return Err("Investment loading root has an unsupported assignment map".into());
    }
    dependencies(&read(manager, COMPANION, 8, 0x8080_9EF9)?, COMPANION, ROOT)
}

pub(crate) fn read(
    manager: &PackageManager,
    tag: u32,
    file_type: u8,
    class: u32,
) -> Result<Vec<u8>, String> {
    let entry = manager
        .get_entry(TagHash(tag))
        .ok_or_else(|| format!("Loading resource 0x{tag:08X} is not installed"))?;
    if entry.file_type != file_type || entry.reference != class {
        return Err(format!(
            "Loading resource 0x{tag:08X} has unsupported type or class"
        ));
    }
    let payload = manager
        .read_tag(TagHash(tag))
        .map_err(|error| format!("Could not read loading resource 0x{tag:08X}: {error}"))?;
    if payload.len() != entry.file_size as usize || u64_at(&payload, 0)? != payload.len() as u64 {
        return Err(format!("Loading resource 0x{tag:08X} has an invalid size"));
    }
    Ok(payload)
}

pub(crate) fn dependencies(
    payload: &[u8],
    companion: u32,
    owner: u32,
) -> Result<BTreeSet<u32>, String> {
    let groups = index::decode(payload, companion, owner)?;
    let result: BTreeSet<_> = index::entries(&groups)
        .into_iter()
        .map(|(package, entry)| TagHash::new(package, entry).0)
        .collect();
    if !result.contains(&owner) || !result.contains(&companion) {
        return Err("Loading index omits its owner or companion".into());
    }
    Ok(result)
}

#[cfg(test)]
mod tests;

//! Wheel settings use a raw pointer, outside reflected reference traversal.
use super::*;
use crate::package_payload::{bytes_at, i64_at, relative_offset, rows_fit};

pub(super) fn requirements(
    manager: &PackageManager,
    owner: u32,
    payload: &[u8],
    definition: usize,
) -> Result<Vec<Requirement>, String> {
    bytes_at::<0x1D0>(payload, definition)?;
    let descriptor = definition + 0x130;
    if u64_at(payload, descriptor)? == 0 {
        return Ok(Vec::new());
    }
    let (count, _, rows, class) = native_array_at(payload, descriptor)?;
    if class != 0x8080_3D4F {
        return Err(format!(
            "Component 0x{owner:08X} has an unsupported wheel array"
        ));
    }
    rows_fit(payload, rows, count, 0x88)?;
    let mut required = BTreeMap::new();
    for index in 0..count {
        let pointer = rows + index * 0x88 + 0x78;
        let relative = i64_at(payload, pointer)?;
        if relative == 0 {
            return Err(format!(
                "Component 0x{owner:08X} has a wheel without settings"
            ));
        }
        let settings = relative_offset(pointer, 0, relative)?;
        // Native 80803D52 settings do not require a preceding class header.
        // Their concrete layout follows from this field of 80803D4F.
        bytes_at::<0x58>(payload, settings)?;
        let table = u32_at(payload, settings + 0x50)?;
        if matches!(table, 0 | u32::MAX | 0x811C_9DC5) {
            continue;
        }
        if let std::collections::btree_map::Entry::Vacant(entry) = required.entry(table) {
            loading::read(manager, table, 8, 0x8080_8BCD)?;
            entry.insert(Requirement {
                tag: table,
                referenced_by: owner,
                role: format!("wheel impact table at +0x{:X}", settings + 0x50),
            });
        }
    }
    // Tables also select effect graphs and audio events. Follow their declared
    // resources and backing data with the existing bounded, cycle-aware walk.
    for reference in references::closure(manager, required.keys().copied())? {
        required
            .entry(reference.tag)
            .or_insert_with(|| Requirement {
                tag: reference.tag,
                referenced_by: reference.parent,
                role: if reference.offset == usize::MAX {
                    "wheel effect backing data".into()
                } else {
                    format!("wheel effect reference at +0x{:X}", reference.offset)
                },
            });
    }
    Ok(required.into_values().collect())
}

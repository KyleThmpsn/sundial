//! Contact prerequisites a private owner enrolls itself. Wheel settings use a raw pointer, outside
//! reflected reference traversal. A ground sensor's numeric provider names a dispatch resource
//! that only the stock tank's loading index holds, so an owner carrying copied sensors loads it.
use super::*;
use crate::package_payload::{bytes_at, i64_at, relative_offset, rows_fit};

pub(super) fn requirements(
    manager: &PackageManager,
    owner: u32,
    payload: &[u8],
    definition: usize,
) -> Result<Vec<Requirement>, String> {
    bytes_at::<0x1D0>(payload, definition)?;
    let mut required = wheels(manager, owner, payload, definition)?;
    for requirement in sensors(manager, owner, payload, definition)? {
        required.entry(requirement.tag).or_insert(requirement);
    }
    Ok(required.into_values().collect())
}

fn wheels(
    manager: &PackageManager,
    owner: u32,
    payload: &[u8],
    definition: usize,
) -> Result<BTreeMap<u32, Requirement>, String> {
    let descriptor = definition + 0x130;
    if u64_at(payload, descriptor)? == 0 {
        return Ok(BTreeMap::new());
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
    Ok(required)
}

/// The dispatch resource of each ground sensor's numeric provider. Sensor definitions are the
/// `0xA8`-byte `80803D68` rows at definition `+0x140`, and a row's `80809AE1` provider at `+0x88`
/// names its resource at `+0x90`. A channel bank input fed by the provider resolves that resource
/// with no null test (`005866F5`), so a sensor copied from the stock tank needs the tank's
/// `80C0D7B1` loaded wherever the copy is. An unresolved placeholder names a private resource the
/// owner's own references already load.
fn sensors(
    manager: &PackageManager,
    owner: u32,
    payload: &[u8],
    definition: usize,
) -> Result<Vec<Requirement>, String> {
    let descriptor = definition + 0x140;
    if u64_at(payload, descriptor)? == 0 {
        return Ok(Vec::new());
    }
    let (count, _, rows, class) = native_array_at(payload, descriptor)?;
    if class != 0x8080_3D68 {
        return Err(format!(
            "Component 0x{owner:08X} has an unsupported ground sensor array"
        ));
    }
    rows_fit(payload, rows, count, 0xA8)?;
    let mut required = BTreeMap::new();
    for index in 0..count {
        let provider = rows + index * 0xA8 + 0x88;
        if relative_offset(provider, 0, i64_at(payload, provider)?)? != provider - 0x18 {
            return Err(format!(
                "Component 0x{owner:08X} has a ground sensor without its value provider"
            ));
        }
        let dispatch = u32_at(payload, provider + 8)?;
        if matches!(dispatch, 0 | u32::MAX | 0x811C_9DC5) {
            continue;
        }
        if let std::collections::btree_map::Entry::Vacant(entry) = required.entry(dispatch) {
            loading::read(manager, dispatch, 8, 0x8080_9C54)?;
            entry.insert(Requirement {
                tag: dispatch,
                referenced_by: owner,
                role: format!("ground sensor dispatch at +0x{:X}", provider + 8),
            });
        }
    }
    Ok(required.into_values().collect())
}

//! Qualification of the Glide bank's selected profile route.
use super::*;

/// Checks the actual event destination and interface dispatch before offering or compiling a
/// Glide profile. The destination object lives in the entity's event row. Its bank holds a
/// descriptor, not a serialized 4553 object.
pub fn validate_context(
    manager: &crate::package_runtime::reader::PackageManager,
    entity: &[u8],
    bank: &[u8],
) -> Result<(), String> {
    let source = relative_offset(0x10, 0, i64_at(bank, 0x10)?)?;
    if u32_at(bank, source + 4)? != 0x8080_4252 {
        return Ok(());
    }
    validate_glide_bank(bank)?;
    let definition = relative_offset(0x18, 0, i64_at(bank, 0x18)?)?;
    let descriptor = definition + 0x48;
    let interface = manager
        .read_tag(u32_at(bank, descriptor + 8)?)
        .map_err(|error| error.to_string())?;
    if u64_at(&interface, 0)? != interface.len() as u64
        || u32_at(&interface, 8)? != 0x8080_4553
        || u32_at(&interface, 12)? != 0x8080_4552
    {
        return Err("Glide bank has an incompatible interface".into());
    }
    let (count, _, rows, class) = native_array_at(&interface, 16)?;
    rows_fit(&interface, rows, count, 24)?;
    if count != 9
        || class != 0x8080_9C56
        || u32_at(&interface, rows + 2 * 24)? != 0x8080_44FC
        || u32_at(&interface, rows + 2 * 24 + 4)? != 3
    {
        return Err("Glide bank does not dispatch the verified profile handler".into());
    }
    let (count, _, rows, class) = native_array_at(entity, 0x20)?;
    rows_fit(entity, rows, count, 0x48)?;
    if class != 0x8080_9BC9 {
        return Err("Glide entity has an incompatible event table".into());
    }
    let (component_count, _, components, component_class) = native_array_at(entity, 0x10)?;
    rows_fit(entity, components, component_count, 12)?;
    if component_class != 0x8080_9C04 {
        return Err("Glide entity has an incompatible component table".into());
    }
    for row in (0..count).map(|index| rows + index * 0x48) {
        let destination = row + 0x28;
        if u32_at(entity, destination)? != u32_at(bank, source)?
            || u32_at(entity, destination + 4)? != 0x8080_4553
            || u64_at(entity, destination + 8)? != descriptor as u64
        {
            continue;
        }
        bytes_at::<32>(entity, destination)?;
        if u64_at(entity, destination + 0x18)? != 0 {
            return Err("Glide bank destination uses an unsupported selector".into());
        }
        let origin = row + 8;
        for end in [origin, destination] {
            let index = usize::try_from(u64_at(entity, end + 0x10)?)
                .map_err(|_| "Glide component index overflows")?;
            if index >= component_count
                || u32_at(entity, components + index * 12)? != u32_at(entity, end)?
            {
                return Err("Glide connection names a different component".into());
            }
        }
        if u32_at(entity, origin + 4)? != 0x8080_9BD9 {
            continue;
        }
        let owner = manager
            .read_tag(u32_at(entity, origin)?)
            .map_err(|error| error.to_string())?;
        let at = usize::try_from(u64_at(entity, origin + 8)?)
            .map_err(|_| "Glide reference offset overflows")?;
        bytes_at::<32>(&owner, at)?;
        let twin = usize::try_from(u64_at(&owner, at + 8)?)
            .map_err(|_| "Glide reference twin overflows")?;
        bytes_at::<80>(&owner, twin)?;
        if u32_at(&owner, at)? == u32_at(entity, origin)?
            && u32_at(&owner, twin)? == u32_at(entity, origin)?
            && u32_at(&owner, at + 4)? == 0x8080_9BD8
            && u32_at(&owner, twin + 4)? == 0x8080_9BD9
            && u64_at(&owner, twin + 8)? == at as u64
            && u64_at(&owner, at + 0x10)? == 1
            && u64_at(&owner, at + 0x18)? == 0x8080_4552
        {
            return Ok(());
        }
    }
    Err("Glide profile has no complete bank reference connection".into())
}

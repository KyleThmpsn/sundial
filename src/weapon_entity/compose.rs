//! Opt-in composition of additional native component owners with checked event wiring.
use super::*;
use crate::package_runtime::reader::PackageManager;

/// Fixed-width native table rows.
type Rows = Vec<Vec<u8>>;

/// Add complete owner partitions while retaining the host's rig and other components.
/// Singleton selection is explicit. Shared broadcast bindings gain the selected owners.
/// Conflicting events and bindings fail atomically, before the target is changed.
pub fn extend_weapon_components(
    manager: &PackageManager,
    target: &mut Vec<u8>,
    source: &[u8],
    owners: &BTreeSet<u32>,
    select: &BTreeSet<u32>,
) -> Result<(), String> {
    validate_weapon_entity(target)?;
    validate_weapon_entity(source)?;
    if owners.is_empty() {
        return Err("Component extension has no owners".into());
    }
    let original = target.clone();
    let mut result = original.clone();
    let mut components = rows(target, ENTITY_COMPONENTS_DESCRIPTOR, 12)?;
    let source_components = rows(source, ENTITY_COMPONENTS_DESCRIPTOR, 12)?;
    push_owners(&mut components, &source_components, owners)?;
    let source_map = rows(source, ENTITY_RESOURCE_MAP_DESCRIPTOR, 40)?;
    let source_descriptors = rows(source, ENTITY_RESOURCE_DESCRIPTORS_DESCRIPTOR, 24)?;
    let mut maps = rows(target, ENTITY_RESOURCE_MAP_DESCRIPTOR, 40)?;
    let mut descriptors = rows(target, ENTITY_RESOURCE_DESCRIPTORS_DESCRIPTOR, 24)?;
    let mut masks = rows(target, 0x78, 2)?;
    let mask_offset = masks.len();
    let source_masks = rows(source, 0x78, 2)?;
    for entity in [target.as_slice(), source] {
        let mask_array = native_array(entity, 0x78)?;
        if read_u32(entity, mask_array.rows - 8)? != 0x8080_0006 {
            return Err("Component condition mask array has an unexpected type".into());
        }
    }
    if mask_offset + source_masks.len() > i16::MAX as usize {
        return Err("Component condition mask table is too large".into());
    }
    masks.extend(source_masks.iter().cloned());
    let old_defs = rows(target, ENTITY_DEFINITION_MAP_DESCRIPTOR, 8)?;
    let mut definitions = old_defs
        .iter()
        .filter(|r| read_u32(r, 0) != Ok(u32::MAX))
        .map(|r| Ok((read_u32(r, 0)?, r.clone())))
        .collect::<Result<BTreeMap<_, _>, String>>()?;
    let mut selected = BTreeSet::new();
    for mut definition in rows(source, ENTITY_DEFINITION_MAP_DESCRIPTOR, 8)? {
        let key = read_u32(&definition, 0)?;
        if key == u32::MAX {
            continue;
        }
        let encoded = read_u32(&definition, 4)?;
        let start = (encoded & 0xFFFF) as usize;
        let count = ((encoded >> 16) & 0x7FFF) as usize;
        let indices = (start..start + count)
            .filter(|&i| read_u32(&source_descriptors[i], 0).is_ok_and(|o| owners.contains(&o)))
            .collect::<Vec<_>>();
        if indices.is_empty() {
            continue;
        }
        let previous = definitions.get(&key);
        let replacement = select.contains(&key);
        if replacement {
            if indices.len() != count {
                return Err(format!(
                    "Selected binding 0x{key:08X} needs owners absent from the extension"
                ));
            }
            selected.insert(key);
        }
        let old_encoded = previous.map(|r| read_u32(r, 4)).transpose()?;
        let old_count = old_encoded.map_or(0, |v| ((v >> 16) & 0x7FFF) as usize);
        // A single existing resource is not automatically displaced by an unrelated owner.
        // Its untouched definition remains the selected role. Broadcast lists are extended.
        if previous.is_some() && !replacement && old_count == 1 && count == 1 {
            continue;
        }
        let first = descriptors.len();
        if let Some(old) = old_encoded.filter(|_| !replacement) {
            let first_old = (old & 0xFFFF) as usize;
            for i in first_old..first_old + old_count {
                descriptors.push(descriptors[i].clone());
                maps.push(maps[i].clone());
            }
        }
        for i in indices {
            let mut row = source_descriptors[i].clone();
            let owner = read_u32(&row, 0)?;
            let component = components
                .iter()
                .position(|r| read_u32(r, 0) == Ok(owner))
                .ok_or("Extension owner missing")?;
            write_u32(
                &mut row,
                16,
                u32::try_from(component).map_err(|_| "Too many component owners")?,
            )?;
            descriptors.push(row);
            let mut map = source_map[i].clone();
            let flags = read_u32(&map, 0)?;
            if flags >> 16 != 0 {
                let word = (flags & 0xFFFF) as usize;
                if word >= source_masks.len() {
                    return Err("Component condition mask points outside its source table".into());
                }
                write_u32(
                    &mut map,
                    0,
                    (flags & 0xFFFF_0000) | (word + mask_offset) as u32,
                )?;
            }
            maps.push(map);
        }
        let count = descriptors.len() - first;
        if first > 0x7FFF || count > 0x7FFF || descriptors.len() > 0x7FFF {
            return Err("Component extension exceeds the native selector limits".into());
        }
        write_u32(&mut definition, 4, ((count as u32) << 16) | first as u32)?;
        definitions.insert(key, definition);
    }
    if &selected != select {
        return Err("A selected component binding is absent from the extension".into());
    }
    let (live_maps, live_descriptors) = pack_spans(&mut definitions, &maps, &descriptors)?;
    let defs = super::lookup::build(read_u32(&original, 0x40)?, &definitions)?;
    for (descriptor, class, data) in [
        (
            ENTITY_COMPONENTS_DESCRIPTOR,
            WEAPON_ENTITY_COMPONENT_ROW_CLASS,
            components,
        ),
        (
            ENTITY_DEFINITION_MAP_DESCRIPTOR,
            WEAPON_ENTITY_DEFINITION_MAP_ROW_CLASS,
            defs,
        ),
        (
            ENTITY_RESOURCE_MAP_DESCRIPTOR,
            WEAPON_ENTITY_RESOURCE_MAP_ROW_CLASS,
            live_maps,
        ),
        (
            ENTITY_RESOURCE_DESCRIPTORS_DESCRIPTOR,
            WEAPON_ENTITY_RESOURCE_DESCRIPTOR_ROW_CLASS,
            live_descriptors,
        ),
    ] {
        append_array(&mut result, descriptor, class, &data)?;
    }
    append_array(&mut result, 0x78, 0x8080_0006, &masks)?;
    // 80809C1C covers +0x40..+0x88. The words after it belong to the host entity.
    let len = result.len() as u64;
    write_u64(&mut result, 0, len)?;
    validate_weapon_entity(&result)?;
    rewire::extend(&original, &mut result, source, owners, &|tag| {
        manager.read_tag(tag)
    })?;
    validate_weapon_entity(&result)?;
    *target = result;
    Ok(())
}

/// Every owner must be new to the host and unique in the source before its row is added.
fn push_owners(
    components: &mut Vec<Vec<u8>>,
    source_components: &[Vec<u8>],
    owners: &BTreeSet<u32>,
) -> Result<(), String> {
    for &owner in owners {
        if components.iter().any(|r| read_u32(r, 0) == Ok(owner)) {
            return Err(format!(
                "Component extension repeats existing owner 0x{owner:08X}"
            ));
        }
        let matches = source_components
            .iter()
            .filter(|r| read_u32(r, 0) == Ok(owner))
            .collect::<Vec<_>>();
        if matches.len() != 1 {
            return Err(format!(
                "Component extension source does not uniquely own 0x{owner:08X}"
            ));
        }
    }
    // Keep the source's component order, which also orders native callbacks.
    for row in source_components {
        if owners.contains(&read_u32(row, 0)?) {
            components.push(row.clone());
        }
    }
    Ok(())
}

/// Pack the spans before rebuilding the native hash table. No layout donor is needed.
fn pack_spans(
    definitions: &mut BTreeMap<u32, Vec<u8>>,
    maps: &[Vec<u8>],
    descriptors: &[Vec<u8>],
) -> Result<(Rows, Rows), String> {
    let mut live_maps = Vec::new();
    let mut live_descriptors = Vec::new();
    for row in definitions.values_mut() {
        let encoded = read_u32(row, 4)?;
        let start = (encoded & 0xFFFF) as usize;
        let count = ((encoded >> 16) & 0x7FFF) as usize;
        let first = live_descriptors.len();
        let mut indices = (start..start + count).collect::<Vec<_>>();
        indices.sort_by_key(|&i| std::cmp::Reverse(read_u32(&maps[i], 8).unwrap_or(0) as i32));
        for i in indices {
            live_maps.push(maps[i].clone());
            live_descriptors.push(descriptors[i].clone());
        }
        write_u32(row, 4, ((count as u32) << 16) | first as u32)?;
    }
    Ok((live_maps, live_descriptors))
}

fn rows(entity: &[u8], descriptor: usize, size: usize) -> Result<Vec<Vec<u8>>, String> {
    let a = native_array(entity, descriptor)?;
    checked_rows_end(a, size, entity.len(), "Component extension array")?;
    Ok((0..a.count)
        .map(|i| entity[a.rows + i * size..a.rows + (i + 1) * size].to_vec())
        .collect())
}

fn append_array(
    entity: &mut Vec<u8>,
    descriptor: usize,
    class: u32,
    rows: &[Vec<u8>],
) -> Result<(), String> {
    let stride = match class {
        WEAPON_ENTITY_COMPONENT_ROW_CLASS => 12,
        WEAPON_ENTITY_DEFINITION_MAP_ROW_CLASS => 8,
        WEAPON_ENTITY_RESOURCE_MAP_ROW_CLASS => 40,
        WEAPON_ENTITY_RESOURCE_DESCRIPTOR_ROW_CLASS => 24,
        0x8080_0006 => 2,
        _ => return Err("Unknown component extension array class".into()),
    };
    let previous = native_array(entity, descriptor)?;
    let previous_end = checked_rows_end(previous, stride, entity.len(), "Retired extension array")?;
    if rows.iter().any(|row| row.len() != stride) {
        return Err("Component extension array row has the wrong size".into());
    }
    entity[previous.rows..previous_end].fill(0);
    entity.resize((entity.len() + 8).next_multiple_of(16), 0);
    let header = entity.len();
    write_u32(entity, header - 4, 0x8080_9FBD)?;
    entity.extend_from_slice(&(rows.len() as u64).to_le_bytes());
    entity.extend_from_slice(&class.to_le_bytes());
    entity.extend_from_slice(&0u32.to_le_bytes());
    for row in rows {
        entity.extend_from_slice(row);
    }
    write_u64(entity, descriptor, rows.len() as u64)?;
    write_relative_pointer(entity, descriptor + 8, header)
}

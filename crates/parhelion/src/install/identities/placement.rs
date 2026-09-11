//! Compare retained native weapon placements and read capacity facts from the incoming generation.
use super::*;
use sundial::investment::{AuthoredSlotChange, AuthoredSlotReplacement};

pub(in crate::install) fn slot_replacement(
    target: &Path,
    staged: &Path,
    retained: &BTreeSet<u32>,
) -> Result<Option<AuthoredSlotReplacement>, String> {
    if retained.is_empty() {
        return Ok(None);
    }
    let previous = with_generation(target, target, |directory| slots(directory, retained))?;
    let incoming = with_generation(target, staged, |directory| slots(directory, retained))?;
    if previous.keys().ne(incoming.keys()) {
        return Err("A retained definition changed between weapon and nonweapon placement".into());
    }
    let changes = incoming
        .into_iter()
        .filter_map(|(definition_hash, incoming_bucket)| {
            let previous_bucket = previous[&definition_hash];
            (incoming_bucket != previous_bucket).then_some(AuthoredSlotChange {
                definition_hash,
                previous_bucket,
                incoming_bucket,
            })
        })
        .collect::<Vec<_>>();
    if changes.is_empty() {
        return Ok(None);
    }
    with_generation(target, staged, |directory| {
        let manager = open_shadowkeep_package_manager(directory)?;
        let (root, table) = tables(&manager)?;
        let weapon_capacities =
            sundial::package_authoring::weapon_bucket_capacities(&manager, &root)?;
        let mut incoming_buckets = BTreeMap::new();
        for row in rows(&table, ITEM_DEFINITION_INDEX_ROW_CLASS, ITEM_INDEX_ROW_SIZE)? {
            let hash = read_u32(row, 0).map_err(|e| e.to_string())?;
            let item = manager
                .read_tag(TagHash(read_u32(row, 16).map_err(|e| e.to_string())?))
                .map_err(|e| e.to_string())?;
            let bucket = *item
                .get(ITEM_INVENTORY_SLOT_OFFSET)
                .ok_or("An incoming item has no inventory bucket")?;
            if read_u32(&item, ITEM_DEFINITION_HASH_OFFSET).map_err(|e| e.to_string())? != hash {
                return Err("An incoming inventory definition has an inconsistent hash".into());
            }
            if incoming_buckets.insert(hash, bucket).is_some() {
                return Err("The incoming inventory table repeats a definition hash".into());
            }
        }
        Ok(Some(AuthoredSlotReplacement {
            changes,
            incoming_buckets,
            weapon_capacities,
        }))
    })
}

fn tables(manager: &tiger_pkg::PackageManager) -> Result<(Vec<u8>, Vec<u8>), String> {
    let read = |tag| manager.read_tag(TagHash(tag)).map_err(|e| e.to_string());
    let globals = resolve_live_named_tag(manager, "investment_globals", None)?;
    let globals = read(globals.0)?;
    let root = read(investment_globals_table_tag(&globals, 0)?)?;
    let table = read(investment_root_table_tag(
        &root,
        ROOT_ITEM_DEFINITION_TABLE_SLOT,
    )?)?;
    Ok((root, table))
}

fn slots(directory: &Path, hashes: &BTreeSet<u32>) -> Result<BTreeMap<u32, u8>, String> {
    let manager = open_shadowkeep_package_manager(directory)?;
    let (_, table) = tables(&manager)?;
    let mut found = BTreeSet::new();
    let mut result = BTreeMap::new();
    for row in rows(&table, ITEM_DEFINITION_INDEX_ROW_CLASS, ITEM_INDEX_ROW_SIZE)? {
        let hash = read_u32(row, 0).map_err(|e| e.to_string())?;
        if !hashes.contains(&hash) {
            continue;
        }
        if !found.insert(hash) {
            return Err("A retained native definition is duplicated".into());
        }
        let item = manager
            .read_tag(TagHash(read_u32(row, 16).map_err(|e| e.to_string())?))
            .map_err(|e| e.to_string())?;
        if read_u32(&item, ITEM_DEFINITION_HASH_OFFSET).map_err(|e| e.to_string())? != hash {
            return Err("A retained native definition has an inconsistent hash".into());
        }
        if let Some(bucket) = native_weapon_slot(&item)? {
            result.insert(hash, bucket);
        }
    }
    if &found != hashes {
        return Err("A retained authored definition is missing from its native generation".into());
    }
    Ok(result)
}

fn native_weapon_slot(item: &[u8]) -> Result<Option<u8>, String> {
    let bucket = *item
        .get(ITEM_INVENTORY_SLOT_OFFSET)
        .ok_or("A retained item has no inventory bucket")?;
    if bucket > 2 {
        return Ok(None);
    }
    let inventory = crate::weapon::weapon_inventory_slot(item).map_err(|e| e.to_string())?;
    let equipment = crate::weapon::weapon_equipment_slot(item).map_err(|e| e.to_string())?;
    if inventory != equipment {
        return Err("A retained weapon's native inventory and equipment slots disagree".into());
    }
    Ok(Some(bucket))
}

#[cfg(test)]
mod tests;

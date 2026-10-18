//! Preserve an imported trajectory through the final private launch and stat owners.
use super::payload::{private_index, read};
use super::*;
use crate::tag_payload::{append_native_array, write_u64};
use sundial::package_authoring::entity::{
    WEAPON_BARREL_COMPONENT_KEY, WEAPON_STAT_TRANSLATOR_COMPONENT_KEY,
};

fn save(
    manager_owner: u32,
    mut owner: Vec<u8>,
    entity: &mut [u8],
    allocator: AppendedTagAllocator,
    tags: &mut Vec<NewTagSpec>,
) -> AuthoringResult<()> {
    let length = owner.len() as u64;
    write_u64(&mut owner, 0, length)?;
    if let Some(index) = private_index(manager_owner, allocator, tags)? {
        tags[index].payload = owner;
    } else {
        let private =
            allocator.assigned_tag(tags.len(), "Imported trajectory", "runtime component owner")?;
        retarget_weapon_component_owner_payload(&mut owner, entity, manager_owner, private.0)
            .map_err(invalid)?;
        retarget_weapon_component_owner(entity, manager_owner, private.0).map_err(invalid)?;
        tags.push(NewTagSpec {
            template_tag: TagHash(manager_owner),
            payload: owner,
            storage: crate::NewTagStorageMode::InheritTemplate,
        });
    }
    Ok(())
}

pub(super) fn apply(
    manager: &PackageManager,
    entity: &mut [u8],
    allocator: AppendedTagAllocator,
    tags: &mut Vec<NewTagSpec>,
) -> AuthoringResult<()> {
    let barrels =
        weapon_component_bindings(entity, WEAPON_BARREL_COMPONENT_KEY).map_err(invalid)?;
    if barrels.len() != 1 || barrels[0].concrete_class != 0x80803889 {
        return Err(invalid("Imported trajectory requires one supported Barrel"));
    }
    let barrel = barrels[0];
    let mut owner = read(manager, barrel.owner_tag, allocator, tags)?;
    let source =
        usize::try_from(barrel.resource_offset).map_err(|_| invalid("Barrel offset overflow"))?;
    let definition = usize::try_from(read_u64(&owner, source + 8)?)
        .map_err(|_| invalid("Barrel definition overflow"))?;
    if read_u32(&owner, definition + 4)? != 0x80803889
        || read_u64(&owner, definition + 8)? != source as u64
    {
        return Err(invalid("Imported trajectory Barrel reciprocal differs"));
    }
    for index in 43..=47 {
        let config = definition + 0x10 + index * 0x20;
        write_u64(&mut owner, config, 0)?;
        write_u32(&mut owner, config + 12, 1f32.to_bits())?;
    }
    save(barrel.owner_tag, owner, entity, allocator, tags)?;
    // Filter each final table after appearance copies and explicit edits. Other
    // routes can share the same output column, so the column itself stays intact.
    for binding in
        weapon_component_bindings(entity, WEAPON_STAT_TRANSLATOR_COMPONENT_KEY).map_err(invalid)?
    {
        let mut owner = read(manager, binding.owner_tag, allocator, tags)?;
        let (count, tables) = crate::weapon::rig::translator_array(&owner, 0x80803975)?;
        let mut changed = false;
        for index in 0..count {
            let descriptor = tables + index * 0x30 + 0x10;
            let (count, _, rows, class) = array_at(&owner, descriptor)?;
            if count == 0 {
                continue;
            }
            if class != 0x80803981 {
                return Err(invalid("Stat translator routing layout differs"));
            }
            let mut kept = Vec::new();
            for row in (0..count).map(|i| rows + i * 0x50) {
                if read_u32(&owner, row)? == 2 && (43..=47).contains(&read_u32(&owner, row + 4)?) {
                    continue;
                }
                kept.extend_from_slice(
                    owner
                        .get(row..row + 0x50)
                        .ok_or_else(|| invalid("Stat route exceeds its owner"))?,
                );
            }
            if kept.len() != count * 0x50 {
                append_native_array(&mut owner, descriptor, class, kept.len() / 0x50, &kept)?;
                changed = true;
            }
        }
        if changed {
            save(binding.owner_tag, owner, entity, allocator, tags)?;
        }
    }
    Ok(())
}

//! Replace the native shell-reload subclass with its magazine-reload base in a private owner.
use super::*;
use crate::{
    package_payload::{i64_at, relative_offset},
    package_runtime::{reader::PackageManager, references},
};
use tiger_pkg::TagHash;
mod methods;

/// These are package-declared controller classes, not weapon identities. Both compositions
/// embed the same Trigger, Barrel and Magazine at the same offsets. Only Reload differs.
const CLASSES: [(u32, u32); 4] = [
    (0x80C1_C534, 0x80BF_DFEA),
    (0x80C1_C533, 0x80BF_DFEB),
    (0x8080_3842, 0x8080_3844),
    (0x8080_3843, 0x8080_3845),
];

fn mapped(class: u32) -> Option<u32> {
    CLASSES
        .iter()
        .find_map(|&(from, to)| (from == class).then_some(to))
}

fn single(entity: &[u8], key: u32) -> Result<Option<WeaponComponentBinding>, String> {
    let rows = weapon_component_bindings(entity, key)?;
    Ok(match rows.as_slice() {
        [row] => Some(*row),
        _ => None,
    })
}

/// A shell-reload composition for which the native magazine counterpart has the same storage.
/// The operation still validates both declarations and the actual private payload before writing.
pub fn magazine_candidate(entity: &[u8]) -> Result<Option<WeaponComponentBinding>, String> {
    let Some(controller) = single(entity, WEAPON_CONTROLLER_COMPONENT_KEY)? else {
        return Ok(None);
    };
    let Some(reload) = single(entity, WEAPON_RELOAD_COMPONENT_KEY)? else {
        return Ok(None);
    };
    Ok((controller.concrete_class == CLASSES[0].0
        && reload.concrete_class == CLASSES[2].0
        && reload.owner_tag == controller.owner_tag
        && reload
            .resource_offset
            .checked_sub(controller.resource_offset)
            == Some(0x2710))
    .then_some(controller))
}

fn declarations(manager: &PackageManager) -> Result<(), String> {
    let mut registry = references::schema::Registry::new()?;
    for (index, &(from, to)) in CLASSES.iter().enumerate() {
        let read = |class| {
            let entry = manager
                .get_entry(TagHash(class))
                .ok_or("Reload declaration is absent")?;
            if entry.reference != 0x8080_0000 {
                return Err("Reload composition has another declaration format".into());
            }
            manager.read_tag(TagHash(class)).map_err(|e| e.to_string())
        };
        let left = registry.record(from, read)?;
        let right = registry.record(to, read)?;
        if left.size != right.size || left.fields != right.fields {
            return Err("Native reload compositions have different storage".into());
        }
        if index < 2 {
            let left = read(from)?;
            let right = read(to)?;
            let base = if index == 0 { 0x8080_39F5 } else { 0x8080_3A10 };
            if read_u32(&left, 16)? != base || read_u32(&right, 16)? != base {
                return Err("Native reload composition has another controller base".into());
            }
        } else {
            let child =
                crate::runtime::native_declaration(from)?.ok_or("Reload subclass is absent")?;
            if child.base != to || !child.members.is_empty() {
                return Err("Shell reload no longer inherits the magazine layout directly".into());
            }
        }
    }
    Ok(())
}

fn root(data: &[u8], field: usize) -> Result<usize, String> {
    relative_offset(field, 0, i64_at(data, field)?)
}

/// Prepare exact class-word writes. Only root headers, reflected object references and native
/// entity resource-map class fields participate. Aligned integers outside those fields stay intact.
fn owner_fields(
    manager: &PackageManager,
    template: u32,
    owner: &[u8],
    binding: WeaponComponentBinding,
) -> Result<BTreeMap<usize, u32>, String> {
    validate_file_size(owner, "Reload owner")?;
    let instance = root(owner, 16)?;
    let instance_header = instance
        .checked_sub(4)
        .ok_or("Reload instance has no class header")?;
    if instance as u64 != binding.resource_offset
        || read_u32(owner, instance_header)? != CLASSES[0].0
    {
        return Err("Shell reload is not the selected controller root".into());
    }
    let definition = usize::try_from(read_u64(owner, instance + 8)?)
        .map_err(|_| "Reload definition offset overflow")?;
    let mut fields = BTreeMap::new();
    for (at, class) in [
        (instance, CLASSES[0].0),
        (definition, CLASSES[1].0),
        (root(owner, 24)?, CLASSES[1].0),
    ] {
        let header = at.checked_sub(4).ok_or("Reload root has no header")?;
        if read_u32(owner, header)? != class || read_u32(owner, at)? != binding.owner_tag {
            return Err("Reload root has an inconsistent class or owner".into());
        }
        fields.insert(header, mapped(class).ok_or("Unmapped reload root")?);
    }
    for (at, tag) in references::payload_object_references(manager, template, owner)? {
        if tag == binding.owner_tag
            && let Some(class) = mapped(read_u32(owner, at + 4)?)
        {
            fields.insert(at + 4, class);
        }
    }
    // The reciprocal prefixes must have been traversed, including the embedded Reload.
    let reload = instance + 0x2710;
    let reload_definition = usize::try_from(read_u64(owner, reload + 8)?)
        .map_err(|_| "Embedded reload definition offset overflow")?;
    for (at, class) in [
        (instance + 4, CLASSES[1].1),
        (definition + 4, CLASSES[0].1),
        (reload + 4, CLASSES[3].1),
        (reload_definition + 4, CLASSES[2].1),
    ] {
        if fields.get(&at) != Some(&class) {
            return Err("Reload reciprocal prefix is outside the declared reference layout".into());
        }
    }
    Ok(fields)
}

fn entity_fields(entity: &[u8], owner: u32) -> Result<BTreeMap<usize, u32>, String> {
    let descriptors = native_array(entity, ENTITY_RESOURCE_DESCRIPTORS_DESCRIPTOR)?;
    let resources = native_array(entity, ENTITY_RESOURCE_MAP_DESCRIPTOR)?;
    let mut fields = BTreeMap::new();
    for index in 0..descriptors.count {
        let descriptor = descriptors.rows + index * WEAPON_ENTITY_RESOURCE_DESCRIPTOR_ROW_SIZE;
        if read_u32(entity, descriptor)? == owner
            && let Some(class) = mapped(read_u32(entity, descriptor + 4)?)
        {
            fields.insert(descriptor + 4, class);
        }
        let resource = resources.rows + index * WEAPON_ENTITY_RESOURCE_MAP_ROW_SIZE;
        if read_u32(entity, resource + 16)? == owner
            && let Some(class) = mapped(read_u32(entity, resource + 12)?)
        {
            fields.insert(resource + 12, class);
        }
    }
    for row in super::owner::event_rows(entity)? {
        for at in [row + 8, row + 0x28] {
            if read_u32(entity, at)? == owner
                && let Some(class) = mapped(read_u32(entity, at + 4)?)
            {
                fields.insert(at + 4, class);
            }
        }
    }
    Ok(fields)
}

/// Change an already private shell-reload owner and its entity to the native magazine
/// composition. Its reload publisher selects the base implementation's method table. Numeric
/// defaults, firing components, lifecycle registrations and offsets stay in place.
/// Callers must provide magazine animation actions and a full-magazine refill quantum separately.
pub fn magazine(
    manager: &PackageManager,
    template: u32,
    entity: &mut [u8],
    owner: &mut [u8],
) -> Result<(), String> {
    validate_weapon_entity(entity)?;
    let binding = magazine_candidate(entity)?.ok_or("Unsupported magazine reload composition")?;
    declarations(manager)?;
    let owner_writes = owner_fields(manager, template, owner, binding)?;
    let entity_writes = entity_fields(entity, binding.owner_tag)?;
    let reload = single(entity, WEAPON_RELOAD_COMPONENT_KEY)?.ok_or("Reload binding is absent")?;
    let (method_field, method_table) = methods::magazine(manager, owner, reload)?;
    let mut authored_owner = owner.to_vec();
    let mut authored_entity = entity.to_vec();
    for (at, class) in owner_writes {
        write_u32(&mut authored_owner, at, class)?;
    }
    write_u32(&mut authored_owner, method_field, method_table)?;
    for (at, class) in entity_writes {
        write_u32(&mut authored_entity, at, class)?;
    }
    validate_weapon_entity(&authored_entity)?;
    // Traversal checks the changed root and sealed/original definition views together.
    references::payload_objects(manager, template, &authored_owner)?;
    for key in weapon_component_binding_hashes(&authored_entity)? {
        for resource in weapon_component_bindings(&authored_entity, key)? {
            if resource.owner_tag != binding.owner_tag {
                continue;
            }
            let at = usize::try_from(resource.resource_offset)
                .map_err(|_| "Reload resource offset overflow")?;
            let definition = usize::try_from(read_u64(&authored_owner, at + 8)?)
                .map_err(|_| "Reload definition offset overflow")?;
            if read_u32(&authored_owner, definition + 4)? != resource.concrete_class {
                return Err("Reload conversion left a mismatched entity alias".into());
            }
        }
    }
    owner.copy_from_slice(&authored_owner);
    entity.copy_from_slice(&authored_entity);
    Ok(())
}

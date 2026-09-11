//! Shadowkeep weapon sandbox-pattern and runtime entity graph helpers.

use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
};

mod owner;

pub const SANDBOX_PATTERN_ENTITY_ASSIGNMENT_TAG: u32 = 0x80EC_3F60;
pub const SANDBOX_PATTERN_ENTITY_ASSIGNMENT_CLASS: u32 = 0x8080_9780;
pub const SANDBOX_PATTERN_ENTITY_ASSIGNMENT_ROW_CLASS: u32 = 0x8080_9252;
pub const SANDBOX_PATTERN_ENTITY_ASSIGNMENT_ROW_SIZE: usize = 0x08;

pub const SANDBOX_PATTERN_ROW_SIZE: usize = 0x30;
pub const SANDBOX_PATTERN_ROW_CLASS: u32 = 0x8080_5B7C;
pub const SANDBOX_PATTERN_NESTED_OFFSET: usize = 0x20;
pub const SANDBOX_PATTERN_NESTED_CLASS: u32 = 0x8080_5B7E;
pub const SANDBOX_PATTERN_INDEX_ROW_SIZE: usize = 0x08;
pub const SANDBOX_PATTERN_INDEX_ROW_CLASS: u32 = 0x8080_7A65;
pub const SANDBOX_PATTERN_GLOBAL_ID_OFFSET: usize = 0x04;
pub const SANDBOX_PATTERN_WEAPON_CONTENT_GROUP_HASH_OFFSET: usize = 0x10;
pub const SANDBOX_PATTERN_WEAPON_TRANSLATION_GROUP_HASH_OFFSET: usize = 0x14;

pub const WEAPON_ENTITY_CLASS: u32 = 0x8080_9C0F;
pub const WEAPON_ENTITY_COMPONENT_ROW_CLASS: u32 = 0x8080_9C04;
pub const WEAPON_ENTITY_COMPONENT_ROW_SIZE: usize = 0x0C;
pub const WEAPON_ENTITY_DEFINITION_MAP_ROW_CLASS: u32 = 0x8080_9C25;
pub const WEAPON_ENTITY_RESOURCE_MAP_ROW_CLASS: u32 = 0x8080_9C22;
pub const WEAPON_ENTITY_RESOURCE_MAP_ROW_SIZE: usize = 0x28;
pub const WEAPON_ENTITY_RESOURCE_DESCRIPTOR_ROW_CLASS: u32 = 0x8080_9C20;
pub const WEAPON_ENTITY_RESOURCE_DESCRIPTOR_ROW_SIZE: usize = 0x18;
pub const WEAPON_INPUT_COMPONENT_KEY: u32 = 0xC18B_D28D;
pub const WEAPON_TRIGGER_COMPONENT_KEY: u32 = 0xD5A1_23FF;
pub const WEAPON_BARREL_COMPONENT_KEY: u32 = 0xEC71_1FA3;
pub const WEAPON_CONTROLLER_COMPONENT_KEY: u32 = 0x39AF_D7D3;
pub const WEAPON_MAGAZINE_COMPONENT_KEY: u32 = 0xB1AA_A2CB;
pub const WEAPON_RELOAD_COMPONENT_KEY: u32 = 0xB9CA_A3BC;
pub const WEAPON_TRIGGER_CHARGE_COMPONENT_KEY: u32 = 0x7E4A_5223;
pub const WEAPON_STAT_TRANSLATOR_COMPONENT_KEY: u32 = 0x2F98_1564;

const FILE_SIZE_OFFSET: usize = 0x00;
const ENTITY_COMPONENTS_DESCRIPTOR: usize = 0x10;
const ENTITY_DEFINITION_MAP_DESCRIPTOR: usize = 0x48;
const ENTITY_RESOURCE_MAP_DESCRIPTOR: usize = 0x58;
const ENTITY_RESOURCE_DESCRIPTORS_DESCRIPTOR: usize = 0x68;
const ARRAY_TRAILER_SIZE: usize = 0x08;
const ARRAY_HEADER_SIZE: usize = 0x10;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SandboxPatternIdentity {
    pub item_hash: u32,
    pub row_index: usize,
    pub row_offset: usize,
    pub pattern_global_id_hash: u32,
    pub weapon_content_group_hash: u32,
    pub weapon_translation_group_hash: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WeaponComponentBinding {
    pub binding_hash: u32,
    pub descriptor_index: usize,
    /// Zero-based resource within this binding's native selector span.
    pub resource_index: usize,
    /// Number of consecutive resource descriptors selected by the binding.
    pub resource_count: usize,
    pub component_index: usize,
    pub owner_tag: u32,
    pub concrete_class: u32,
    pub resource_offset: u64,
}

#[derive(Clone, Copy)]
struct NativeArray {
    count: usize,
    header: usize,
    rows: usize,
    row_class: u32,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ComponentAlias {
    binding_hash: u32,
    resource_index: usize,
    descriptor_index: usize,
    owner_tag: u32,
    concrete_class: u32,
    resource_offset: u64,
}

#[derive(Clone, Copy)]
struct PlannedOwnerGraft<'a> {
    requested_binding_hash: u32,
    donor_owner_tag: u32,
    donor: &'a [u8],
}

/// Finds an item-keyed sandbox-pattern row and decodes its runtime entity identity.
pub fn sandbox_pattern_identity(
    table: &[u8],
    item_hash: u32,
) -> Result<Option<SandboxPatternIdentity>, String> {
    let array = native_array(table, 0x08)?;
    if array.row_class != SANDBOX_PATTERN_ROW_CLASS {
        return Err(format!(
            "Sandbox-pattern table has row class 0x{:08X}, expected 0x{SANDBOX_PATTERN_ROW_CLASS:08X}",
            array.row_class
        ));
    }
    checked_rows_end(
        array,
        SANDBOX_PATTERN_ROW_SIZE,
        table.len(),
        "sandbox-pattern",
    )?;
    for row_index in 0..array.count {
        let row_offset = array.rows + row_index * SANDBOX_PATTERN_ROW_SIZE;
        if read_u32(table, row_offset)? == item_hash {
            return Ok(Some(SandboxPatternIdentity {
                item_hash,
                row_index,
                row_offset,
                pattern_global_id_hash: read_u32(
                    table,
                    row_offset + SANDBOX_PATTERN_GLOBAL_ID_OFFSET,
                )?,
                weapon_content_group_hash: read_u32(
                    table,
                    row_offset + SANDBOX_PATTERN_WEAPON_CONTENT_GROUP_HASH_OFFSET,
                )?,
                weapon_translation_group_hash: read_u32(
                    table,
                    row_offset + SANDBOX_PATTERN_WEAPON_TRANSLATION_GROUP_HASH_OFFSET,
                )?,
            }));
        }
    }
    Ok(None)
}

/// Decodes one authoritative sandbox-pattern row by its investment-table index.
pub fn sandbox_pattern_identity_at(
    table: &[u8],
    row_index: usize,
) -> Result<Option<SandboxPatternIdentity>, String> {
    let array = native_array(table, 0x08)?;
    if array.row_class != SANDBOX_PATTERN_ROW_CLASS {
        return Err(format!(
            "Sandbox-pattern table has row class 0x{:08X}, expected 0x{SANDBOX_PATTERN_ROW_CLASS:08X}",
            array.row_class
        ));
    }
    checked_rows_end(
        array,
        SANDBOX_PATTERN_ROW_SIZE,
        table.len(),
        "sandbox-pattern",
    )?;
    if row_index >= array.count {
        return Ok(None);
    }
    let row_offset = array
        .rows
        .checked_add(
            row_index
                .checked_mul(SANDBOX_PATTERN_ROW_SIZE)
                .ok_or("Sandbox-pattern row offset overflowed")?,
        )
        .ok_or("Sandbox-pattern row offset overflowed")?;
    Ok(Some(SandboxPatternIdentity {
        item_hash: read_u32(table, row_offset)?,
        row_index,
        row_offset,
        pattern_global_id_hash: read_u32(table, row_offset + SANDBOX_PATTERN_GLOBAL_ID_OFFSET)?,
        weapon_content_group_hash: read_u32(
            table,
            row_offset + SANDBOX_PATTERN_WEAPON_CONTENT_GROUP_HASH_OFFSET,
        )?,
        weapon_translation_group_hash: read_u32(
            table,
            row_offset + SANDBOX_PATTERN_WEAPON_TRANSLATION_GROUP_HASH_OFFSET,
        )?,
    }))
}

/// Resolves a sandbox-pattern global identity through the stock runtime entity map.
pub fn weapon_entity_assignment(
    assignments: &[u8],
    pattern_global_id_hash: u32,
) -> Result<Option<u32>, String> {
    let array = assignment_rows(assignments)?;
    let rows_end = checked_rows_end(
        array,
        SANDBOX_PATTERN_ENTITY_ASSIGNMENT_ROW_SIZE,
        assignments.len(),
        "sandbox-pattern entity assignment",
    )?;
    if rows_end > assignments.len() {
        return Err("Sandbox-pattern entity assignments extend beyond their payload".into());
    }
    let mut low = 0;
    let mut high = array.count;
    while low < high {
        let middle = low + (high - low) / 2;
        let row = array.rows + middle * SANDBOX_PATTERN_ENTITY_ASSIGNMENT_ROW_SIZE;
        match read_u32(assignments, row)?.cmp(&pattern_global_id_hash) {
            Ordering::Less => low = middle + 1,
            Ordering::Greater => high = middle,
            Ordering::Equal => return read_u32(assignments, row + 4).map(Some),
        }
    }
    Ok(None)
}

/// Returns every concrete resource selected by one abstract weapon-component binding.
///
/// The upper half of the native selector stores a resource count plus flags; the lower half is
/// the first descriptor index. Most weapon-specific slots select one resource, while broader
/// entity bindings can select a consecutive span.
pub fn weapon_component_bindings(
    entity: &[u8],
    binding_hash: u32,
) -> Result<Vec<WeaponComponentBinding>, String> {
    validate_file_size(entity, "Weapon entity")?;
    let components = native_array(entity, ENTITY_COMPONENTS_DESCRIPTOR)?;
    let definitions = native_array(entity, ENTITY_DEFINITION_MAP_DESCRIPTOR)?;
    let resource_map = native_array(entity, ENTITY_RESOURCE_MAP_DESCRIPTOR)?;
    let descriptors = native_array(entity, ENTITY_RESOURCE_DESCRIPTORS_DESCRIPTOR)?;
    validate_entity_arrays(entity, components, definitions, resource_map, descriptors)?;

    let mut selector = None;
    for index in 0..definitions.count {
        let row = definitions.rows + index * SANDBOX_PATTERN_ENTITY_ASSIGNMENT_ROW_SIZE;
        if read_u32(entity, row)? == binding_hash
            && selector
                .replace((row, read_u32(entity, row + 4)?))
                .is_some()
        {
            return Err(format!(
                "Weapon entity contains more than one mapping for component binding 0x{binding_hash:08X}"
            ));
        }
    }
    let (_, encoded) = selector
        .ok_or_else(|| format!("Weapon entity has no component binding 0x{binding_hash:08X}"))?;
    let resource_count = usize::from(((encoded >> 16) as u16) & 0x7FFF);
    if resource_count == 0 {
        return Err(format!(
            "Weapon component binding 0x{binding_hash:08X} has an empty native selector"
        ));
    }
    let first_descriptor_index = usize::from((encoded & 0xFFFF) as u16);
    let descriptor_end = first_descriptor_index
        .checked_add(resource_count)
        .ok_or_else(|| {
            format!("Weapon component binding 0x{binding_hash:08X} descriptor range overflowed")
        })?;
    if descriptor_end > descriptors.count || descriptor_end > resource_map.count {
        return Err(format!(
            "Weapon component binding 0x{binding_hash:08X} descriptor range {first_descriptor_index}..{descriptor_end} is outside its resource tables"
        ));
    }
    (0..resource_count)
        .map(|resource_index| {
            let descriptor_index = first_descriptor_index + resource_index;
            let descriptor = descriptors.rows
                + descriptor_index * WEAPON_ENTITY_RESOURCE_DESCRIPTOR_ROW_SIZE;
            let resource =
                resource_map.rows + descriptor_index * WEAPON_ENTITY_RESOURCE_MAP_ROW_SIZE;
            let owner_tag = read_u32(entity, descriptor)?;
            let concrete_class = read_u32(entity, descriptor + 4)?;
            let resource_offset = read_u64(entity, descriptor + 8)?;
            let component_index = usize::try_from(read_u32(entity, descriptor + 0x10)?)
                .map_err(|_| "Weapon component index does not fit usize")?;
            if component_index >= components.count {
                return Err(format!(
                    "Weapon component binding 0x{binding_hash:08X} resource {resource_index} component index {component_index} is outside the component list"
                ));
            }
            let component = components.rows + component_index * WEAPON_ENTITY_COMPONENT_ROW_SIZE;
            if read_u32(entity, component)? != owner_tag {
                return Err(format!(
                    "Weapon component binding 0x{binding_hash:08X} resource {resource_index} descriptor and component-list owner disagree"
                ));
            }
            if read_u32(entity, resource + 0x0C)? != concrete_class
                || read_u32(entity, resource + 0x10)? != owner_tag
            {
                return Err(format!(
                    "Weapon component binding 0x{binding_hash:08X} resource {resource_index} descriptor and resource-map row disagree"
                ));
            }
            Ok(WeaponComponentBinding {
                binding_hash,
                descriptor_index,
                resource_index,
                resource_count,
                component_index,
                owner_tag,
                concrete_class,
                resource_offset,
            })
        })
        .collect()
}

/// Returns every active component-binding hash declared by a weapon runtime entity.
///
/// The definition map is authoritative. Callers must not assume that all weapon families expose
/// the same bindings or limit authoring to the small set of bindings currently understood by name.
pub fn weapon_component_binding_hashes(entity: &[u8]) -> Result<Vec<u32>, String> {
    validate_file_size(entity, "Weapon entity")?;
    let components = native_array(entity, ENTITY_COMPONENTS_DESCRIPTOR)?;
    let definitions = native_array(entity, ENTITY_DEFINITION_MAP_DESCRIPTOR)?;
    let resource_map = native_array(entity, ENTITY_RESOURCE_MAP_DESCRIPTOR)?;
    let descriptors = native_array(entity, ENTITY_RESOURCE_DESCRIPTORS_DESCRIPTOR)?;
    validate_entity_arrays(entity, components, definitions, resource_map, descriptors)?;

    let mut bindings = Vec::with_capacity(definitions.count);
    for index in 0..definitions.count {
        let row = definitions.rows + index * SANDBOX_PATTERN_ENTITY_ASSIGNMENT_ROW_SIZE;
        let binding_hash = read_u32(entity, row)?;
        if binding_hash == u32::MAX {
            continue;
        }
        if bindings.contains(&binding_hash) {
            return Err(format!(
                "Weapon entity contains duplicate component binding 0x{binding_hash:08X}"
            ));
        }
        weapon_component_bindings(entity, binding_hash)?;
        bindings.push(binding_hash);
    }
    Ok(bindings)
}

fn weapon_component_selector_flags(entity: &[u8], binding_hash: u32) -> Result<u16, String> {
    let definitions = native_array(entity, ENTITY_DEFINITION_MAP_DESCRIPTOR)?;
    let mut encoded = None;
    for index in 0..definitions.count {
        let row = definitions.rows + index * SANDBOX_PATTERN_ENTITY_ASSIGNMENT_ROW_SIZE;
        if read_u32(entity, row)? == binding_hash
            && encoded.replace(read_u32(entity, row + 4)?).is_some()
        {
            return Err(format!(
                "Weapon entity contains more than one mapping for component binding 0x{binding_hash:08X}"
            ));
        }
    }
    let encoded = encoded
        .ok_or_else(|| format!("Weapon entity has no component binding 0x{binding_hash:08X}"))?;
    Ok((encoded >> 16) as u16 & 0x8000)
}

/// Returns the sole concrete resource selected by a one-resource component binding.
pub fn weapon_component_binding(
    entity: &[u8],
    binding_hash: u32,
) -> Result<WeaponComponentBinding, String> {
    let mut bindings = weapon_component_bindings(entity, binding_hash)?;
    if bindings.len() != 1 {
        return Err(format!(
            "Weapon component binding 0x{binding_hash:08X} selects {} resources; a resource index is required",
            bindings.len()
        ));
    }
    Ok(bindings.remove(0))
}

/// Transplants one complete component-binding span from `donor` into a private entity clone.
pub fn graft_weapon_component_binding(
    target: &mut Vec<u8>,
    donor: &[u8],
    binding_hash: u32,
) -> Result<WeaponComponentBinding, String> {
    graft_weapon_component_bindings(target, &[(binding_hash, donor)])?;
    weapon_component_bindings(target, binding_hash)?
        .into_iter()
        .next()
        .ok_or_else(|| "Authored component binding unexpectedly became empty".into())
}

/// Atomically transplants complete component-owner partitions into a private entity clone.
///
/// Abstract bindings are not independent storage units. A single structured owner commonly backs
/// Trigger, Barrel, Controller, Magazine, Reload, and their aliases. Mixing descriptor rows from
/// two such owners makes the runtime traverse incompatible owner roots for one logical component.
/// Each requested binding therefore promotes its complete owner partition from the donor.
pub fn graft_weapon_component_bindings(
    target: &mut Vec<u8>,
    grafts: &[(u32, &[u8])],
) -> Result<(), String> {
    let target_aliases = weapon_component_aliases(target)?;
    let mut requested = BTreeSet::new();
    let mut planned = BTreeMap::<u32, PlannedOwnerGraft<'_>>::new();
    let mut donor_targets = BTreeMap::<u32, u32>::new();

    for &(binding_hash, donor) in grafts {
        if !requested.insert(binding_hash) {
            return Err(format!(
                "Weapon component binding 0x{binding_hash:08X} has more than one donor"
            ));
        }
        let target_bindings = weapon_component_bindings(target, binding_hash)?;
        let donor_bindings = weapon_component_bindings(donor, binding_hash)?;
        if target_bindings.len() != donor_bindings.len() {
            return Err(format!(
                "Weapon component binding 0x{binding_hash:08X} selects {} target resources but {} donor resources",
                target_bindings.len(),
                donor_bindings.len()
            ));
        }
        let target_flags = weapon_component_selector_flags(target, binding_hash)?;
        let donor_flags = weapon_component_selector_flags(donor, binding_hash)?;
        if target_flags != donor_flags {
            return Err(format!(
                "Weapon component binding 0x{binding_hash:08X} has incompatible selector flags: target 0x{target_flags:04X}, donor 0x{donor_flags:04X}"
            ));
        }

        for (target_binding, donor_binding) in target_bindings.iter().zip(&donor_bindings) {
            let target_owner_tag = target_binding.owner_tag;
            let donor_owner_tag = donor_binding.owner_tag;
            if donor_owner_tag != target_owner_tag
                && target_aliases
                    .iter()
                    .any(|alias| alias.owner_tag == donor_owner_tag)
            {
                return Err(format!(
                    "Runtime component donor conflict: target entity already uses donor owner 0x{donor_owner_tag:08X} outside target owner 0x{target_owner_tag:08X}"
                ));
            }

            if let Some(previous_target) = donor_targets.get(&donor_owner_tag) {
                if *previous_target != target_owner_tag {
                    return Err(format!(
                        "Runtime component donor conflict: donor owner 0x{donor_owner_tag:08X} would replace target owners 0x{previous_target:08X} and 0x{target_owner_tag:08X}"
                    ));
                }
            } else {
                donor_targets.insert(donor_owner_tag, target_owner_tag);
            }

            if let Some(previous) = planned.get(&target_owner_tag) {
                if previous.donor_owner_tag != donor_owner_tag || previous.donor != donor {
                    return Err(format!(
                        "Runtime component donor conflict: bindings 0x{:08X} and 0x{binding_hash:08X} split target owner 0x{target_owner_tag:08X} across different donor owners",
                        previous.requested_binding_hash
                    ));
                }
            } else {
                planned.insert(
                    target_owner_tag,
                    PlannedOwnerGraft {
                        requested_binding_hash: binding_hash,
                        donor_owner_tag,
                        donor,
                    },
                );
            }
        }
    }

    let original_component_count = native_array(target, ENTITY_COMPONENTS_DESCRIPTOR)?.count;
    let mut authored = target.clone();
    for (target_owner_tag, plan) in planned {
        graft_weapon_component_owner_in_place(
            &mut authored,
            plan.donor,
            target_owner_tag,
            plan.donor_owner_tag,
            plan.requested_binding_hash,
        )?;
    }
    validate_weapon_entity(&authored)?;
    if native_array(&authored, ENTITY_COMPONENTS_DESCRIPTOR)?.count != original_component_count {
        return Err("Runtime component graft changed the entity component count".into());
    }
    for &(binding_hash, donor) in grafts {
        let authored_bindings = weapon_component_bindings(&authored, binding_hash)?;
        let donor_bindings = weapon_component_bindings(donor, binding_hash)?;
        for (authored_binding, donor_binding) in authored_bindings.iter().zip(&donor_bindings) {
            if authored_binding.owner_tag != donor_binding.owner_tag
                || authored_binding.concrete_class != donor_binding.concrete_class
                || authored_binding.resource_offset != donor_binding.resource_offset
            {
                return Err(format!(
                    "Authored component binding 0x{binding_hash:08X} did not converge on its donor owner partition"
                ));
            }
        }
    }
    *target = authored;
    Ok(())
}

fn graft_weapon_component_owner_in_place(
    target: &mut [u8],
    donor: &[u8],
    target_owner_tag: u32,
    donor_owner_tag: u32,
    requested_binding_hash: u32,
) -> Result<(), String> {
    let target_partition = weapon_component_aliases(target)?
        .into_iter()
        .filter(|alias| alias.owner_tag == target_owner_tag)
        .map(|alias| ((alias.binding_hash, alias.resource_index), alias))
        .collect::<BTreeMap<_, _>>();
    let donor_partition = weapon_component_aliases(donor)?
        .into_iter()
        .filter(|alias| alias.owner_tag == donor_owner_tag)
        .map(|alias| ((alias.binding_hash, alias.resource_index), alias))
        .collect::<BTreeMap<_, _>>();
    if target_partition.is_empty() || donor_partition.is_empty() {
        return Err(format!(
            "Runtime component binding 0x{requested_binding_hash:08X} resolves to an empty owner partition"
        ));
    }
    if target_partition.keys().ne(donor_partition.keys()) {
        let target_only = target_partition
            .keys()
            .find(|identity| !donor_partition.contains_key(identity))
            .map_or_else(
                || "none".to_owned(),
                |(binding_hash, resource_index)| {
                    format!("0x{binding_hash:08X} resource {resource_index}")
                },
            );
        let donor_only = donor_partition
            .keys()
            .find(|identity| !target_partition.contains_key(identity))
            .map_or_else(
                || "none".to_owned(),
                |(binding_hash, resource_index)| {
                    format!("0x{binding_hash:08X} resource {resource_index}")
                },
            );
        return Err(format!(
            "Weapon component binding 0x{requested_binding_hash:08X} has incompatible owner topology: target owner 0x{target_owner_tag:08X} selects {} binding resources and donor owner 0x{donor_owner_tag:08X} selects {}; first target-only identity {target_only}, first donor-only identity {donor_only}",
            target_partition.len(),
            donor_partition.len()
        ));
    }

    let mut descriptor_pairs = BTreeMap::<usize, usize>::new();
    let mut reverse_pairs = BTreeMap::<usize, usize>::new();
    for (identity, target_alias) in &target_partition {
        let donor_alias = donor_partition
            .get(identity)
            .ok_or("Donor owner partition changed during graft planning")?;
        if let Some(previous) =
            descriptor_pairs.insert(target_alias.descriptor_index, donor_alias.descriptor_index)
        {
            if previous != donor_alias.descriptor_index {
                return Err(format!(
                    "Weapon component binding 0x{requested_binding_hash:08X} has incompatible alias topology: target descriptor {} maps to donor descriptors {previous} and {}",
                    target_alias.descriptor_index, donor_alias.descriptor_index
                ));
            }
        }
        if let Some(previous) =
            reverse_pairs.insert(donor_alias.descriptor_index, target_alias.descriptor_index)
        {
            if previous != target_alias.descriptor_index {
                return Err(format!(
                    "Weapon component binding 0x{requested_binding_hash:08X} has incompatible alias topology: donor descriptor {} maps to target descriptors {previous} and {}",
                    donor_alias.descriptor_index, target_alias.descriptor_index
                ));
            }
        }
    }

    let target_resources = native_array(target, ENTITY_RESOURCE_MAP_DESCRIPTOR)?;
    let target_descriptors = native_array(target, ENTITY_RESOURCE_DESCRIPTORS_DESCRIPTOR)?;
    let target_components = native_array(target, ENTITY_COMPONENTS_DESCRIPTOR)?;
    let donor_resources = native_array(donor, ENTITY_RESOURCE_MAP_DESCRIPTOR)?;
    let donor_descriptors = native_array(donor, ENTITY_RESOURCE_DESCRIPTORS_DESCRIPTOR)?;
    let donor_components = native_array(donor, ENTITY_COMPONENTS_DESCRIPTOR)?;
    let mut target_component_indices = BTreeSet::new();
    let mut donor_component_indices = BTreeSet::new();
    for (&target_descriptor_index, &donor_descriptor_index) in &descriptor_pairs {
        let target_descriptor = target_descriptors.rows
            + target_descriptor_index * WEAPON_ENTITY_RESOURCE_DESCRIPTOR_ROW_SIZE;
        let donor_descriptor = donor_descriptors.rows
            + donor_descriptor_index * WEAPON_ENTITY_RESOURCE_DESCRIPTOR_ROW_SIZE;
        target_component_indices.insert(
            usize::try_from(read_u32(target, target_descriptor + 0x10)?)
                .map_err(|_| "Target component index does not fit usize")?,
        );
        donor_component_indices.insert(
            usize::try_from(read_u32(donor, donor_descriptor + 0x10)?)
                .map_err(|_| "Donor component index does not fit usize")?,
        );
    }
    if target_component_indices.len() != 1 || donor_component_indices.len() != 1 {
        return Err(format!(
            "Weapon component binding 0x{requested_binding_hash:08X} has incompatible owner topology: an owner partition must use exactly one component row"
        ));
    }
    let target_component_index = *target_component_indices
        .first()
        .ok_or("Target owner partition has no component row")?;
    let donor_component_index = *donor_component_indices
        .first()
        .ok_or("Donor owner partition has no component row")?;
    if target_component_index >= target_components.count {
        return Err(format!(
            "Target owner partition has component index {target_component_index} outside the component list"
        ));
    }
    if donor_component_index >= donor_components.count {
        return Err(format!(
            "Donor owner partition has component index {donor_component_index} outside the component list"
        ));
    }

    let target_component =
        target_components.rows + target_component_index * WEAPON_ENTITY_COMPONENT_ROW_SIZE;
    let donor_component =
        donor_components.rows + donor_component_index * WEAPON_ENTITY_COMPONENT_ROW_SIZE;
    copy_row(
        target,
        target_component,
        donor,
        donor_component,
        WEAPON_ENTITY_COMPONENT_ROW_SIZE,
        "component-owner metadata",
    )?;

    for (&target_descriptor_index, &donor_descriptor_index) in &descriptor_pairs {
        let target_resource =
            target_resources.rows + target_descriptor_index * WEAPON_ENTITY_RESOURCE_MAP_ROW_SIZE;
        let donor_resource =
            donor_resources.rows + donor_descriptor_index * WEAPON_ENTITY_RESOURCE_MAP_ROW_SIZE;
        let target_descriptor = target_descriptors.rows
            + target_descriptor_index * WEAPON_ENTITY_RESOURCE_DESCRIPTOR_ROW_SIZE;
        let donor_descriptor = donor_descriptors.rows
            + donor_descriptor_index * WEAPON_ENTITY_RESOURCE_DESCRIPTOR_ROW_SIZE;
        copy_row(
            target,
            target_resource,
            donor,
            donor_resource,
            WEAPON_ENTITY_RESOURCE_MAP_ROW_SIZE,
            "component resource-map",
        )?;
        copy_row(
            target,
            target_descriptor,
            donor,
            donor_descriptor,
            WEAPON_ENTITY_RESOURCE_DESCRIPTOR_ROW_SIZE,
            "component descriptor",
        )?;
        write_u32(
            target,
            target_descriptor + 0x10,
            u32::try_from(target_component_index)
                .map_err(|_| "Target component index does not fit u32")?,
        )?;
    }

    validate_weapon_entity(target)?;
    if target_owner_tag != donor_owner_tag
        && weapon_component_aliases(target)?
            .iter()
            .any(|alias| alias.owner_tag == target_owner_tag)
    {
        return Err(format!(
            "Runtime component binding 0x{requested_binding_hash:08X} left target owner 0x{target_owner_tag:08X} split across the authored entity"
        ));
    }
    let authored_partition = weapon_component_aliases(target)?
        .into_iter()
        .filter(|alias| alias.owner_tag == donor_owner_tag)
        .map(|alias| ((alias.binding_hash, alias.resource_index), alias))
        .collect::<BTreeMap<_, _>>();
    if authored_partition.keys().ne(donor_partition.keys()) {
        return Err(format!(
            "Runtime component binding 0x{requested_binding_hash:08X} did not preserve the donor owner partition"
        ));
    }
    Ok(())
}

fn weapon_component_aliases(entity: &[u8]) -> Result<Vec<ComponentAlias>, String> {
    let mut aliases = Vec::new();
    for binding_hash in weapon_component_binding_hashes(entity)? {
        aliases.extend(
            weapon_component_bindings(entity, binding_hash)?
                .into_iter()
                .map(|binding| ComponentAlias {
                    binding_hash,
                    resource_index: binding.resource_index,
                    descriptor_index: binding.descriptor_index,
                    owner_tag: binding.owner_tag,
                    concrete_class: binding.concrete_class,
                    resource_offset: binding.resource_offset,
                }),
        );
    }
    Ok(aliases)
}

/// Retargets every reference to one component-owner tag inside a private weapon entity.
pub fn retarget_weapon_component_owner(
    entity: &mut [u8],
    old_owner_tag: u32,
    new_owner_tag: u32,
) -> Result<usize, String> {
    if old_owner_tag == new_owner_tag {
        return Err("Component-owner retarget requires two different tags".into());
    }
    validate_file_size(entity, "Weapon entity")?;
    let components = native_array(entity, ENTITY_COMPONENTS_DESCRIPTOR)?;
    let definitions = native_array(entity, ENTITY_DEFINITION_MAP_DESCRIPTOR)?;
    let resource_map = native_array(entity, ENTITY_RESOURCE_MAP_DESCRIPTOR)?;
    let descriptors = native_array(entity, ENTITY_RESOURCE_DESCRIPTORS_DESCRIPTOR)?;
    validate_entity_arrays(entity, components, definitions, resource_map, descriptors)?;

    if (0..components.count).any(|index| {
        read_u32(
            entity,
            components.rows + index * WEAPON_ENTITY_COMPONENT_ROW_SIZE,
        ) == Ok(new_owner_tag)
    }) {
        return Err(format!(
            "Weapon entity already contains authored component owner 0x{new_owner_tag:08X}"
        ));
    }

    let mut owner_fields = owner::event_owner_fields(entity, old_owner_tag)?;
    for index in 0..components.count {
        let row = components.rows + index * WEAPON_ENTITY_COMPONENT_ROW_SIZE;
        if read_u32(entity, row)? == old_owner_tag {
            owner_fields.insert(row);
        }
    }
    for index in 0..descriptors.count {
        let descriptor = descriptors.rows + index * WEAPON_ENTITY_RESOURCE_DESCRIPTOR_ROW_SIZE;
        if read_u32(entity, descriptor)? == old_owner_tag {
            owner_fields.insert(descriptor);
        }
        let resource = resource_map.rows + index * WEAPON_ENTITY_RESOURCE_MAP_ROW_SIZE;
        if read_u32(entity, resource + 0x10)? == old_owner_tag {
            owner_fields.insert(resource + 0x10);
        }
    }
    if owner_fields.is_empty() {
        return Err(format!(
            "Weapon entity does not reference component owner 0x{old_owner_tag:08X}"
        ));
    }
    let mut authored = entity.to_vec();
    for offset in &owner_fields {
        write_u32(&mut authored, *offset, new_owner_tag)?;
    }
    validate_weapon_entity(&authored)?;
    entity.copy_from_slice(&authored);
    Ok(owner_fields.len())
}

/// Retargets the structurally proven owner-tag fields in a cloned component-owner payload.
///
/// Component resource descriptors store an absolute offset into the owner payload. Both the
/// resource prefix and the pointed-to concrete object begin with the owning tag. Nested
/// objects also carry reciprocal typed references to their instance or definition. Those
/// fields are rewritten while unrelated aligned integers are preserved.
pub fn retarget_weapon_component_owner_payload(
    owner_payload: &mut [u8],
    entity: &[u8],
    old_owner_tag: u32,
    new_owner_tag: u32,
) -> Result<usize, String> {
    if old_owner_tag == new_owner_tag {
        return Err("Component-owner payload retarget requires two different tags".into());
    }
    validate_file_size(owner_payload, "Weapon component owner")?;
    validate_weapon_entity(entity)?;

    let mut resources = BTreeMap::<usize, u32>::new();
    for binding_hash in weapon_component_binding_hashes(entity)? {
        for binding in weapon_component_bindings(entity, binding_hash)? {
            if binding.owner_tag != old_owner_tag {
                continue;
            }
            let resource_offset = usize::try_from(binding.resource_offset)
                .map_err(|_| "Component resource offset does not fit this platform")?;
            match resources.insert(resource_offset, binding.concrete_class) {
                Some(existing) if existing != binding.concrete_class => {
                    return Err(format!(
                        "Component owner 0x{old_owner_tag:08X} resource 0x{resource_offset:X} is aliased with conflicting concrete classes 0x{existing:08X} and 0x{:08X}",
                        binding.concrete_class
                    ));
                }
                _ => {}
            }
        }
    }
    if resources.is_empty() {
        return Err(format!(
            "Weapon entity does not bind component owner 0x{old_owner_tag:08X}"
        ));
    }

    let mut owner_fields = BTreeSet::new();
    for (resource_offset, concrete_class) in resources {
        let prefix_end = resource_offset
            .checked_add(16)
            .ok_or("Component resource prefix range overflowed")?;
        if prefix_end > owner_payload.len() {
            return Err(format!(
                "Component owner 0x{old_owner_tag:08X} resource prefix 0x{resource_offset:X} is out of bounds"
            ));
        }
        if read_u32(owner_payload, resource_offset)? != old_owner_tag {
            return Err(format!(
                "Component owner 0x{old_owner_tag:08X} resource 0x{resource_offset:X} has a mismatched owner tag"
            ));
        }
        let definition_class = read_u32(owner_payload, resource_offset + 4)?;
        if definition_class == 0 || definition_class == u32::MAX {
            return Err(format!(
                "Component owner 0x{old_owner_tag:08X} resource 0x{resource_offset:X} has an invalid definition class"
            ));
        }
        let concrete_offset = usize::try_from(read_u64(owner_payload, resource_offset + 8)?)
            .map_err(|_| "Component concrete-object offset does not fit this platform")?;
        if concrete_offset == resource_offset || concrete_offset % 8 != 0 {
            return Err(format!(
                "Component owner 0x{old_owner_tag:08X} resource 0x{resource_offset:X} has an invalid concrete-object offset 0x{concrete_offset:X}"
            ));
        }
        let concrete_end = concrete_offset
            .checked_add(8)
            .ok_or("Component concrete-object range overflowed")?;
        if concrete_end > owner_payload.len() {
            return Err(format!(
                "Component owner 0x{old_owner_tag:08X} concrete object 0x{concrete_offset:X} is out of bounds"
            ));
        }
        if read_u32(owner_payload, concrete_offset)? != old_owner_tag
            || read_u32(owner_payload, concrete_offset + 4)? != concrete_class
        {
            return Err(format!(
                "Component owner 0x{old_owner_tag:08X} concrete object 0x{concrete_offset:X} does not match class 0x{concrete_class:08X}"
            ));
        }
        owner_fields.insert(resource_offset);
        owner_fields.insert(concrete_offset);
    }

    owner_fields.extend(owner::self_reference_owner_fields(
        owner_payload,
        old_owner_tag,
    ));
    for offset in &owner_fields {
        write_u32(owner_payload, *offset, new_owner_tag)?;
    }
    Ok(owner_fields.len())
}

/// Validates every active binding in a complete weapon runtime entity.
pub fn validate_weapon_entity(entity: &[u8]) -> Result<(), String> {
    validate_file_size(entity, "Weapon entity")?;
    let components = native_array(entity, ENTITY_COMPONENTS_DESCRIPTOR)?;
    let definitions = native_array(entity, ENTITY_DEFINITION_MAP_DESCRIPTOR)?;
    let resource_map = native_array(entity, ENTITY_RESOURCE_MAP_DESCRIPTOR)?;
    let descriptors = native_array(entity, ENTITY_RESOURCE_DESCRIPTORS_DESCRIPTOR)?;
    validate_entity_arrays(entity, components, definitions, resource_map, descriptors)?;
    weapon_component_binding_hashes(entity)?;
    Ok(())
}

/// Adds one sorted global-id to entity assignment without disturbing the map's auxiliary array.
pub fn append_weapon_entity_assignment(
    mut assignments: Vec<u8>,
    pattern_global_id_hash: u32,
    entity_tag: u32,
) -> Result<Vec<u8>, String> {
    validate_file_size(&assignments, "Weapon entity-assignment map")?;
    let primary = assignment_rows(&assignments)?;
    let secondary = native_array(&assignments, 0x18)?;
    let primary_end = checked_rows_end(
        primary,
        SANDBOX_PATTERN_ENTITY_ASSIGNMENT_ROW_SIZE,
        assignments.len(),
        "sandbox-pattern entity assignment",
    )?;
    if primary_end.checked_add(ARRAY_TRAILER_SIZE) != Some(secondary.header) {
        return Err(
            "Weapon entity-assignment rows are not immediately followed by their auxiliary array"
                .into(),
        );
    }
    checked_rows_end(
        secondary,
        4,
        assignments.len(),
        "entity-assignment auxiliary",
    )?;
    for index in 0..secondary.count {
        if read_u32(&assignments, secondary.rows + index * 4)? != 0 {
            return Err(
                "Weapon entity-assignment auxiliary data is not the verified zero table".into(),
            );
        }
    }

    let mut keys = Vec::with_capacity(primary.count);
    for index in 0..primary.count {
        keys.push(read_u32(
            &assignments,
            primary.rows + index * SANDBOX_PATTERN_ENTITY_ASSIGNMENT_ROW_SIZE,
        )?);
    }
    if !keys.windows(2).all(|pair| pair[0] < pair[1]) {
        return Err("Weapon entity-assignment keys are not strictly ascending".into());
    }
    let insertion_index = match keys.binary_search(&pattern_global_id_hash) {
        Ok(_) => {
            return Err(format!(
                "Pattern global identity 0x{pattern_global_id_hash:08X} already exists"
            ));
        }
        Err(index) => index,
    };
    let insertion = primary.rows + insertion_index * SANDBOX_PATTERN_ENTITY_ASSIGNMENT_ROW_SIZE;
    assignments.splice(
        insertion..insertion,
        std::iter::repeat_n(0, SANDBOX_PATTERN_ENTITY_ASSIGNMENT_ROW_SIZE),
    );
    write_u32(&mut assignments, insertion, pattern_global_id_hash)?;
    write_u32(&mut assignments, insertion + 4, entity_tag)?;
    write_u64(
        &mut assignments,
        0x08,
        u64::try_from(primary.count + 1).map_err(|_| "Entity-assignment count is too large")?,
    )?;
    write_u64(
        &mut assignments,
        primary.header,
        u64::try_from(primary.count + 1).map_err(|_| "Entity-assignment count is too large")?,
    )?;
    write_relative_pointer(
        &mut assignments,
        0x20,
        secondary.header + SANDBOX_PATTERN_ENTITY_ASSIGNMENT_ROW_SIZE,
    )?;
    let assignment_len =
        u64::try_from(assignments.len()).map_err(|_| "Entity-assignment map is too large")?;
    write_u64(&mut assignments, FILE_SIZE_OFFSET, assignment_len)?;
    crate::sandbox_perk::ensure_runtime_map_capacity(&mut assignments)?;
    if weapon_entity_assignment(&assignments, pattern_global_id_hash)? != Some(entity_tag) {
        return Err("Authored weapon entity assignment could not be resolved".into());
    }
    Ok(assignments)
}

fn assignment_rows(assignments: &[u8]) -> Result<NativeArray, String> {
    let array = native_array(assignments, 0x08)?;
    if array.row_class != SANDBOX_PATTERN_ENTITY_ASSIGNMENT_ROW_CLASS {
        return Err(format!(
            "Weapon entity-assignment map has row class 0x{:08X}, expected 0x{SANDBOX_PATTERN_ENTITY_ASSIGNMENT_ROW_CLASS:08X}",
            array.row_class
        ));
    }
    Ok(array)
}

fn validate_entity_arrays(
    entity: &[u8],
    components: NativeArray,
    definitions: NativeArray,
    resource_map: NativeArray,
    descriptors: NativeArray,
) -> Result<(), String> {
    for (array, row_class, row_size, label) in [
        (
            components,
            WEAPON_ENTITY_COMPONENT_ROW_CLASS,
            WEAPON_ENTITY_COMPONENT_ROW_SIZE,
            "component",
        ),
        (
            definitions,
            WEAPON_ENTITY_DEFINITION_MAP_ROW_CLASS,
            SANDBOX_PATTERN_ENTITY_ASSIGNMENT_ROW_SIZE,
            "definition-map",
        ),
        (
            resource_map,
            WEAPON_ENTITY_RESOURCE_MAP_ROW_CLASS,
            WEAPON_ENTITY_RESOURCE_MAP_ROW_SIZE,
            "resource-map",
        ),
        (
            descriptors,
            WEAPON_ENTITY_RESOURCE_DESCRIPTOR_ROW_CLASS,
            WEAPON_ENTITY_RESOURCE_DESCRIPTOR_ROW_SIZE,
            "resource-descriptor",
        ),
    ] {
        if array.row_class != row_class {
            return Err(format!(
                "Weapon entity {label} array has class 0x{:08X}, expected 0x{row_class:08X}",
                array.row_class
            ));
        }
        checked_rows_end(array, row_size, entity.len(), label)?;
    }
    if resource_map.count != descriptors.count {
        return Err("Weapon entity resource-map and descriptor counts disagree".into());
    }
    Ok(())
}

fn native_array(data: &[u8], descriptor: usize) -> Result<NativeArray, String> {
    let count = usize::try_from(read_u64(data, descriptor)?)
        .map_err(|_| format!("Array at 0x{descriptor:X} has an excessive count"))?;
    let pointer = descriptor
        .checked_add(8)
        .ok_or("Native array pointer offset overflowed")?;
    let header = relative_target(pointer, read_i64(data, pointer)?)?;
    if read_u64(data, header)?
        != u64::try_from(count).map_err(|_| "Native array count does not fit u64")?
    {
        return Err(format!(
            "Array descriptor and header counts disagree at 0x{descriptor:X}"
        ));
    }
    Ok(NativeArray {
        count,
        header,
        rows: header
            .checked_add(ARRAY_HEADER_SIZE)
            .ok_or("Native array row offset overflowed")?,
        row_class: read_u32(data, header + 8)?,
    })
}

fn checked_rows_end(
    array: NativeArray,
    row_size: usize,
    payload_len: usize,
    label: &str,
) -> Result<usize, String> {
    let end = array
        .count
        .checked_mul(row_size)
        .and_then(|size| array.rows.checked_add(size))
        .ok_or_else(|| format!("{label} row extent overflowed"))?;
    if end > payload_len {
        return Err(format!("{label} rows extend beyond their payload"));
    }
    Ok(end)
}

fn validate_file_size(data: &[u8], label: &str) -> Result<(), String> {
    if usize::try_from(read_u64(data, FILE_SIZE_OFFSET)?)
        .map_err(|_| format!("{label} file size does not fit usize"))?
        != data.len()
    {
        return Err(format!(
            "{label} file-size field disagrees with its payload"
        ));
    }
    Ok(())
}

fn copy_row(
    target: &mut [u8],
    target_offset: usize,
    donor: &[u8],
    donor_offset: usize,
    size: usize,
    label: &str,
) -> Result<(), String> {
    let target_end = target_offset
        .checked_add(size)
        .ok_or_else(|| format!("{label} target range overflowed"))?;
    let donor_end = donor_offset
        .checked_add(size)
        .ok_or_else(|| format!("{label} donor range overflowed"))?;
    let source = donor
        .get(donor_offset..donor_end)
        .ok_or_else(|| format!("{label} donor row is out of bounds"))?;
    target
        .get_mut(target_offset..target_end)
        .ok_or_else(|| format!("{label} target row is out of bounds"))?
        .copy_from_slice(source);
    Ok(())
}

fn relative_target(pointer: usize, relative: i64) -> Result<usize, String> {
    let pointer = i64::try_from(pointer).map_err(|_| "Relative pointer does not fit i64")?;
    let target = pointer
        .checked_add(relative)
        .ok_or("Relative pointer overflowed")?;
    usize::try_from(target).map_err(|_| "Relative pointer is negative".into())
}

fn write_relative_pointer(data: &mut [u8], pointer: usize, target: usize) -> Result<(), String> {
    let relative = i64::try_from(target)
        .and_then(|target| i64::try_from(pointer).map(|pointer| target - pointer))
        .map_err(|_| "Relative pointer does not fit i64")?;
    write_i64(data, pointer, relative)
}

fn write_u32(data: &mut [u8], offset: usize, value: u32) -> Result<(), String> {
    write_array(data, offset, value.to_le_bytes())
}

fn write_u64(data: &mut [u8], offset: usize, value: u64) -> Result<(), String> {
    write_array(data, offset, value.to_le_bytes())
}

fn write_i64(data: &mut [u8], offset: usize, value: i64) -> Result<(), String> {
    write_array(data, offset, value.to_le_bytes())
}

fn write_array<const N: usize>(
    data: &mut [u8],
    offset: usize,
    value: [u8; N],
) -> Result<(), String> {
    crate::package_payload::write_bytes(data, offset, &value)
        .map_err(|error| format!("Weapon-entity write: {error}"))
}

#[cfg(test)]
mod tests;

use crate::package_payload::{i64_at as read_i64, u32_at as read_u32, u64_at as read_u64};

//! The editor reads the same composed Barrel that the final authoring step edits.
use super::payload::{private_index, read};
use super::*;
use sundial::package_authoring::entity::{
    WEAPON_BARREL_COMPONENT_KEY as BARREL, WeaponComponentBinding, spread,
    weapon_component_binding_hashes,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BarrelDefaults {
    pub pattern: Option<spread::Pattern>,
    /// The bullets one pull fires at each stat tier, when the stat translator keeps them in the
    /// one column this setting writes. None where that column holds 0, which is not offered.
    pub bullets_per_shot: Option<crate::weapon::burst::Column>,
    /// Whether each bullet's pattern takes a random angle in the selected Barrel.
    pub random_rotation: bool,
}

fn binding(entity: &[u8]) -> AuthoringResult<Option<WeaponComponentBinding>> {
    if !weapon_component_binding_hashes(entity)
        .map_err(invalid)?
        .contains(&BARREL)
    {
        return Ok(None);
    }
    let bindings = weapon_component_bindings(entity, BARREL).map_err(invalid)?;
    let [binding] = bindings.as_slice() else {
        return Err(invalid("Barrel controls require exactly one Barrel"));
    };
    Ok(Some(*binding))
}

/// Apply component defaults and explicit low-level edits without depending on an unrelated
/// firing graph. These are the same mutation functions the full build uses before these controls.
/// `group` is the gameplay pattern's translation group, which picks the stat translator's table.
pub(crate) fn barrel_defaults(
    manager: &PackageManager,
    entity: &[u8],
    group: u32,
    overrides: &WeaponCloneOverrides,
    splices: &[(u32, Vec<u8>)],
) -> AuthoringResult<Option<BarrelDefaults>> {
    let start = manager
        .lookup
        .tag32_entries_by_pkg
        .get(&HOST_PACKAGE_ID)
        .ok_or_else(|| invalid("The runtime host package is unavailable"))?
        .len();
    let allocator = AppendedTagAllocator::new(HOST_PACKAGE_ID, start);
    let mut entity = entity.to_vec();
    let mut tags = Vec::new();
    let mut appends = Vec::new();
    let spliced = component_splice_edits(manager, &entity, splices, &mut appends)?;
    let mut patches = overrides.runtime_resource_patches.clone();
    let spliced = trim_splices(
        manager,
        &entity,
        &overrides.runtime_values,
        &patches,
        spliced,
        &mut appends,
    )?;
    patches.extend(spliced);
    append_patched_runtime_resource_owners(
        manager,
        &mut entity,
        &overrides.runtime_values,
        &patches,
        &appends,
        allocator,
        &mut tags,
    )?;
    apply_raw_payload_target(
        &mut entity,
        WeaponRawPayloadTarget::RuntimeWeaponEntity,
        &overrides.raw_payload_patches,
    )?;
    validate_raw_payload_target(
        &entity,
        WeaponRawPayloadTarget::RuntimeWeaponEntity,
        &overrides.raw_payload_patches,
    )?;
    let Some(binding) = binding(&entity)? else {
        return Ok(None);
    };
    let owner = read(manager, binding.owner_tag, allocator, &tags)?;
    // A translator this setting cannot read leaves Bullets per Shot out, rather than the Barrel's
    // other settings.
    let bullets_per_shot = crate::weapon::burst::read(manager, &entity, group)
        .ok()
        .flatten()
        .map(|burst| burst.column);
    Ok(Some(BarrelDefaults {
        pattern: spread::read_pattern(&owner, binding).map_err(invalid)?,
        bullets_per_shot,
        random_rotation: spread::read_random_rotation(&owner, binding).map_err(invalid)?,
    }))
}

pub(super) fn apply(
    manager: &PackageManager,
    entity: &mut [u8],
    edits: Option<&crate::weapon::barrel::Edits>,
    allocator: AppendedTagAllocator,
    tags: &mut Vec<NewTagSpec>,
) -> AuthoringResult<()> {
    let Some(edits) =
        edits.filter(|edits| edits.shapes_pattern() || edits.random_rotation.is_some())
    else {
        return Ok(());
    };
    let binding = binding(entity)?.ok_or_else(|| invalid("This weapon has no Barrel to edit"))?;
    let mut owner = read(manager, binding.owner_tag, allocator, tags)?;
    if edits.shapes_pattern() {
        let inherited = spread::read_pattern(&owner, binding).map_err(invalid)?;
        let pattern = edits.resolve(inherited.as_ref()).map_err(invalid)?;
        spread::write_pattern(&mut owner, binding, &pattern).map_err(invalid)?;
    }
    if let Some(enabled) = edits.random_rotation {
        spread::write_random_rotation(&mut owner, binding, enabled).map_err(invalid)?;
    }
    if let Some(index) = private_index(binding.owner_tag, allocator, tags)? {
        tags[index].payload = owner;
    } else {
        let private =
            allocator.assigned_tag(tags.len(), "Barrel controls", "runtime component owner")?;
        retarget_weapon_component_owner_payload(&mut owner, entity, binding.owner_tag, private.0)
            .map_err(invalid)?;
        retarget_weapon_component_owner(entity, binding.owner_tag, private.0).map_err(invalid)?;
        tags.push(NewTagSpec {
            template_tag: TagHash(binding.owner_tag),
            payload: owner,
            storage: crate::NewTagStorageMode::InheritTemplate,
        });
    }
    Ok(())
}

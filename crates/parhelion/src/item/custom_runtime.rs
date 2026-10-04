//! Native custom runtime operations with independent validation.
use super::*;
mod actions;
#[cfg(feature = "d2-model-importer")]
pub(in crate::item) mod animation;
#[cfg(feature = "d2-model-importer")]
pub(in crate::item) mod audio;
#[cfg(feature = "d2-model-importer")]
pub(in crate::item) mod crosshair;
#[cfg(feature = "d2-model-importer")]
pub(in crate::item) mod extensions;
pub(in crate::item) mod palettes;
#[cfg(feature = "d2-model-importer")]
pub(in crate::item) mod particles;
mod sword_profiles;
#[cfg(test)]
mod tests;

/// Resolve both build and preview HUD behavior through the same decision path.
pub(crate) fn runtime_hud_key(
    manager: &PackageManager,
    custom_key: Option<u32>,
    unchanged_content: bool,
    appearance: impl FnOnce() -> AuthoringResult<Option<(Vec<u8>, u32)>>,
) -> AuthoringResult<Option<u32>> {
    if let Some(key) = custom_key {
        return Ok(Some(key));
    }
    if unchanged_content {
        return Ok(None);
    }
    let Some((entity, content_group)) = appearance()? else {
        return Ok(None);
    };
    crate::hud_icon::runtime::inherited_key(manager, &entity, content_group).map(Some)
}

/// Exercise the compiler's runtime mutation in memory. No package or account is written.
pub(crate) fn preflight_runtime_edits(
    manager: &PackageManager,
    entity: &[u8],
    overrides: &WeaponCloneOverrides,
    hud_key: Option<u32>,
    content_group: Option<u32>,
) -> AuthoringResult<()> {
    let start = manager
        .lookup
        .tag32_entries_by_pkg
        .get(&HOST_PACKAGE_ID)
        .ok_or_else(|| invalid("The runtime host package is unavailable"))?
        .len();
    let allocator = AppendedTagAllocator::new(HOST_PACKAGE_ID, start);
    author_runtime_edits(
        manager,
        &mut entity.to_vec(),
        overrides,
        hud_key,
        Sources {
            groups: crate::weapon::behavior::ContentGroups {
                selected: content_group,
                ..Default::default()
            },
            splices: &[],
            actions: &[],
        },
        allocator,
        &mut Vec::new(),
    )
}

/// The writes and appends that copy each spliced component from its donor onto the weapon's own
/// objects. See `sundial::package_authoring::entity::plan_component_splice`.
fn component_splice_edits(
    manager: &PackageManager,
    entity: &[u8],
    splices: &[(u32, Vec<u8>)],
    appends: &mut Vec<WeaponRuntimeResourceAppend>,
) -> AuthoringResult<Vec<WeaponRuntimeResourcePatch>> {
    let mut patches = Vec::new();
    for (binding, donor) in splices {
        let plan = sundial::package_authoring::entity::plan_component_splice(
            manager, entity, donor, *binding,
        )
        .map_err(invalid)?;
        let relative = |at: usize| {
            at.checked_sub(plan.resource_offset)
                .and_then(|offset| u32::try_from(offset).ok())
                .ok_or_else(|| invalid("A spliced component edit lies before its resource"))
        };
        for (at, bytes) in plan.writes {
            patches.push(WeaponRuntimeResourcePatch {
                binding_hash: *binding,
                resource_index: 0,
                offset: relative(at)?,
                bytes,
                graph_values: Vec::new(),
                graph_removals: Vec::new(),
                graph_trajectories: None,
            });
        }
        for (descriptor, bytes, count) in plan.arrays {
            appends.push(WeaponRuntimeResourceAppend {
                binding_hash: *binding,
                resource_index: 0,
                bytes,
                slots: Vec::new(),
                arrays: vec![(relative(descriptor)?, 0, count)],
            });
        }
    }
    Ok(patches)
}

/// The parts of a copied-value write that no other patch of the same component covers.
fn trim(
    splice: WeaponRuntimeResourcePatch,
    others: &[WeaponRuntimeResourcePatch],
) -> Vec<WeaponRuntimeResourcePatch> {
    let start = splice.offset as usize;
    let mut kept = vec![true; splice.bytes.len()];
    for other in others.iter().filter(|other| {
        other.binding_hash == splice.binding_hash && other.resource_index == splice.resource_index
    }) {
        let (from, to) = (
            other.offset as usize,
            other.offset as usize + other.bytes.len(),
        );
        for (index, keep) in kept.iter_mut().enumerate() {
            if (from..to).contains(&(start + index)) {
                *keep = false;
            }
        }
    }
    let mut pieces = Vec::new();
    let mut index = 0;
    while index < kept.len() {
        if !kept[index] {
            index += 1;
            continue;
        }
        let run = kept[index..].iter().take_while(|keep| **keep).count();
        pieces.push(WeaponRuntimeResourcePatch {
            offset: (start + index) as u32,
            bytes: splice.bytes[index..index + run].to_vec(),
            ..splice.clone()
        });
        index += run;
    }
    pieces
}

pub(super) struct Sources<'a> {
    pub groups: crate::weapon::behavior::ContentGroups,
    pub splices: &'a [(u32, Vec<u8>)],
    pub actions: &'a [(
        crate::recipe::AnimationAction,
        crate::weapon::animations::Profile,
    )],
}

pub(super) fn author_runtime_edits(
    manager: &PackageManager,
    entity: &mut [u8],
    overrides: &WeaponCloneOverrides,
    hud_key: Option<u32>,
    sources: Sources<'_>,
    allocator: AppendedTagAllocator,
    tags: &mut Vec<NewTagSpec>,
) -> AuthoringResult<()> {
    let Sources {
        groups,
        splices,
        actions,
    } = sources;
    let mut patches = overrides.runtime_resource_patches.clone();
    // Before any owner is copied, so the private arms rig's tags come first and the attachment
    // row that names it is one more patch on the attachment owner.
    patches.extend(actions::author(
        manager,
        entity,
        groups.selected,
        actions,
        allocator,
        tags,
    )?);
    let mut splice_appends = Vec::new();
    let spliced = component_splice_edits(manager, entity, splices, &mut splice_appends)?;
    if let Some(key) = hud_key {
        patches.extend(crate::hud_icon::runtime::patches(manager, entity, key)?);
    }
    if let Some(hold) = groups.hold {
        patches.extend(crate::weapon::animations::hold_patches(
            manager, entity, hold,
        )?);
    }
    if let Some(donor) = groups.animations {
        patches.extend(crate::weapon::animations::patches(
            manager,
            entity,
            groups.selected,
            donor,
        )?);
    }
    let grafts = crate::weapon::behavior::patches(
        manager,
        entity,
        groups,
        &overrides.additional_behaviors,
        overrides
            .behavior_projectile_speed
            .unwrap_or(crate::weapon::behavior::DEFAULT_PROJECTILE_SPEED_BOOST),
    )?;
    // The profile planner computes relative references from the original owner length.
    // Keep its appends first for that owner, before any independent behavior grafts.
    let mut appends = Vec::new();
    if let Some(profile) = overrides.sword_profile {
        let edits = sword_profiles::edits(manager, entity, profile)?;
        patches.extend(edits.patches);
        appends.extend(edits.appends);
    }
    appends.extend(grafts.appends);
    patches.extend(grafts.patches);
    if let Some(fired) = &overrides.fired_graph {
        if !overrides.additional_behaviors.is_empty() {
            return Err(invalid(
                "A fired graph replaces the weapon's firing graph, so it cannot be combined with an additional behavior.",
            ));
        }
        let source = TagHash(fired.source_graph);
        let payload = read_tag(manager, source, "fired graph")?;
        if sundial::package_authoring::sandbox_perk::entity::kind(&payload).map_err(invalid)?
            != Some(sundial::package_authoring::sandbox_perk::entity::Kind::Projectile)
        {
            return Err(invalid(format!("Fired graph {source} is not a projectile")));
        }
        let private = append_private_asset_graph(
            manager,
            source,
            &fired.patches,
            &fired.appends,
            &BTreeSet::new(),
            "Fired graph",
            allocator,
            tags,
        )?;
        let named = crate::weapon::behavior::fired_graph_patches(manager, entity, private.0)?;
        // Putting the base's own graph back into the selected block writes these same slots.
        patches.retain(|patch| {
            !named.iter().any(|slot| {
                slot.binding_hash == patch.binding_hash
                    && slot.resource_index == patch.resource_index
                    && slot.offset == patch.offset
            })
        });
        patches.extend(named);
    }
    if let Some(ammo) = overrides.ammo_type {
        patches.extend(crate::weapon::ammo::patches(manager, entity, ammo)?);
    }
    // A component's copied values give way to any edit the recipe makes in the same component.
    let spliced = spliced
        .into_iter()
        .flat_map(|splice| trim(splice, &patches))
        .collect::<Vec<_>>();
    patches.extend(spliced);
    appends.extend(splice_appends);
    append_patched_runtime_resource_owners(
        manager,
        entity,
        &overrides.runtime_values,
        &patches,
        &appends,
        allocator,
        tags,
    )?;
    apply_raw_payload_target(
        entity,
        WeaponRawPayloadTarget::RuntimeWeaponEntity,
        &overrides.raw_payload_patches,
    )?;
    validate_raw_payload_target(
        entity,
        WeaponRawPayloadTarget::RuntimeWeaponEntity,
        &overrides.raw_payload_patches,
    )?;
    validate_weapon_entity(entity).map_err(invalid)
}

pub(super) fn resolve_runtime_weapon_entity(
    manager: &PackageManager,
    sandbox_patterns: &[u8],
    entity_assignments: &[u8],
    item_hash: u32,
    description: &str,
) -> AuthoringResult<(TagHash, Vec<u8>)> {
    let pattern = sandbox_pattern_identity(sandbox_patterns, item_hash)
        .map_err(invalid)?
        .ok_or_else(|| {
            invalid(format!(
                "{description} 0x{item_hash:08X} has no sandbox-pattern row"
            ))
        })?;
    let entity_tag = TagHash(
        weapon_entity_assignment(entity_assignments, pattern.pattern_global_id_hash)
            .map_err(invalid)?
            .ok_or_else(|| {
                invalid(format!(
                    "{description} 0x{item_hash:08X} has no runtime weapon-entity assignment"
                ))
            })?,
    );
    let entry = manager.get_entry(entity_tag).ok_or_else(|| {
        invalid(format!(
            "{description} runtime weapon entity {entity_tag} is not live"
        ))
    })?;
    if entry.reference != WEAPON_ENTITY_CLASS {
        return Err(invalid(format!(
            "{description} runtime weapon entity {entity_tag} has class 0x{:08X}, expected 0x{WEAPON_ENTITY_CLASS:08X}",
            entry.reference
        )));
    }
    let payload = read_tag(manager, entity_tag, description)?;
    Ok((entity_tag, payload))
}

pub(super) fn append_patched_runtime_resource_owners(
    manager: &PackageManager,
    entity: &mut [u8],
    values: &[WeaponRuntimeValueOverride],
    patches: &[WeaponRuntimeResourcePatch],
    appends: &[WeaponRuntimeResourceAppend],
    runtime_tag_allocator: AppendedTagAllocator,
    runtime_new_tags: &mut Vec<NewTagSpec>,
) -> AuthoringResult<()> {
    validate_projectile_values(manager, entity, values)?;
    let mut patches_by_owner = BTreeMap::<u32, Vec<ResolvedRuntimeResourcePatch>>::new();
    // One private clone per stock graph and the edits it carries.
    let mut graph_clones = Vec::<(u32, GraphEdit, TagHash)>::new();
    for (value_index, value) in values.iter().enumerate() {
        let resolved = resolve_weapon_runtime_field(manager, entity, &value.locator)
            .map_err(|error| invalid(format!("Runtime value {value_index} is stale: {error}")))?;
        let bytes = sundial::package_authoring::runtime::encode_weapon_runtime_field_value(
            &resolved.field,
            &value.value,
        )
        .map_err(|error| invalid(format!("Runtime value {value_index} is invalid: {error}")))?;
        if bytes.len() != usize::try_from(value.locator.byte_size).unwrap_or(usize::MAX) {
            return Err(invalid(format!(
                "Runtime value {value_index} encoded to {} bytes, expected {}",
                bytes.len(),
                value.locator.byte_size
            )));
        }
        let start = resolved.owner_offset;
        let end = start.checked_add(bytes.len()).ok_or_else(|| {
            invalid(format!(
                "Runtime value {value_index} absolute range overflows"
            ))
        })?;
        patches_by_owner
            .entry(resolved.owner_tag)
            .or_default()
            .push(ResolvedRuntimeResourcePatch {
                label: format!("typed value {value_index} ({})", resolved.field.path_label),
                binding_hash: value.locator.binding_hash.get(),
                resource_index: usize::from(value.locator.resource_index),
                start,
                end,
                bytes,
            });
    }
    for (patch_index, patch) in patches.iter().enumerate() {
        let bindings = weapon_component_bindings(entity, patch.binding_hash).map_err(invalid)?;
        let resource_index = usize::from(patch.resource_index);
        let binding = bindings.get(resource_index).ok_or_else(|| {
            invalid(format!(
                "Runtime resource patch {patch_index} selects resource {resource_index} of component binding 0x{:08X}, but that binding has {} resources",
                patch.binding_hash,
                bindings.len()
            ))
        })?;
        let resource_start = usize::try_from(binding.resource_offset).map_err(|_| {
            invalid(format!(
                "Runtime component binding 0x{:08X} resource {resource_index} offset does not fit this platform",
                patch.binding_hash
            ))
        })?;
        let relative_start = usize::try_from(patch.offset).map_err(|_| {
            invalid(format!(
                "Runtime resource patch {patch_index} offset is too large"
            ))
        })?;
        let start = resource_start.checked_add(relative_start).ok_or_else(|| {
            invalid(format!(
                "Runtime resource patch {patch_index} absolute range overflows"
            ))
        })?;
        let end = start.checked_add(patch.bytes.len()).ok_or_else(|| {
            invalid(format!(
                "Runtime resource patch {patch_index} absolute range overflows"
            ))
        })?;
        let bytes = if patch.graph_values.is_empty()
            && patch.graph_removals.is_empty()
            && patch.graph_trajectories.is_none()
        {
            patch.bytes.clone()
        } else {
            let source = TagHash(read_u32(&patch.bytes, 0)?);
            let edit = (
                patch.graph_values.clone(),
                patch.graph_removals.clone(),
                patch.graph_trajectories,
            );
            let existing = graph_clones
                .iter()
                .find(|(tag, cloned, _)| *tag == source.0 && *cloned == edit);
            let authored = if let Some((_, _, authored)) = existing {
                *authored
            } else {
                let authored = append_private_graph_edit(
                    manager,
                    source,
                    &patch.graph_values,
                    &patch.graph_removals.iter().copied().collect(),
                    patch.graph_trajectories,
                    runtime_tag_allocator,
                    runtime_new_tags,
                )?;
                graph_clones.push((source.0, edit, authored));
                authored
            };
            authored.0.to_le_bytes().to_vec()
        };
        patches_by_owner
            .entry(binding.owner_tag)
            .or_default()
            .push(ResolvedRuntimeResourcePatch {
                label: format!("technical patch {patch_index}"),
                binding_hash: patch.binding_hash,
                resource_index,
                start,
                end,
                bytes,
            });
    }

    // An appended record has to reach its owner even when nothing else patches that owner. The
    // loop below is keyed on the patch map, so a graft that only appends, which is what a record
    // copied across content owners produces, was dropped without a word. That is the shape every
    // element switch takes on a family with no record of its own.
    for append in appends {
        let bindings = weapon_component_bindings(entity, append.binding_hash).map_err(invalid)?;
        if let Some(binding) = bindings.get(usize::from(append.resource_index)) {
            patches_by_owner.entry(binding.owner_tag).or_default();
        }
    }

    for (owner_tag, mut owner_patches) in patches_by_owner {
        let owner_tag = TagHash(owner_tag);
        let owner_entry = manager
            .get_entry(owner_tag)
            .ok_or_else(|| invalid(format!("Runtime component owner {owner_tag} is not live")))?;
        if owner_entry.file_type != 8 {
            return Err(invalid(format!(
                "Runtime component owner {owner_tag} is file type {}, not a structured resource",
                owner_entry.file_type
            )));
        }
        let mut owner_payload = read_tag(manager, owner_tag, "runtime component owner")?;
        if usize::try_from(read_u64(&owner_payload, 0)?).ok() != Some(owner_payload.len()) {
            return Err(invalid(format!(
                "Runtime component owner {owner_tag} has an inconsistent file-size field"
            )));
        }
        // Growth first, so the slots that point into the new bytes are ordinary patches from here.
        let mut grown = Vec::new();
        let original_owner_len = owner_payload.len();
        for append in appends {
            let bindings =
                weapon_component_bindings(entity, append.binding_hash).map_err(invalid)?;
            let Some(binding) = bindings.get(usize::from(append.resource_index)) else {
                return Err(invalid(format!(
                    "Appended record selects resource {} of component binding 0x{:08X}, which has {}",
                    append.resource_index,
                    append.binding_hash,
                    bindings.len()
                )));
            };
            if binding.owner_tag != owner_tag.0 {
                continue;
            }
            let resource = usize::try_from(binding.resource_offset)
                .map_err(|_| invalid("Appended record resource offset does not fit"))?;
            if !append.arrays.is_empty() {
                owner_payload.resize(owner_payload.len().next_multiple_of(16), 0);
            }
            let at = owner_payload.len();
            owner_payload.extend_from_slice(&append.bytes);
            for (descriptor, header, count) in &append.arrays {
                let descriptor = resource
                    .checked_add(usize::try_from(*descriptor).unwrap_or(usize::MAX))
                    .ok_or_else(|| invalid("Appended array descriptor offset overflows"))?;
                let header = at
                    .checked_add(*header)
                    .filter(|header| header + 16 <= owner_payload.len())
                    .ok_or_else(|| invalid("Appended array header is outside the added bytes"))?;
                let relative = i64::try_from(header)
                    .and_then(|header| i64::try_from(descriptor + 8).map(|from| header - from))
                    .map_err(|_| invalid("Appended array pointer overflows"))?;
                let mut bytes = count.to_le_bytes().to_vec();
                bytes.extend_from_slice(&relative.to_le_bytes());
                grown.push(ResolvedRuntimeResourcePatch {
                    label: format!("appended array at 0x{header:X}"),
                    binding_hash: append.binding_hash,
                    resource_index: usize::from(append.resource_index),
                    start: descriptor,
                    end: descriptor + bytes.len(),
                    bytes,
                });
            }
            for (slot, target, count) in &append.slots {
                let slot = resource
                    .checked_add(usize::try_from(*slot).unwrap_or(usize::MAX))
                    .ok_or_else(|| invalid("Appended record slot offset overflows"))?;
                let target = at
                    .checked_add(*target)
                    .filter(|target| *target <= owner_payload.len())
                    .ok_or_else(|| invalid("Appended record target is outside the added bytes"))?;
                let relative = i64::try_from(target)
                    .and_then(|target| i64::try_from(slot).map(|slot| target - slot))
                    .map_err(|_| invalid("Appended record pointer overflows"))?;
                let mut bytes = relative.to_le_bytes().to_vec();
                bytes.extend_from_slice(&count.to_le_bytes());
                grown.push(ResolvedRuntimeResourcePatch {
                    label: format!("appended record at 0x{at:X}"),
                    binding_hash: append.binding_hash,
                    resource_index: usize::from(append.resource_index),
                    start: slot,
                    end: slot + bytes.len(),
                    bytes,
                });
            }
        }
        if owner_payload.len() != original_owner_len {
            let length = u64::try_from(owner_payload.len())
                .map_err(|_| invalid("Grown component owner is too large"))?;
            owner_payload[..8].copy_from_slice(&length.to_le_bytes());
        }
        owner_patches.extend(grown);
        owner_patches.sort_by_key(|patch| (patch.start, patch.end, patch.label.clone()));
        for pair in owner_patches.windows(2) {
            if pair[1].start < pair[0].end {
                return Err(invalid(format!(
                    "Runtime edits {} and {} overlap inside component owner {owner_tag}",
                    pair[0].label, pair[1].label
                )));
            }
        }

        let authored_owner_tag = runtime_tag_allocator.assigned_tag(
            runtime_new_tags.len(),
            "Authored runtime component owner",
            "runtime component owner",
        )?;
        for patch in &owner_patches {
            let owner_len = owner_payload.len();
            let target = owner_payload
                .get_mut(patch.start..patch.end)
                .ok_or_else(|| {
                    invalid(format!(
                        "Runtime edit {} for binding 0x{:08X} resource {} range 0x{:X}..0x{:X} exceeds component owner {owner_tag} (0x{owner_len:X} bytes)",
                        patch.label,
                        patch.binding_hash,
                        patch.resource_index,
                        patch.start,
                        patch.end
                    ))
                })?;
            target.copy_from_slice(&patch.bytes);
        }
        // Byte-range edits can include native references. Retarget the final payload
        // so copying source bytes cannot restore a link to the stock owner.
        retarget_weapon_component_owner_payload(
            &mut owner_payload,
            entity,
            owner_tag.0,
            authored_owner_tag.0,
        )
        .map_err(invalid)?;
        retarget_weapon_component_owner(entity, owner_tag.0, authored_owner_tag.0)
            .map_err(invalid)?;
        runtime_new_tags.push(NewTagSpec {
            template_tag: owner_tag,
            payload: owner_payload,
            storage: crate::NewTagStorageMode::InheritTemplate,
        });
    }
    validate_weapon_entity(entity).map_err(invalid)
}

/// Clone only the referenced graph and edited component owners. All stock tags remain intact.
#[cfg(test)]
pub(super) fn append_private_referenced_graph(
    manager: &PackageManager,
    source: TagHash,
    values: &[WeaponRuntimeValueOverride],
    allocator: AppendedTagAllocator,
    tags: &mut Vec<NewTagSpec>,
) -> AuthoringResult<TagHash> {
    append_private_patched_graph(manager, source, values, &[], allocator, tags)
}

/// A private graph clone's edits: its values, the owners it leaves out and its trajectory pool.
type GraphEdit = (Vec<WeaponRuntimeValueOverride>, Vec<u32>, Option<u16>);

/// As [`append_private_patched_graph`], with whole component owners left out of the clone and
/// its projectile given room for `trajectories` at once. Behavior grafts and attachment edits use
/// this for a host that cannot carry the graph as it is.
fn append_private_graph_edit(
    manager: &PackageManager,
    source: TagHash,
    values: &[WeaponRuntimeValueOverride],
    removals: &BTreeSet<u32>,
    trajectories: Option<u16>,
    allocator: AppendedTagAllocator,
    tags: &mut Vec<NewTagSpec>,
) -> AuthoringResult<TagHash> {
    let first = tags.len();
    let authored = append_private_patched_graph(manager, source, values, &[], allocator, tags)?;
    // The graph is the last tag the copy appends, after any owner its values changed.
    let graph = tags
        .len()
        .checked_sub(1)
        .filter(|&graph| graph >= first && tags[graph].template_tag == source)
        .ok_or_else(|| invalid("A private graph copy did not end with its graph"))?;
    if !removals.is_empty() {
        sundial::package_authoring::entity::remove_weapon_components(
            &mut tags[graph].payload,
            removals,
        )
        .map_err(|error| invalid(format!("Graph 0x{:08X}: {error}", source.0)))?;
    }
    if let Some(capacity) = trajectories {
        let mut entity = std::mem::take(&mut tags[graph].payload);
        let widened = widen_trajectory_pool(
            manager,
            &mut entity,
            first..graph,
            usize::from(capacity),
            allocator,
            tags,
        );
        tags[graph].payload = entity;
        widened.map_err(|error| invalid(format!("Graph 0x{:08X}: {error}", source.0)))?;
    }
    Ok(authored)
}

/// Gives the projectile of a private graph room for `capacity` trajectories. Its owner is copied
/// privately after the graph, unless this clone already copied it for its values, in which case
/// that copy grows in place. The graph's tag, assigned before any of this, stays as it was.
fn widen_trajectory_pool(
    manager: &PackageManager,
    entity: &mut [u8],
    copied: std::ops::Range<usize>,
    capacity: usize,
    allocator: AppendedTagAllocator,
    tags: &mut Vec<NewTagSpec>,
) -> AuthoringResult<()> {
    use sundial::package_authoring::entity::{
        grow_projectile_trajectories, retarget_weapon_component_owner,
        retarget_weapon_component_owner_payload,
    };
    /// A graph's Projectile Movement binding.
    const PROJECTILE_MOVEMENT: u32 = 0x0437_756D;
    let bindings = weapon_component_bindings(entity, PROJECTILE_MOVEMENT).map_err(invalid)?;
    let [projectile] = bindings.as_slice() else {
        return Err(invalid("The graph does not have exactly one projectile"));
    };
    let owner_tag = projectile.owner_tag;
    let instance = usize::try_from(projectile.resource_offset)
        .map_err(|_| invalid("Projectile resource offset overflow"))?;
    for index in copied {
        if allocator
            .assigned_tag(index, "Private projectile owner", "runtime component owner")?
            .0
            == owner_tag
        {
            return grow_projectile_trajectories(
                entity,
                owner_tag,
                &mut tags[index].payload,
                instance,
                capacity,
            )
            .map_err(invalid);
        }
    }
    let stock = TagHash(owner_tag);
    let mut owner = read_tag(manager, stock, "projectile owner")?;
    grow_projectile_trajectories(entity, owner_tag, &mut owner, instance, capacity)
        .map_err(invalid)?;
    let authored = allocator.assigned_tag(
        tags.len(),
        "Private projectile owner",
        "runtime component owner",
    )?;
    retarget_weapon_component_owner_payload(&mut owner, entity, owner_tag, authored.0)
        .map_err(invalid)?;
    retarget_weapon_component_owner(entity, owner_tag, authored.0).map_err(invalid)?;
    tags.push(NewTagSpec {
        template_tag: stock,
        payload: owner,
        storage: crate::NewTagStorageMode::InheritTemplate,
    });
    Ok(())
}

/// Most levels of graphs a copy follows below its source: the graphs an ability spawns, and
/// theirs.
const SPAWN_DEPTH: usize = 3;

/// Copies `source` and every graph below it, up to [`SPAWN_DEPTH`] levels, whose values
/// `values` change or that `patches` patch, each graph once. A value names its graph by its
/// locator's graph tag, and one without a tag belongs to `source`. Each copy names the copies of
/// the graphs under it in place of the stock ones, so the stock graphs and every other graph
/// naming them stay as they are.
/// Refuses a value whose graph `source` does not reach, and a copy of an ability bank: the build
/// adds its property rows to the stock banks, which a copy would leave behind, so a graph a bank
/// names is not reached through the bank.
pub(super) fn append_private_graph_tree(
    manager: &PackageManager,
    source: TagHash,
    values: &[WeaponRuntimeValueOverride],
    patches: &BTreeMap<u32, Vec<WeaponRuntimeResourcePatch>>,
    allocator: AppendedTagAllocator,
    tags: &mut Vec<NewTagSpec>,
) -> AuthoringResult<TagHash> {
    let mut by_graph = BTreeMap::<u32, Vec<WeaponRuntimeValueOverride>>::new();
    for value in values {
        by_graph
            .entry(
                value
                    .locator
                    .graph_tag
                    .map(|tag| tag.get())
                    .unwrap_or(source.0),
            )
            .or_default()
            .push(value.clone());
    }
    let mut tree = GraphTree {
        manager,
        edited: by_graph.keys().chain(patches.keys()).copied().collect(),
        spawns: BTreeMap::new(),
        needed: BTreeSet::new(),
        copies: BTreeMap::new(),
    };
    tree.walk(source.0, 0, &mut BTreeSet::new())?;
    if let Some(unreached) = by_graph
        .keys()
        .chain(patches.keys())
        .find(|graph| **graph != source.0 && !tree.needed.contains(*graph))
    {
        return Err(invalid(format!(
            "A value names graph 0x{unreached:08X}, which {source} does not spawn"
        )));
    }
    tree.copy(source.0, (&by_graph, patches), allocator, tags)
}

/// The graphs below a source that lead to edited ones, and the copies made of them.
/// Each graph's own value overrides and resource patches, by graph.
type GraphEdits<'a> = (
    &'a BTreeMap<u32, Vec<WeaponRuntimeValueOverride>>,
    &'a BTreeMap<u32, Vec<WeaponRuntimeResourcePatch>>,
);

struct GraphTree<'a> {
    manager: &'a PackageManager,
    edited: BTreeSet<u32>,
    /// Where each walked graph names the graphs below it.
    spawns: BTreeMap<u32, Vec<sundial::package_authoring::ability_spawns::Spawn>>,
    /// Graphs below the source that are edited or lead to an edited one.
    needed: BTreeSet<u32>,
    copies: BTreeMap<u32, TagHash>,
}

impl GraphTree<'_> {
    /// Walks `graph` and the graphs below it, and returns whether any of them is edited.
    fn walk(
        &mut self,
        graph: u32,
        depth: usize,
        visiting: &mut BTreeSet<u32>,
    ) -> AuthoringResult<bool> {
        let mut leads = self.edited.contains(&graph);
        if depth >= SPAWN_DEPTH || !visiting.insert(graph) {
            return Ok(leads);
        }
        let payload = read_tag(self.manager, TagHash(graph), "spawned graph")?;
        let spawns =
            sundial::package_authoring::ability_spawns::spawns(self.manager, graph, &payload)
                .map_err(invalid)?
                .into_iter()
                .filter(|spawn| !sundial::package_authoring::ability_modifier::is_bank(spawn.owner))
                .collect::<Vec<_>>();
        let below = spawns
            .iter()
            .map(|spawn| spawn.graph)
            .collect::<BTreeSet<_>>();
        self.spawns.insert(graph, spawns);
        for child in below {
            if self.walk(child, depth + 1, visiting)? {
                self.needed.insert(child);
                leads = true;
            }
        }
        visiting.remove(&graph);
        Ok(leads)
    }

    /// Copies `graph` with its own values and patches, naming the copies of the needed graphs
    /// below it.
    fn copy(
        &mut self,
        graph: u32,
        (values, own_patches): GraphEdits<'_>,
        allocator: AppendedTagAllocator,
        tags: &mut Vec<NewTagSpec>,
    ) -> AuthoringResult<TagHash> {
        if let Some(copy) = self.copies.get(&graph) {
            return Ok(*copy);
        }
        let mut patches = own_patches.get(&graph).cloned().unwrap_or_default();
        for spawn in self.spawns.get(&graph).cloned().unwrap_or_default() {
            if !self.needed.contains(&spawn.graph) {
                continue;
            }
            let child = self.copy(spawn.graph, (values, own_patches), allocator, tags)?;
            patches.push(WeaponRuntimeResourcePatch {
                binding_hash: spawn.binding_hash,
                resource_index: spawn.resource_index,
                offset: spawn.offset,
                bytes: child.0.to_le_bytes().to_vec(),
                graph_values: Vec::new(),
                graph_removals: Vec::new(),
                graph_trajectories: None,
            });
        }
        let own = values.get(&graph).map_or(&[][..], Vec::as_slice);
        let first = tags.len();
        let copy = append_private_patched_graph(
            self.manager,
            TagHash(graph),
            own,
            &patches,
            allocator,
            tags,
        )?;
        if let Some(bank) = tags[first..]
            .iter()
            .find(|tag| sundial::package_authoring::ability_modifier::is_bank(tag.template_tag.0))
        {
            return Err(invalid(format!(
                "A value changes ability bank {}, which the build keeps stock. Use Parameters.",
                bank.template_tag
            )));
        }
        self.copies.insert(graph, copy);
        Ok(copy)
    }
}

/// Clone the graph with its edited values and raw patches, and every component owner they
/// touch. All stock tags remain intact.
fn append_private_patched_graph(
    manager: &PackageManager,
    source: TagHash,
    values: &[WeaponRuntimeValueOverride],
    patches: &[WeaponRuntimeResourcePatch],
    allocator: AppendedTagAllocator,
    tags: &mut Vec<NewTagSpec>,
) -> AuthoringResult<TagHash> {
    if manager
        .get_entry(source)
        .is_none_or(|entry| entry.reference != WEAPON_ENTITY_CLASS)
    {
        return Err(invalid(format!(
            "Referenced runtime graph {source} is not a live weapon entity graph"
        )));
    }
    let mut graph = read_tag(manager, source, "referenced runtime graph")?;
    validate_weapon_entity(&graph).map_err(invalid)?;
    // Validate the source prerequisites now. Final assembly enrolls the required native
    // records along with every authored graph and action.
    sundial::package_authoring::sandbox_perk::entity::residency::inspect(manager, source.0)
        .map_err(invalid)?;
    let values = values
        .iter()
        .map(|value| {
            Ok(WeaponRuntimeValueOverride {
                locator: value.locator.for_graph(source.0.into()).map_err(invalid)?,
                value: value.value.clone(),
            })
        })
        .collect::<AuthoringResult<Vec<_>>>()?;
    append_patched_runtime_resource_owners(
        manager,
        &mut graph,
        &values,
        patches,
        &[],
        allocator,
        tags,
    )?;
    validate_weapon_entity(&graph).map_err(invalid)?;
    let authored =
        allocator.assigned_tag(tags.len(), "Private referenced graph", "runtime graph")?;
    tags.push(NewTagSpec {
        template_tag: source,
        payload: graph,
        storage: crate::NewTagStorageMode::InheritTemplate,
    });
    Ok(authored)
}

pub(super) fn validate_exact_tag_occurrences(
    payload: &[u8],
    tag: TagHash,
    expected_offsets: &[usize],
    description: &str,
) -> AuthoringResult<()> {
    let actual_offsets = matching_u32_offsets(payload, tag.0);
    if actual_offsets != expected_offsets {
        return Err(invalid(format!(
            "{description} expected tag {tag} exactly at offsets {expected_offsets:?}, found {actual_offsets:?}"
        )));
    }
    Ok(())
}

pub(super) fn retarget_exact_tag_occurrences(
    payload: &mut [u8],
    source_tag: TagHash,
    authored_tag: TagHash,
    expected_offsets: &[usize],
    description: &str,
) -> AuthoringResult<()> {
    validate_exact_tag_occurrences(payload, source_tag, expected_offsets, description)?;
    for &offset in expected_offsets {
        write_u32(payload, offset, authored_tag.0)?;
    }
    if !matching_u32_offsets(payload, source_tag.0).is_empty() {
        return Err(validation(format!(
            "{description} retained source tag {source_tag} after retargeting"
        )));
    }
    Ok(())
}

pub(super) fn read_private_perk_residency_template(
    manager: &PackageManager,
    tag: TagHash,
    expected_file_type: u8,
    expected_reference: u32,
    expected_size: usize,
    description: &str,
) -> AuthoringResult<Vec<u8>> {
    let entry = manager
        .get_entry(tag)
        .ok_or_else(|| invalid(format!("{description} template {tag} is not live")))?;
    if entry.file_type != expected_file_type
        || entry.file_subtype != 0
        || entry.reference != expected_reference
    {
        return Err(invalid(format!(
            "{description} template {tag} has type/reference {:02X}/{:02X}/0x{:08X}; expected {expected_file_type:02X}/00/0x{expected_reference:08X}",
            entry.file_type, entry.file_subtype, entry.reference
        )));
    }
    if entry.file_size as usize != expected_size {
        return Err(invalid(format!(
            "{description} template {tag} declares size 0x{:X}; expected 0x{expected_size:X}",
            entry.file_size
        )));
    }
    let payload = read_tag(manager, tag, description)?;
    if payload.len() != expected_size
        || usize::try_from(read_u64(&payload, 0)?).ok() != Some(payload.len())
    {
        return Err(invalid(format!(
            "{description} template {tag} decoded with a non-stock payload extent"
        )));
    }
    Ok(payload)
}

pub(super) fn build_private_perk_residency_chain(
    manager: &PackageManager,
    runtime_tag_allocator: AppendedTagAllocator,
    first_ordinal: usize,
    authored_action_tag: TagHash,
) -> AuthoringResult<Vec<NewTagSpec>> {
    let b9_ordinal = first_ordinal;
    let ba_ordinal =
        AppendedTagAllocator::checked_ordinal(first_ordinal, 1, "Private perk residency BA")?;
    let root_ordinal =
        AppendedTagAllocator::checked_ordinal(first_ordinal, 2, "Private perk residency root")?;
    let companion_ordinal = AppendedTagAllocator::checked_ordinal(
        first_ordinal,
        3,
        "Private perk residency companion",
    )?;
    let authored_b9_tag = runtime_tag_allocator.assigned_tag(
        b9_ordinal,
        "Private perk residency B9",
        "private perk residency B9",
    )?;
    let authored_ba_tag = runtime_tag_allocator.assigned_tag(
        ba_ordinal,
        "Private perk residency BA",
        "private perk residency BA",
    )?;
    let authored_root_tag = runtime_tag_allocator.assigned_tag(
        root_ordinal,
        "Private perk residency root",
        "private perk residency root",
    )?;
    let authored_companion_tag = runtime_tag_allocator.assigned_tag(
        companion_ordinal,
        "Private perk residency companion",
        "private perk residency companion",
    )?;

    let mut b9 = read_private_perk_residency_template(
        manager,
        PRIVATE_PERK_RESIDENCY_B9_TEMPLATE,
        0x08,
        PRIVATE_PERK_RESIDENCY_B9_CLASS,
        PRIVATE_PERK_RESIDENCY_B9_SIZE,
        "private perk residency B9",
    )?;
    let mut ba = read_private_perk_residency_template(
        manager,
        PRIVATE_PERK_RESIDENCY_BA_TEMPLATE,
        0x08,
        PRIVATE_PERK_RESIDENCY_BA_CLASS,
        PRIVATE_PERK_RESIDENCY_BA_SIZE,
        "private perk residency BA",
    )?;
    let mut root = read_private_perk_residency_template(
        manager,
        PRIVATE_PERK_RESIDENCY_ROOT_TEMPLATE,
        0x10,
        PRIVATE_PERK_RESIDENCY_ROOT_CLASS,
        PRIVATE_PERK_RESIDENCY_ROOT_SIZE,
        "private perk residency root",
    )?;
    let companion_template = read_private_perk_residency_template(
        manager,
        PRIVATE_PERK_RESIDENCY_COMPANION_TEMPLATE,
        0x08,
        crate::format::SHARED_TAG_COMPANION_CLASS,
        PRIVATE_PERK_RESIDENCY_COMPANION_TEMPLATE_SIZE,
        "private perk residency companion",
    )?;

    retarget_exact_tag_occurrences(
        &mut root,
        PRIVATE_PERK_RESIDENCY_BA_TEMPLATE,
        authored_ba_tag,
        &PRIVATE_PERK_RESIDENCY_ROOT_BA_OFFSETS,
        "Private perk residency root-to-BA reference",
    )?;
    validate_exact_tag_occurrences(
        &root,
        PRIVATE_PERK_RESIDENCY_ROOT_TEMPLATE,
        &[],
        "Private perk residency root self-reference",
    )?;
    retarget_exact_tag_occurrences(
        &mut ba,
        PRIVATE_PERK_RESIDENCY_ROOT_TEMPLATE,
        authored_root_tag,
        &PRIVATE_PERK_RESIDENCY_BA_ROOT_OFFSETS,
        "Private perk residency BA-to-root reference",
    )?;
    retarget_exact_tag_occurrences(
        &mut ba,
        PRIVATE_PERK_RESIDENCY_B9_TEMPLATE,
        authored_b9_tag,
        &PRIVATE_PERK_RESIDENCY_BA_B9_OFFSETS,
        "Private perk residency BA-to-B9 references",
    )?;
    validate_exact_tag_occurrences(
        &ba,
        PRIVATE_PERK_RESIDENCY_BA_TEMPLATE,
        &[],
        "Private perk residency BA self-reference",
    )?;
    retarget_exact_tag_occurrences(
        &mut b9,
        PRIVATE_PERK_RESIDENCY_B9_TEMPLATE,
        authored_b9_tag,
        &PRIVATE_PERK_RESIDENCY_B9_SELF_OFFSETS,
        "Private perk residency B9 self-references",
    )?;
    retarget_exact_tag_occurrences(
        &mut b9,
        PRIVATE_PERK_RESIDENCY_DONOR_ACTION_TAG,
        authored_action_tag,
        &PRIVATE_PERK_RESIDENCY_B9_ACTION_OFFSETS,
        "Private perk residency B9-to-action reference",
    )?;

    validate_exact_tag_occurrences(
        &companion_template,
        PRIVATE_PERK_RESIDENCY_COMPANION_TEMPLATE,
        &PRIVATE_PERK_RESIDENCY_COMPANION_SELF_OFFSETS,
        "Private perk residency companion self-reference",
    )?;
    validate_exact_tag_occurrences(
        &companion_template,
        PRIVATE_PERK_RESIDENCY_ROOT_TEMPLATE,
        &PRIVATE_PERK_RESIDENCY_COMPANION_OWNER_OFFSETS,
        "Private perk residency companion owner reference",
    )?;
    let stock_common_dependencies = [
        TagHash::new(0x0238, 0x0B90),
        TagHash::new(0x0238, 0x0EDC),
        TagHash::new(0x0238, 0x0EDD),
    ];
    let stock_dependencies = dependency_set(
        [
            PRIVATE_PERK_RESIDENCY_ROOT_TEMPLATE,
            PRIVATE_PERK_RESIDENCY_COMPANION_TEMPLATE,
        ]
        .into_iter()
        .chain(stock_common_dependencies),
    );
    let parsed_stock_dependencies = validate_shared_tag_companion_payload(
        &companion_template,
        PRIVATE_PERK_RESIDENCY_COMPANION_TEMPLATE,
        PRIVATE_PERK_RESIDENCY_ROOT_TEMPLATE,
    )?;
    if parsed_stock_dependencies != stock_dependencies {
        return Err(invalid(format!(
            "Private perk residency companion dependency closure changed: expected {stock_dependencies:?}, found {parsed_stock_dependencies:?}"
        )));
    }
    let authored_dependencies = dependency_set(
        [authored_root_tag, authored_companion_tag]
            .into_iter()
            .chain(stock_common_dependencies),
    );
    let companion = build_shared_tag_companion_payload(
        &companion_template,
        authored_companion_tag,
        authored_root_tag,
        &authored_dependencies,
    )?;
    // The canonical writer validates the envelope and exact dependency closure.
    // Package order changes the last sparse list and therefore the encoded length.
    // A size captured from one allocation cannot validate another package.
    validate_exact_tag_occurrences(
        &companion,
        authored_companion_tag,
        &PRIVATE_PERK_RESIDENCY_COMPANION_SELF_OFFSETS,
        "Authored private perk residency companion self-reference",
    )?;
    validate_exact_tag_occurrences(
        &companion,
        authored_root_tag,
        &PRIVATE_PERK_RESIDENCY_COMPANION_OWNER_OFFSETS,
        "Authored private perk residency companion owner reference",
    )?;

    Ok(vec![
        NewTagSpec {
            template_tag: PRIVATE_PERK_RESIDENCY_B9_TEMPLATE,
            payload: b9,
            storage: crate::NewTagStorageMode::InheritTemplate,
        },
        NewTagSpec {
            template_tag: PRIVATE_PERK_RESIDENCY_BA_TEMPLATE,
            payload: ba,
            storage: crate::NewTagStorageMode::InheritTemplate,
        },
        NewTagSpec {
            template_tag: PRIVATE_PERK_RESIDENCY_ROOT_TEMPLATE,
            payload: root,
            storage: crate::NewTagStorageMode::InheritTemplate,
        },
        NewTagSpec {
            template_tag: PRIVATE_PERK_RESIDENCY_COMPANION_TEMPLATE,
            payload: companion,
            storage: crate::NewTagStorageMode::InheritTemplate,
        },
    ])
}

#[derive(Default)]
pub(super) struct PrivateRuntimeEdits<'a> {
    pub program: Option<&'a sundial::package_authoring::sandbox_perk::program::Program>,
    pub values: &'a [WeaponRuntimeValueOverride],
    pub action_float_values: &'a [WeaponSandboxPerkActionFloatOverride],
    pub projectiles: &'a [sundial::package_authoring::sandbox_perk::entity::Selection],
    pub activation: Option<sundial::package_authoring::sandbox_perk::activation::PerkActivation>,
    /// The tags of the imported particle nodes of the weapon whose plug carries the program.
    pub particle_symbols: Option<&'a BTreeMap<String, TagHash>>,
}

/// Program asset patches with every imported particle symbol replaced by the tag the weapon's
/// particle nodes were allocated, checked against the four-byte width the recipe reserved.
fn resolve_program_patches(
    patches: &[sundial::package_authoring::sandbox_perk::program::NativeAssetResourcePatch],
    symbols: Option<&BTreeMap<String, TagHash>>,
) -> AuthoringResult<Vec<sundial::package_authoring::sandbox_perk::program::NativeAssetResourcePatch>>
{
    patches
        .iter()
        .map(|patch| {
            let Some(symbol) = &patch.imported_particle else {
                return Ok(patch.clone());
            };
            if patch.bytes.len() != 4 || patch.expected.len() != 4 {
                return Err(invalid(format!(
                    "Imported particle patch {symbol} does not replace one tag"
                )));
            }
            let tag = symbols
                .and_then(|symbols| symbols.get(symbol))
                .ok_or_else(|| {
                    invalid(format!(
                        "A program patch names imported particle {symbol}, which this weapon's imported particles lack"
                    ))
                })?;
            let mut resolved = patch.clone();
            resolved.bytes = tag.0.to_le_bytes().to_vec();
            resolved.imported_particle = None;
            Ok(resolved)
        })
        .collect()
}

/// Whether a program names imported particles, so its weapon's particle tags must be known
/// when the program compiles.
pub(in crate::item) fn program_names_imported_particles(
    program: &sundial::package_authoring::sandbox_perk::program::Program,
) -> bool {
    program
        .native_asset_patches
        .iter()
        .flat_map(|edit| &edit.patches)
        .any(|patch| patch.imported_particle.is_some())
}

pub(super) fn clone_private_sandbox_perk_runtime(
    manager: &PackageManager,
    runtime_action: &SandboxPerkRuntimeAction,
    edits: PrivateRuntimeEdits<'_>,
    runtime_tag_allocator: AppendedTagAllocator,
    runtime_new_tags: &mut Vec<NewTagSpec>,
) -> AuthoringResult<TagHash> {
    let PrivateRuntimeEdits {
        program,
        values,
        action_float_values,
        projectiles,
        activation,
        particle_symbols,
    } = edits;
    if let Some(program) = program {
        if !values.is_empty()
            || !action_float_values.is_empty()
            || !projectiles.is_empty()
            || activation.is_some()
        {
            return Err(invalid(
                "A custom effect program cannot also carry stock action overrides.",
            ));
        }
        return append_private_program_runtime(
            manager,
            runtime_action.action_tag,
            program,
            particle_symbols,
            runtime_tag_allocator,
            runtime_new_tags,
        );
    }
    let source_runtime_tag = runtime_action.action_tag;
    if values.is_empty()
        && action_float_values.is_empty()
        && activation.is_none()
        && projectiles.is_empty()
    {
        return Ok(source_runtime_tag);
    }
    let mut action = if let Some(activation) = activation {
        sundial::package_authoring::sandbox_perk::activation::with_activation(
            source_runtime_tag.0,
            &runtime_action.action_payload,
            activation,
            &read_tag(
                manager,
                TagHash(sundial::package_authoring::sandbox_perk::activation::LABEL_GLOBALS),
                "activation label registry",
            )?,
        )
        .map_err(invalid)?
    } else {
        runtime_action.action_payload.clone()
    };
    for (value_index, value) in action_float_values.iter().enumerate() {
        let offset = sandbox_perk_action_boxed_value_offset(
            &action,
            value.node_type_handle,
            value.node_occurrence,
            value.value_pointer_offset,
            value.value_type_handle,
            size_of::<f32>(),
        )
        .map_err(|error| {
            invalid(format!(
                "Sandbox-perk action float {value_index} does not resolve in action {source_runtime_tag}: {error}"
            ))
        })?;
        let actual = read_u32(&action, offset)?;
        if actual != value.expected_bits {
            return Err(invalid(format!(
                "Sandbox-perk action float {value_index} expected {} (0x{:08X}) at resolved offset 0x{offset:X}, found {} (0x{actual:08X})",
                f32::from_bits(value.expected_bits),
                value.expected_bits,
                f32::from_bits(actual),
            )));
        }
        write_u32(&mut action, offset, value.value_bits)?;
    }
    let graphs = sundial::package_authoring::sandbox_perk::entity::resolve(
        manager,
        runtime_action,
        projectiles,
    )
    .map_err(invalid)?;
    if graphs.is_empty() && !values.is_empty() {
        return Err(invalid(format!(
            "Sandbox-perk runtime action {source_runtime_tag} has no directly referenced runtime graphs"
        )));
    }

    let mut values_by_graph = vec![Vec::<WeaponRuntimeValueOverride>::new(); graphs.len()];
    for (value_index, value) in values.iter().enumerate() {
        let matching_graphs = graphs
            .iter()
            .enumerate()
            .filter_map(|(index, graph)| {
                value
                    .locator
                    .for_graph(graph.tag.0.into())
                    .and_then(|locator| {
                        resolve_weapon_runtime_field(manager, &graph.payload, &locator)
                    })
                    .is_ok()
                    .then_some(index)
            })
            .collect::<Vec<_>>();
        let [graph_index] = matching_graphs.as_slice() else {
            return Err(invalid(if matching_graphs.is_empty() {
                format!(
                    "Sandbox-perk runtime value {value_index} does not resolve in any graph referenced by action {source_runtime_tag}"
                )
            } else {
                format!(
                    "Sandbox-perk runtime value {value_index} ambiguously resolves in {} graphs referenced by action {source_runtime_tag}",
                    matching_graphs.len()
                )
            }));
        };
        values_by_graph[*graph_index].push(WeaponRuntimeValueOverride {
            locator: value
                .locator
                .for_graph((graphs[*graph_index].tag.0).into())
                .map_err(invalid)?,
            value: value.value.clone(),
        });
    }

    for ((source, graph), graph_values) in runtime_action
        .graphs
        .iter()
        .zip(&graphs)
        .zip(values_by_graph)
    {
        // A different graph's scalar edits must not waive this graph's enrollment check.
        let cloned = !graph_values.is_empty() || source.tag != graph.tag;
        sundial::package_authoring::sandbox_perk::entity::residency::inspect(manager, graph.tag.0)
            .map_err(invalid)?;
        if !cloned {
            continue;
        }
        let mut authored_graph = graph.payload.clone();
        append_patched_runtime_resource_owners(
            manager,
            &mut authored_graph,
            &graph_values,
            &[],
            &[],
            runtime_tag_allocator,
            runtime_new_tags,
        )?;
        let authored_graph_tag = runtime_tag_allocator.assigned_tag(
            runtime_new_tags.len(),
            "Private sandbox-perk graph",
            "sandbox-perk graph",
        )?;
        for &offset in &graph.action_offsets {
            if read_u32(&action, offset)? != source.tag.0 {
                return Err(validation(
                    "Sandbox-perk runtime action graph reference moved while authoring",
                ));
            }
            write_u32(&mut action, offset, authored_graph_tag.0)?;
        }
        runtime_new_tags.push(NewTagSpec {
            template_tag: graph.tag,
            payload: authored_graph,
            storage: crate::NewTagStorageMode::InheritTemplate,
        });
    }

    append_private_action(
        manager,
        source_runtime_tag,
        action,
        runtime_tag_allocator,
        runtime_new_tags,
    )
}

/// Appends `action` as a private copy of the stock action `source`, followed by the residency
/// chain that loads it. Returns the copy's tag.
pub(super) fn append_private_action(
    manager: &PackageManager,
    source: TagHash,
    action: Vec<u8>,
    runtime_tag_allocator: AppendedTagAllocator,
    runtime_new_tags: &mut Vec<NewTagSpec>,
) -> AuthoringResult<TagHash> {
    let authored_action_tag = runtime_tag_allocator.assigned_tag(
        runtime_new_tags.len(),
        "Private sandbox-perk action",
        "sandbox-perk action",
    )?;
    let action = synchronize_payload_size(action)?;
    let first_residency_ordinal = AppendedTagAllocator::checked_ordinal(
        runtime_new_tags.len(),
        1,
        "Private sandbox-perk residency chain",
    )?;
    let residency_chain = build_private_perk_residency_chain(
        manager,
        runtime_tag_allocator,
        first_residency_ordinal,
        authored_action_tag,
    )?;
    runtime_new_tags.push(NewTagSpec {
        template_tag: source,
        payload: action,
        storage: crate::NewTagStorageMode::InheritTemplate,
    });
    runtime_new_tags.extend(residency_chain);
    Ok(authored_action_tag)
}

/// Clones a stock graph privately with checked raw patches and definition appends, and every
/// component owner they touch. Each patch must find its expected stock bytes, and each append
/// its expected owner size, so a different package version is refused rather than edited.
#[allow(clippy::too_many_arguments)]
fn append_private_asset_graph(
    manager: &PackageManager,
    source: TagHash,
    edit_patches: &[sundial::package_authoring::sandbox_perk::program::NativeAssetResourcePatch],
    edit_appends: &[sundial::package_authoring::sandbox_perk::program::NativeAssetResourceAppend],
    removals: &BTreeSet<u32>,
    label: &str,
    allocator: AppendedTagAllocator,
    tags: &mut Vec<NewTagSpec>,
) -> AuthoringResult<TagHash> {
    let graph = read_tag(manager, source, "native asset graph")?;
    let mut appends = Vec::with_capacity(edit_appends.len());
    let mut appended_owners = BTreeSet::new();
    for (append_index, append) in edit_appends.iter().enumerate() {
        let bindings = weapon_component_bindings(&graph, append.binding_hash).map_err(invalid)?;
        let binding = bindings
            .get(usize::from(append.resource_index))
            .ok_or_else(|| invalid("An asset append selects a missing component resource"))?;
        let owner = read_tag(manager, TagHash(binding.owner_tag), "native asset owner")?;
        if owner.len() != append.expected_owner_size as usize
            || !appended_owners.insert(binding.owner_tag)
        {
            return Err(invalid(format!(
                "{label} append {append_index} has a stale owner size or an aliased owner"
            )));
        }
        appends.push(WeaponRuntimeResourceAppend {
            binding_hash: append.binding_hash,
            resource_index: append.resource_index,
            bytes: append.bytes.clone(),
            slots: Vec::new(),
            arrays: Vec::new(),
        });
    }
    let mut patches = Vec::with_capacity(edit_patches.len());
    for (patch_index, patch) in edit_patches.iter().enumerate() {
        let bindings = weapon_component_bindings(&graph, patch.binding_hash).map_err(invalid)?;
        let binding = bindings
            .get(usize::from(patch.resource_index))
            .ok_or_else(|| {
                invalid(format!(
                    "{label} patch {patch_index} selects missing component resource"
                ))
            })?;
        let owner = read_tag(manager, TagHash(binding.owner_tag), "native asset owner")?;
        let start = usize::try_from(binding.resource_offset)
            .ok()
            .and_then(|offset| offset.checked_add(patch.offset as usize))
            .ok_or_else(|| invalid("Native asset patch resource offset overflows"))?;
        let end = start
            .checked_add(patch.expected.len())
            .ok_or_else(|| invalid("Native asset patch length overflows"))?;
        if appended_owners.contains(&binding.owner_tag) {
            let definition = crate::tag_payload::relative_target(&owner, 24)?;
            if start < definition {
                return Err(invalid(
                    "An asset append can patch serialized definitions only",
                ));
            }
        }
        let expected = owner.get(start..end).ok_or_else(|| {
            invalid(format!(
                "{label} patch {patch_index} is outside its stock owner"
            ))
        })?;
        if expected != patch.expected.as_slice() {
            return Err(invalid(format!(
                "{label} patch {patch_index} has stale stock owner bytes"
            )));
        }
        patches.push(WeaponRuntimeResourcePatch {
            binding_hash: patch.binding_hash,
            resource_index: patch.resource_index,
            offset: patch.offset,
            bytes: patch.bytes.clone(),
            graph_values: Vec::new(),
            graph_removals: Vec::new(),
            graph_trajectories: None,
        });
    }
    // Appends extend serialized definitions only. The graph still uses its
    // native runtime allocation and interface topology.
    let mut authored_graph = graph;
    sundial::package_authoring::sandbox_perk::entity::residency::inspect(manager, source.0)
        .map_err(invalid)?;
    append_patched_runtime_resource_owners(
        manager,
        &mut authored_graph,
        &[],
        &patches,
        &appends,
        allocator,
        tags,
    )?;
    if !removals.is_empty() {
        sundial::package_authoring::entity::remove_weapon_components(&mut authored_graph, removals)
            .map_err(|error| invalid(format!("{label}: {error}")))?;
    }
    let private = allocator.assigned_tag(tags.len(), "Private asset graph", "runtime graph")?;
    tags.push(NewTagSpec {
        template_tag: source,
        payload: authored_graph,
        storage: crate::NewTagStorageMode::InheritTemplate,
    });
    Ok(private)
}

fn append_private_program_runtime(
    manager: &PackageManager,
    action_template: TagHash,
    program: &sundial::package_authoring::sandbox_perk::program::Program,
    particle_symbols: Option<&BTreeMap<String, TagHash>>,
    allocator: AppendedTagAllocator,
    tags: &mut Vec<NewTagSpec>,
) -> AuthoringResult<TagHash> {
    let mut compiled = sundial::package_authoring::sandbox_perk::program::compile(manager, program)
        .map_err(invalid)?;
    for (index, offsets) in &compiled.asset_offsets {
        let asset = program
            .asset(*index)
            .ok_or_else(|| invalid("A compiled asset has no authored component settings."))?;
        if asset.values.is_empty() {
            sundial::package_authoring::sandbox_perk::entity::residency::inspect(
                manager,
                asset.graph,
            )
            .map_err(invalid)?;
            continue;
        }
        // A HUD status of the project's own joins the copy the settings make.
        let graph = append_private_patched_graph(
            manager,
            TagHash(asset.graph),
            &asset.values,
            &super::hud_status::value_copy_patches(manager, asset)?,
            allocator,
            tags,
        )?;
        for &offset in offsets {
            write_u32(&mut compiled.payload, offset, graph.0)?;
        }
    }
    // A HUD status of the project's own on an asset without settings joins its action's asset
    // edits.
    let edits = super::hud_status::asset_edits(manager, program)?;
    for (edit_index, edit) in edits.iter().enumerate() {
        let offset = compiled
            .graph_offsets
            .get(edit.action_index)
            .copied()
            .flatten()
            .ok_or_else(|| {
                invalid(format!(
                    "Native asset edit {edit_index} has no compiled graph reference"
                ))
            })?;
        if read_u32(&compiled.payload, offset)? != edit.source_graph {
            return Err(invalid(format!(
                "Native asset edit {edit_index} changed its source graph during compilation"
            )));
        }
        let patches = resolve_program_patches(&edit.patches, particle_symbols)?;
        let private = append_private_asset_graph(
            manager,
            TagHash(edit.source_graph),
            &patches,
            &edit.appends,
            &edit.remove_owners.iter().copied().collect(),
            &format!("Native asset edit {edit_index}"),
            allocator,
            tags,
        )?;
        write_u32(&mut compiled.payload, offset, private.0)?;
    }
    let action =
        allocator.assigned_tag(tags.len(), "Custom effect action", "custom effect action")?;
    rebind_program_callbacks(&mut compiled.payload, action)?;
    let residency = build_private_perk_residency_chain(
        manager,
        allocator,
        AppendedTagAllocator::checked_ordinal(tags.len(), 1, "Custom effect residency")?,
        action,
    )?;
    tags.push(NewTagSpec {
        template_tag: action_template,
        payload: compiled.payload,
        storage: crate::NewTagStorageMode::InheritTemplate,
    });
    tags.extend(residency);
    Ok(action)
}

/// The native callback must resolve settings in this authored action, after the
/// program emitter has moved nodes and the allocator has chosen the owner tag.
fn rebind_program_callbacks(payload: &mut [u8], owner: TagHash) -> AuthoringResult<()> {
    let references = sundial::package_authoring::sandbox_perk::action::self_references(payload)
        .map_err(invalid)?;
    for reference in references {
        write_u32(payload, reference.reference_offset, owner.0)?;
        let at = reference.reference_offset + 8;
        payload[at..at + 8].copy_from_slice(&(reference.target_offset as u64).to_le_bytes());
    }
    Ok(())
}

fn validate_projectile_values(
    manager: &PackageManager,
    entity: &[u8],
    values: &[WeaponRuntimeValueOverride],
) -> AuthoringResult<()> {
    if values
        .iter()
        .any(|value| matches!(value.locator.root_schema.get(), 0x8080_3B73 | 0x8080_388F))
        && sundial::package_authoring::sandbox_perk::entity::kind(entity)
            .map_err(invalid)?
            .is_some()
    {
        let graph = sundial::package_authoring::runtime::load_weapon_runtime_graph_for_entity(
            manager, 0, 0, 0, entity,
        )
        .map_err(invalid)?;
        for parameter in
            sundial::package_authoring::sandbox_perk::entity::projectile::parameters::discover(
                &graph,
            )
        {
            parameter.value(values).map_err(invalid)?;
        }
    }
    Ok(())
}

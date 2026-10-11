//! Native custom runtime operations with independent validation.
use super::*;
mod actions;
#[cfg(feature = "d2-model-importer")]
pub(in crate::item) mod animation;
#[cfg(feature = "d2-model-importer")]
pub(in crate::item) mod audio;
mod banks;
mod barrel;
pub(crate) use barrel::{BarrelDefaults, barrel_defaults};
#[cfg(feature = "d2-model-importer")]
pub(in crate::item) mod crosshair;
pub(in crate::item) mod damage;
#[cfg(feature = "d2-model-importer")]
pub(in crate::item) mod extensions;
mod firing;
mod graph_tree;
#[cfg(feature = "d2-model-importer")]
pub(in crate::item) mod imports;
mod modifier_rows;
pub(in crate::item) mod palettes;
#[cfg(feature = "d2-model-importer")]
pub(in crate::item) mod particles;
mod payload;
mod private_perk;
mod residency;
mod swaps;
mod sword_profiles;
#[cfg(test)]
mod tests;
pub(super) use banks::{attached_glyph_patches, bank_glyph_patches, bank_value_patches};
use graph_tree::*;
pub(super) use graph_tree::{append_private_graph_tree, split_swapped_values};
use private_perk::*;
pub(super) use private_perk::{
    PrivateRuntimeEdits, append_private_action, clone_private_sandbox_perk_runtime,
    program_names_imported_particles,
};
pub(super) use residency::build_private_perk_residency_chain;
pub(super) use swaps::swap_patches;
use swaps::*;

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
    splices: &[(u32, Vec<u8>)],
) -> AuthoringResult<()> {
    let start = manager
        .lookup
        .tag32_entries_by_pkg
        .get(&HOST_PACKAGE_ID)
        .ok_or_else(|| invalid("The runtime host package is unavailable"))?
        .len();
    let allocator = AppendedTagAllocator::new(HOST_PACKAGE_ID, start);
    let projectile = if let Some(fired) = &overrides.fired_graph {
        fired.validate()?;
        if let Some(symbol) = &fired.imported {
            #[cfg(feature = "d2-model-importer")]
            {
                let graph = overrides
                    .imported_graph
                    .as_ref()
                    .ok_or_else(|| invalid("Imported projectile has no model graph"))?;
                let inputs = imports::Inputs::open(graph)?;
                let assets = particles::load(&inputs)?
                    .ok_or_else(|| invalid("Imported projectile has no assets"))?;
                assets.projectile(symbol)?;
                // Preflight only mutates an in-memory weapon. The build allocates this root
                // together with its complete asset group before performing the same edit.
                Some(allocator.assigned_tag(0, "Imported projectile", "preflight")?)
            }
            #[cfg(not(feature = "d2-model-importer"))]
            return Err(invalid(format!(
                "Imported projectile {symbol} requires model import support"
            )));
        } else {
            None
        }
    } else {
        None
    };
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
            splices,
            actions: &[],
            markers: &[],
            projectile,
        },
        allocator,
        &mut Vec::new(),
    )
}

/// The class word every native array carries in the four bytes before its header, which the
/// readers of a typed pointer check before reading the array's count and element class.
const ARRAY_MARKER: u32 = 0x8080_9FBD;

/// The writes the recipe's component splices make, as the build makes them, for an editor that
/// must see what a spliced component brings.
pub(crate) fn splice_writes(
    manager: &PackageManager,
    entity: &[u8],
    splices: &[(u32, Vec<u8>)],
) -> AuthoringResult<Vec<WeaponRuntimeResourcePatch>> {
    component_splice_edits(manager, entity, splices, &mut Vec::new())
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
                pointers: Vec::new(),
                references: Vec::new(),
            });
        }
        for record in plan.records {
            appends.push(WeaponRuntimeResourceAppend {
                binding_hash: *binding,
                resource_index: 0,
                bytes: record.bytes,
                slots: Vec::new(),
                arrays: Vec::new(),
                pointers: record
                    .pointers
                    .into_iter()
                    .map(|(at, target)| relative(at).map(|at| (at, target)))
                    .collect::<AuthoringResult<_>>()?,
                references: record.references,
            });
        }
    }
    Ok(patches)
}

/// The parts of a copied-value write that no other patch of the same component covers.
fn trim(
    splice: WeaponRuntimeResourcePatch,
    resource: usize,
    others: &[std::ops::Range<usize>],
) -> Vec<WeaponRuntimeResourcePatch> {
    let start = resource + splice.offset as usize;
    let mut kept = vec![true; splice.bytes.len()];
    for other in others {
        for (index, keep) in kept.iter_mut().enumerate() {
            if other.contains(&(start + index)) {
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
            offset: splice.offset + index as u32,
            bytes: splice.bytes[index..index + run].to_vec(),
            ..splice.clone()
        });
        index += run;
    }
    pieces
}

/// Spliced defaults give way to final technical and typed edits, including aliases of the same
/// owner. A structural pointer is either wholly overridden or wholly relocated.
fn trim_splices(
    manager: &PackageManager,
    entity: &[u8],
    values: &[WeaponRuntimeValueOverride],
    patches: &[WeaponRuntimeResourcePatch],
    spliced: Vec<WeaponRuntimeResourcePatch>,
    appends: &mut Vec<WeaponRuntimeResourceAppend>,
) -> AuthoringResult<Vec<WeaponRuntimeResourcePatch>> {
    if spliced.is_empty() && appends.is_empty() {
        return Ok(spliced);
    }
    let mut ranges = BTreeMap::<u32, Vec<std::ops::Range<usize>>>::new();
    let binding = |hash, index| {
        weapon_component_bindings(entity, hash)
            .map_err(invalid)?
            .get(usize::from(index))
            .copied()
            .ok_or_else(|| invalid("A spliced component override has no selected resource"))
    };
    for value in values {
        let field =
            resolve_weapon_runtime_field(manager, entity, &value.locator).map_err(invalid)?;
        let end = field
            .owner_offset
            .checked_add(value.locator.byte_size as usize)
            .ok_or_else(|| invalid("A typed component override overflows its owner"))?;
        ranges
            .entry(field.owner_tag)
            .or_default()
            .push(field.owner_offset..end);
    }
    for patch in patches {
        let selected = binding(patch.binding_hash, patch.resource_index)?;
        let start = usize::try_from(selected.resource_offset)
            .ok()
            .and_then(|start| start.checked_add(patch.offset as usize))
            .ok_or_else(|| invalid("A technical component override offset overflows"))?;
        let end = start
            .checked_add(patch.bytes.len())
            .ok_or_else(|| invalid("A technical component override range overflows"))?;
        ranges
            .entry(selected.owner_tag)
            .or_default()
            .push(start..end);
    }
    let mut trimmed = Vec::new();
    for patch in spliced {
        let selected = binding(patch.binding_hash, patch.resource_index)?;
        trimmed.extend(trim(
            patch,
            selected.resource_offset as usize,
            ranges
                .get(&selected.owner_tag)
                .map_or(&[][..], Vec::as_slice),
        ));
    }
    for append in appends.iter_mut() {
        let selected = binding(append.binding_hash, append.resource_index)?;
        let edited = ranges
            .get(&selected.owner_tag)
            .map_or(&[][..], Vec::as_slice);
        let keep = |relative: u32, width: usize| -> AuthoringResult<bool> {
            let start = selected.resource_offset as usize + relative as usize;
            let covered = (start..start + width)
                .filter(|byte| edited.iter().any(|range| range.contains(byte)))
                .count();
            if covered != 0 && covered != width {
                return Err(invalid(
                    "A component edit partially overrides a spliced native pointer or array descriptor",
                ));
            }
            Ok(covered == 0)
        };
        let mut pointers = Vec::new();
        for pointer in &append.pointers {
            if keep(pointer.0, 8)? {
                pointers.push(*pointer);
            }
        }
        append.pointers = pointers;
        let mut arrays = Vec::new();
        for array in &append.arrays {
            if keep(array.0, 16)? {
                arrays.push(*array);
            }
        }
        append.arrays = arrays;
    }
    appends.retain(|append| !append.pointers.is_empty() || !append.arrays.is_empty());
    Ok(trimmed)
}

pub(super) struct Sources<'a> {
    pub groups: crate::weapon::behavior::ContentGroups,
    pub splices: &'a [(u32, Vec<u8>)],
    pub actions: &'a [(
        crate::recipe::AnimationAction,
        crate::weapon::animations::Profile,
    )],
    /// The moved rig's marker set, grown by the base's own markers.
    pub markers: &'a [WeaponRuntimeResourceAppend],
    /// A validated projectile root allocated with the imported asset group.
    pub projectile: Option<TagHash>,
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
        markers,
        projectile,
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
    appends.extend_from_slice(markers);
    patches.extend(grafts.patches);
    if let Some(fired) = &overrides.fired_graph {
        fired.validate()?;
        if !overrides.additional_behaviors.is_empty() {
            return Err(invalid(
                "A fired graph replaces the weapon's firing graph, so it cannot be combined with an additional behavior.",
            ));
        }
        let private = if fired.imported.is_some() {
            projectile.ok_or_else(|| invalid("Imported projectile has not been allocated"))?
        } else {
            let source = TagHash(fired.source_graph);
            let payload = read_tag(manager, source, "fired graph")?;
            if sundial::package_authoring::sandbox_perk::entity::kind(&payload).map_err(invalid)?
                != Some(sundial::package_authoring::sandbox_perk::entity::Kind::Projectile)
            {
                return Err(invalid(format!("Fired graph {source} is not a projectile")));
            }
            append_private_asset_graph(
                manager,
                source,
                &fired.patches,
                &fired.appends,
                &BTreeSet::new(),
                "Fired graph",
                allocator,
                tags,
            )?
        };
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
    if let Some(projectile) = &overrides.projectile {
        if overrides.fired_graph.is_some() {
            return Err(invalid(
                "An imported weapon fires a projectile of its own, so it cannot take Projectile values.",
            ));
        }
        projectile_patches(
            manager,
            entity,
            projectile,
            (&mut patches, &spliced),
            allocator,
            tags,
        )?;
    }
    if let Some(ammo) = overrides.ammo_type {
        patches.extend(crate::weapon::ammo::patches(manager, entity, ammo)?);
    }
    // A component's copied values give way to any edit the recipe makes in the same component.
    let spliced = trim_splices(
        manager,
        entity,
        &overrides.runtime_values,
        &patches,
        spliced,
        &mut splice_appends,
    )?;
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
    barrel::apply(manager, entity, overrides.barrel.as_ref(), allocator, tags)?;
    firing::fit(
        manager,
        entity,
        (projectile, groups.selected),
        allocator,
        tags,
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

/// Encodes each typed runtime value as a byte patch on the owner that holds it.
fn resolve_value_patches(
    manager: &PackageManager,
    entity: &[u8],
    values: &[WeaponRuntimeValueOverride],
    patches_by_owner: &mut BTreeMap<u32, Vec<ResolvedRuntimeResourcePatch>>,
) -> AuthoringResult<()> {
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
    Ok(())
}

/// Gives each appended record's owner an entry, so an owner that is only appended to is written.
fn include_append_owners(
    entity: &[u8],
    appends: &[WeaponRuntimeResourceAppend],
    patches_by_owner: &mut BTreeMap<u32, Vec<ResolvedRuntimeResourcePatch>>,
) -> AuthoringResult<()> {
    for append in appends {
        let bindings = weapon_component_bindings(entity, append.binding_hash).map_err(invalid)?;
        if let Some(binding) = bindings.get(usize::from(append.resource_index)) {
            patches_by_owner.entry(binding.owner_tag).or_default();
        }
    }
    Ok(())
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
    sundial::package_authoring::ability_settings::validate_values(manager, entity, values)
        .map_err(invalid)?;
    let mut patches_by_owner = BTreeMap::<u32, Vec<ResolvedRuntimeResourcePatch>>::new();
    // One private clone per stock graph and the edits it carries.
    let mut graph_clones = Vec::<(u32, GraphEdit, TagHash)>::new();
    resolve_value_patches(manager, entity, values, &mut patches_by_owner)?;
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
    include_append_owners(entity, appends, &mut patches_by_owner)?;

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
                // A native array's header sits on a 16-byte boundary with its typed-array marker
                // in the four bytes before it, as every stock array is laid out, so the padding
                // leaves room for the marker.
                owner_payload.resize((owner_payload.len() + 8).next_multiple_of(16), 0);
            } else if !append.pointers.is_empty() || !append.references.is_empty() {
                owner_payload.resize(owner_payload.len().next_multiple_of(16), 0);
            }
            let at = owner_payload.len();
            owner_payload.extend_from_slice(&append.bytes);
            for &(word, target) in &append.references {
                let word = word
                    .checked_add(8)
                    .filter(|end| *end <= append.bytes.len())
                    .map(|end| at + end - 8)
                    .ok_or_else(|| invalid("Appended reference is outside the added bytes"))?;
                let target = at
                    .checked_add(target)
                    .filter(|target| *target < owner_payload.len())
                    .ok_or_else(|| {
                        invalid("Appended reference target is outside the added bytes")
                    })?;
                owner_payload[word..word + 8].copy_from_slice(&(target as u64).to_le_bytes());
            }
            for &(slot, target) in &append.pointers {
                let slot = resource
                    .checked_add(slot as usize)
                    .filter(|slot| {
                        slot.checked_add(8)
                            .is_some_and(|end| end <= original_owner_len)
                    })
                    .ok_or_else(|| {
                        invalid("Appended pointer slot is outside the original owner")
                    })?;
                let target = at
                    .checked_add(target)
                    .filter(|target| *target < owner_payload.len())
                    .ok_or_else(|| invalid("Appended pointer target is outside the added bytes"))?;
                let relative = i64::try_from(target)
                    .and_then(|target| i64::try_from(slot).map(|slot| target - slot))
                    .map_err(|_| invalid("Appended pointer overflows"))?;
                grown.push(ResolvedRuntimeResourcePatch {
                    label: format!("appended pointer at 0x{slot:X}"),
                    binding_hash: append.binding_hash,
                    resource_index: usize::from(append.resource_index),
                    start: slot,
                    end: slot + 8,
                    bytes: relative.to_le_bytes().to_vec(),
                });
            }
            for (descriptor, header, count) in &append.arrays {
                let descriptor = resource
                    .checked_add(usize::try_from(*descriptor).unwrap_or(usize::MAX))
                    .ok_or_else(|| invalid("Appended array descriptor offset overflows"))?;
                let header = at
                    .checked_add(*header)
                    .filter(|header| header + 16 <= owner_payload.len())
                    .ok_or_else(|| invalid("Appended array header is outside the added bytes"))?;
                let marker = header
                    .checked_sub(4)
                    .filter(|marker| *marker >= original_owner_len)
                    .ok_or_else(|| invalid("Appended array header has no room for its marker"))?;
                owner_payload[marker..marker + 4].copy_from_slice(&ARRAY_MARKER.to_le_bytes());
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
        // A wider edit sorts before the edits inside it.
        owner_patches.sort_by_key(|patch| {
            (
                patch.start,
                std::cmp::Reverse(patch.end),
                patch.label.clone(),
            )
        });
        let owner_patches = nest_runtime_patches(owner_patches, owner_tag)?;

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

/// Writes each edit that lies wholly inside a wider one over the wider one's bytes, so the
/// narrower and more specific edit wins: a movement value rewrites a whole opaque field, and a
/// named field inside that field can be set on its own. `patches` are sorted by start, wider
/// first. Edits that only partly overlap, or cover one range with different bytes, are refused.
fn nest_runtime_patches(
    patches: Vec<ResolvedRuntimeResourcePatch>,
    owner_tag: TagHash,
) -> AuthoringResult<Vec<ResolvedRuntimeResourcePatch>> {
    let mut nested = Vec::<ResolvedRuntimeResourcePatch>::with_capacity(patches.len());
    for patch in patches {
        let Some(outer) = nested.last_mut().filter(|outer| patch.start < outer.end) else {
            nested.push(patch);
            continue;
        };
        let same_range = (patch.start, patch.end) == (outer.start, outer.end);
        if patch.end > outer.end || (same_range && patch.bytes != outer.bytes) {
            return Err(invalid(format!(
                "Runtime edits {} and {} overlap inside component owner {owner_tag}",
                outer.label, patch.label
            )));
        }
        outer.bytes[patch.start - outer.start..patch.end - outer.start]
            .copy_from_slice(&patch.bytes);
    }
    Ok(nested)
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
    append_private_patched_graph(manager, source, values, &[], &[], allocator, tags)
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
    let authored =
        append_private_patched_graph(manager, source, values, &[], &[], allocator, tags)?;
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

/// Points every variant block and Barrel slot that fires `edits.graph` at a private copy of that
/// graph, with its values and the graphs below it they change. A graft naming the graph with a
/// launch speed or a wider trajectory pool gives the copy both, except a field the values set on
/// the graph itself. Refuses values for a graph nothing fires once `patches` and the splice writes
/// in `spliced` apply.
fn projectile_patches(
    manager: &PackageManager,
    entity: &[u8],
    edits: &crate::weapon::projectile::Edits,
    (patches, spliced): (
        &mut Vec<WeaponRuntimeResourcePatch>,
        &[WeaponRuntimeResourcePatch],
    ),
    allocator: AppendedTagAllocator,
    tags: &mut Vec<NewTagSpec>,
) -> AuthoringResult<()> {
    if edits.values.is_empty() {
        return Ok(());
    }
    let graph = edits.graph;
    let slots = crate::weapon::behavior::fired_slots(manager, entity, patches, spliced)?
        .into_iter()
        .filter(|slot| slot.graph == Some(graph))
        .collect::<Vec<_>>();
    if slots.is_empty() {
        return Err(invalid(format!(
            "The Projectile values were set for 0x{graph:08X}, which the weapon no longer fires. Reset them on the Gameplay tab."
        )));
    }
    let field = |value: &WeaponRuntimeValueOverride| {
        let mut locator = value.locator.clone();
        locator.graph_tag = None;
        locator
    };
    let mut own = edits
        .values
        .iter()
        .filter(|value| value.locator.graph_tag.is_none_or(|tag| tag.get() == graph))
        .map(field)
        .collect::<BTreeSet<_>>();
    let mut values = edits.values.clone();
    let mut trajectories = None::<u16>;
    for patch in slots.iter().filter_map(|slot| slot.patch) {
        let patch = &patches[patch];
        for value in &patch.graph_values {
            if own.insert(field(value)) {
                values.push(value.clone());
            }
        }
        trajectories = trajectories.max(patch.graph_trajectories);
    }
    let first = tags.len();
    let copy = append_private_graph_tree(
        manager,
        TagHash(graph),
        &values,
        &BTreeMap::new(),
        &BTreeMap::new(),
        allocator,
        tags,
    )?;
    if let Some(capacity) = trajectories {
        // The copy of the graph is the first tag the tree appends, ahead of the graphs below it
        // and the owners its values change.
        if tags
            .get(first)
            .is_none_or(|tag| tag.template_tag != TagHash(graph))
        {
            return Err(invalid(
                "A private projectile copy did not start with its graph",
            ));
        }
        let mut payload = std::mem::take(&mut tags[first].payload);
        let widened = widen_trajectory_pool(
            manager,
            &mut payload,
            first + 1..tags.len(),
            usize::from(capacity),
            allocator,
            tags,
        );
        tags[first].payload = payload;
        widened.map_err(|error| invalid(format!("Graph 0x{graph:08X}: {error}")))?;
    }
    // A graft's patch names the copy in its place, which carries what the graft asked for.
    for slot in slots {
        let named = slot.naming(copy.0);
        match slot.patch {
            Some(index) => patches[index] = named,
            None => patches.push(named),
        }
    }
    Ok(())
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

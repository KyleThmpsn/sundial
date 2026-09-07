//! Native custom runtime operations with independent validation.
use super::*;

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
    runtime_tag_allocator: AppendedTagAllocator,
    runtime_new_tags: &mut Vec<NewTagSpec>,
) -> AuthoringResult<()> {
    let mut patches_by_owner = BTreeMap::<u32, Vec<ResolvedRuntimeResourcePatch>>::new();
    let mut graph_clones = Vec::<(u32, Vec<WeaponRuntimeValueOverride>, TagHash)>::new();
    for (value_index, value) in values.iter().enumerate() {
        let resolved = resolve_weapon_runtime_field(manager, entity, &value.locator)
            .map_err(|error| invalid(format!("Runtime value {value_index} is stale: {error}")))?;
        let bytes = encode_weapon_runtime_value(&resolved.field.kind, &value.value)
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
                binding_hash: value.locator.binding_hash,
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
        let bytes = if patch.graph_values.is_empty() {
            patch.bytes.clone()
        } else {
            let source = TagHash(read_u32(&patch.bytes, 0)?);
            let existing = graph_clones
                .iter()
                .find(|(tag, values, _)| *tag == source.0 && *values == patch.graph_values);
            let authored = if let Some((_, _, authored)) = existing {
                *authored
            } else {
                let authored = append_private_referenced_graph(
                    manager,
                    source,
                    &patch.graph_values,
                    runtime_tag_allocator,
                    runtime_new_tags,
                )?;
                graph_clones.push((source.0, patch.graph_values.clone(), authored));
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
        retarget_weapon_component_owner_payload(
            &mut owner_payload,
            entity,
            owner_tag.0,
            authored_owner_tag.0,
        )
        .map_err(invalid)?;
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
fn append_private_referenced_graph(
    manager: &PackageManager,
    source: TagHash,
    values: &[WeaponRuntimeValueOverride],
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
    append_patched_runtime_resource_owners(manager, &mut graph, values, &[], allocator, tags)?;
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
    if companion.len() != PRIVATE_PERK_RESIDENCY_COMPANION_SIZE {
        return Err(validation(format!(
            "Private perk residency companion serialized to 0x{:X} bytes instead of stock-shaped 0x{PRIVATE_PERK_RESIDENCY_COMPANION_SIZE:X}",
            companion.len()
        )));
    }
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

pub(super) fn clone_private_sandbox_perk_runtime(
    manager: &PackageManager,
    runtime_action: &SandboxPerkRuntimeAction,
    values: &[WeaponRuntimeValueOverride],
    action_float_values: &[WeaponSandboxPerkActionFloatOverride],
    activation: Option<sundial::package_authoring::sandbox_perk::activation::PerkActivation>,
    runtime_tag_allocator: AppendedTagAllocator,
    runtime_new_tags: &mut Vec<NewTagSpec>,
) -> AuthoringResult<TagHash> {
    let source_runtime_tag = runtime_action.action_tag;
    if values.is_empty() && action_float_values.is_empty() && activation.is_none() {
        return Ok(source_runtime_tag);
    }
    let mut action = if let Some(activation) = activation {
        sundial::package_authoring::sandbox_perk::activation::with_activation(
            source_runtime_tag.0,
            &runtime_action.action_payload,
            activation,
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
    let graphs = &runtime_action.graphs;
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
                resolve_weapon_runtime_field(manager, &graph.payload, &value.locator)
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
        values_by_graph[*graph_index].push(value.clone());
    }

    for (graph, graph_values) in graphs.iter().zip(values_by_graph) {
        if graph_values.is_empty() {
            continue;
        }
        let mut authored_graph = graph.payload.clone();
        append_patched_runtime_resource_owners(
            manager,
            &mut authored_graph,
            &graph_values,
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
            if read_u32(&action, offset)? != graph.tag.0 {
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

    let authored_action_tag = runtime_tag_allocator.assigned_tag(
        runtime_new_tags.len(),
        "Private sandbox-perk action",
        "sandbox-perk action",
    )?;
    action = synchronize_payload_size(action)?;
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
        template_tag: source_runtime_tag,
        payload: action,
        storage: crate::NewTagStorageMode::InheritTemplate,
    });
    runtime_new_tags.extend(residency_chain);
    Ok(authored_action_tag)
}

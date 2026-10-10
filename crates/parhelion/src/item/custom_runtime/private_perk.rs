//! A private sandbox perk's runtime: its cloned action, asset graphs and programs, with
//! the patches that bind them.
use super::*;

#[derive(Default)]
pub(in crate::item) struct PrivateRuntimeEdits<'a> {
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
pub(super) fn resolve_program_patches(
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
    !program.imported_assets.is_empty()
        || program
            .native_asset_patches
            .iter()
            .flat_map(|edit| &edit.patches)
            .any(|patch| patch.imported_particle.is_some())
}

pub(in crate::item) fn clone_private_sandbox_perk_runtime(
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
        let damage_type = projectiles
            .iter()
            .find(|selection| selection.source_graph == source.tag.0)
            .and_then(|selection| selection.damage_type);
        // A different graph's scalar edits must not waive this graph's enrollment check.
        let cloned = !graph_values.is_empty() || source.tag != graph.tag || damage_type.is_some();
        sundial::package_authoring::sandbox_perk::entity::residency::inspect(manager, graph.tag.0)
            .map_err(invalid)?;
        if !cloned {
            continue;
        }
        let authored_graph_tag = match damage_type {
            // The damage type reaches the profiles of the graphs below the projectile too, so
            // the copy is of its tree, each graph leading to a retyped profile copied with it.
            Some(mode) => {
                let patches = damage::retyped(
                    manager,
                    (graph.tag.0, mode.byte()),
                    runtime_tag_allocator,
                    runtime_new_tags,
                )?;
                append_private_graph_tree(
                    manager,
                    graph.tag,
                    &graph_values,
                    &patches,
                    &BTreeMap::new(),
                    runtime_tag_allocator,
                    runtime_new_tags,
                )?
            }
            None => {
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
                runtime_new_tags.push(NewTagSpec {
                    template_tag: graph.tag,
                    payload: authored_graph,
                    storage: crate::NewTagStorageMode::InheritTemplate,
                });
                authored_graph_tag
            }
        };
        for &offset in &graph.action_offsets {
            if read_u32(&action, offset)? != source.tag.0 {
                return Err(validation(
                    "Sandbox-perk runtime action graph reference moved while authoring",
                ));
            }
            write_u32(&mut action, offset, authored_graph_tag.0)?;
        }
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
pub(in crate::item) fn append_private_action(
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
pub(super) fn append_private_asset_graph(
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
            pointers: Vec::new(),
            references: Vec::new(),
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

pub(super) fn append_private_program_runtime(
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
        // A guided program's HUD status without settings joins its action's asset edits below.
        // The native form has no per-action edits, so its asset takes the private copy here.
        let native_status = program.native.is_some() && asset.hud_status.is_some();
        if asset.values.is_empty()
            && asset.damage_type.is_none()
            && asset.rows.is_empty()
            && !native_status
        {
            sundial::package_authoring::sandbox_perk::entity::residency::inspect(
                manager,
                asset.graph,
            )
            .map_err(invalid)?;
            continue;
        }
        // A HUD status of the project's own joins the copy the settings make. A damage type
        // reaches the damage profiles of the graphs below the asset too, so that copy is of the
        // tree, each graph leading to a retyped profile copied with it.
        let status = super::hud_status::value_copy_patches(manager, asset)?;
        // Rows added to the asset's modifiers grow its copy's records.
        let appends =
            super::modifier_rows::appends(manager, asset.graph, &asset.values, &asset.rows)?;
        let mut grown = BTreeMap::new();
        if !appends.is_empty() {
            grown.insert(asset.graph, appends.clone());
        }
        let graph = match asset.damage_type {
            Some(mode) => {
                let mut patches =
                    damage::retyped(manager, (asset.graph, mode.byte()), allocator, tags)?;
                patches.entry(asset.graph).or_default().extend(status);
                append_private_graph_tree(
                    manager,
                    TagHash(asset.graph),
                    &asset.values,
                    &patches,
                    &grown,
                    allocator,
                    tags,
                )?
            }
            None => append_private_patched_graph(
                manager,
                TagHash(asset.graph),
                &asset.values,
                &status,
                &appends,
                allocator,
                tags,
            )?,
        };
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
    for imported in &program.imported_assets {
        let offset = compiled
            .graph_offsets
            .get(imported.action_index)
            .copied()
            .flatten()
            .ok_or_else(|| invalid("Imported attachment has no compiled entity operand"))?;
        let asset = program
            .actions
            .get(imported.action_index)
            .and_then(|action| action.asset())
            .ok_or_else(|| invalid("Imported attachment has no entity action"))?;
        if read_u32(&compiled.payload, offset)? != asset.graph {
            return Err(invalid(
                "Imported attachment envelope changed during compilation",
            ));
        }
        let tag = particle_symbols
            .and_then(|symbols| symbols.get(&imported.symbol))
            .ok_or_else(|| {
                invalid(format!(
                    "Imported attachment {} was not allocated",
                    imported.symbol
                ))
            })?;
        write_u32(&mut compiled.payload, offset, tag.0)?;
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
pub(super) fn rebind_program_callbacks(payload: &mut [u8], owner: TagHash) -> AuthoringResult<()> {
    let references = sundial::package_authoring::sandbox_perk::action::self_references(payload)
        .map_err(invalid)?;
    for reference in references {
        write_u32(payload, reference.reference_offset, owner.0)?;
        let at = reference.reference_offset + 8;
        payload[at..at + 8].copy_from_slice(&(reference.target_offset as u64).to_le_bytes());
    }
    Ok(())
}

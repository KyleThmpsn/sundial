use super::sources::ProjectSources;
use super::*;

pub(super) struct DonorItem {
    pub(super) item_index: usize,
    pub(super) definition_tag: TagHash,
    pub(super) string_tag: TagHash,
}

pub(super) fn resolve_donor_item(
    sources: &ProjectSources,
    item_hash: u32,
    context: &str,
) -> AuthoringResult<DonorItem> {
    let item_index = sources
        .stock_item_rows_by_hash
        .get(&item_hash)
        .and_then(|rows| rows.first())
        .copied()
        .ok_or_else(|| invalid(format!("{context} item 0x{item_hash:08X} is missing")))?;
    let string_row = sources.string_rows + item_index * ITEM_ROW_SIZE;
    if read_u32(&sources.stock_item_strings, string_row)? != item_hash {
        return Err(invalid(format!(
            "{context} item and item-string rows are not aligned"
        )));
    }
    Ok(DonorItem {
        item_index,
        definition_tag: TagHash(read_u32(
            &sources.stock_item_table,
            sources.item_rows + item_index * ITEM_ROW_SIZE + 16,
        )?),
        string_tag: TagHash(read_u32(&sources.stock_item_strings, string_row + 16)?),
    })
}

pub(super) fn validate_donor_name(
    sources: &ProjectSources,
    string_tag: TagHash,
    expected_name: Option<&str>,
    context: &str,
) -> AuthoringResult<()> {
    let actual_name = resolve_item_name(&sources.manager, string_tag).map_err(invalid)?;
    if let Some(expected_name) = expected_name
        && expected_name != actual_name
    {
        return Err(invalid(format!(
            "{context} item resolves to {actual_name:?}, not {expected_name:?}"
        )));
    }
    Ok(())
}

pub(super) fn resolve_icon_donor(
    sources: &ProjectSources,
    reference: &WeaponIconDonorReference,
) -> AuthoringResult<ResolvedIconDonor> {
    let manager = &sources.manager;
    let item_icons = &sources.stock_item_icons;
    let DonorItem {
        item_index,
        string_tag,
        ..
    } = resolve_donor_item(sources, reference.item_hash, "Icon donor")?;
    if reference.expected_name.is_some() {
        validate_donor_name(
            sources,
            string_tag,
            reference.expected_name.as_deref(),
            "Icon donor",
        )?;
    }
    let strings = read_tag(manager, string_tag, "icon donor item-string")?;
    let icon_index = read_u16(&strings, ITEM_STRING_ICON_INDEX_OFFSET)?;
    validate_reused_stock_item_icon(item_icons, &strings, icon_index)?;
    let icon_container = stock_item_icon_container(item_icons, icon_index)?;
    read_tag(manager, icon_container, "icon donor container")?;
    Ok(ResolvedIconDonor {
        item_index,
        icon_index,
        icon_container,
    })
}

pub(super) fn resolve_render_gear_donor(
    sources: &ProjectSources,
    reference: &WeaponRenderGearDonorReference,
) -> AuthoringResult<ResolvedRenderGearDonor> {
    let manager = &sources.manager;
    let DonorItem {
        definition_tag,
        string_tag,
        ..
    } = resolve_donor_item(sources, reference.item_hash, "Render-gear donor")?;
    if reference.expected_name.is_some() {
        validate_donor_name(
            sources,
            string_tag,
            reference.expected_name.as_deref(),
            "Render-gear donor",
        )?;
    }
    let definition = read_tag(manager, definition_tag, "render-gear donor weapon")?;
    weapon_translation_topology(&definition)?;
    weapon_render_dye_rows(&definition)?;
    Ok(ResolvedRenderGearDonor { definition })
}

pub(super) fn resolve_presentation_donor(
    sources: &ProjectSources,
    reference: &WeaponPresentationDonorReference,
) -> AuthoringResult<ResolvedPresentationDonor> {
    let manager = &sources.manager;
    let collectibles = &sources.stock_collectibles;
    let collectible_rows = sources.collectible_rows;
    let collectible_count = sources.stock_collectible_count;
    let collectible_displays = &sources.stock_collectible_displays;
    let presentation_nodes = &sources.stock_nodes;
    let item_icons = &sources.stock_item_icons;
    let DonorItem {
        item_index,
        definition_tag,
        string_tag,
    } = resolve_donor_item(sources, reference.item_hash, "Geometry donor")?;
    validate_donor_name(
        sources,
        string_tag,
        reference.expected_name.as_deref(),
        "Geometry donor",
    )?;
    let item_index_u16 = u16::try_from(item_index)
        .map_err(|_| invalid("Geometry donor item index does not fit 16 bits"))?;
    let donor_collectibles = (0..collectible_count)
        .filter(|index| {
            read_u16(
                collectibles,
                collectible_rows + *index * COLLECTIBLE_ROW_SIZE + COLLECTIBLE_ITEM_INDEX_OFFSET,
            )
            .ok()
                == Some(item_index_u16)
        })
        .collect::<Vec<_>>();
    let [collectible_index] = donor_collectibles.as_slice() else {
        return Err(invalid(format!(
            "Geometry donor resolves to {} collectible rows; exactly one is required",
            donor_collectibles.len()
        )));
    };
    let parents =
        template_presentation_parents(presentation_nodes, collectibles, *collectible_index)?;
    // Require a real weapon collection entry, but do not equate its page with weapon family:
    // Exotic and Legendary weapons of the same family live on different Collections pages.
    donor_weapon_collection_page(presentation_nodes, &parents)?;
    let definition = read_tag(manager, definition_tag, "geometry donor weapon")?;
    let strings = read_tag(manager, string_tag, "geometry donor item-string")?;
    let icon_index = read_u16(&strings, ITEM_STRING_ICON_INDEX_OFFSET)?;
    validate_reused_stock_item_icon(item_icons, &strings, icon_index)?;
    let (display_count, _, display_rows, display_class) = array_at(collectible_displays, 8)?;
    let collectible_row = collectible_rows + *collectible_index * COLLECTIBLE_ROW_SIZE;
    let display_row = display_rows + *collectible_index * COLLECTIBLE_DISPLAY_ROW_SIZE;
    if display_class != COLLECTIBLE_DISPLAY_ROW_CLASS
        || display_count != collectible_count
        || read_u32(collectible_displays, display_row)?
            != read_u32(collectibles, collectible_row + COLLECTIBLE_HASH_OFFSET)?
        || read_u32(
            collectible_displays,
            display_row + COLLECTIBLE_DISPLAY_ICON_INDEX_OFFSET,
        )? != u32::from(icon_index)
    {
        return Err(invalid(
            "Geometry donor item-string and collectible-display icons are not aligned",
        ));
    }
    let icon_container = stock_item_icon_container(item_icons, icon_index)?;
    read_tag(manager, icon_container, "geometry donor icon container")?;
    weapon_translation_topology(&definition)?;
    let inventory_slot = weapon_inventory_slot(&definition)?;
    Ok(ResolvedPresentationDonor {
        item_index,
        definition,
        strings,
        icon_index,
        icon_container,
        inventory_slot,
    })
}

pub(super) fn resolve_runtime_component_donor(
    sources: &ProjectSources,
    reference: &WeaponRuntimeComponentDonorReference,
) -> AuthoringResult<ResolvedRuntimeComponentDonor> {
    let manager = &sources.manager;
    let sandbox_patterns = &sources.stock_sandbox_patterns;
    let context = format!("Runtime component 0x{:08X} donor", reference.binding_hash);
    let DonorItem {
        definition_tag,
        string_tag,
        ..
    } = resolve_donor_item(sources, reference.item_hash, &context)?;
    if reference.expected_name.is_some() {
        validate_donor_name(
            sources,
            string_tag,
            reference.expected_name.as_deref(),
            &context,
        )?;
    }
    let definition = read_tag(manager, definition_tag, "runtime-component donor weapon")?;
    let pattern_index = weapon_pattern_index(&definition)?.ok_or_else(|| {
        invalid(format!(
            "Runtime component 0x{:08X} donor item 0x{:08X} has no weapon pattern",
            reference.binding_hash, reference.item_hash
        ))
    })?;
    let pattern_item_hash = sandbox_pattern_source_at(sandbox_patterns, pattern_index)?.item_hash;
    Ok(ResolvedRuntimeComponentDonor {
        binding_hash: reference.binding_hash,
        pattern_item_hash,
    })
}

pub(super) fn sandbox_pattern_source_at(
    sandbox_patterns: &[u8],
    row_index: u16,
) -> AuthoringResult<ResolvedSandboxPatternSource> {
    let pattern = sandbox_pattern_identity_at(sandbox_patterns, usize::from(row_index))
        .map_err(invalid)?
        .ok_or_else(|| {
            invalid(format!(
                "Weapon pattern index {row_index} is outside the installed sandbox-pattern table"
            ))
        })?;
    let item_hash = pattern.item_hash;
    let global_id_hash = pattern.pattern_global_id_hash;
    if matches!(item_hash, 0 | FNV1_EMPTY_HASH) || matches!(global_id_hash, 0 | FNV1_EMPTY_HASH) {
        return Err(invalid(format!(
            "Weapon pattern index {row_index} has an inactive item or runtime identity"
        )));
    }
    ResolvedSandboxPatternSource::try_from(pattern)
}

pub(super) fn resolve_added_damage_carrier_source(
    sources: &ProjectSources,
    runtime_source: Option<ResolvedSandboxPatternSource>,
    gameplay_definition: &[u8],
    target_slot: WeaponInventorySlot,
    presentation_definition: Option<&[u8]>,
) -> AuthoringResult<Option<ResolvedDamageCarrierSource>> {
    let manager = &sources.manager;
    let sandbox_patterns = &sources.stock_sandbox_patterns;
    let entity_assignments = &sources.stock_entity_assignments;
    let item_table = &sources.stock_item_table;
    let item_rows = sources.item_rows;
    if target_slot == WeaponInventorySlot::Kinetic {
        return Ok(None);
    }

    let gameplay_damage_lanes = weapon_damage_socket_lanes(gameplay_definition)?;
    let gameplay_rarity = weapon_rarity(gameplay_definition).ok();
    let gameplay_socket_count =
        relative_target(gameplay_definition, ITEM_ORDINARY_SOCKET_POINTER_OFFSET)
            .ok()
            .and_then(|resource| array_at(gameplay_definition, resource).ok())
            .map(|(count, _, _, _)| count);
    let mut runtime_peers = Vec::new();
    if let Some(runtime_source) = runtime_source {
        let source_entity =
            weapon_entity_assignment(entity_assignments, runtime_source.pattern_global_id_hash)
                .map_err(invalid)?;
        if let Some(source_entity) = source_entity {
            let (pattern_count, _, _, _) = array_at(sandbox_patterns, 8)?;
            for row_index in 0..pattern_count {
                let Some(pattern) =
                    sandbox_pattern_identity_at(sandbox_patterns, row_index).map_err(invalid)?
                else {
                    continue;
                };
                if weapon_entity_assignment(entity_assignments, pattern.pattern_global_id_hash)
                    .map_err(invalid)?
                    != Some(source_entity)
                {
                    continue;
                }
                let Some(&item_index) = sources
                    .stock_item_rows_by_hash
                    .get(&pattern.item_hash)
                    .and_then(|rows| rows.first())
                else {
                    continue;
                };
                let definition_tag = TagHash(read_u32(
                    item_table,
                    item_rows + item_index * ITEM_ROW_SIZE + 16,
                )?);
                let Ok(definition) = read_tag(manager, definition_tag, "runtime-peer weapon")
                else {
                    continue;
                };
                let peer_slot = weapon_inventory_slot(&definition).ok();
                if !matches!(
                    peer_slot,
                    Some(WeaponInventorySlot::Energy | WeaponInventorySlot::Power)
                ) {
                    continue;
                }
                let Ok(carrier) = weapon_damage_carrier(&definition) else {
                    continue;
                };
                let Some(family) = carrier.family() else {
                    continue;
                };
                // A shared runtime entity can back both fixed-perk and socket-driven weapons.
                // Rank only carriers that fit the gameplay definition's existing topology.
                if family == WeaponDamageCarrierFamily::PlugDriven
                    && gameplay_damage_lanes.len() != 1
                {
                    continue;
                }
                let peer_socket_count =
                    relative_target(&definition, ITEM_ORDINARY_SOCKET_POINTER_OFFSET)
                        .ok()
                        .and_then(|resource| array_at(&definition, resource).ok())
                        .map(|(count, _, _, _)| count);
                // Slot and element are authored independently. Prefer a peer in the
                // requested slot, but a compatible elemental peer of this exact
                // runtime entity also proves the carrier when that slot has none.
                let score = u8::from(peer_slot == Some(target_slot)) * 4
                    + u8::from(weapon_rarity(&definition).ok() == gameplay_rarity) * 2
                    + u8::from(peer_socket_count == gameplay_socket_count);
                runtime_peers.push((pattern.item_hash, family, definition, score));
            }
        }
    }

    let best_score = runtime_peers.iter().map(|(_, _, _, score)| *score).max();
    let families = runtime_peers
        .iter()
        .filter(|(_, _, _, score)| Some(*score) == best_score)
        .map(|(_, family, _, _)| *family)
        .collect::<BTreeSet<_>>();
    if families.len() > 1 {
        let peers = runtime_peers
            .iter()
            .filter(|(_, _, _, score)| Some(*score) == best_score)
            .map(|(hash, family, _, _)| format!("0x{hash:08X} ({family:?})"))
            .collect::<Vec<_>>()
            .join(", ");
        return Err(invalid(format!(
            "The selected runtime family has conflicting stock {target_slot:?} damage carriers: {peers}"
        )));
    }
    if let Some(family) = families.first().copied() {
        let topology_definition = (family == WeaponDamageCarrierFamily::PlugDriven)
            .then(|| {
                runtime_peers
                    .iter()
                    .filter(|(_, _, _, score)| Some(*score) == best_score)
                    .find(|(_, candidate, _, _)| *candidate == family)
                    .map(|(_, _, definition, _)| definition.clone())
            })
            .flatten();
        return Ok(Some(ResolvedDamageCarrierSource {
            family,
            topology_definition,
        }));
    }

    let presentation = presentation_definition.ok_or_else(|| {
        invalid(
            "No stock elemental runtime peer or presentation donor proves the requested elemental damage carrier family",
        )
    })?;
    let carrier = weapon_damage_carrier(presentation)?;
    let family = carrier.family().ok_or_else(|| {
        invalid("The target-slot presentation donor has no fixed elemental damage carrier")
    })?;
    if family == WeaponDamageCarrierFamily::PlugDriven && gameplay_damage_lanes.len() != 1 {
        return Err(invalid(
            "The presentation donor requires a type-68 damage socket absent from the gameplay definition",
        ));
    }
    Ok(Some(ResolvedDamageCarrierSource {
        family,
        topology_definition: (family == WeaponDamageCarrierFamily::PlugDriven)
            .then(|| presentation.to_vec()),
    }))
}

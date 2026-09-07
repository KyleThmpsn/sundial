use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn resolve_icon_donor(
    manager: &PackageManager,
    reference: &WeaponIconDonorReference,
    item_table: &[u8],
    item_rows: usize,
    item_count: usize,
    item_strings: &[u8],
    string_rows: usize,
    item_icons: &[u8],
) -> AuthoringResult<ResolvedIconDonor> {
    let item_index = find_u32_row_key(
        item_table,
        item_rows,
        item_count,
        ITEM_ROW_SIZE,
        reference.item_hash,
    )?
    .ok_or_else(|| {
        invalid(format!(
            "Icon donor item 0x{:08X} is missing",
            reference.item_hash
        ))
    })?;
    let string_row = string_rows + item_index * ITEM_ROW_SIZE;
    if read_u32(item_strings, string_row)? != reference.item_hash {
        return Err(invalid(
            "Icon donor item and item-string rows are not aligned",
        ));
    }
    let string_tag = TagHash(read_u32(item_strings, string_row + 16)?);
    if let Some(expected_name) = &reference.expected_name {
        let actual_name = resolve_item_name(manager, string_tag).map_err(invalid)?;
        if expected_name != &actual_name {
            return Err(invalid(format!(
                "Icon donor item resolves to {actual_name:?}, not {expected_name:?}"
            )));
        }
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

#[allow(clippy::too_many_arguments)]
pub(super) fn resolve_render_gear_donor(
    manager: &PackageManager,
    reference: &WeaponRenderGearDonorReference,
    item_table: &[u8],
    item_rows: usize,
    item_count: usize,
    item_strings: &[u8],
    string_rows: usize,
) -> AuthoringResult<ResolvedRenderGearDonor> {
    let item_index = find_u32_row_key(
        item_table,
        item_rows,
        item_count,
        ITEM_ROW_SIZE,
        reference.item_hash,
    )?
    .ok_or_else(|| {
        invalid(format!(
            "Render-gear donor item 0x{:08X} is missing",
            reference.item_hash
        ))
    })?;
    let string_row = string_rows + item_index * ITEM_ROW_SIZE;
    if read_u32(item_strings, string_row)? != reference.item_hash {
        return Err(invalid(
            "Render-gear donor item and item-string rows are not aligned",
        ));
    }
    let definition_tag = TagHash(read_u32(
        item_table,
        item_rows + item_index * ITEM_ROW_SIZE + 16,
    )?);
    let string_tag = TagHash(read_u32(item_strings, string_row + 16)?);
    if let Some(expected_name) = &reference.expected_name {
        let actual_name = resolve_item_name(manager, string_tag).map_err(invalid)?;
        if expected_name != &actual_name {
            return Err(invalid(format!(
                "Render-gear donor item resolves to {actual_name:?}, not {expected_name:?}"
            )));
        }
    }
    let definition = read_tag(manager, definition_tag, "render-gear donor weapon")?;
    weapon_translation_topology(&definition)?;
    weapon_render_dye_rows(&definition)?;
    Ok(ResolvedRenderGearDonor { definition })
}

#[allow(clippy::too_many_arguments)]
pub(super) fn resolve_presentation_donor(
    manager: &PackageManager,
    reference: &WeaponPresentationDonorReference,
    item_table: &[u8],
    item_rows: usize,
    item_count: usize,
    item_strings: &[u8],
    string_rows: usize,
    collectibles: &[u8],
    collectible_rows: usize,
    collectible_count: usize,
    collectible_displays: &[u8],
    presentation_nodes: &[u8],
    item_icons: &[u8],
) -> AuthoringResult<ResolvedPresentationDonor> {
    let item_index = find_u32_row_key(
        item_table,
        item_rows,
        item_count,
        ITEM_ROW_SIZE,
        reference.item_hash,
    )?
    .ok_or_else(|| {
        invalid(format!(
            "Geometry donor item 0x{:08X} is missing",
            reference.item_hash
        ))
    })?;
    let string_row = string_rows + item_index * ITEM_ROW_SIZE;
    if read_u32(item_strings, string_row)? != reference.item_hash {
        return Err(invalid(
            "Geometry donor item and item-string rows are not aligned",
        ));
    }
    let definition_tag = TagHash(read_u32(
        item_table,
        item_rows + item_index * ITEM_ROW_SIZE + 16,
    )?);
    let string_tag = TagHash(read_u32(item_strings, string_row + 16)?);
    let actual_name = resolve_item_name(manager, string_tag).map_err(invalid)?;
    if let Some(expected_name) = &reference.expected_name
        && expected_name != &actual_name
    {
        return Err(invalid(format!(
            "Geometry donor item resolves to {actual_name:?}, not {expected_name:?}"
        )));
    }
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

#[allow(clippy::too_many_arguments)]
pub(super) fn resolve_runtime_component_donor(
    manager: &PackageManager,
    reference: &WeaponRuntimeComponentDonorReference,
    item_table: &[u8],
    item_rows: usize,
    item_count: usize,
    item_strings: &[u8],
    string_rows: usize,
    sandbox_patterns: &[u8],
) -> AuthoringResult<ResolvedRuntimeComponentDonor> {
    let item_index = find_u32_row_key(
        item_table,
        item_rows,
        item_count,
        ITEM_ROW_SIZE,
        reference.item_hash,
    )?
    .ok_or_else(|| {
        invalid(format!(
            "Runtime component 0x{:08X} donor item 0x{:08X} is missing",
            reference.binding_hash, reference.item_hash
        ))
    })?;
    let string_row = string_rows + item_index * ITEM_ROW_SIZE;
    if read_u32(item_strings, string_row)? != reference.item_hash {
        return Err(invalid(
            "Runtime-component donor item and item-string rows are not aligned",
        ));
    }
    if let Some(expected_name) = &reference.expected_name {
        let string_tag = TagHash(read_u32(item_strings, string_row + 16)?);
        let actual_name = resolve_item_name(manager, string_tag).map_err(invalid)?;
        if expected_name != &actual_name {
            return Err(invalid(format!(
                "Runtime component 0x{:08X} donor item resolves to {actual_name:?}, not {expected_name:?}",
                reference.binding_hash
            )));
        }
    }
    let definition_tag = TagHash(read_u32(
        item_table,
        item_rows + item_index * ITEM_ROW_SIZE + 16,
    )?);
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

#[allow(clippy::too_many_arguments)]
pub(super) fn resolve_added_damage_carrier_source(
    manager: &PackageManager,
    sandbox_patterns: &[u8],
    entity_assignments: &[u8],
    item_table: &[u8],
    item_rows: usize,
    item_count: usize,
    runtime_source: Option<ResolvedSandboxPatternSource>,
    gameplay_definition: &[u8],
    target_slot: WeaponInventorySlot,
    presentation_definition: Option<&[u8]>,
) -> AuthoringResult<Option<ResolvedDamageCarrierSource>> {
    if target_slot == WeaponInventorySlot::Kinetic {
        return Ok(None);
    }

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
                let Some(item_index) = find_u32_row_key(
                    item_table,
                    item_rows,
                    item_count,
                    ITEM_ROW_SIZE,
                    pattern.item_hash,
                )?
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
                if weapon_inventory_slot(&definition).ok() != Some(target_slot) {
                    continue;
                }
                let Ok(carrier) = weapon_damage_carrier(&definition) else {
                    continue;
                };
                let Some(family) = carrier.family() else {
                    continue;
                };
                let peer_socket_count =
                    relative_target(&definition, ITEM_ORDINARY_SOCKET_POINTER_OFFSET)
                        .ok()
                        .and_then(|resource| array_at(&definition, resource).ok())
                        .map(|(count, _, _, _)| count);
                let score = u8::from(weapon_rarity(&definition).ok() == gameplay_rarity) * 2
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
            "No stock runtime peer or target-slot presentation donor proves the requested elemental damage carrier family",
        )
    })?;
    let carrier = weapon_damage_carrier(presentation)?;
    let family = carrier.family().ok_or_else(|| {
        invalid("The target-slot presentation donor has no fixed elemental damage carrier")
    })?;
    Ok(Some(ResolvedDamageCarrierSource {
        family,
        topology_definition: (family == WeaponDamageCarrierFamily::PlugDriven)
            .then(|| presentation.to_vec()),
    }))
}

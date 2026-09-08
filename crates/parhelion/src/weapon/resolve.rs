//! Resolve all weapon dependencies against a validated, read-only source generation.
use super::sources::ProjectSources;
use super::*;

#[derive(Clone)]
pub(super) struct ResolvedWeapon {
    pub(super) weapon: WeaponCloneSpec,
    pub(super) donor_item_index: usize,
    pub(super) donor_collectible_index: usize,
    pub(super) collection_donor_index: usize,
    pub(super) source_unlock_index: usize,
    pub(super) source_acquired_flag: u16,
    pub(super) definition_tag: TagHash,
    pub(super) string_tag: TagHash,
    pub(super) definition: Vec<u8>,
    pub(super) strings: Vec<u8>,
    pub(super) icon_template_item_index: usize,
    pub(super) donor_icon_index: u16,
    pub(super) donor_icon_container: TagHash,
    pub(super) presentation_donor: Option<ResolvedPresentationDonor>,
    pub(super) damage_carrier_source: Option<ResolvedDamageCarrierSource>,
    pub(super) render_gear_donor: Option<ResolvedRenderGearDonor>,
    pub(super) runtime_component_donors: Vec<ResolvedRuntimeComponentDonor>,
    pub(super) runtime_pattern_source: Option<ResolvedSandboxPatternSource>,
    pub(super) gear_art_pattern_source: Option<ResolvedSandboxPatternSource>,
    pub(super) weapon_page: u16,
    pub(super) count_selection: SunriseAcquiredPoolSelection,
    pub(super) socket_column_indices: Vec<Option<ResolvedSocketColumn>>,
}

pub(super) fn resolve_project_weapons(
    sources: &ProjectSources,
    weapons: &[WeaponCloneSpec],
) -> AuthoringResult<Vec<ResolvedWeapon>> {
    let manager = &sources.manager;
    let stock_item_table = &sources.stock_item_table;
    let stock_item_strings = &sources.stock_item_strings;
    let stock_sandbox_patterns = &sources.stock_sandbox_patterns;
    let stock_item_icons = &sources.stock_item_icons;
    let stock_collectibles = &sources.stock_collectibles;
    let stock_collectible_displays = &sources.stock_collectible_displays;
    let stock_objectives = &sources.stock_objectives;
    let stock_nodes = &sources.stock_nodes;
    let stock_pools = &sources.stock_pools;
    let stock_unlocks = &sources.stock_unlocks;
    let stock_entity_assignments = &sources.stock_entity_assignments;
    let stock_item_count = sources.stock_item_count;
    let item_rows = sources.item_rows;
    let stock_item_rows_by_hash = &sources.stock_item_rows_by_hash;
    let string_rows = sources.string_rows;
    let stock_collectible_count = sources.stock_collectible_count;
    let collectible_rows = sources.collectible_rows;
    let stock_unlock_count = sources.stock_unlock_count;
    let unlock_rows = sources.unlock_rows;
    let mut resolved = Vec::with_capacity(weapons.len());
    for weapon in weapons {
        let resolved_weapon = (|| -> AuthoringResult<ResolvedWeapon> {
            let identity = weapon.identity;
        if contains_u32_row_key(
            stock_item_table,
            item_rows,
            stock_item_count,
            ITEM_ROW_SIZE,
            identity.item_hash,
        )? {
            return Err(AuthoringError::InvalidInput(format!(
                "Item hash 0x{:08X} already exists in stock",
                identity.item_hash
            )));
        }
        if contains_u32_at_offset(
            stock_collectibles,
            collectible_rows,
            stock_collectible_count,
            COLLECTIBLE_ROW_SIZE,
            COLLECTIBLE_HASH_OFFSET,
            identity.collectible_hash,
        )? {
            return Err(AuthoringError::InvalidInput(format!(
                "Collectible hash 0x{:08X} already exists in stock",
                identity.collectible_hash
            )));
        }
        if contains_u32_row_key(
            stock_unlocks,
            unlock_rows,
            stock_unlock_count,
            UNLOCK_ROW_SIZE,
            identity.unlock_hash,
        )? {
            return Err(AuthoringError::InvalidInput(format!(
                "Unlock hash 0x{:08X} already exists in stock",
                identity.unlock_hash
            )));
        }
        let donor_item_index = find_u32_row_key(
            stock_item_table,
            item_rows,
            stock_item_count,
            ITEM_ROW_SIZE,
            weapon.donor_item_hash,
        )?
        .ok_or_else(|| {
            invalid(format!(
                "Donor item 0x{:08X} is missing",
                weapon.donor_item_hash
            ))
        })?;
        if read_u32(
            stock_item_strings,
            string_rows + donor_item_index * ITEM_ROW_SIZE,
        )? != weapon.donor_item_hash
        {
            return Err(invalid("Donor item and item-string rows are not aligned"));
        }
        let definition_tag = TagHash(read_u32(
            stock_item_table,
            item_rows + donor_item_index * ITEM_ROW_SIZE + 16,
        )?);
        let string_tag = TagHash(read_u32(
            stock_item_strings,
            string_rows + donor_item_index * ITEM_ROW_SIZE + 16,
        )?);
        let donor_name = resolve_item_name(manager, string_tag).map_err(invalid)?;
        if let Some(expected) = &weapon.expected_donor_name
            && expected != &donor_name
        {
            return Err(invalid(format!(
                "Donor item resolves to {donor_name:?}, not {expected:?}"
            )));
        }
        let donor_item_index_u16 = u16::try_from(donor_item_index)
            .map_err(|_| invalid("Donor item index does not fit 16 bits"))?;
        let donor_collectibles = (0..stock_collectible_count)
            .filter(|index| {
                read_u16(
                    stock_collectibles,
                    collectible_rows
                        + *index * COLLECTIBLE_ROW_SIZE
                        + COLLECTIBLE_ITEM_INDEX_OFFSET,
                )
                .ok()
                    == Some(donor_item_index_u16)
            })
            .collect::<Vec<_>>();
        let [donor_collectible_index] = donor_collectibles.as_slice() else {
            return Err(invalid(format!(
                "Donor resolves to {} collectible rows; exactly one is required",
                donor_collectibles.len()
            )));
        };
        let donor_collectible_index = *donor_collectible_index;
        let source_unlock_index = collection_unlock_index(
            stock_collectibles,
            collectible_rows + donor_collectible_index * COLLECTIBLE_ROW_SIZE,
        )?;
        if source_unlock_index >= stock_unlock_count {
            return Err(invalid(
                "Donor collectible references an unavailable unlock",
            ));
        }
        let definition = read_tag(manager, definition_tag, "donor weapon")?;
        let strings = read_tag(manager, string_tag, "donor item-string")?;
        if matching_u32_offsets(&definition, weapon.donor_item_hash)
            != [ITEM_DEFINITION_HASH_OFFSET]
            || !matching_u32_offsets(&strings, weapon.donor_item_hash).is_empty()
        {
            return Err(invalid(
                "Donor embeds its item identity at unsupported offsets",
            ));
        }
        let donor_icon_index = read_u16(&strings, ITEM_STRING_ICON_INDEX_OFFSET)?;
        validate_reused_stock_item_icon(stock_item_icons, &strings, donor_icon_index)?;
        let donor_icon_container =
            stock_item_icon_container(stock_item_icons, donor_icon_index)?;
        read_tag(manager, donor_icon_container, "gameplay donor icon container")?;
        let gameplay_inventory_slot = weapon_inventory_slot(&definition)?;
        let gameplay_pattern_index = weapon_pattern_index(&definition)?;
        let selected_pattern_index = weapon
            .overrides
            .weapon_pattern_index
            .or(gameplay_pattern_index);
        let runtime_pattern_source = selected_pattern_index
            .map(|row_index| sandbox_pattern_source_at(stock_sandbox_patterns, row_index))
            .transpose()?;
        if weapon.overrides.ammo_type.is_some() && runtime_pattern_source.is_none() {
            return Err(invalid("Native ammo authoring requires an active weapon sandbox pattern"));
        }
        if weapon.overrides.hud_icon.is_some() && runtime_pattern_source.is_none() {
            return Err(invalid("HUD icon authoring requires an active weapon sandbox pattern"));
        }
        let authored_inventory_slot = weapon
            .overrides
            .inventory_slot
            .unwrap_or(gameplay_inventory_slot);
        // Collections classification is independent of gameplay and appearance. In stock,
        // Exotic weapons are grouped by slot; other weapons are grouped by weapon family.
        // Use a real child of the target page as the placement/count exemplar, never move
        // a gameplay donor's count terms into an unrelated hierarchy.
        let authored_rarity = weapon.overrides.rarity.unwrap_or(weapon_rarity(&definition)?);
        let collection_candidates = resolve_weapon_collection_donor(
            manager, stock_item_table, item_rows, stock_item_count,
            stock_item_strings, string_rows, stock_collectibles, collectible_rows,
            stock_collectible_count, donor_collectible_index, &definition, &strings,
            authored_rarity, authored_inventory_slot,
        )?;
        let mut placement_error = None;
        let placement = collection_candidates.into_iter().find_map(|index| {
            let attempt = (|| -> AuthoringResult<_> {
                let parents = template_presentation_parents(stock_nodes, stock_collectibles, index)?;
                let page = donor_weapon_collection_page(stock_nodes, &parents)?;
                let unlock = collection_unlock_index(stock_collectibles, collectible_rows + index * COLLECTIBLE_ROW_SIZE)?;
                if unlock >= stock_unlock_count { return Err(invalid("Collection exemplar unlock is outside the table")); }
                let flag = u16::try_from(unlock).map_err(|_| invalid("Collection acquired flag does not fit 16 bits"))?;
                let counts = classify_sunrise_count_pools(stock_pools, stock_nodes, stock_collectibles,
                    stock_objectives, &parents, page, flag)?;
                Ok((index, page, flag, counts))
            })();
            match attempt {
                Ok(placement) => Some(placement),
                Err(error) => { placement_error = Some(error); None }
            }
        });
        let (collection_donor_index, weapon_page, source_acquired_flag, count_selection) = placement
            .ok_or_else(|| placement_error.unwrap_or_else(|| invalid("No compatible Collections placement exemplar")))?;
        let presentation_donor = weapon
            .presentation_donor
            .as_ref()
            .map(|reference| {
                resolve_presentation_donor(
                    manager,
                    reference,
                    stock_item_table,
                    item_rows,
                    stock_item_count,
                    stock_item_strings,
                    string_rows,
                    stock_collectibles,
                    collectible_rows,
                    stock_collectible_count,
                    stock_collectible_displays,
                    stock_nodes,
                    stock_item_icons,
                )
            })
            .transpose()?;
        let damage_carrier_source = if weapon_damage_carrier(&definition)?.family().is_none()
            && weapon
                .overrides
                .modern_damage_type
                .is_some_and(|damage| damage != ModernDamageType::Kinetic)
        {
            resolve_added_damage_carrier_source(
                manager,
                stock_sandbox_patterns,
                stock_entity_assignments,
                stock_item_table,
                item_rows,
                stock_item_count,
                runtime_pattern_source,
                &definition,
                // Kinetic-slot elements use the same proven carrier source as Energy conversion.
                if authored_inventory_slot == WeaponInventorySlot::Kinetic {
                    WeaponInventorySlot::Energy
                } else {
                    authored_inventory_slot
                },
                presentation_donor
                    .as_ref()
                    .map(|presentation| presentation.definition.as_slice()),
            )?
        } else {
            None
        };
        let gear_art_pattern_index = if let Some(presentation) = &presentation_donor {
            Some(
                weapon_pattern_index(&presentation.definition)?.ok_or_else(|| {
                    invalid("Geometry donor has no active gear-art/runtime row")
                })?,
            )
        } else {
            gameplay_pattern_index
        };
        let gear_art_pattern_source = gear_art_pattern_index
            .map(|row_index| sandbox_pattern_source_at(stock_sandbox_patterns, row_index))
            .transpose()?;
        if gear_art_pattern_source.is_none() && runtime_pattern_source.is_some() {
            return Err(invalid(
                "Runtime baseline cannot be attached because the appearance baseline has no gear-art/runtime row",
            ));
        }
        if gear_art_pattern_source.is_some() && runtime_pattern_source.is_none() {
            return Err(invalid(
                "Appearance baseline has a gear-art/runtime row, but the gameplay runtime baseline is disabled",
            ));
        }
        if let (Some(gear_art), Some(runtime)) =
            (gear_art_pattern_source, runtime_pattern_source)
            && sundial::package_authoring::native_weapon::animation_compatibility(
                Some(gear_art.weapon_translation_group_hash), Some(runtime.weapon_translation_group_hash)
            ) != sundial::package_authoring::native_weapon::AnimationCompatibility::Compatible
        {
            return Err(invalid(format!(
                "Geometry and runtime donors use different weapon translation groups (0x{:08X} vs 0x{:08X}); this combination cannot produce a coherent equipped model",
                gear_art.weapon_translation_group_hash, runtime.weapon_translation_group_hash
            )));
        }
        let icon_donor = weapon
            .icon_donor
            .as_ref()
            .map(|reference| {
                resolve_icon_donor(
                    manager,
                    reference,
                    stock_item_table,
                    item_rows,
                    stock_item_count,
                    stock_item_strings,
                    string_rows,
                    stock_item_icons,
                )
            })
            .transpose()?;
        let render_gear_donor = weapon
            .render_gear_donor
            .as_ref()
            .map(|reference| {
                resolve_render_gear_donor(
                    manager,
                    reference,
                    stock_item_table,
                    item_rows,
                    stock_item_count,
                    stock_item_strings,
                    string_rows,
                )
            })
            .transpose()?;
        let runtime_component_donors = weapon
            .runtime_component_donors
            .iter()
            .map(|reference| {
                resolve_runtime_component_donor(
                    manager,
                    reference,
                    stock_item_table,
                    item_rows,
                    stock_item_count,
                    stock_item_strings,
                    string_rows,
                    stock_sandbox_patterns,
                )
            })
            .collect::<AuthoringResult<Vec<_>>>()?;
        if let Some(presentation) = &presentation_donor {
            // Family is checked against the catalog, and the native translation group above
            // must match. Collections placement follows authored rarity independently.
            if presentation.inventory_slot != authored_inventory_slot
                && !matches!(
                    (presentation.inventory_slot, authored_inventory_slot),
                    (WeaponInventorySlot::Kinetic, WeaponInventorySlot::Energy)
                        | (WeaponInventorySlot::Energy, WeaponInventorySlot::Kinetic)
                )
            {
                return Err(invalid(format!(
                    "Geometry donor uses {:?}, but the authored weapon targets {authored_inventory_slot:?}",
                    presentation.inventory_slot
                )));
            }
        }
        let socket_column_indices = resolve_socket_column_indices(
            stock_item_rows_by_hash,
            &definition,
            &weapon.overrides.socket_columns,
        )?;
        let inherited_icon = presentation_donor.as_ref().map_or(
            ResolvedIconDonor {
                item_index: donor_item_index,
                icon_index: donor_icon_index,
                icon_container: donor_icon_container,
            },
            |presentation| ResolvedIconDonor {
                item_index: presentation.item_index,
                icon_index: presentation.icon_index,
                icon_container: presentation.icon_container,
            },
        );
        let selected_icon = icon_donor.unwrap_or(inherited_icon);
            Ok(ResolvedWeapon {
                weapon: weapon.clone(),
                donor_item_index,
                donor_collectible_index,
                collection_donor_index,
                source_unlock_index,
                source_acquired_flag,
                definition_tag,
                string_tag,
                definition,
                strings,
                icon_template_item_index: selected_icon.item_index,
                donor_icon_index: selected_icon.icon_index,
                donor_icon_container: selected_icon.icon_container,
                presentation_donor,
                damage_carrier_source,
                render_gear_donor,
                runtime_component_donors,
                runtime_pattern_source,
                gear_art_pattern_source,
                weapon_page,
                count_selection,
                socket_column_indices,
            })
        })()
        .map_err(|error| {
            error.context(format!(
                "Weapon {:?} ({})",
                weapon.text.name, weapon.namespace
            ))
        })?;
        resolved.push(resolved_weapon);
    }

    Ok(resolved)
}

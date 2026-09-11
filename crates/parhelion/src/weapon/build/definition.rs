use super::*;

pub(super) fn apply_scalar_overrides(
    definition: &mut Vec<u8>,
    overrides: &WeaponCloneOverrides,
) -> AuthoringResult<()> {
    if !overrides.investment_stats.is_empty() || !overrides.removed_investment_stats.is_empty() {
        set_weapon_stats(
            definition,
            &overrides.investment_stats,
            &overrides.removed_investment_stats,
        )?;
    }
    if let Some(traits) = &overrides.trait_indices {
        set_weapon_item_traits(definition, traits)?;
    }
    if let Some(max_stack_size) = overrides.max_stack_size {
        set_weapon_max_stack_size(definition, max_stack_size)?;
    }
    if let Some(index) = overrides.socket_entry_list_index {
        set_weapon_socket_entry_list_index(definition, index)?;
    }
    if let Some(hash) = overrides.plug_category_hash {
        set_weapon_plug_category_hash(definition, hash)?;
    }
    if let Some(index) = overrides.roll_set_index {
        set_weapon_roll_set_index(definition, index)?;
    }
    if let Some(index) = overrides.linked_plug_index {
        set_weapon_linked_plug_index(definition, index)?;
    }
    if let Some(group) = overrides.power_cap_group {
        set_weapon_power_cap(definition, group)?;
    }
    if let Some(groups) = &overrides.power_cap_groups {
        set_weapon_power_cap_groups(definition, groups)?;
    }
    Ok(())
}

pub(super) fn apply_socket_overrides(
    definition: &mut Vec<u8>,
    donor: &resolve::ResolvedWeapon,
    custom_plugs: &[ResolvedCustomPlug],
    ordinal: usize,
) -> AuthoringResult<Vec<Option<ResolvedSocketColumn>>> {
    let mut expected_socket_columns = donor.socket_column_indices.clone();
    for (custom_plug, usage) in custom_plugs
        .iter()
        .flat_map(|plug| plug.uses.iter().map(move |usage| (plug, usage)))
        .filter(|(_, usage)| usage.weapon_ordinal == ordinal)
    {
        let source_item_index = u16::try_from(custom_plug.source_item_index)
            .map_err(|_| invalid("Private socket-plug donor index does not fit 16 bits"))?;
        if let Some(Some(column)) = expected_socket_columns.get_mut(usage.socket_index) {
            let selected = column.choices.get_mut(usage.choice_index).ok_or_else(|| {
                invalid(format!(
                    "Private socket {} choice {} is outside the authored column",
                    usage.socket_index, usage.choice_index
                ))
            })?;
            if *selected != source_item_index {
                return Err(invalid(format!(
                    "Private socket {} choice {} does not select source plug 0x{:08X}",
                    usage.socket_index, usage.choice_index, custom_plug.source_item_hash
                )));
            }
            *selected = custom_plug.authored_item_index;
        } else {
            replace_socket_choice_item_index(
                definition,
                usage.socket_index,
                usage.choice_index,
                source_item_index,
                custom_plug.authored_item_index,
            )?;
        }
    }
    // Resolve private choices before writing the column. Distinct variants can share a stock
    // source while their final native item indices must remain unique.
    if !expected_socket_columns.is_empty() {
        set_weapon_socket_columns(definition, &expected_socket_columns)?;
    }
    Ok(expected_socket_columns)
}

pub(super) fn apply_presentation(
    definition: &mut Vec<u8>,
    strings: &mut [u8],
    donor: &resolve::ResolvedWeapon,
    donor_inventory_slot: WeaponInventorySlot,
    authored_icon_index: u16,
) -> AuthoringResult<u16> {
    let identity = donor.weapon.identity;
    let authored_inventory_slot = weapon_inventory_slot(definition)?;
    if let Some(presentation) = &donor.presentation_donor {
        transplant_weapon_geometry(definition, &presentation.definition)?;
        transplant_item_string_client_classification(
            strings,
            donor_inventory_slot,
            &presentation.strings,
            presentation.inventory_slot,
            authored_inventory_slot,
        )?;
    } else if authored_inventory_slot != donor_inventory_slot {
        // Retaining the original geometry also retains its type keys and animations.
        // The native definition's bucket and equipment slot were updated earlier.
        // Here, update the matching client classification without changing geometry.
        set_item_string_inventory_slot(strings, donor_inventory_slot, authored_inventory_slot)?;
    }
    if let Some(render_gear) = &donor.render_gear_donor {
        transplant_weapon_render_gear(definition, &render_gear.definition)?;
    } else if let Some(presentation) = &donor.presentation_donor {
        transplant_weapon_render_gear(definition, &presentation.definition)?;
    }
    if let Some(rows) = &donor.weapon.overrides.art_arrangements {
        set_weapon_art_arrangements(definition, rows)?;
    }
    if let Some(arrays) = &donor.weapon.overrides.render_dye_rows {
        set_weapon_render_dye_rows(definition, arrays)?;
    } else if donor.has_authored_shader {
        unlock_weapon_shader_dyes(definition)?;
    }
    // Structured geometry and render-dye overrides intentionally win over their donors.
    if let Some(rarity) = donor.weapon.overrides.rarity {
        set_weapon_rarity(definition, rarity)?;
    }
    if donor.weapon.overrides.rarity.is_some()
        || weapon_rarity(definition)? != AuthoredWeaponRarity::Exotic
    {
        sync_weapon_equipment_rarity(definition)?;
    }
    let equipment_label = weapon_equipment_label(definition)?;
    let collection_material_set = collection_material_set_for_rarity(weapon_rarity(definition)?);
    if let Some(stat_group_index) = donor.weapon.overrides.stat_group_index {
        set_item_string_stat_group_index(strings, stat_group_index)?;
    }
    if let Some(ammo_type) = donor.weapon.overrides.ammo_type {
        set_item_string_ammo_type(strings, ammo_type)?;
    }
    write_u16(strings, ITEM_STRING_ICON_INDEX_OFFSET, authored_icon_index)?;
    clear_item_string_watermark_overrides(strings)?;
    write_localized_reference(
        strings,
        ITEM_NAME_REFERENCE_OFFSET,
        LOCALIZATION_DONOR_TABLE_INDEX as u32,
        identity.name_hash,
    )?;
    if donor.weapon.text.type_name.is_some() {
        write_localized_reference(
            strings,
            ITEM_TYPE_REFERENCE_OFFSET,
            LOCALIZATION_DONOR_TABLE_INDEX as u32,
            identity.type_hash,
        )?;
    }
    write_localized_reference(
        strings,
        ITEM_DESCRIPTION_REFERENCE_OFFSET,
        LOCALIZATION_DONOR_TABLE_INDEX as u32,
        identity.flavor_hash,
    )?;
    // The item-string displaySource is the inventory tooltip's independent acquisition
    // hint. It remains blank by default; Collections Source uses the collectible row.
    if donor.weapon.text.inventory_hint.is_some() {
        write_localized_reference(
            strings,
            ITEM_DISPLAY_SOURCE_REFERENCE_OFFSET,
            LOCALIZATION_DONOR_TABLE_INDEX as u32,
            identity.inventory_hint_hash,
        )?;
    } else {
        write_localized_reference(
            strings,
            ITEM_DISPLAY_SOURCE_REFERENCE_OFFSET,
            BLANK_LOCALIZED_REFERENCE_TABLE_INDEX,
            BLANK_LOCALIZED_REFERENCE_HASH,
        )?;
    }
    item_string_client_classification(strings, authored_inventory_slot)?;
    apply_weapon_raw_payload_patches(
        definition,
        strings,
        &donor.weapon.overrides.raw_payload_patches,
    )?;
    if weapon_equipment_label(definition)? != equipment_label {
        return Err(invalid(
            "Raw equipment patches changed the rarity-controlled unique-equip restriction",
        ));
    }

    Ok(collection_material_set)
}

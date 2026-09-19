use super::*;

#[allow(clippy::too_many_arguments)]
pub(in crate::weapon) fn resolve_weapon_collection_donor(
    manager: &PackageManager,
    items: &[u8],
    item_rows: usize,
    item_count: usize,
    item_strings: &[u8],
    string_rows: usize,
    collectibles: &[u8],
    collectible_rows: usize,
    collectible_count: usize,
    sandbox_patterns: &[u8],
    gameplay_collectible: Option<usize>,
    gameplay: &[u8],
    gameplay_strings: &[u8],
    rarity: AuthoredWeaponRarity,
    slot: WeaponInventorySlot,
) -> AuthoringResult<Vec<usize>> {
    let exotic = rarity == AuthoredWeaponRarity::Exotic;
    let prefer_gameplay = (weapon_rarity(gameplay)? == AuthoredWeaponRarity::Exotic) == exotic
        && (!exotic || weapon_inventory_slot(gameplay)? == slot);
    let family_hash = read_u32(gameplay_strings, ITEM_TYPE_REFERENCE_OFFSET + 4)?;
    let fallback_group = if !exotic && gameplay_collectible.is_none() {
        weapon_pattern_index(gameplay)?
            .map(|index| {
                sandbox_pattern_source_at(sandbox_patterns, index)
                    .map(|source| source.weapon_translation_group_hash)
            })
            .transpose()?
    } else {
        None
    };
    let mut fallback_candidates = Vec::new();
    // A native collectible can belong to the right page without contributing to
    // its acquired-count pools. Keep compatible alternatives for placement validation.
    let mut candidates = if prefer_gameplay {
        gameplay_collectible.into_iter().collect()
    } else {
        Vec::new()
    };
    for index in 0..collectible_count {
        if prefer_gameplay && Some(index) == gameplay_collectible {
            continue;
        }
        let item = usize::from(read_u16(
            collectibles,
            collectible_rows + index * COLLECTIBLE_ROW_SIZE + COLLECTIBLE_ITEM_INDEX_OFFSET,
        )?);
        if item >= item_count {
            continue;
        }
        let definition = read_tag(
            manager,
            TagHash(read_u32(items, item_rows + item * ITEM_ROW_SIZE + 16)?),
            "Collections placement exemplar",
        )?;
        let Ok(candidate_rarity) = weapon_rarity(&definition) else {
            continue;
        };
        if (candidate_rarity == AuthoredWeaponRarity::Exotic) != exotic {
            continue;
        }
        let Ok(candidate_slot) = weapon_inventory_slot(&definition) else {
            continue;
        };
        if exotic {
            if candidate_slot != slot {
                continue;
            }
        } else {
            let strings = read_tag(
                manager,
                TagHash(read_u32(
                    item_strings,
                    string_rows + item * ITEM_ROW_SIZE + 16,
                )?),
                "Collections exemplar type",
            )?;
            if read_u32(&strings, ITEM_TYPE_REFERENCE_OFFSET + 4)? != family_hash {
                // Some collectible-free variants have no family text. Their native
                // translation group can still identify compatible stock page exemplars.
                if let Some(group) = fallback_group
                    && let Some(pattern) = weapon_pattern_index(&definition)?
                    && sandbox_pattern_source_at(sandbox_patterns, pattern)?
                        .weapon_translation_group_hash
                        == group
                {
                    fallback_candidates.push(index);
                }
                continue;
            }
        }
        candidates.push(index);
    }
    if candidates.is_empty() {
        candidates = fallback_candidates;
    }
    if !candidates.is_empty() {
        return Ok(candidates);
    }
    Err(invalid(if exotic {
        format!("No stock Exotics Collections page is available for {slot:?} weapons")
    } else {
        "This weapon family has no non-Exotic Collections page in this game version. Keep Exotic rarity or choose another gameplay family.".to_owned()
    }))
}

pub(in crate::weapon) fn weapon_rarity(data: &[u8]) -> AuthoringResult<AuthoredWeaponRarity> {
    // Rarity is a root item field, but require the verified weapon translation topology so an
    // arbitrary item-like payload cannot pass through the weapon-only authoring path.
    weapon_translation_topology(data)?;
    AuthoredWeaponRarity::from_package_value(read_u8(data, ITEM_RARITY_OFFSET)?)
}

pub(in crate::weapon) const fn collection_material_set_for_rarity(
    rarity: AuthoredWeaponRarity,
) -> u16 {
    match rarity {
        AuthoredWeaponRarity::Exotic => COLLECTIBLE_EXOTIC_WEAPON_MATERIAL_SET,
        AuthoredWeaponRarity::Common
        | AuthoredWeaponRarity::Uncommon
        | AuthoredWeaponRarity::Rare
        | AuthoredWeaponRarity::Legendary => COLLECTIBLE_CURATED_WEAPON_MATERIAL_SET,
    }
}

pub(in crate::weapon) fn set_weapon_rarity(
    data: &mut [u8],
    rarity: AuthoredWeaponRarity,
) -> AuthoringResult<()> {
    let original = weapon_rarity(data)?.package_value();
    let before = data.to_vec();

    write_bytes(data, ITEM_RARITY_OFFSET, &[rarity.package_value()])?;
    if weapon_rarity(data)? != rarity {
        return Err(validation(
            "Authored weapon did not retain the requested rarity tier",
        ));
    }

    let mut normalized = data.to_vec();
    write_bytes(&mut normalized, ITEM_RARITY_OFFSET, &[original])?;
    if normalized != before {
        return Err(validation(
            "Weapon rarity authoring changed bytes outside the audited one-byte field",
        ));
    }
    Ok(())
}

// Shadowkeep equipment blocks carry the FNV-1 label separately from the display tier.
// `exotic_weapon` gates equipping across all three weapon slots. Its companion hash
// selects the restriction text. Ordinary weapons use the empty-string hash and zero.
const EQUIPMENT_UNIQUE_LABEL_OFFSET: usize = 0x10;
const EMPTY_EQUIPMENT_LABEL: u32 = 0x811C_9DC5;
pub(in crate::weapon) const EXOTIC_WEAPON_EQUIPMENT_LABEL: u32 = 0xDAD1_1536;
const EXOTIC_WEAPON_EQUIPMENT_TEXT: u32 = 0xEF7B_6AD3;

pub(in crate::weapon) fn weapon_equipment_label(data: &[u8]) -> AuthoringResult<(u32, u32)> {
    weapon_equipment_slot(data)?;
    let block = relative_target(data, ITEM_EQUIPMENT_BLOCK_POINTER_OFFSET)?;
    Ok((
        read_u32(data, block + EQUIPMENT_UNIQUE_LABEL_OFFSET)?,
        read_u32(data, block + EQUIPMENT_UNIQUE_LABEL_OFFSET + 4)?,
    ))
}

pub(in crate::weapon) fn sync_weapon_equipment_rarity(data: &mut [u8]) -> AuthoringResult<()> {
    let rarity = weapon_rarity(data)?;
    let original = weapon_equipment_label(data)?;
    let expected = if rarity == AuthoredWeaponRarity::Exotic {
        (EXOTIC_WEAPON_EQUIPMENT_LABEL, EXOTIC_WEAPON_EQUIPMENT_TEXT)
    } else if matches!(
        original.0,
        EXOTIC_WEAPON_EQUIPMENT_LABEL | EMPTY_EQUIPMENT_LABEL
    ) {
        (EMPTY_EQUIPMENT_LABEL, 0)
    } else {
        // Preserve unrelated unique-equip groups rather than removing all restrictions.
        original
    };
    let block = relative_target(data, ITEM_EQUIPMENT_BLOCK_POINTER_OFFSET)?;
    write_u32(data, block + EQUIPMENT_UNIQUE_LABEL_OFFSET, expected.0)?;
    write_u32(data, block + EQUIPMENT_UNIQUE_LABEL_OFFSET + 4, expected.1)?;
    Ok(())
}

pub(in crate::weapon) fn weapon_version_array(
    data: &[u8],
) -> AuthoringResult<Option<ItemVersionArray>> {
    item_version_array(data).map_err(invalid)
}

pub(in crate::weapon) fn set_weapon_power_cap(
    data: &mut [u8],
    power_cap_group: u16,
) -> AuthoringResult<()> {
    let version = weapon_version_array(data)?
        .ok_or_else(|| invalid("Weapon quality block has no version rows"))?;
    for index in 0..version.groups.len() {
        write_u16(
            data,
            version.rows + index * ITEM_VERSION_ROW_SIZE,
            power_cap_group,
        )?;
    }
    let authored = weapon_version_array(data)?;
    if authored.as_ref().is_none_or(|authored| {
        authored.groups.is_empty()
            || authored
                .groups
                .iter()
                .any(|group| *group != power_cap_group)
    }) {
        return Err(validation(
            "Authored weapon did not retain the requested power-cap group",
        ));
    }
    Ok(())
}

pub(in crate::weapon) fn set_weapon_power_cap_groups(
    data: &mut [u8],
    groups: &[u16],
) -> AuthoringResult<()> {
    let version = weapon_version_array(data)?
        .ok_or_else(|| invalid("Weapon quality block has no version rows"))?;
    if groups.len() != version.groups.len() {
        return Err(invalid(format!(
            "Advanced power-cap override has {} version rows but the gameplay donor has {}",
            groups.len(),
            version.groups.len()
        )));
    }
    if groups.is_empty() {
        return Err(invalid(
            "Advanced power-cap groups must contain at least one table index",
        ));
    }
    for (index, group) in groups.iter().copied().enumerate() {
        write_u16(data, version.rows + index * ITEM_VERSION_ROW_SIZE, group)?;
    }
    if weapon_version_array(data)?.is_none_or(|authored| authored.groups != groups) {
        return Err(validation(
            "Authored weapon did not retain every requested power-cap version row",
        ));
    }
    Ok(())
}

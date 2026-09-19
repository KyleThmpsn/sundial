use super::*;

pub(in crate::weapon) const fn fixed_damage_perk(
    perk: u16,
) -> Option<(WeaponDamageCarrierFamily, ModernDamageType)> {
    use sundial::package_authoring::native_weapon::{DamageFamily, Element, fixed_damage_marker};
    match fixed_damage_marker(perk) {
        Some((family, element)) => Some((
            match family {
                DamageFamily::Legacy => WeaponDamageCarrierFamily::LegacyFixed,
                DamageFamily::Modern => WeaponDamageCarrierFamily::ModernFixed,
            },
            match element {
                Element::Arc => ModernDamageType::Arc,
                Element::Solar => ModernDamageType::Solar,
                Element::Void => ModernDamageType::Void,
            },
        )),
        None => None,
    }
}

pub(in crate::weapon) const fn damage_plug_type(index: u16) -> Option<ModernDamageType> {
    match index {
        ARC_DAMAGE_PLUG_ITEM_INDEX => Some(ModernDamageType::Arc),
        SOLAR_DAMAGE_PLUG_ITEM_INDEX => Some(ModernDamageType::Solar),
        VOID_DAMAGE_PLUG_ITEM_INDEX => Some(ModernDamageType::Void),
        _ => None,
    }
}

pub(in crate::weapon) fn weapon_damage_socket_lanes(
    data: &[u8],
) -> AuthoringResult<Vec<(usize, usize)>> {
    let resource = relative_target(data, ITEM_ORDINARY_SOCKET_POINTER_OFFSET)?;
    let (count, _, rows, class) = array_at(data, resource)?;
    if class != ITEM_ORDINARY_SOCKET_ROW_CLASS || count > 64 {
        return Err(invalid("Weapon ordinary-socket rows are incompatible"));
    }
    let end = rows
        .checked_add(count * ITEM_ORDINARY_SOCKET_ROW_SIZE)
        .ok_or_else(|| invalid("Weapon ordinary-socket row extent overflowed"))?;
    if end > data.len() {
        return Err(invalid("Weapon ordinary-socket rows are truncated"));
    }
    Ok((0..count)
        .filter_map(|lane| {
            let row = rows + lane * ITEM_ORDINARY_SOCKET_ROW_SIZE;
            (read_u16(data, row).ok() == Some(ELEMENTAL_DAMAGE_SOCKET_TYPE)).then_some((lane, row))
        })
        .collect())
}

pub(in crate::weapon) fn weapon_damage_carrier(
    data: &[u8],
) -> AuthoringResult<WeaponDamageCarrier> {
    use sundial::package_authoring::native_weapon::{BaseDamage, classify_fixed_damage};
    let perks = weapon_sandbox_perks(data)?;
    let fixed = match classify_fixed_damage(&perks) {
        BaseDamage::Fixed(_, _) => perks.iter().find_map(|perk| fixed_damage_perk(*perk)),
        BaseDamage::NoMarker => None,
        BaseDamage::Duplicate | BaseDamage::Variable => {
            return Err(invalid(
                "Weapon base sandbox-perk array contains duplicate or variable elemental damage markers",
            ));
        }
    };
    let plug_lanes = weapon_damage_socket_lanes(data)?;
    if plug_lanes.len() > 1 {
        return Err(invalid(
            "Weapon has more than one native type-68 elemental carrier socket",
        ));
    }
    if fixed.is_some() && !plug_lanes.is_empty() {
        return Err(invalid(
            "Weapon duplicates its elemental carrier in both the parent and a type-68 plug socket",
        ));
    }
    if let Some((family, damage_type)) = fixed {
        return Ok(WeaponDamageCarrier::Fixed {
            family,
            damage_type,
        });
    }
    if let Some(&(lane, row)) = plug_lanes.first() {
        let default = read_u16(data, row + ITEM_ORDINARY_SOCKET_DEFAULT_PLUG_OFFSET)?;
        let damage_type = damage_plug_type(default).ok_or_else(|| {
            invalid(format!(
                "Weapon type-68 elemental carrier lane {lane} has unsupported default plug index {default}"
            ))
        })?;
        return Ok(WeaponDamageCarrier::PlugDriven { damage_type, lane });
    }
    Ok(WeaponDamageCarrier::Empty)
}

pub(in crate::weapon) fn weapon_damage_descriptor(
    data: &[u8],
) -> AuthoringResult<WeaponDamageDescriptor> {
    Ok(weapon_damage_carrier(data)?.descriptor())
}

pub(in crate::weapon) fn set_weapon_fixed_damage_type(
    data: &mut Vec<u8>,
    damage_type: ModernDamageType,
    family: WeaponDamageCarrierFamily,
    row_template: &[u8; ITEM_SANDBOX_PERK_ROW_SIZE],
) -> AuthoringResult<()> {
    if fixed_damage_perk(read_u16(row_template, 0)?).is_none() {
        return Err(invalid(
            "Sandbox-perk row template is not a recognized elemental row",
        ));
    }
    let mut perks = weapon_sandbox_perks(data)?;
    let existing = perks
        .iter()
        .position(|perk| fixed_damage_perk(*perk).is_some());
    let requested = family.base_sandbox_perk_index(damage_type.shared());
    match (existing, requested) {
        (Some(index), Some(requested)) => perks[index] = requested,
        (None, Some(requested)) => perks.push(requested),
        (Some(index), None) => {
            perks.remove(index);
        }
        (None, None) => {}
    }
    set_weapon_base_sandbox_perks(data, &perks, row_template)?;
    if damage_type == ModernDamageType::Kinetic {
        if weapon_sandbox_perks(data)?
            .into_iter()
            .any(|perk| fixed_damage_perk(perk).is_some())
        {
            return Err(validation(
                "Authored weapon retained a fixed elemental damage perk",
            ));
        }
        return Ok(());
    }
    if weapon_damage_carrier(data)?
        != (WeaponDamageCarrier::Fixed {
            family,
            damage_type,
        })
    {
        return Err(validation(
            "Authored weapon did not retain the requested damage perk",
        ));
    }
    Ok(())
}

pub(in crate::weapon) fn validate_damage_plug_item_rows(
    item_table: &[u8],
    rows: usize,
    count: usize,
) -> AuthoringResult<()> {
    for (index, hash) in [
        (ARC_DAMAGE_PLUG_ITEM_INDEX, ARC_DAMAGE_PLUG_ITEM_HASH),
        (SOLAR_DAMAGE_PLUG_ITEM_INDEX, SOLAR_DAMAGE_PLUG_ITEM_HASH),
        (VOID_DAMAGE_PLUG_ITEM_INDEX, VOID_DAMAGE_PLUG_ITEM_HASH),
    ] {
        let index = usize::from(index);
        if index >= count || read_u32(item_table, rows + index * ITEM_ROW_SIZE)? != hash {
            return Err(invalid(format!(
                "Installed elemental carrier plug index {index} does not resolve to 0x{hash:08X}"
            )));
        }
    }
    Ok(())
}

pub(in crate::weapon) fn type_68_row_is_self_contained(
    data: &[u8],
    row: usize,
) -> AuthoringResult<bool> {
    for descriptor in [
        row + ITEM_ORDINARY_SOCKET_RANDOMIZED_SELECTION_PROGRAM_OFFSET,
        row + ITEM_ORDINARY_SOCKET_EMBEDDED_PLUGS_OFFSET,
    ] {
        if read_u64(data, descriptor)? != 0 || read_i64(data, descriptor + 8)? != 0 {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(in crate::weapon) fn set_weapon_plug_damage_type(
    data: &mut Vec<u8>,
    damage_type: ModernDamageType,
    topology_donor: Option<&[u8]>,
    row_template: &[u8; ITEM_SANDBOX_PERK_ROW_SIZE],
) -> AuthoringResult<()> {
    if damage_type == ModernDamageType::Kinetic {
        return Err(invalid(
            "A type-68 elemental carrier cannot represent Kinetic damage",
        ));
    }
    let damage_lanes = weapon_damage_socket_lanes(data)?;
    let [(lane, row)] = damage_lanes.as_slice() else {
        return Err(invalid(
            "Plug-driven damage requires exactly one native type-68 carrier lane in the gameplay definition; Parhelion will not repurpose a disabled, mod, or unrelated socket",
        ));
    };
    let lane = *lane;
    let row = *row;
    if let Some(topology_donor) = topology_donor {
        let topology_lanes = weapon_damage_socket_lanes(topology_donor)?;
        let [(_, source_row)] = topology_lanes.as_slice() else {
            return Err(invalid(
                "The target-slot presentation donor does not have one unambiguous type-68 carrier lane",
            ));
        };
        if !type_68_row_is_self_contained(topology_donor, *source_row)? {
            return Err(invalid(
                "The target-slot type-68 carrier uses pointer-backed choices that cannot be transplanted independently",
            ));
        }
        let source = topology_donor
            .get(*source_row..*source_row + ITEM_ORDINARY_SOCKET_ROW_SIZE)
            .ok_or_else(|| invalid("Target-slot type-68 carrier row is truncated"))?;
        write_bytes(data, row, source)?;
    }
    write_u16(
        data,
        row + ITEM_ORDINARY_SOCKET_DEFAULT_PLUG_OFFSET,
        WeaponDamageCarrierFamily::PlugDriven
            .default_plug_item_index(damage_type.shared())
            .ok_or_else(|| invalid("Requested damage has no stock type-68 carrier plug"))?,
    )?;
    set_weapon_fixed_damage_type(
        data,
        ModernDamageType::Kinetic,
        WeaponDamageCarrierFamily::ModernFixed,
        row_template,
    )?;
    if weapon_damage_carrier(data)? != (WeaponDamageCarrier::PlugDriven { damage_type, lane }) {
        return Err(validation(
            "Authored weapon did not retain its requested plug-driven damage carrier",
        ));
    }
    Ok(())
}

pub(in crate::weapon) fn apply_weapon_slot_and_damage_overrides(
    data: &mut Vec<u8>,
    strings: &mut Vec<u8>,
    overrides: &WeaponCloneOverrides,
    target_slot_carrier: Option<&ResolvedDamageCarrierSource>,
    sandbox_perk_definition_template: &[u8; ITEM_SANDBOX_PERK_ROW_SIZE],
    sandbox_perk_string_template: &[u8],
) -> AuthoringResult<()> {
    if overrides.inventory_slot.is_none()
        && overrides.modern_damage_type.is_none()
        && overrides.base_sandbox_perks.is_none()
    {
        return Ok(());
    }
    validate_weapon_sandbox_perk_parallelism(data, strings, sandbox_perk_string_template)?;

    let donor_slot = weapon_inventory_slot(data)?;
    if weapon_equipment_slot(data)? != donor_slot {
        return Err(invalid(
            "Weapon inventory bucket and equipment slot disagree",
        ));
    }
    let donor_carrier = weapon_damage_carrier(data)?;

    if let Some(perks) = &overrides.base_sandbox_perks {
        set_weapon_base_sandbox_perks_with_strings(
            data,
            strings,
            perks,
            sandbox_perk_definition_template,
            sandbox_perk_string_template,
        )?;
    }

    let authored_slot = overrides.inventory_slot.unwrap_or(donor_slot);
    let base_damage = weapon_damage_descriptor(data)?;
    let requested_damage = overrides
        .modern_damage_type
        .map_or(base_damage, |damage_type| {
            if damage_type == ModernDamageType::Kinetic {
                WeaponDamageDescriptor::Empty
            } else {
                WeaponDamageDescriptor::Elemental(damage_type)
            }
        });

    if let Some(inventory_slot) = overrides.inventory_slot {
        set_weapon_inventory_slot(data, inventory_slot)?;
    }
    if let Some(damage_type) = overrides.modern_damage_type {
        // Damage topology follows the gameplay definition, not its inventory placement.
        // A stock source is needed only when introducing a previously absent carrier.
        let carrier_family = donor_carrier
            .family()
            .or_else(|| target_slot_carrier.map(|source| source.family));
        match (damage_type, carrier_family) {
            (ModernDamageType::Kinetic, _) => {
                if !weapon_damage_socket_lanes(data)?.is_empty() {
                    return Err(invalid(
                        "The gameplay definition carries elemental damage through a type-68 socket; removing that carrier for Kinetic damage is not package-proven",
                    ));
                }
                set_weapon_fixed_damage_type(
                    data,
                    damage_type,
                    WeaponDamageCarrierFamily::ModernFixed,
                    sandbox_perk_definition_template,
                )?;
            }
            (_, Some(WeaponDamageCarrierFamily::PlugDriven)) => {
                set_weapon_plug_damage_type(
                    data,
                    damage_type,
                    target_slot_carrier.and_then(|source| source.topology_definition.as_deref()),
                    sandbox_perk_definition_template,
                )?;
            }
            (
                _,
                Some(
                    family @ (WeaponDamageCarrierFamily::LegacyFixed
                    | WeaponDamageCarrierFamily::ModernFixed),
                ),
            ) => {
                if !weapon_damage_socket_lanes(data)?.is_empty() {
                    return Err(invalid(
                        "The gameplay definition already has a type-68 elemental carrier; adding a parent damage marker would duplicate the carrier",
                    ));
                }
                set_weapon_fixed_damage_type(
                    data,
                    damage_type,
                    family,
                    sandbox_perk_definition_template,
                )?;
            }
            (_, None) => {
                return Err(invalid(
                    "No compatible stock target-slot damage carrier topology was found",
                ));
            }
        }
    }
    let actual_damage = weapon_damage_descriptor(data)?;
    set_item_string_sandbox_perk_count(
        strings,
        weapon_sandbox_perks(data)?.len(),
        sandbox_perk_string_template,
    )?;
    if weapon_inventory_slot(data)? != authored_slot || actual_damage != requested_damage {
        return Err(validation(
            "Authored weapon slot/damage pair does not match its requested values",
        ));
    }
    validate_weapon_sandbox_perk_parallelism(data, strings, sandbox_perk_string_template)?;
    Ok(())
}

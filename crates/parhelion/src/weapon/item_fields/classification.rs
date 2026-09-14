use super::*;

pub(crate) fn weapon_inventory_slot(data: &[u8]) -> AuthoringResult<WeaponInventorySlot> {
    let root_value = *data
        .get(ITEM_INVENTORY_SLOT_OFFSET)
        .ok_or_else(|| invalid("Weapon inventory-slot root byte is unavailable"))?;
    let root_slot = WeaponInventorySlot::from_root_value(root_value).ok_or_else(|| {
        invalid(format!(
            "Weapon inventory-slot root byte {root_value} is not Kinetic, Energy, or Power"
        ))
    })?;
    Ok(root_slot)
}

pub(crate) fn weapon_equipment_slot(data: &[u8]) -> AuthoringResult<WeaponInventorySlot> {
    let block = relative_target(data, ITEM_EQUIPMENT_BLOCK_POINTER_OFFSET)?;
    if block < 4 || read_u32(data, block - 4)? != ITEM_EQUIPMENT_BLOCK_CLASS {
        return Err(invalid("Weapon has no recognized equipment-slot block"));
    }
    let equipment_value = read_u16(data, block + ITEM_EQUIPMENT_SLOT_OFFSET)?;
    let sentinel = read_u16(data, block + ITEM_EQUIPMENT_SLOT_SENTINEL_OFFSET)?;
    if sentinel != u16::MAX {
        return Err(invalid(
            "Weapon equipment-slot value is not followed by the native FFFF sentinel",
        ));
    }
    let equipment_slot =
        WeaponInventorySlot::from_equipment_value(equipment_value).ok_or_else(|| {
            invalid(format!(
                "Weapon equipment-slot value {equipment_value} is not Kinetic, Energy, or Power"
            ))
        })?;
    Ok(equipment_slot)
}

pub(in crate::weapon) fn set_weapon_inventory_slot(
    data: &mut [u8],
    inventory_slot: WeaponInventorySlot,
) -> AuthoringResult<()> {
    // Parse the donor pair before writing so an old-style or ambiguous block is never normalized
    // into something that merely looks authorable.
    let donor_inventory_slot = weapon_inventory_slot(data)?;
    let donor_equipment_slot = weapon_equipment_slot(data)?;
    if donor_inventory_slot != donor_equipment_slot {
        return Err(invalid(format!(
            "Weapon has a {donor_inventory_slot:?} inventory bucket but a {donor_equipment_slot:?} equipment slot"
        )));
    }
    let block = relative_target(data, ITEM_EQUIPMENT_BLOCK_POINTER_OFFSET)?;
    write_bytes(
        data,
        ITEM_INVENTORY_SLOT_OFFSET,
        &[inventory_slot.root_value()],
    )?;
    write_u16(
        data,
        block + ITEM_EQUIPMENT_SLOT_OFFSET,
        inventory_slot.equipment_value(),
    )?;
    if read_u16(data, block + ITEM_EQUIPMENT_SLOT_SENTINEL_OFFSET)? != u16::MAX {
        return Err(validation(
            "Authoring the weapon inventory slot changed its FFFF sentinel",
        ));
    }
    if weapon_inventory_slot(data)? != inventory_slot
        || weapon_equipment_slot(data)? != inventory_slot
    {
        return Err(validation(
            "Authored weapon did not retain the requested inventory slot",
        ));
    }
    Ok(())
}

pub(in crate::weapon) fn item_string_client_classification(
    data: &[u8],
    expected_slot: WeaponInventorySlot,
) -> AuthoringResult<[u8; ITEM_STRING_CLIENT_CLASSIFICATION_SIZE]> {
    let end = ITEM_STRING_CLIENT_CLASSIFICATION_OFFSET
        .checked_add(ITEM_STRING_CLIENT_CLASSIFICATION_SIZE)
        .ok_or_else(|| invalid("Item-string client classification range overflowed"))?;
    let tuple: [u8; ITEM_STRING_CLIENT_CLASSIFICATION_SIZE] = data
        .get(ITEM_STRING_CLIENT_CLASSIFICATION_OFFSET..end)
        .ok_or_else(|| invalid("Item-string client classification tuple is truncated"))?
        .try_into()
        .map_err(|_| invalid("Item-string client classification tuple has the wrong size"))?;
    let bucket_hash = u32::from_le_bytes(tuple[0..4].try_into().map_err(|_| {
        invalid("Item-string client classification bucket hash has the wrong size")
    })?);
    let slot = WeaponInventorySlot::from_bucket_hash(bucket_hash).ok_or_else(|| {
        invalid(format!(
            "Item-string client classification uses unknown bucket hash 0x{bucket_hash:08X}"
        ))
    })?;
    if bucket_hash != expected_slot.bucket_hash() || slot != expected_slot {
        return Err(invalid(format!(
            "Item-string client classification uses {slot:?}, but the definition uses {expected_slot:?}"
        )));
    }
    let first_type_key = u32::from_le_bytes(
        tuple[4..8]
            .try_into()
            .map_err(|_| invalid("Item-string client type key has the wrong size"))?,
    );
    let second_type_key = u32::from_le_bytes(
        tuple[8..12]
            .try_into()
            .map_err(|_| invalid("Item-string second type key has the wrong size"))?,
    );
    // These are independent stock keys, not duplicates. Traveler's Chosen uses
    // 0x0D51B658 / 0x3EB02F1A. Preserve both when moving its appearance or slot.
    if first_type_key == 0 || second_type_key == 0 {
        return Err(invalid(format!(
            "Item-string client type keys must both be nonzero (0x{first_type_key:08X}, 0x{second_type_key:08X})"
        )));
    }
    Ok(tuple)
}

/// Slot and weapon type are independent: +B8 is the bucket hash, while +BC/+C0
/// are type-name hashes (for example FNV-1("sword")), not destination-slot keys.
pub(in crate::weapon) fn set_item_string_inventory_slot(
    data: &mut [u8],
    donor_slot: WeaponInventorySlot,
    authored_slot: WeaponInventorySlot,
) -> AuthoringResult<()> {
    item_string_client_classification(data, donor_slot)?;
    write_u32(
        data,
        ITEM_STRING_CLIENT_CLASSIFICATION_OFFSET,
        authored_slot.bucket_hash(),
    )
}

pub(in crate::weapon) fn transplant_item_string_client_classification(
    target: &mut [u8],
    target_donor_slot: WeaponInventorySlot,
    source: &[u8],
    source_slot: WeaponInventorySlot,
    authored_slot: WeaponInventorySlot,
) -> AuthoringResult<()> {
    let target_tuple = item_string_client_classification(target, target_donor_slot)?;
    let mut source_tuple = item_string_client_classification(source, source_slot)?;
    // Preserve the appearance's type keys, but not its inventory placement.
    source_tuple[..4].copy_from_slice(&authored_slot.bucket_hash().to_le_bytes());
    let before = target.to_vec();
    write_bytes(
        target,
        ITEM_STRING_CLIENT_CLASSIFICATION_OFFSET,
        &source_tuple,
    )?;
    if item_string_client_classification(target, authored_slot)? != source_tuple {
        return Err(validation(
            "Authored item-string did not retain the presentation donor classification tuple",
        ));
    }
    let mut normalized = target.to_vec();
    write_bytes(
        &mut normalized,
        ITEM_STRING_CLIENT_CLASSIFICATION_OFFSET,
        &target_tuple,
    )?;
    if normalized != before {
        return Err(validation(
            "Item-string classification authoring changed bytes outside the audited 12-byte tuple",
        ));
    }
    Ok(())
}

pub(in crate::weapon) fn item_string_ammo_type(
    data: &[u8],
) -> AuthoringResult<Option<WeaponAmmoType>> {
    if read_u32(data, ITEM_STRING_AMMO_CLASS_OFFSET)? != ITEM_STRING_AMMO_CLASS {
        return Err(invalid(
            "Item string has no canonical 0x80805D1A ammunition-classification field",
        ));
    }
    WeaponAmmoType::from_package_value(read_u16(data, ITEM_STRING_AMMO_TYPE_OFFSET)?)
}

pub(in crate::weapon) fn set_item_string_ammo_type(
    data: &mut [u8],
    ammo_type: WeaponAmmoType,
) -> AuthoringResult<()> {
    let original = item_string_ammo_type(data)?;
    let before = data.to_vec();
    write_u16(
        data,
        ITEM_STRING_AMMO_TYPE_OFFSET,
        ammo_type.package_value(),
    )?;
    if item_string_ammo_type(data)? != Some(ammo_type) {
        return Err(validation(
            "Authored item string did not retain the requested ammunition classification",
        ));
    }
    let mut normalized = data.to_vec();
    write_u16(
        &mut normalized,
        ITEM_STRING_AMMO_TYPE_OFFSET,
        original.map_or(0, WeaponAmmoType::package_value),
    )?;
    if normalized != before {
        return Err(validation(
            "Ammunition-classification authoring changed bytes outside the audited 16-bit field",
        ));
    }
    Ok(())
}

pub(in crate::weapon) fn item_string_stat_group_field_offset(
    data: &[u8],
) -> AuthoringResult<usize> {
    let resource = relative_target(data, ITEM_STRING_STAT_GROUP_POINTER_OFFSET)?;
    let field = resource
        .checked_add(ITEM_STRING_STAT_GROUP_INDEX_OFFSET)
        .ok_or_else(|| invalid("Item-string stat-display group field overflowed"))?;
    let end = field
        .checked_add(size_of::<i32>())
        .ok_or_else(|| invalid("Item-string stat-display group range overflowed"))?;
    if resource < size_of::<u32>()
        || end > data.len()
        || read_u32(data, resource - size_of::<u32>())? != ITEM_STRING_STAT_GROUP_RESOURCE_CLASS
    {
        return Err(invalid(
            "Item string has no canonical 0x80805CF1 stat-display resource",
        ));
    }
    Ok(field)
}

pub(in crate::weapon) fn item_string_stat_group_index(data: &[u8]) -> AuthoringResult<u16> {
    let field = item_string_stat_group_field_offset(data)?;
    u16::try_from(read_i32(data, field)?).map_err(|_| {
        invalid("Item-string stat-display group is negative or does not fit a 16-bit index")
    })
}

pub(in crate::weapon) fn set_item_string_stat_group_index(
    data: &mut [u8],
    index: u16,
) -> AuthoringResult<()> {
    let field = item_string_stat_group_field_offset(data)?;
    let original = read_i32(data, field)?;
    // Refuse to mutate a malformed donor resource even though the authored value itself is valid.
    item_string_stat_group_index(data)?;
    let before = data.to_vec();

    write_i32(data, field, i32::from(index))?;
    if item_string_stat_group_index(data)? != index {
        return Err(validation(
            "Authored item string did not retain the requested stat-display group",
        ));
    }

    let mut normalized = data.to_vec();
    write_i32(&mut normalized, field, original)?;
    if normalized != before {
        return Err(validation(
            "Stat-display group authoring changed bytes outside the audited signed 32-bit field",
        ));
    }
    Ok(())
}

pub(in crate::weapon) fn weapon_max_stack_size(data: &[u8]) -> AuthoringResult<u32> {
    weapon_translation_topology(data)?;
    let value = read_i32(data, ITEM_MAX_STACK_SIZE_OFFSET)?;
    u32::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| invalid("Weapon max stack size is not a positive signed 32-bit value"))
}

pub(in crate::weapon) fn set_weapon_max_stack_size(
    data: &mut [u8],
    value: u32,
) -> AuthoringResult<()> {
    let value_i32 = i32::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| invalid("Weapon max stack size must fit signed 32 bits and be positive"))?;
    let original = weapon_max_stack_size(data)?;
    let before = data.to_vec();
    write_i32(data, ITEM_MAX_STACK_SIZE_OFFSET, value_i32)?;
    if weapon_max_stack_size(data)? != value {
        return Err(validation(
            "Authored weapon did not retain the requested max stack size",
        ));
    }
    let mut normalized = data.to_vec();
    write_i32(
        &mut normalized,
        ITEM_MAX_STACK_SIZE_OFFSET,
        i32::try_from(original).map_err(|_| invalid("Donor max stack size overflowed"))?,
    )?;
    if normalized != before {
        return Err(validation(
            "Weapon max-stack authoring changed bytes outside the audited signed 32-bit field",
        ));
    }
    Ok(())
}

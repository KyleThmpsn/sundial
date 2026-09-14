use super::*;

pub(in crate::weapon) fn weapon_pattern_index(data: &[u8]) -> AuthoringResult<Option<u16>> {
    let topology = weapon_translation_topology(data)?;
    let index = read_u16(
        data,
        topology.root + TRANSLATION_WEAPON_PATTERN_INDEX_OFFSET,
    )?;
    Ok((index != u16::MAX).then_some(index))
}

pub(in crate::weapon) fn set_weapon_pattern_index(
    data: &mut [u8],
    index: u16,
) -> AuthoringResult<()> {
    if index == u16::MAX {
        return Err(invalid(
            "Weapon-pattern index 65535 is a native disabled sentinel",
        ));
    }
    let topology = weapon_translation_topology(data)?;
    let field = topology.root + TRANSLATION_WEAPON_PATTERN_INDEX_OFFSET;
    let original = weapon_pattern_index(data)?
        .ok_or_else(|| invalid("Weapon sandbox-pattern selector is disabled"))?;
    let before = data.to_vec();

    write_u16(data, field, index)?;
    if weapon_pattern_index(data)? != Some(index) {
        return Err(validation(
            "Authored weapon did not retain the requested weapon-pattern selector",
        ));
    }

    let mut normalized = data.to_vec();
    write_u16(&mut normalized, field, original)?;
    if normalized != before {
        return Err(validation(
            "Weapon-pattern authoring changed bytes outside the audited 16-bit selector",
        ));
    }
    Ok(())
}

pub(in crate::weapon) fn weapon_socket_entry_list_index(
    data: &[u8],
) -> AuthoringResult<Option<u16>> {
    weapon_translation_topology(data)?;
    if read_i64(data, ITEM_SOCKET_ENTRY_LIST_BLOCK_POINTER_OFFSET)? == 0 {
        return Ok(None);
    }
    let block = relative_target(data, ITEM_SOCKET_ENTRY_LIST_BLOCK_POINTER_OFFSET)?;
    let end = block
        .checked_add(12)
        .ok_or_else(|| invalid("Weapon socket-entry-list holder overflows"))?;
    if end > data.len() {
        return Err(invalid(
            "Weapon socket-entry-list holder extends beyond its definition",
        ));
    }
    Ok(Some(read_u16(
        data,
        block + ITEM_SOCKET_ENTRY_LIST_INDEX_OFFSET,
    )?))
}

pub(in crate::weapon) fn set_weapon_socket_entry_list_index(
    data: &mut [u8],
    index: u16,
) -> AuthoringResult<()> {
    let Some(original) = weapon_socket_entry_list_index(data)? else {
        return Err(invalid(
            "Weapon has no native socket-entry-list holder to author",
        ));
    };
    let block = relative_target(data, ITEM_SOCKET_ENTRY_LIST_BLOCK_POINTER_OFFSET)?;
    let field = block + ITEM_SOCKET_ENTRY_LIST_INDEX_OFFSET;
    let before = data.to_vec();
    write_u16(data, field, index)?;
    if weapon_socket_entry_list_index(data)? != Some(index) {
        return Err(validation(
            "Authored weapon did not retain the requested socket-entry-list index",
        ));
    }
    let mut normalized = data.to_vec();
    write_u16(&mut normalized, field, original)?;
    if normalized != before {
        return Err(validation(
            "Socket-entry-list authoring changed bytes outside the audited 16-bit field",
        ));
    }
    Ok(())
}

pub(in crate::weapon) fn aligned_record_offset(
    data: &[u8],
    class: u32,
    start: usize,
    end: usize,
) -> Option<usize> {
    let limit = end.min(data.len());
    (start..limit.saturating_sub(3))
        .step_by(4)
        .find(|offset| read_u32(data, *offset).ok() == Some(class))
}

pub(in crate::weapon) fn weapon_plug_category_field(data: &[u8]) -> AuthoringResult<usize> {
    weapon_translation_topology(data)?;
    if let Some(block) = aligned_record_offset(
        data,
        ITEM_PLUG_BLOCK_CLASS,
        ITEM_PLUG_BLOCK_SEARCH_START,
        ITEM_PLUG_BLOCK_SEARCH_END,
    ) {
        return block
            .checked_add(ITEM_PLUG_BLOCK_CATEGORY_OFFSET)
            .ok_or_else(|| invalid("Weapon plug-category field overflows"));
    }
    let end = ITEM_PLUG_CATEGORY_FALLBACK_OFFSET
        .checked_add(size_of::<u32>())
        .ok_or_else(|| invalid("Weapon fallback plug-category field overflows"))?;
    if end > data.len() {
        return Err(invalid(
            "Weapon has no native plug-category field to author",
        ));
    }
    Ok(ITEM_PLUG_CATEGORY_FALLBACK_OFFSET)
}

pub(in crate::weapon) fn weapon_plug_category_hash(data: &[u8]) -> AuthoringResult<u32> {
    read_u32(data, weapon_plug_category_field(data)?)
}

pub(in crate::weapon) fn set_weapon_plug_category_hash(
    data: &mut [u8],
    hash: u32,
) -> AuthoringResult<()> {
    let field = weapon_plug_category_field(data)?;
    let original = read_u32(data, field)?;
    let before = data.to_vec();
    write_u32(data, field, hash)?;
    if weapon_plug_category_hash(data)? != hash {
        return Err(validation(
            "Authored weapon did not retain the requested plug-category hash",
        ));
    }
    let mut normalized = data.to_vec();
    write_u32(&mut normalized, field, original)?;
    if normalized != before {
        return Err(validation(
            "Weapon plug-category authoring changed bytes outside the audited 32-bit field",
        ));
    }
    Ok(())
}

pub(in crate::weapon) fn weapon_roll_set_field(data: &[u8]) -> AuthoringResult<usize> {
    weapon_translation_topology(data)?;
    let block = aligned_record_offset(
        data,
        ITEM_PLUG_BLOCK_CLASS,
        ITEM_PLUG_BLOCK_SEARCH_START,
        ITEM_PLUG_BLOCK_SEARCH_END,
    )
    .ok_or_else(|| invalid("Weapon has no native plug block containing a roll-set field"))?;
    block
        .checked_add(ITEM_PLUG_BLOCK_ROLL_SET_OFFSET)
        .ok_or_else(|| invalid("Weapon roll-set field overflows"))
}

pub(in crate::weapon) fn weapon_roll_set_index(data: &[u8]) -> AuthoringResult<u16> {
    read_u16(data, weapon_roll_set_field(data)?)
}

pub(in crate::weapon) fn set_weapon_roll_set_index(
    data: &mut [u8],
    index: u16,
) -> AuthoringResult<()> {
    let field = weapon_roll_set_field(data)?;
    let original = read_u16(data, field)?;
    let before = data.to_vec();
    write_u16(data, field, index)?;
    if weapon_roll_set_index(data)? != index {
        return Err(validation(
            "Authored weapon did not retain the requested roll-set index",
        ));
    }
    let mut normalized = data.to_vec();
    write_u16(&mut normalized, field, original)?;
    if normalized != before {
        return Err(validation(
            "Weapon roll-set authoring changed bytes outside the audited 16-bit field",
        ));
    }
    Ok(())
}

pub(in crate::weapon) fn weapon_linked_plug_field(data: &[u8]) -> AuthoringResult<usize> {
    weapon_translation_topology(data)?;
    let block = aligned_record_offset(data, ITEM_LINKED_PLUG_BLOCK_CLASS, 0, data.len())
        .ok_or_else(|| invalid("Weapon has no native linked-plug block to author"))?;
    block
        .checked_add(ITEM_LINKED_PLUG_INDEX_OFFSET)
        .ok_or_else(|| invalid("Weapon linked-plug field overflows"))
}

pub(in crate::weapon) fn weapon_linked_plug_index(data: &[u8]) -> AuthoringResult<u16> {
    read_u16(data, weapon_linked_plug_field(data)?)
}

pub(in crate::weapon) fn set_weapon_linked_plug_index(
    data: &mut [u8],
    index: u16,
) -> AuthoringResult<()> {
    let field = weapon_linked_plug_field(data)?;
    let original = read_u16(data, field)?;
    let before = data.to_vec();
    write_u16(data, field, index)?;
    if weapon_linked_plug_index(data)? != index {
        return Err(validation(
            "Authored weapon did not retain the requested linked-plug index",
        ));
    }
    let mut normalized = data.to_vec();
    write_u16(&mut normalized, field, original)?;
    if normalized != before {
        return Err(validation(
            "Weapon linked-plug authoring changed bytes outside the audited 16-bit field",
        ));
    }
    Ok(())
}

pub(in crate::weapon) fn weapon_item_traits(data: &[u8]) -> AuthoringResult<Vec<u16>> {
    if data
        .get(ITEM_TRAITS_DESCRIPTOR_OFFSET..ITEM_TRAITS_DESCRIPTOR_OFFSET + 16)
        .is_some_and(|descriptor| descriptor == [0; 16])
    {
        return Ok(Vec::new());
    }
    let (count, _, rows, class) = array_at(data, ITEM_TRAITS_DESCRIPTOR_OFFSET)?;
    if class != ITEM_TRAIT_ROW_CLASS || count > 256 {
        return Err(invalid(
            "Weapon has an unknown or oversized item-trait array",
        ));
    }
    let traits = (0..count)
        .map(|index| read_u16(data, rows + index * ITEM_TRAIT_ROW_SIZE))
        .collect::<AuthoringResult<Vec<_>>>()?;
    if traits.contains(&u16::MAX)
        || traits.iter().copied().collect::<BTreeSet<_>>().len() != traits.len()
    {
        return Err(invalid(
            "Weapon item-trait array contains a disabled or duplicate index",
        ));
    }
    Ok(traits)
}

pub(in crate::weapon) fn set_weapon_item_traits(
    data: &mut Vec<u8>,
    traits: &[u16],
) -> AuthoringResult<()> {
    if traits.len() > 256
        || traits.contains(&u16::MAX)
        || traits.iter().copied().collect::<BTreeSet<_>>().len() != traits.len()
    {
        return Err(invalid(
            "Item traits must contain at most 256 distinct active indices",
        ));
    }
    weapon_translation_topology(data)?;
    if traits.is_empty() {
        write_bytes(data, ITEM_TRAITS_DESCRIPTOR_OFFSET, &[0; 16])?;
    } else {
        while data.len() % 16 != 0 {
            data.push(0);
        }
        let header = data.len();
        data.extend_from_slice(
            &u64::try_from(traits.len())
                .map_err(|_| invalid("Item-trait count does not fit 64 bits"))?
                .to_le_bytes(),
        );
        data.extend_from_slice(&ITEM_TRAIT_ROW_CLASS.to_le_bytes());
        data.extend_from_slice(&0_u32.to_le_bytes());
        for trait_index in traits {
            data.extend_from_slice(&trait_index.to_le_bytes());
        }
        write_u64(
            data,
            ITEM_TRAITS_DESCRIPTOR_OFFSET,
            u64::try_from(traits.len())
                .map_err(|_| invalid("Item-trait count does not fit 64 bits"))?,
        )?;
        write_relative_pointer(data, ITEM_TRAITS_DESCRIPTOR_OFFSET + 8, header)?;
    }
    if weapon_item_traits(data)? != traits {
        return Err(validation(
            "Authored weapon did not retain its requested item-trait indices",
        ));
    }
    Ok(())
}

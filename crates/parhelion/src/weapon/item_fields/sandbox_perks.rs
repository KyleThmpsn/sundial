use super::*;

pub(in crate::weapon) fn weapon_sandbox_perks(data: &[u8]) -> AuthoringResult<Vec<u16>> {
    sundial::package_authoring::native_weapon::base_sandbox_perks(data).map_err(invalid)
}

pub(in crate::weapon) fn replace_weapon_sandbox_perk_index(
    data: &mut [u8],
    source: u16,
    replacement: u16,
) -> AuthoringResult<()> {
    if source == u16::MAX || replacement == u16::MAX || source == replacement {
        return Err(invalid(
            "Private sandbox-perk replacement requires two distinct active indices",
        ));
    }
    let resource = relative_target(data, ITEM_INVESTMENT_STAT_POINTER_OFFSET)?;
    if resource < 4 || read_u32(data, resource - 4)? != ITEM_INVESTMENT_STAT_RESOURCE_CLASS {
        return Err(invalid("Item has no recognized investment resource"));
    }
    let descriptor = resource + ITEM_SANDBOX_PERK_DESCRIPTOR_OFFSET;
    let (count, _, rows, class) = array_at(data, descriptor)?;
    if class != ITEM_SANDBOX_PERK_ROW_CLASS || count > 64 {
        return Err(invalid(
            "Item has an unknown or oversized base sandbox-perk array",
        ));
    }
    let matches = (0..count)
        .filter_map(|index| {
            let row = rows + index * ITEM_SANDBOX_PERK_ROW_SIZE;
            (read_u16(data, row).ok() == Some(source)).then_some(row)
        })
        .collect::<Vec<_>>();
    let [row] = matches.as_slice() else {
        return Err(invalid(format!(
            "Source finished sandbox-perk index {source} occurs {} times; exactly one is required",
            matches.len()
        )));
    };
    write_u16(data, *row, replacement)?;
    if weapon_sandbox_perks(data)?.contains(&source)
        || !weapon_sandbox_perks(data)?.contains(&replacement)
    {
        return Err(validation(
            "Private finished sandbox-perk replacement did not round-trip",
        ));
    }
    Ok(())
}

pub(in crate::weapon) fn weapon_sandbox_perk_rows(data: &[u8]) -> AuthoringResult<Vec<&[u8]>> {
    let resource = relative_target(data, ITEM_INVESTMENT_STAT_POINTER_OFFSET)?;
    if resource < 4 || read_u32(data, resource - 4)? != ITEM_INVESTMENT_STAT_RESOURCE_CLASS {
        return Err(invalid("Weapon has no recognized investment resource"));
    }
    let descriptor = resource + ITEM_SANDBOX_PERK_DESCRIPTOR_OFFSET;
    if data
        .get(descriptor..descriptor + 16)
        .is_some_and(|bytes| bytes == [0; 16])
    {
        return Ok(Vec::new());
    }
    let (count, _, rows, class) = array_at(data, descriptor)?;
    if class != ITEM_SANDBOX_PERK_ROW_CLASS || count > 64 {
        return Err(invalid(
            "Weapon has an unknown or oversized base sandbox-perk array",
        ));
    }
    (0..count)
        .map(|index| {
            let row = rows
                .checked_add(index.saturating_mul(ITEM_SANDBOX_PERK_ROW_SIZE))
                .ok_or_else(|| invalid("Weapon sandbox-perk row offset overflowed"))?;
            data.get(row..row + ITEM_SANDBOX_PERK_ROW_SIZE)
                .ok_or_else(|| invalid("Weapon sandbox-perk row is truncated"))
        })
        .collect()
}

pub(in crate::weapon) fn item_string_sandbox_perk_descriptor(
    data: &[u8],
) -> AuthoringResult<usize> {
    let resource = relative_target(data, ITEM_STRING_SANDBOX_PERK_RESOURCE_POINTER_OFFSET)?;
    if resource < size_of::<u32>()
        || read_u32(data, resource - size_of::<u32>())? != ITEM_STRING_SANDBOX_PERK_RESOURCE_CLASS
    {
        return Err(invalid(
            "Item-string tag has no recognized sandbox-perk companion resource",
        ));
    }
    resource
        .checked_add(ITEM_STRING_SANDBOX_PERK_DESCRIPTOR_OFFSET)
        .ok_or_else(|| invalid("Item-string sandbox-perk descriptor overflowed"))
}

pub(in crate::weapon) fn validate_item_string_sandbox_perk_segment(
    segment: &[u8],
) -> AuthoringResult<()> {
    // The first eight bytes precede the relocation marker, not the array itself.
    // Coldheart's stock layout stores a neighboring relative pointer (0x10) there.
    // Preserve that context; only the marker, header and rows belong to this array.
    if segment.len() < 32
        || segment.get(8..16) != Some(&NESTED_ARRAY_TRAILER)
        || read_u32(segment, 24)? != ITEM_STRING_SANDBOX_PERK_ROW_CLASS
        || read_u32(segment, 28)? != 0
    {
        return Err(invalid(
            "Item-string sandbox-perk companion does not match the canonical stock row",
        ));
    }
    let count = usize::try_from(read_u64(segment, 16)?)
        .map_err(|_| invalid("Item-string sandbox-perk count does not fit memory"))?;
    let expected = 32_usize
        .checked_add(count.saturating_mul(ITEM_STRING_SANDBOX_PERK_ROW_SIZE))
        .ok_or_else(|| invalid("Item-string sandbox-perk segment overflowed"))?;
    if count > 64 || segment.len() != expected {
        return Err(invalid(
            "Item-string sandbox-perk companion has an invalid row count",
        ));
    }
    for index in 0..count {
        let row = 32 + index * ITEM_STRING_SANDBOX_PERK_ROW_SIZE;
        if read_u16(segment, row)? != u16::MAX
            || read_u16(segment, row + 2)? != 0
            || read_u32(segment, row + 4)? != ITEM_STRING_SANDBOX_PERK_EMPTY_EXPRESSION_TAG
            || segment
                .get(row + 8..row + ITEM_STRING_SANDBOX_PERK_ROW_SIZE)
                .is_none_or(|tail| tail.iter().any(|byte| *byte != 0))
        {
            return Err(invalid(
                "Item-string sandbox-perk companion contains a noncanonical row",
            ));
        }
    }
    Ok(())
}

pub(in crate::weapon) fn item_string_sandbox_perk_segment(
    data: &[u8],
) -> AuthoringResult<Option<&[u8]>> {
    let descriptor = item_string_sandbox_perk_descriptor(data)?;
    if data
        .get(descriptor..descriptor + 16)
        .is_some_and(|bytes| bytes == [0; 16])
    {
        return Ok(None);
    }
    let (count, header, rows, class) = array_at(data, descriptor)?;
    let segment = header
        .checked_sub(16)
        .ok_or_else(|| invalid("Item-string sandbox-perk header has no framing prefix"))?;
    let row_bytes = count
        .checked_mul(ITEM_STRING_SANDBOX_PERK_ROW_SIZE)
        .ok_or_else(|| invalid("Item-string sandbox-perk array overflowed"))?;
    let end = rows
        .checked_add(row_bytes)
        .ok_or_else(|| invalid("Item-string sandbox-perk array overflowed"))?;
    if count > 64 || class != ITEM_STRING_SANDBOX_PERK_ROW_CLASS || rows != header + 16 {
        return Err(invalid(
            "Item-string tag has an incompatible sandbox-perk companion array",
        ));
    }
    let segment = data
        .get(segment..end)
        .ok_or_else(|| invalid("Item-string sandbox-perk companion is truncated"))?;
    validate_item_string_sandbox_perk_segment(segment)?;
    Ok(Some(segment))
}

pub(in crate::weapon) fn item_string_sandbox_perk_count(data: &[u8]) -> AuthoringResult<usize> {
    item_string_sandbox_perk_segment(data)?
        .map(|segment| {
            usize::try_from(read_u64(segment, 16)?)
                .map_err(|_| invalid("Item-string sandbox-perk count does not fit memory"))
        })
        .transpose()
        .map(Option::unwrap_or_default)
}

pub(in crate::weapon) fn validate_weapon_sandbox_perk_parallelism(
    definition: &[u8],
    strings: &[u8],
    sandbox_perk_string_template: &[u8],
) -> AuthoringResult<()> {
    let definition_count = weapon_sandbox_perks(definition)?.len();
    let string_count = item_string_sandbox_perk_count(strings)?;
    if definition_count != string_count {
        return Err(validation(format!(
            "Weapon definition has {definition_count} sandbox-perk rows but its item-string companion has {string_count}"
        )));
    }
    if let Some(segment) = item_string_sandbox_perk_segment(strings)? {
        validate_item_string_sandbox_perk_segment(sandbox_perk_string_template)?;
        let template_row = sandbox_perk_string_template
            .get(32..32 + ITEM_STRING_SANDBOX_PERK_ROW_SIZE)
            .ok_or_else(|| invalid("Item-string sandbox-perk template row is truncated"))?;
        let framing_matches = segment.get(8..16) == sandbox_perk_string_template.get(8..16)
            && segment.get(24..32) == sandbox_perk_string_template.get(24..32);
        let rows_match = (0..string_count).all(|index| {
            let row = 32 + index * ITEM_STRING_SANDBOX_PERK_ROW_SIZE;
            segment.get(row..row + ITEM_STRING_SANDBOX_PERK_ROW_SIZE) == Some(template_row)
        });
        if !framing_matches || !rows_match {
            return Err(validation(
                "Weapon item-string sandbox-perk companion differs from the audited stock exemplar",
            ));
        }
    }
    Ok(())
}

pub(in crate::weapon) fn canonical_item_sandbox_perk_string_template(
    manager: &PackageManager,
    item_strings: &[u8],
) -> AuthoringResult<Vec<u8>> {
    let (count, _, rows, _) = array_at(item_strings, 8)?;
    for item_index in 0..count {
        let string_tag = TagHash(read_u32(
            item_strings,
            rows + item_index * ITEM_ROW_SIZE + 16,
        )?);
        let Ok(strings) = read_tag(manager, string_tag, "stock item-string exemplar") else {
            continue;
        };
        let Ok(Some(segment)) = item_string_sandbox_perk_segment(&strings) else {
            continue;
        };
        let mut template = segment
            .get(..32 + ITEM_STRING_SANDBOX_PERK_ROW_SIZE)
            .ok_or_else(|| invalid("Stock item-string sandbox-perk row is truncated"))?
            .to_owned();
        write_u64(&mut template, 16, 1)?;
        validate_item_string_sandbox_perk_segment(&template)?;
        return Ok(template);
    }
    Err(invalid(
        "No stock item-string row provides a sandbox-perk companion exemplar",
    ))
}

pub(in crate::weapon) fn canonical_weapon_sandbox_perk_row_template(
    manager: &PackageManager,
    item_table: &[u8],
) -> AuthoringResult<[u8; ITEM_SANDBOX_PERK_ROW_SIZE]> {
    let (count, _, rows, _) = array_at(item_table, 8)?;
    for item_index in 0..count {
        let definition_tag = TagHash(read_u32(
            item_table,
            rows + item_index * ITEM_ROW_SIZE + 16,
        )?);
        let Ok(definition) = read_tag(manager, definition_tag, "stock sandbox-perk exemplar")
        else {
            continue;
        };
        if !matches!(
            weapon_damage_carrier(&definition),
            Ok(WeaponDamageCarrier::Fixed { .. })
        ) {
            continue;
        }
        let Some(row) = weapon_sandbox_perk_rows(&definition)?.into_iter().next() else {
            continue;
        };
        return row
            .try_into()
            .map_err(|_| invalid("Stock sandbox-perk exemplar has the wrong row size"));
    }
    Err(invalid(
        "No stock weapon definition provides a compatible sandbox-perk row exemplar",
    ))
}

pub(in crate::weapon) fn set_item_string_sandbox_perk_count(
    data: &mut Vec<u8>,
    count: usize,
    one_row_template: &[u8],
) -> AuthoringResult<()> {
    if count > 64 {
        return Err(invalid(
            "An item-string sandbox-perk companion cannot exceed 64 rows",
        ));
    }
    validate_item_string_sandbox_perk_segment(one_row_template)?;
    if read_u64(one_row_template, 16)? != 1 {
        return Err(invalid(
            "Item-string sandbox-perk template must contain exactly one row",
        ));
    }
    if item_string_sandbox_perk_count(data)? == count {
        return Ok(());
    }
    let descriptor = item_string_sandbox_perk_descriptor(data)?;
    if count == 0 {
        write_bytes(data, descriptor, &[0; 16])?;
        return Ok(());
    }
    while data.len() % 16 != 0 {
        data.push(0);
    }
    data.extend_from_slice(&[0; 8]);
    data.extend_from_slice(&NESTED_ARRAY_TRAILER);
    let header = data.len();
    data.extend_from_slice(
        &u64::try_from(count)
            .map_err(|_| invalid("Sandbox-perk row count does not fit 64 bits"))?
            .to_le_bytes(),
    );
    data.extend_from_slice(&ITEM_STRING_SANDBOX_PERK_ROW_CLASS.to_le_bytes());
    data.extend_from_slice(&0_u32.to_le_bytes());
    let row = one_row_template
        .get(32..32 + ITEM_STRING_SANDBOX_PERK_ROW_SIZE)
        .ok_or_else(|| invalid("Item-string sandbox-perk template row is truncated"))?;
    for _ in 0..count {
        data.extend_from_slice(row);
    }
    write_u64(
        data,
        descriptor,
        u64::try_from(count).map_err(|_| invalid("Sandbox-perk row count does not fit 64 bits"))?,
    )?;
    write_relative_pointer(data, descriptor + 8, header)?;
    if item_string_sandbox_perk_count(data)? != count {
        return Err(validation(
            "Authored item-string sandbox-perk companion has the wrong row count",
        ));
    }
    Ok(())
}

pub(in crate::weapon) fn set_weapon_base_sandbox_perks(
    data: &mut Vec<u8>,
    perks: &[u16],
    row_template: &[u8; ITEM_SANDBOX_PERK_ROW_SIZE],
) -> AuthoringResult<()> {
    if perks.len() > 64 || perks.contains(&u16::MAX) {
        return Err(invalid(
            "Base sandbox-perk rows must contain at most 64 active indices",
        ));
    }
    if perks.iter().copied().collect::<BTreeSet<_>>().len() != perks.len() {
        return Err(invalid("Base sandbox-perk rows cannot contain duplicates"));
    }
    let resource = relative_target(data, ITEM_INVESTMENT_STAT_POINTER_OFFSET)?;
    if resource < 4 || read_u32(data, resource - 4)? != ITEM_INVESTMENT_STAT_RESOURCE_CLASS {
        return Err(invalid("Weapon has no recognized investment resource"));
    }
    let descriptor = resource + ITEM_SANDBOX_PERK_DESCRIPTOR_OFFSET;
    let current_layout = if data
        .get(descriptor..descriptor + 16)
        .is_some_and(|bytes| bytes == [0; 16])
    {
        None
    } else {
        let (count, _, rows, class) = array_at(data, descriptor)?;
        if class != ITEM_SANDBOX_PERK_ROW_CLASS || count > 64 {
            return Err(invalid(
                "Weapon has an unknown or oversized base sandbox-perk array",
            ));
        }
        Some((count, rows))
    };
    if let Some((count, rows)) = current_layout.filter(|(count, _)| *count == perks.len()) {
        for (index, &perk) in perks.iter().enumerate() {
            write_u16(data, rows + index * ITEM_SANDBOX_PERK_ROW_SIZE, perk)?;
        }
        debug_assert_eq!(count, perks.len());
    } else if perks.is_empty() {
        write_bytes(data, descriptor, &[0; 16])?;
    } else {
        while data.len() % 16 != 0 {
            data.push(0);
        }
        // Nested arrays require the relocation marker before their count/class header.
        // Without it Sunrise's production reader rejects the otherwise valid perk rows.
        data.extend_from_slice(&[0; 8]);
        data.extend_from_slice(&NESTED_ARRAY_TRAILER);
        let header = data.len();
        data.extend_from_slice(
            &u64::try_from(perks.len())
                .map_err(|_| invalid("Sandbox-perk row count does not fit 64 bits"))?
                .to_le_bytes(),
        );
        data.extend_from_slice(&ITEM_SANDBOX_PERK_ROW_CLASS.to_le_bytes());
        data.extend_from_slice(&0_u32.to_le_bytes());
        for &perk in perks {
            let mut row = *row_template;
            write_u16(&mut row, 0, perk)?;
            data.extend_from_slice(&row);
        }
        write_u64(
            data,
            descriptor,
            u64::try_from(perks.len())
                .map_err(|_| invalid("Sandbox-perk row count does not fit 64 bits"))?,
        )?;
        write_relative_pointer(data, descriptor + 8, header)?;
    }
    if weapon_sandbox_perks(data)? != perks {
        return Err(validation(
            "Authored weapon did not retain its requested base sandbox-perk indices",
        ));
    }
    // Perk arrays also belong to plugs, which have no weapon socket topology.
    // Damage-carrier validation belongs to the weapon damage authoring operations.
    Ok(())
}

pub(in crate::weapon) fn set_weapon_base_sandbox_perks_with_strings(
    definition: &mut Vec<u8>,
    strings: &mut Vec<u8>,
    perks: &[u16],
    definition_template: &[u8; ITEM_SANDBOX_PERK_ROW_SIZE],
    string_template: &[u8],
) -> AuthoringResult<()> {
    set_weapon_base_sandbox_perks(definition, perks, definition_template)?;
    set_item_string_sandbox_perk_count(strings, perks.len(), string_template)?;
    validate_weapon_sandbox_perk_parallelism(definition, strings, string_template)
}

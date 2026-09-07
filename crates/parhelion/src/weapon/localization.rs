//! Native localization operations with independent validation.
use super::*;

pub(super) fn author_project_localized_strings(
    manager: &PackageManager,
    index: Vec<u8>,
    weapons: &[WeaponCloneSpec],
    custom_plugs: &[ResolvedCustomPlug],
) -> AuthoringResult<AuthoredLocalization> {
    let (table_count, _, table_rows, table_class) = array_at(&index, 8)?;
    let table_end = table_rows
        .checked_add(
            table_count
                .checked_mul(LOCALIZED_INDEX_ROW_SIZE)
                .ok_or_else(|| invalid("Localized-string index row size overflowed"))?,
        )
        .ok_or_else(|| invalid("Localized-string index row range overflowed"))?;
    if table_class != LOCALIZED_INDEX_ROW_CLASS
        || table_count != LOCALIZATION_STOCK_TABLE_COUNT
        || table_end != index.len()
    {
        return Err(invalid(
            "Localized-string index does not have the expected terminal row layout",
        ));
    }
    let donor_row = table_rows + LOCALIZATION_DONOR_TABLE_INDEX * LOCALIZED_INDEX_ROW_SIZE;
    if read_u32(&index, donor_row)? != LOCALIZATION_DONOR_TABLE_KEY {
        return Err(invalid("Localized-string donor bank key changed"));
    }
    let donor_header_tag = TagHash(read_u32(&index, donor_row + 4)?);
    let donor_header = read_tag(manager, donor_header_tag, "localization donor header")?;
    let header_values = project_authored_localized_values(weapons, custom_plugs, 0)?;
    let merged_header = rewrite_localized_header(&donor_header, &header_values)?;
    let mut locale_data = Vec::with_capacity(LOCALIZATION_LOCALE_COUNT);
    let mut custom_part_template = None;
    for (locale_index, offset) in (LOCALIZATION_DATA_TAG_START..LOCALIZATION_DATA_TAG_END)
        .step_by(4)
        .enumerate()
    {
        let donor_tag = TagHash(read_u32(&donor_header, offset)?);
        let donor_data = read_tag(manager, donor_tag, "localization donor locale data")?;
        if custom_part_template.is_none() {
            let (_, _, donor_parts, donor_part_class) = array_at(&donor_data, 8)?;
            if donor_part_class != LOCALIZATION_PART_CLASS {
                return Err(invalid("Localization donor part class changed"));
            }
            custom_part_template =
                Some(donor_data[donor_parts..donor_parts + LOCALIZATION_PART_ROW_SIZE].to_vec());
        }
        let custom_values = project_authored_localized_values(weapons, custom_plugs, locale_index)?;
        let payload = rewrite_localized_data(
            &donor_data,
            custom_part_template
                .as_deref()
                .ok_or_else(|| invalid("Localization custom part template is missing"))?,
            &custom_values,
        )?;
        locale_data.push(AuthoredLocaleData { donor_tag, payload });
    }
    Ok(AuthoredLocalization {
        index,
        merged_header,
        locale_data,
        donor_header_tag,
    })
}

pub(super) fn project_authored_localized_values<'a>(
    weapons: &'a [WeaponCloneSpec],
    custom_plugs: &'a [ResolvedCustomPlug],
    locale_index: usize,
) -> AuthoringResult<Vec<(u32, &'a str)>> {
    let mut custom_values = Vec::with_capacity(weapons.len() * 4 + custom_plugs.len() + 2);
    for weapon in weapons {
        let locale = weapon
            .text
            .locale_overrides
            .iter()
            .find(|locale| usize::from(locale.locale_index) == locale_index);
        custom_values.extend([
            (
                weapon.identity.flavor_hash,
                locale
                    .and_then(|locale| locale.flavor.as_deref())
                    .unwrap_or(&weapon.text.flavor),
            ),
            (
                weapon.identity.name_hash,
                locale
                    .and_then(|locale| locale.name.as_deref())
                    .unwrap_or(&weapon.text.name),
            ),
            (
                weapon.identity.source_hash,
                locale
                    .and_then(|locale| locale.source.as_deref())
                    .unwrap_or(&weapon.text.source),
            ),
        ]);
        if let Some(type_name) = &weapon.text.type_name {
            custom_values.push((
                weapon.identity.type_hash,
                locale
                    .and_then(|locale| locale.type_name.as_deref())
                    .unwrap_or(type_name),
            ));
        }
        if let Some(value) = &weapon.text.collection_name {
            custom_values.push((
                weapon.identity.collection_name_hash,
                locale
                    .and_then(|locale| locale.collection_name.as_deref())
                    .unwrap_or(value),
            ));
        }
        if let Some(value) = &weapon.text.collection_description {
            custom_values.push((
                weapon.identity.collection_description_hash,
                locale
                    .and_then(|locale| locale.collection_description.as_deref())
                    .unwrap_or(value),
            ));
        }
        if let Some(value) = &weapon.text.inventory_hint {
            custom_values.push((
                weapon.identity.inventory_hint_hash,
                locale
                    .and_then(|locale| locale.inventory_hint.as_deref())
                    .unwrap_or(value),
            ));
        }
        if let Some(value) = &weapon.text.collection_requirement {
            custom_values.push((
                weapon.identity.collection_requirement_hash,
                locale
                    .and_then(|locale| locale.collection_requirement.as_deref())
                    .unwrap_or(value),
            ));
        }
    }
    for custom_plug in custom_plugs {
        if let (Some(hash), Some(name)) = (
            custom_plug.authored_name_hash,
            custom_plug.authored_name.as_deref(),
        ) {
            custom_values.push((hash, name));
        }
        if let (Some(hash), Some(description)) = (
            custom_plug.authored_description_hash,
            custom_plug.authored_description.as_deref(),
        ) {
            custom_values.push((hash, description));
        }
    }
    custom_values.extend([
        (SUNRISE_BADGE_DESCRIPTION_HASH, SUNRISE_BADGE_DESCRIPTION),
        (SUNRISE_BADGE_NAME_HASH, SUNRISE_BADGE_NAME),
    ]);
    custom_values.sort_unstable_by_key(|value| value.0);
    if custom_values
        .iter()
        .any(|value| value.0 <= LOCALIZATION_DONOR_STRING_HASHES[1])
        || custom_values.windows(2).any(|pair| pair[0].0 == pair[1].0)
    {
        return Err(AuthoringError::InvalidInput(
            "Project localized hashes collide with stock or one another".to_owned(),
        ));
    }
    Ok(custom_values)
}

pub(super) fn rewrite_localized_header(
    template: &[u8],
    custom_values: &[(u32, &str)],
) -> AuthoringResult<Vec<u8>> {
    let mut header = template.to_vec();
    let (hash_count, hash_header, hash_rows, hash_class) = array_at(&header, 8)?;
    if hash_count != 2
        || hash_class != LOCALIZATION_HEADER_HASH_CLASS
        || hash_rows + 8 != header.len()
        || [
            read_u32(&header, hash_rows)?,
            read_u32(&header, hash_rows + 4)?,
        ] != LOCALIZATION_DONOR_STRING_HASHES
        || custom_values.is_empty()
        || custom_values.windows(2).any(|pair| pair[0].0 >= pair[1].0)
        || custom_values[0].0 <= LOCALIZATION_DONOR_STRING_HASHES[1]
    {
        return Err(invalid(
            "Localization donor header is not the audited terminal two-hash table",
        ));
    }
    let merged_count = LOCALIZATION_DONOR_STRING_HASHES
        .len()
        .checked_add(custom_values.len())
        .ok_or_else(|| validation("Localized value count overflowed"))?;
    header.resize(hash_rows + merged_count * 4, 0);
    set_array_count(&mut header, 8, hash_header, merged_count)?;
    for (index, (hash, _)) in custom_values.iter().enumerate() {
        write_u32(&mut header, hash_rows + (index + 2) * 4, *hash)?;
    }
    Ok(header)
}

pub(super) fn rewrite_localized_data(
    template: &[u8],
    custom_part_template: &[u8],
    custom_values: &[(u32, &str)],
) -> AuthoringResult<Vec<u8>> {
    let (part_count, part_header, parts, part_class) = array_at(template, 8)?;
    let (aux_count, aux_header, aux_rows, aux_class) =
        array_at(template, LOCALIZATION_AUX_DESCRIPTOR_OFFSET)?;
    let (byte_count, byte_header, bytes, byte_class) =
        array_at(template, LOCALIZATION_BYTE_DESCRIPTOR_OFFSET)?;
    let (combo_count, combo_header, combos, combo_class) =
        array_at(template, LOCALIZATION_COMBO_DESCRIPTOR_OFFSET)?;
    let aux_end = byte_header
        .checked_sub(4)
        .ok_or_else(|| invalid("Localization auxiliary table has no trailer"))?;
    if part_count != 2
        || aux_count == 0
        || combo_count != 2
        || part_class != LOCALIZATION_PART_CLASS
        || aux_class != LOCALIZATION_AUX_CLASS
        || byte_class != LOCALIZATION_BYTE_CLASS
        || combo_class != LOCALIZATION_COMBO_CLASS
        || (0..2).any(|index| {
            let combo = combos + index * LOCALIZATION_COMBO_ROW_SIZE;
            relative_target(template, combo).ok()
                != Some(parts + index * LOCALIZATION_PART_ROW_SIZE)
                || read_i64(template, combo + 8).ok() != Some(1)
        })
        || parts + 2 * LOCALIZATION_PART_ROW_SIZE > aux_header.saturating_sub(4)
        || aux_rows + aux_count.saturating_mul(2) > aux_end
        || bytes + byte_count > combo_header.saturating_sub(4)
        || combos + 2 * LOCALIZATION_COMBO_ROW_SIZE != template.len()
        || [part_header, aux_header, byte_header, combo_header]
            .into_iter()
            .any(|header| {
                header < 4
                    || read_u32(template, header - 4).ok() != Some(LOCALIZATION_ARRAY_SENTINEL)
            })
    {
        return Err(invalid(
            "Localization donor data is not a simple fixed two-part, two-value table",
        ));
    }
    if custom_part_template.len() != LOCALIZATION_PART_ROW_SIZE {
        return Err(invalid("Localization custom part template is not 32 bytes"));
    }
    let custom_shift = read_u16(custom_part_template, 0x18)?;
    let encoded_custom = custom_values
        .iter()
        .map(|(_, value)| {
            let encoded = encode_localized_value(value, custom_shift)?;
            let encoded_length = u16::try_from(encoded.len())
                .map_err(|_| invalid("Localized byte length does not fit 16 bits"))?;
            let character_count = u16::try_from(value.chars().count())
                .map_err(|_| invalid("Localized character count does not fit 16 bits"))?;
            Ok((encoded, encoded_length, character_count))
        })
        .collect::<AuthoringResult<Vec<_>>>()?;
    let custom_count = encoded_custom.len();
    let merged_count = part_count + custom_count;

    let donor_values = (0..part_count)
        .map(|index| decode_localized_value_at(template, index))
        .collect::<AuthoringResult<Vec<_>>>()?;

    let mut data = template
        .get(..part_header)
        .ok_or_else(|| invalid("Localization donor part header is truncated"))?
        .to_vec();
    data.extend_from_slice(
        template
            .get(part_header..parts)
            .ok_or_else(|| invalid("Localization donor part array header is truncated"))?,
    );
    let new_parts = data.len();
    data.extend_from_slice(
        template
            .get(parts..parts + part_count * LOCALIZATION_PART_ROW_SIZE)
            .ok_or_else(|| invalid("Localization donor part rows are truncated"))?,
    );
    let custom_parts = data.len();
    data.resize(custom_parts + custom_count * LOCALIZATION_PART_ROW_SIZE, 0);

    let new_aux_header = append_localization_array_header(
        &mut data,
        template
            .get(aux_header..aux_rows)
            .ok_or_else(|| invalid("Localization auxiliary header is truncated"))?,
    )?;
    data.extend_from_slice(
        template
            .get(aux_rows..aux_end)
            .ok_or_else(|| invalid("Localization auxiliary payload is truncated"))?,
    );
    let new_byte_header = append_localization_array_header(
        &mut data,
        template
            .get(byte_header..bytes)
            .ok_or_else(|| invalid("Localization byte-array header is truncated"))?,
    )?;
    let new_bytes = data.len();
    data.extend_from_slice(
        template
            .get(bytes..bytes + byte_count)
            .ok_or_else(|| invalid("Localization donor byte payload is truncated"))?,
    );
    let mut custom_starts = Vec::with_capacity(custom_count);
    for (encoded, _, _) in &encoded_custom {
        custom_starts.push(data.len());
        data.extend_from_slice(encoded);
    }
    data.push(0);
    let new_byte_count = data.len() - new_bytes;

    let new_combo_header = append_localization_array_header(
        &mut data,
        template
            .get(combo_header..combos)
            .ok_or_else(|| invalid("Localization combo-array header is truncated"))?,
    )?;
    let new_combos = data.len();
    data.extend_from_slice(
        template
            .get(combos..combos + combo_count * LOCALIZATION_COMBO_ROW_SIZE)
            .ok_or_else(|| invalid("Localization donor combo rows are truncated"))?,
    );
    let custom_combos = data.len();
    for index in 0..custom_count {
        let donor_index = index.min(combo_count - 1);
        let combo = combos + donor_index * LOCALIZATION_COMBO_ROW_SIZE;
        data.extend_from_slice(
            template
                .get(combo..combo + LOCALIZATION_COMBO_ROW_SIZE)
                .ok_or_else(|| invalid("Localization donor combo row is truncated"))?,
        );
    }

    write_relative_pointer(&mut data, 16, part_header)?;
    set_array_count(&mut data, 8, part_header, merged_count)?;
    write_relative_pointer(
        &mut data,
        LOCALIZATION_AUX_DESCRIPTOR_OFFSET + 8,
        new_aux_header,
    )?;
    write_relative_pointer(
        &mut data,
        LOCALIZATION_BYTE_DESCRIPTOR_OFFSET + 8,
        new_byte_header,
    )?;
    set_array_count(
        &mut data,
        LOCALIZATION_BYTE_DESCRIPTOR_OFFSET,
        new_byte_header,
        new_byte_count,
    )?;
    write_relative_pointer(
        &mut data,
        LOCALIZATION_COMBO_DESCRIPTOR_OFFSET + 8,
        new_combo_header,
    )?;
    set_array_count(
        &mut data,
        LOCALIZATION_COMBO_DESCRIPTOR_OFFSET,
        new_combo_header,
        merged_count,
    )?;

    for index in 0..part_count {
        let donor_part = parts + index * LOCALIZATION_PART_ROW_SIZE;
        let donor_target = relative_target(template, donor_part + 8)?;
        let donor_length = usize::from(read_u16(template, donor_part + 0x14)?);
        let donor_aux_end = aux_rows + aux_count * 2;
        let new_aux_rows = new_aux_header + 16;
        let rebased_target = if donor_length == 0 {
            new_bytes
        } else if donor_target >= bytes && donor_target + donor_length <= bytes + byte_count {
            new_bytes + (donor_target - bytes)
        } else if donor_target >= aux_rows && donor_target + donor_length * 2 <= donor_aux_end {
            new_aux_rows + (donor_target - aux_rows)
        } else {
            return Err(invalid(format!(
                "Localization donor part {index} points to 0x{donor_target:X} for length 0x{donor_length:X}, outside byte range 0x{bytes:X}..0x{:X} and wide range 0x{aux_rows:X}..0x{donor_aux_end:X}",
                bytes + byte_count,
            )));
        };
        write_relative_pointer(
            &mut data,
            new_parts + index * LOCALIZATION_PART_ROW_SIZE + 8,
            rebased_target,
        )?;
    }
    for index in 0..combo_count {
        let donor_combo = combos + index * LOCALIZATION_COMBO_ROW_SIZE;
        let donor_target = relative_target(template, donor_combo)?;
        if donor_target < parts || donor_target >= parts + part_count * LOCALIZATION_PART_ROW_SIZE {
            return Err(invalid(
                "Localization donor combo points outside its part array",
            ));
        }
        write_relative_pointer(
            &mut data,
            new_combos + index * LOCALIZATION_COMBO_ROW_SIZE,
            new_parts + (donor_target - parts),
        )?;
    }

    for index in 0..custom_count {
        write_bytes(
            &mut data,
            custom_parts + index * LOCALIZATION_PART_ROW_SIZE,
            custom_part_template,
        )?;
    }
    for (index, ((_, encoded_length, character_count), start)) in
        encoded_custom.iter().zip(custom_starts).enumerate()
    {
        let part = custom_parts + index * LOCALIZATION_PART_ROW_SIZE;
        write_relative_pointer(&mut data, part + 8, start)?;
        write_u16(&mut data, part + 0x14, *encoded_length)?;
        write_u16(&mut data, part + 0x16, *character_count)?;
        write_u16(&mut data, part + 0x18, custom_shift)?;
    }
    for index in 0..custom_count {
        let combo = custom_combos + index * LOCALIZATION_COMBO_ROW_SIZE;
        write_relative_pointer(
            &mut data,
            combo,
            custom_parts + index * LOCALIZATION_PART_ROW_SIZE,
        )?;
        write_i64(&mut data, combo + 8, 1)?;
    }
    if (0..part_count).any(|index| {
        decode_localized_value_at(&data, index).ok() != donor_values.get(index).cloned()
    }) || custom_values
        .iter()
        .enumerate()
        .any(|(index, (_, expected))| {
            decode_localized_value_at(&data, part_count + index)
                .ok()
                .as_deref()
                != Some(*expected)
        })
    {
        return Err(validation(
            "Merged localized values did not preserve donor strings and append the authored text in hash order",
        ));
    }
    Ok(data)
}

pub(super) fn append_localization_array_header(
    data: &mut Vec<u8>,
    source_header: &[u8],
) -> AuthoringResult<usize> {
    if source_header.len() != 16 {
        return Err(invalid("Localization array header is not 16 bytes"));
    }
    let padded = data
        .len()
        .checked_add(4)
        .and_then(|value| value.checked_add(15))
        .ok_or_else(|| invalid("Localization array alignment overflowed"))?;
    let header = padded & !15;
    data.resize(header, 0);
    write_u32(data, header - 4, LOCALIZATION_ARRAY_SENTINEL)?;
    data.extend_from_slice(source_header);
    Ok(header)
}

pub(super) fn encode_localized_value(value: &str, shift: u16) -> AuthoringResult<Vec<u8>> {
    let mut encoded = Vec::new();
    for character in value.chars() {
        let shifted = (character as u32)
            .checked_sub(u32::from(shift))
            .and_then(char::from_u32)
            .ok_or_else(|| invalid("Localized string cannot use the donor character shift"))?;
        let mut buffer = [0; 4];
        encoded.extend_from_slice(shifted.encode_utf8(&mut buffer).as_bytes());
    }
    Ok(encoded)
}

pub(super) fn decode_localized_value_at(data: &[u8], index: usize) -> AuthoringResult<String> {
    let (part_count, _, parts, _) = array_at(data, 8)?;
    let (aux_count, _, aux_rows, _) = array_at(data, LOCALIZATION_AUX_DESCRIPTOR_OFFSET)?;
    let (byte_count, _, bytes, _) = array_at(data, LOCALIZATION_BYTE_DESCRIPTOR_OFFSET)?;
    let (combo_count, _, combos, _) = array_at(data, LOCALIZATION_COMBO_DESCRIPTOR_OFFSET)?;
    if index >= combo_count {
        return Err(invalid("Localized combo index is outside the data table"));
    }
    let combo = combos + index * LOCALIZATION_COMBO_ROW_SIZE;
    let first_part = relative_target(data, combo)?;
    let selected_count = usize::try_from(read_i64(data, combo + 8)?)
        .map_err(|_| invalid("Localized combo part count is negative"))?;
    let selected_end = first_part
        .checked_add(
            selected_count
                .checked_mul(LOCALIZATION_PART_ROW_SIZE)
                .ok_or_else(|| invalid("Localized combo part range overflowed"))?,
        )
        .ok_or_else(|| invalid("Localized combo part range overflowed"))?;
    if first_part < parts
        || (first_part - parts) % LOCALIZATION_PART_ROW_SIZE != 0
        || selected_end > parts + part_count * LOCALIZATION_PART_ROW_SIZE
    {
        return Err(invalid("Localized combo points outside its part table"));
    }
    let mut value = String::new();
    for part_index in 0..selected_count {
        let part = first_part + part_index * LOCALIZATION_PART_ROW_SIZE;
        let start = relative_target(data, part + 8)?;
        let length = usize::from(read_u16(data, part + 0x14)?);
        let shift = read_u16(data, part + 0x18)?;
        let byte_end = bytes
            .checked_add(byte_count)
            .ok_or_else(|| invalid("Localized packed byte range overflowed"))?;
        let aux_end = aux_rows
            .checked_add(
                aux_count
                    .checked_mul(2)
                    .ok_or_else(|| invalid("Localized wide-character range overflowed"))?,
            )
            .ok_or_else(|| invalid("Localized wide-character range overflowed"))?;
        if start >= bytes && start.checked_add(length).is_some_and(|end| end <= byte_end) {
            for character in String::from_utf8_lossy(
                data.get(start..start + length)
                    .ok_or_else(|| invalid("Localized bytes extend past the payload"))?,
            )
            .chars()
            {
                value
                    .push(char::from_u32(character as u32 + u32::from(shift)).unwrap_or(character));
            }
        } else if start >= aux_rows
            && start
                .checked_add(length.saturating_mul(2))
                .is_some_and(|end| end <= aux_end)
        {
            let mut units = Vec::with_capacity(length);
            for unit_index in 0..length {
                units.push(read_u16(data, start + unit_index * 2)?);
            }
            if shift == 0 {
                value.push_str(&String::from_utf16_lossy(&units));
            } else {
                for unit in units {
                    value.push(
                        char::from_u32(u32::from(unit) + u32::from(shift))
                            .unwrap_or(char::REPLACEMENT_CHARACTER),
                    );
                }
            }
        } else {
            return Err(invalid(format!(
                "Localized part at 0x{start:X} length {length} is outside byte range 0x{bytes:X}..0x{byte_end:X} and wide range 0x{aux_rows:X}..0x{aux_end:X}"
            )));
        }
    }
    Ok(value)
}

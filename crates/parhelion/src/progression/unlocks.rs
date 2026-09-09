use super::*;

pub(crate) fn first_free_unlock_slot(
    data: &[u8],
    rows: usize,
    count: usize,
    bank: u8,
    capacity: usize,
) -> AuthoringResult<u16> {
    let mut used = BTreeSet::new();
    for index in 0..count {
        let row = rows + index * UNLOCK_ROW_SIZE;
        if read_u16(data, row + 4)? == u16::from(bank) {
            let slot = read_u16(data, row + 6)?;
            if slot != u16::MAX {
                used.insert(usize::from(slot));
            }
        }
    }
    (0..capacity)
        .find(|slot| !used.contains(slot))
        .and_then(|slot| u16::try_from(slot).ok())
        .ok_or_else(|| invalid(format!("Unlock bank {bank} has no free compact slot")))
}

pub(crate) fn unlock_flag_bank_descriptor(bank: u8) -> AuthoringResult<usize> {
    let bank_index = usize::from(
        bank.checked_sub(1)
            .ok_or_else(|| invalid("Unlock flag banks are one-based"))?,
    );
    8usize
        .checked_add(
            bank_index
                .checked_mul(16)
                .ok_or_else(|| invalid("Unlock flag-bank descriptor overflowed"))?,
        )
        .ok_or_else(|| invalid("Unlock flag-bank descriptor overflowed"))
}

pub(super) fn unlock_table_layout(data: &[u8]) -> AuthoringResult<UnlockTableLayout> {
    let (count, primary_header, primary_rows, class) = array_at(data, 8)?;
    let (secondary_count, secondary_header, secondary_rows, secondary_class) =
        array_at(data, 0x18)?;
    let primary_end = primary_rows
        .checked_add(
            count
                .checked_mul(UNLOCK_ROW_SIZE)
                .ok_or_else(|| invalid("Unlock definition row extent overflowed"))?,
        )
        .ok_or_else(|| invalid("Unlock definition row extent overflowed"))?;
    let secondary_end = secondary_rows
        .checked_add(
            secondary_count
                .checked_mul(UNLOCK_FLAG_SORTED_INDEX_ROW_SIZE)
                .ok_or_else(|| invalid("Unlock sorted-index extent overflowed"))?,
        )
        .ok_or_else(|| invalid("Unlock sorted-index extent overflowed"))?;
    if class != UNLOCK_FLAG_DEFINITION_ROW_CLASS
        || secondary_class != UNLOCK_FLAG_SORTED_INDEX_ROW_CLASS
        || count != secondary_count
        || primary_end > data.len()
        || secondary_end != data.len()
    {
        return Err(invalid(
            "Unlock table primary and sorted-index arrays are inconsistent",
        ));
    }
    let order = (0..secondary_count)
        .map(|index| {
            let row = secondary_rows
                .checked_add(
                    index
                        .checked_mul(UNLOCK_FLAG_SORTED_INDEX_ROW_SIZE)
                        .ok_or_else(|| invalid("Unlock sorted-index row overflowed"))?,
                )
                .ok_or_else(|| invalid("Unlock sorted-index row overflowed"))?;
            read_u16(data, row).map(usize::from)
        })
        .collect::<AuthoringResult<Vec<_>>>()?;
    if order.iter().any(|index| *index >= count)
        || order.iter().copied().collect::<BTreeSet<_>>().len() != count
    {
        return Err(invalid(
            "Unlock sorted index is not a complete permutation of the definition rows",
        ));
    }
    let ordered_hashes = order
        .iter()
        .map(|index| {
            let row = primary_rows
                .checked_add(
                    index
                        .checked_mul(UNLOCK_ROW_SIZE)
                        .ok_or_else(|| invalid("Unlock definition row overflowed"))?,
                )
                .ok_or_else(|| invalid("Unlock definition row overflowed"))?;
            read_u32(data, row)
        })
        .collect::<AuthoringResult<Vec<_>>>()?;
    if ordered_hashes.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(invalid("Unlock secondary index is not sorted by hash"));
    }
    Ok(UnlockTableLayout {
        count,
        primary_header,
        primary_rows,
        primary_end,
        secondary_header,
        secondary_rows,
        order,
        ordered_hashes,
    })
}

pub(super) fn pointer_targets(
    data: &[u8],
    row: usize,
    rows_end: usize,
) -> AuthoringResult<Vec<(usize, usize)>> {
    let mut targets = Vec::new();
    for field in COLLECTIBLE_POINTER_FIELDS {
        let nested_count = read_u64(data, row + field)? as usize;
        if nested_count == 0 {
            continue;
        }
        let target = relative_target(data, row + field + 8)?;
        if target < rows_end {
            return Err(invalid("Collectible nested array points into fixed rows"));
        }
        targets.push((field, target));
    }
    Ok(targets)
}

pub(super) fn rebase_row_pointers(
    data: &mut [u8],
    row: usize,
    old_rows_end: usize,
    shift: usize,
) -> AuthoringResult<()> {
    for field in COLLECTIBLE_POINTER_FIELDS {
        if read_u64(data, row + field)? == 0 {
            continue;
        }
        let target = relative_target(data, row + field + 8)?;
        if target < old_rows_end {
            return Err(invalid(
                "Collectible pointer unexpectedly targets fixed rows",
            ));
        }
        write_relative_pointer(data, row + field + 8, target + shift)?;
    }
    Ok(())
}

pub(crate) fn append_unlock_flag_bank_row(
    mut data: Vec<u8>,
    bank: u8,
    slot: u16,
    hash: u32,
    unlock_definition_index: u16,
) -> AuthoringResult<Vec<u8>> {
    let descriptor = unlock_flag_bank_descriptor(bank)?;
    let (count, header, rows, class) = array_at(&data, descriptor)?;
    if class != UNLOCK_FLAG_BANK_ROW_CLASS || rows != header + 16 || count != usize::from(slot) {
        return Err(invalid(
            "Unlock definition does not target the terminal slot of its flag bank",
        ));
    }
    let rows_end = rows
        .checked_add(
            count
                .checked_mul(UNLOCK_FLAG_BANK_ROW_SIZE)
                .ok_or_else(|| invalid("Unlock flag-bank row size overflowed"))?,
        )
        .ok_or_else(|| invalid("Unlock flag-bank row range overflowed"))?;
    if rows_end > data.len() {
        return Err(invalid("Unlock flag-bank rows extend beyond the table"));
    }
    if (0..count)
        .any(|index| read_u32(&data, rows + index * UNLOCK_FLAG_BANK_ROW_SIZE).ok() == Some(hash))
    {
        return Err(invalid(
            "The authored unlock already exists in the unlock flag bank",
        ));
    }

    let descriptor_end = data
        .get(0..header)
        .ok_or_else(|| invalid("Unlock flag-bank header is outside the table"))?
        .len();
    let mut arrays = Vec::new();
    for candidate in (8..descriptor_end).step_by(16) {
        let Ok((array_count, array_header, array_rows, array_class)) = array_at(&data, candidate)
        else {
            continue;
        };
        if array_class != UNLOCK_FLAG_BANK_ROW_CLASS || array_rows != array_header + 16 {
            return Err(invalid(
                "Unlock flag-bank table contains an incompatible bank",
            ));
        }
        arrays.push((candidate, array_count, array_header));
    }
    if arrays
        .iter()
        .all(|(candidate, _, array_header)| *candidate != descriptor || *array_header != header)
    {
        return Err(invalid(
            "Unlock flag bank is missing from its descriptor table",
        ));
    }
    let segment_end = arrays
        .iter()
        .filter_map(|(_, _, array_header)| (*array_header > header).then_some(*array_header))
        .min()
        .unwrap_or(data.len());
    let trailer_start = segment_end
        .checked_sub(NESTED_ARRAY_TRAILER.len())
        .ok_or_else(|| invalid("Unlock flag-bank trailer underflowed"))?;
    if rows_end > trailer_start
        || data[rows_end..trailer_start].iter().any(|byte| *byte != 0)
        || data.get(trailer_start..segment_end) != Some(&NESTED_ARRAY_TRAILER)
    {
        return Err(invalid("Unlock flag bank has an unexpected array trailer"));
    }

    let tail = data[segment_end..].to_vec();
    let mut authored = data[header..rows_end].to_vec();
    write_u64(&mut authored, 0, (count + 1) as u64)?;
    authored.extend_from_slice(&hash.to_le_bytes());
    authored.extend_from_slice(&u32::from(unlock_definition_index).to_le_bytes());
    while (authored.len() + NESTED_ARRAY_TRAILER.len()) % 16 != 0 {
        authored.push(0);
    }
    authored.extend_from_slice(&NESTED_ARRAY_TRAILER);
    let old_segment_size = segment_end - header;
    let new_segment_size = authored.len();
    if new_segment_size < old_segment_size {
        return Err(invalid("Unlock flag-bank append shrank its array"));
    }
    let shift = new_segment_size - old_segment_size;
    data.splice(header..segment_end, authored);
    set_array_count(&mut data, descriptor, header, count + 1)?;
    for (candidate, _, array_header) in &arrays {
        if *array_header > header {
            write_relative_pointer(&mut data, *candidate + 8, *array_header + shift)?;
        }
    }
    if data.get(header + new_segment_size..) != Some(tail.as_slice()) {
        return Err(validation(
            "Appending the unlock flag changed a later flag bank",
        ));
    }
    validate_authored_unlock_flag_bank_row(&data, bank, slot, hash, unlock_definition_index)?;
    Ok(data)
}

pub(crate) fn validate_authored_unlock_flag_bank_row(
    data: &[u8],
    bank: u8,
    slot: u16,
    hash: u32,
    unlock_definition_index: u16,
) -> AuthoringResult<()> {
    let descriptor = unlock_flag_bank_descriptor(bank)?;
    let (count, _, rows, class) = array_at(data, descriptor)?;
    let row_index = usize::from(slot);
    let row = rows
        .checked_add(
            row_index
                .checked_mul(UNLOCK_FLAG_BANK_ROW_SIZE)
                .ok_or_else(|| validation("Authored unlock flag-bank row overflowed"))?,
        )
        .ok_or_else(|| validation("Authored unlock flag-bank row overflowed"))?;
    if class != UNLOCK_FLAG_BANK_ROW_CLASS
        || row_index >= count
        || read_u32(data, row)? != hash
        || read_u32(data, row + 4)? != u32::from(unlock_definition_index)
    {
        return Err(validation("Authored unlock flag-bank row is inconsistent"));
    }
    Ok(())
}

pub(crate) fn append_unlock(
    mut data: Vec<u8>,
    hash: u32,
    bank: u8,
    slot: u16,
) -> AuthoringResult<Vec<u8>> {
    let layout = unlock_table_layout(&data)?;
    if layout.ordered_hashes.binary_search(&hash).is_ok() {
        return Err(invalid(format!(
            "Authored unlock hash 0x{hash:08X} already exists in the unlock table"
        )));
    }
    let insertion = layout.ordered_hashes.partition_point(|value| *value < hash);
    let appended_index = u16::try_from(layout.count)
        .map_err(|_| invalid("Unlock sorted index exceeds its native 16-bit field"))?;
    data.splice(layout.primary_end..layout.primary_end, [0; UNLOCK_ROW_SIZE]);
    write_u32(&mut data, layout.primary_end, hash)?;
    write_u16(&mut data, layout.primary_end + 4, u16::from(bank))?;
    write_u16(&mut data, layout.primary_end + 6, slot)?;
    set_array_count(&mut data, 8, layout.primary_header, layout.count + 1)?;
    let shifted_secondary_header = layout
        .secondary_header
        .checked_add(UNLOCK_ROW_SIZE)
        .ok_or_else(|| invalid("Unlock sorted-index header overflowed"))?;
    let shifted_secondary_rows = layout
        .secondary_rows
        .checked_add(UNLOCK_ROW_SIZE)
        .ok_or_else(|| invalid("Unlock sorted-index rows overflowed"))?;
    write_relative_pointer(&mut data, 0x20, shifted_secondary_header)?;
    let sorted_insertion = insertion
        .checked_mul(size_of::<u16>())
        .and_then(|offset| shifted_secondary_rows.checked_add(offset))
        .ok_or_else(|| invalid("Unlock sorted-index insertion offset overflowed"))?;
    data.splice(
        sorted_insertion..sorted_insertion,
        appended_index.to_le_bytes(),
    );
    set_array_count(&mut data, 0x18, shifted_secondary_header, layout.count + 1)?;
    let authored = unlock_table_layout(&data)?;
    if authored.count != layout.count + 1
        || authored.primary_rows != layout.primary_rows
        || authored.order.get(insertion).copied() != Some(layout.count)
        || authored.ordered_hashes.get(insertion).copied() != Some(hash)
        || read_u32(&data, layout.primary_end)? != hash
        || read_u16(&data, layout.primary_end + 4)? != u16::from(bank)
        || read_u16(&data, layout.primary_end + 6)? != slot
    {
        return Err(validation("Authored unlock table is inconsistent"));
    }
    Ok(data)
}

pub(crate) fn unlock_sorted_index_position(
    data: &[u8],
    unlock_definition_index: u16,
) -> AuthoringResult<usize> {
    let layout = unlock_table_layout(data)?;
    let positions = layout
        .order
        .iter()
        .enumerate()
        .filter_map(|(position, index)| {
            (*index == usize::from(unlock_definition_index)).then_some(position)
        })
        .collect::<Vec<_>>();
    let [position] = positions.as_slice() else {
        return Err(validation(
            "Authored unlock definition does not have exactly one sorted-index row",
        ));
    };
    Ok(*position)
}

pub(super) fn unlock_display_table_layout(
    data: &[u8],
) -> AuthoringResult<UnlockDisplayTableLayout> {
    let (count, primary_header, primary_rows, class) = array_at(data, 8)?;
    let (content_count, content_header, content_rows, content_class) = array_at(data, 0x18)?;
    let primary_end = primary_rows
        .checked_add(
            count
                .checked_mul(UNLOCK_FLAG_DISPLAY_ROW_SIZE)
                .ok_or_else(|| invalid("Unlock-display row extent overflowed"))?,
        )
        .ok_or_else(|| invalid("Unlock-display row extent overflowed"))?;
    let content_end = content_rows
        .checked_add(
            content_count
                .checked_mul(UNLOCK_FLAG_DISPLAY_CONTENT_ROW_SIZE)
                .ok_or_else(|| invalid("Unlock-display content extent overflowed"))?,
        )
        .ok_or_else(|| invalid("Unlock-display content extent overflowed"))?;
    let trailer_start = content_header
        .checked_sub(NESTED_ARRAY_TRAILER.len())
        .ok_or_else(|| invalid("Unlock-display array trailer underflowed"))?;
    if class != UNLOCK_FLAG_DISPLAY_ROW_CLASS
        || content_class != UNLOCK_FLAG_DISPLAY_CONTENT_ROW_CLASS
        || content_count == 0
        || primary_end > trailer_start
        || data[primary_end..trailer_start]
            .iter()
            .any(|byte| *byte != 0)
        || data.get(trailer_start..content_header) != Some(&NESTED_ARRAY_TRAILER)
        || content_rows != content_header + 16
        || content_end != data.len()
    {
        return Err(invalid(
            "Unlock-display rows and content array have incompatible native layouts",
        ));
    }
    for index in 0..count {
        let pointer = primary_rows
            .checked_add(
                index
                    .checked_mul(UNLOCK_FLAG_DISPLAY_ROW_SIZE)
                    .ok_or_else(|| invalid("Unlock-display row offset overflowed"))?,
            )
            .and_then(|row| row.checked_add(8))
            .ok_or_else(|| invalid("Unlock-display row offset overflowed"))?;
        let target = relative_target(data, pointer)?;
        if target < content_rows
            || target >= content_end
            || (target - content_rows) % UNLOCK_FLAG_DISPLAY_CONTENT_ROW_SIZE != 0
        {
            return Err(invalid(
                "Unlock-display row does not reference a complete content row",
            ));
        }
    }
    Ok(UnlockDisplayTableLayout {
        count,
        primary_header,
        primary_rows,
        primary_end,
        content_header,
        content_rows,
        content_count,
    })
}

pub(crate) fn append_unlock_display(
    mut data: Vec<u8>,
    template_index: usize,
    hash: u32,
    expected_count: usize,
) -> AuthoringResult<Vec<u8>> {
    let layout = unlock_display_table_layout(&data)?;
    if layout.count != expected_count || template_index >= layout.count {
        return Err(invalid(
            "Unlock display table does not match its definitions",
        ));
    }
    if (0..layout.count).any(|index| {
        read_u32(
            &data,
            layout.primary_rows + index * UNLOCK_FLAG_DISPLAY_ROW_SIZE,
        )
        .ok()
            == Some(hash)
    }) {
        return Err(invalid(
            "Authored unlock hash already exists in the unlock-display table",
        ));
    }
    let template_pointer = layout.primary_rows + template_index * UNLOCK_FLAG_DISPLAY_ROW_SIZE + 8;
    let template_target = relative_target(&data, template_pointer)?;
    data.splice(
        layout.primary_end..layout.primary_end,
        [0; UNLOCK_FLAG_DISPLAY_ROW_SIZE],
    );
    for index in 0..layout.count {
        let pointer = layout.primary_rows + index * UNLOCK_FLAG_DISPLAY_ROW_SIZE + 8;
        let target = relative_target(&data, pointer)?;
        write_relative_pointer(&mut data, pointer, target + UNLOCK_DISPLAY_ROW_SIZE)?;
    }
    write_u32(&mut data, layout.primary_end, hash)?;
    write_u32(&mut data, layout.primary_end + 4, 0)?;
    write_relative_pointer(
        &mut data,
        layout.primary_end + 8,
        template_target + UNLOCK_DISPLAY_ROW_SIZE,
    )?;
    set_array_count(&mut data, 8, layout.primary_header, layout.count + 1)?;
    write_relative_pointer(
        &mut data,
        0x20,
        layout.content_header + UNLOCK_DISPLAY_ROW_SIZE,
    )?;
    let authored = unlock_display_table_layout(&data)?;
    let authored_row = authored.primary_end - UNLOCK_FLAG_DISPLAY_ROW_SIZE;
    if authored.count != layout.count + 1
        || authored.content_count != layout.content_count
        || authored.content_rows != layout.content_rows + UNLOCK_FLAG_DISPLAY_ROW_SIZE
        || read_u32(&data, authored_row)? != hash
        || relative_target(&data, authored_row + 8)?
            != template_target + UNLOCK_FLAG_DISPLAY_ROW_SIZE
    {
        return Err(validation("Authored unlock-display table is inconsistent"));
    }
    Ok(data)
}

pub(crate) fn validate_authored_unlock_display_row(
    data: &[u8],
    unlock_definition_index: u16,
    hash: u32,
) -> AuthoringResult<()> {
    let layout = unlock_display_table_layout(data)?;
    let row_index = usize::from(unlock_definition_index);
    if row_index >= layout.count {
        return Err(validation(
            "Authored unlock display is outside its final table",
        ));
    }
    let row = layout
        .primary_rows
        .checked_add(
            row_index
                .checked_mul(UNLOCK_FLAG_DISPLAY_ROW_SIZE)
                .ok_or_else(|| validation("Authored unlock-display row overflowed"))?,
        )
        .ok_or_else(|| validation("Authored unlock-display row overflowed"))?;
    if read_u32(data, row)? != hash {
        return Err(validation(
            "Authored unlock-display hash does not match its definition",
        ));
    }
    Ok(())
}

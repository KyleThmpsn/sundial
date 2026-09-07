//! Native table rows operations with independent validation.
use super::*;

pub(super) fn dense_item_presentation_next_header(rows_end: usize) -> AuthoringResult<usize> {
    rows_end
        .checked_add(size_of::<u32>())
        .and_then(|value| value.checked_add(15))
        .map(|value| value & !15)
        .ok_or_else(|| invalid("Dense item-presentation array alignment overflowed"))
}

pub(super) fn dense_item_presentation_arrays(
    data: &[u8],
) -> AuthoringResult<[DenseItemPresentationArray; 5]> {
    let mut arrays = Vec::with_capacity(DENSE_ITEM_PRESENTATION_ARRAYS.len());
    for spec in DENSE_ITEM_PRESENTATION_ARRAYS {
        let (count, header, rows, class) = array_at(data, spec.descriptor)?;
        let rows_end = rows
            .checked_add(
                count
                    .checked_mul(spec.row_size)
                    .ok_or_else(|| invalid("Dense item-presentation row size overflowed"))?,
            )
            .ok_or_else(|| invalid("Dense item-presentation row range overflowed"))?;
        if class != spec.row_class
            || header % 16 != 0
            || read_u32(data, header + 12)? != 0
            || rows_end > data.len()
        {
            return Err(invalid(
                "Dense item-presentation array has an unsupported shape",
            ));
        }
        arrays.push(DenseItemPresentationArray {
            spec,
            count,
            header,
            rows,
            rows_end,
        });
    }
    let arrays: [DenseItemPresentationArray; 5] = arrays
        .try_into()
        .map_err(|_| invalid("Dense item-presentation array count is invalid"))?;
    let first_header = (ITEM_DENSE_PRESENTATION_NEXT_DESCRIPTOR + 16 + 15) & !15;
    if arrays[0].header != first_header {
        return Err(invalid(
            "Dense item-presentation first array is not aligned after its descriptors",
        ));
    }
    for pair in arrays.windows(2) {
        let current = pair[0];
        let next = pair[1];
        let expected_header = dense_item_presentation_next_header(current.rows_end)?;
        if next.header != expected_header {
            return Err(invalid(
                "Dense item-presentation arrays are not canonically aligned",
            ));
        }
        let trailer = &data[current.rows_end..next.header];
        if trailer.len() < size_of::<u32>()
            || trailer[..trailer.len() - size_of::<u32>()]
                .iter()
                .any(|byte| *byte != 0)
            || read_u32(data, next.header - size_of::<u32>())?
                != ITEM_DENSE_PRESENTATION_TRAILER_CLASS
        {
            return Err(invalid(
                "Dense item-presentation array trailer is not canonical",
            ));
        }
    }
    let payload_size = usize::try_from(read_u64(data, 0)?)
        .map_err(|_| invalid("Dense item-presentation payload size is too large"))?;
    if arrays[4].rows_end != data.len() || payload_size != data.len() {
        return Err(invalid(
            "Dense item-presentation terminal array does not end at the payload extent",
        ));
    }
    Ok(arrays)
}

pub(super) fn rebuild_dense_item_presentation_arrays(
    data: &[u8],
    additions: [Vec<u8>; 5],
) -> AuthoringResult<Vec<u8>> {
    let arrays = dense_item_presentation_arrays(data)?;
    let added_size = additions.iter().try_fold(0usize, |total, rows| {
        total
            .checked_add(rows.len())
            .ok_or_else(|| invalid("Dense item-presentation additions overflowed"))
    })?;
    let capacity = data
        .len()
        .checked_add(added_size)
        .and_then(|value| value.checked_add(64))
        .ok_or_else(|| invalid("Dense item-presentation payload size overflowed"))?;
    let mut rebuilt = Vec::with_capacity(capacity);
    rebuilt.extend_from_slice(&data[..arrays[0].header]);
    let mut rebuilt_headers = [0usize; 5];
    let mut rebuilt_counts = [0usize; 5];

    for (index, (array, appended_rows)) in arrays.iter().zip(additions).enumerate() {
        if appended_rows.len() % array.spec.row_size != 0 || rebuilt.len() % 16 != 0 {
            return Err(invalid(
                "Dense item-presentation appended rows are not aligned",
            ));
        }
        rebuilt_headers[index] = rebuilt.len();
        rebuilt.extend_from_slice(&data[array.header..array.rows]);
        rebuilt.extend_from_slice(&data[array.rows..array.rows_end]);
        rebuilt.extend_from_slice(&appended_rows);
        rebuilt_counts[index] = array
            .count
            .checked_add(appended_rows.len() / array.spec.row_size)
            .ok_or_else(|| invalid("Dense item-presentation array count overflowed"))?;
        if index + 1 < arrays.len() {
            let next_header = dense_item_presentation_next_header(rebuilt.len())?;
            rebuilt.resize(next_header, 0);
            write_u32(
                &mut rebuilt,
                next_header - size_of::<u32>(),
                ITEM_DENSE_PRESENTATION_TRAILER_CLASS,
            )?;
        }
    }

    for (index, spec) in DENSE_ITEM_PRESENTATION_ARRAYS.iter().enumerate() {
        set_array_count(
            &mut rebuilt,
            spec.descriptor,
            rebuilt_headers[index],
            rebuilt_counts[index],
        )?;
        write_relative_pointer(&mut rebuilt, spec.descriptor + 8, rebuilt_headers[index])?;
    }
    let payload_size = u64::try_from(rebuilt.len())
        .map_err(|_| invalid("Dense item-presentation payload size is too large"))?;
    write_u64(&mut rebuilt, 0, payload_size)?;
    dense_item_presentation_arrays(&rebuilt)?;
    Ok(rebuilt)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn append_dense_item_presentation(
    data: Vec<u8>,
    template_item_index: usize,
    expected_item_count: usize,
    donor_icon_container: TagHash,
    authored_icon_container: TagHash,
) -> AuthoringResult<Vec<u8>> {
    let arrays = dense_item_presentation_arrays(&data)?;
    let icon_tags = arrays[0];
    let icon_selectors = arrays[2];
    let presentations = arrays[3];
    if presentations.count != expected_item_count || template_item_index >= presentations.count {
        return Err(invalid(
            "Dense item-presentation table is not aligned with the stock item table",
        ));
    }

    let template_row = presentations.rows + template_item_index * ITEM_DENSE_PRESENTATION_ROW_SIZE;
    let template_selector_index = usize::try_from(read_u32(
        &data,
        template_row + ITEM_DENSE_PRESENTATION_SELECTOR_OFFSET,
    )?)
    .map_err(|_| invalid("Dense item-presentation selector index is too large"))?;
    if template_selector_index >= icon_selectors.count {
        return Err(invalid("Dense item-presentation donor selector is invalid"));
    }
    let template_selector_row =
        icon_selectors.rows + template_selector_index * ITEM_DENSE_ICON_SELECTOR_ROW_SIZE;
    let template_icon_tag_index = usize::try_from(read_u32(&data, template_selector_row)?)
        .map_err(|_| invalid("Dense icon tag index is too large"))?;
    if template_icon_tag_index >= icon_tags.count
        || read_u32(
            &data,
            icon_tags.rows + template_icon_tag_index * ITEM_DENSE_ICON_TAG_ROW_SIZE,
        )? != donor_icon_container.0
    {
        return Err(invalid(
            "Dense item-presentation donor selector does not resolve to its item icon container",
        ));
    }

    let authored_icon_tag_index = u32::try_from(icon_tags.count)
        .map_err(|_| invalid("Authored dense icon tag index does not fit 32 bits"))?;
    let authored_selector_index = u32::try_from(icon_selectors.count)
        .map_err(|_| invalid("Authored dense icon selector index does not fit 32 bits"))?;
    let mut authored_selector = data
        [template_selector_row..template_selector_row + ITEM_DENSE_ICON_SELECTOR_ROW_SIZE]
        .to_vec();
    write_u32(&mut authored_selector, 0, authored_icon_tag_index)?;
    let mut authored_presentation =
        data[template_row..template_row + ITEM_DENSE_PRESENTATION_ROW_SIZE].to_vec();
    write_u32(
        &mut authored_presentation,
        ITEM_DENSE_PRESENTATION_SELECTOR_OFFSET,
        authored_selector_index,
    )?;
    rebuild_dense_item_presentation_arrays(
        &data,
        [
            authored_icon_container.0.to_le_bytes().to_vec(),
            Vec::new(),
            authored_selector,
            authored_presentation,
            Vec::new(),
        ],
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn validate_dense_item_presentation(
    data: &[u8],
    template_item_index: usize,
    expected_count: usize,
    donor_icon_container: TagHash,
    authored_icon_container: TagHash,
) -> AuthoringResult<()> {
    let arrays = dense_item_presentation_arrays(data)?;
    let icon_tags = arrays[0];
    let icon_selectors = arrays[2];
    let presentations = arrays[3];
    if presentations.count != expected_count
        || presentations.count == 0
        || icon_tags.count == 0
        || icon_selectors.count == 0
        || template_item_index >= presentations.count - 1
    {
        return Err(validation(
            "Authored dense item-presentation table has an invalid shape",
        ));
    }
    let template_row = presentations.rows + template_item_index * ITEM_DENSE_PRESENTATION_ROW_SIZE;
    let authored_row =
        presentations.rows + (presentations.count - 1) * ITEM_DENSE_PRESENTATION_ROW_SIZE;
    let template = &data[template_row..template_row + ITEM_DENSE_PRESENTATION_ROW_SIZE];
    let template_selector_index =
        usize::try_from(read_u32(template, ITEM_DENSE_PRESENTATION_SELECTOR_OFFSET)?)
            .map_err(|_| validation("Dense donor icon selector index is too large"))?;
    let authored_selector_index = usize::try_from(read_u32(
        data,
        authored_row + ITEM_DENSE_PRESENTATION_SELECTOR_OFFSET,
    )?)
    .map_err(|_| validation("Dense authored icon selector index is too large"))?;
    if template_selector_index >= icon_selectors.count
        || authored_selector_index != icon_selectors.count - 1
        || authored_selector_index == template_selector_index
    {
        return Err(validation(
            "Authored dense item-presentation row does not use a private icon selector",
        ));
    }
    let template_selector_row =
        icon_selectors.rows + template_selector_index * ITEM_DENSE_ICON_SELECTOR_ROW_SIZE;
    let authored_selector_row =
        icon_selectors.rows + authored_selector_index * ITEM_DENSE_ICON_SELECTOR_ROW_SIZE;
    let template_icon_tag_index = usize::try_from(read_u32(data, template_selector_row)?)
        .map_err(|_| validation("Dense donor icon tag index is too large"))?;
    let authored_icon_tag_index = usize::try_from(read_u32(data, authored_selector_row)?)
        .map_err(|_| validation("Dense authored icon tag index is too large"))?;
    if template_icon_tag_index >= icon_tags.count
        || authored_icon_tag_index != icon_tags.count - 1
        || read_u32(
            data,
            icon_tags.rows + template_icon_tag_index * ITEM_DENSE_ICON_TAG_ROW_SIZE,
        )? != donor_icon_container.0
        || read_u32(
            data,
            icon_tags.rows + authored_icon_tag_index * ITEM_DENSE_ICON_TAG_ROW_SIZE,
        )? != authored_icon_container.0
        || data[template_selector_row + 4..template_selector_row + 8]
            != data[authored_selector_row + 4..authored_selector_row + 8]
    {
        return Err(validation(
            "Authored dense icon selector does not privately resolve to the authored icon container",
        ));
    }
    let mut normalized_authored =
        data[authored_row..authored_row + ITEM_DENSE_PRESENTATION_ROW_SIZE].to_vec();
    write_u32(
        &mut normalized_authored,
        ITEM_DENSE_PRESENTATION_SELECTOR_OFFSET,
        u32::try_from(template_selector_index)
            .map_err(|_| validation("Dense donor icon selector index does not fit 32 bits"))?,
    )?;
    if template != normalized_authored {
        return Err(validation(
            "The authored dense item-presentation row changed outside its icon selector",
        ));
    }
    Ok(())
}

/// Classification changes must reach the dense UI cache as well as the plug and string
/// definitions. Preserve the source icon selector and every other presentation field.
pub(super) fn set_dense_item_presentation_type(
    data: &mut [u8],
    authored_index: usize,
    classification_index: usize,
) -> AuthoringResult<()> {
    let presentations = dense_item_presentation_arrays(data)?[3];
    if authored_index >= presentations.count || classification_index >= authored_index {
        return Err(invalid(
            "Dense private-plug classification indices are invalid",
        ));
    }
    let field = |index| {
        presentations.rows
            + index * ITEM_DENSE_PRESENTATION_ROW_SIZE
            + ITEM_DENSE_PRESENTATION_TYPE_OFFSET
    };
    let expected = read_u32(data, field(classification_index))?;
    write_u32(data, field(authored_index), expected)
}

pub(super) fn append_terminal_keyed_row(
    mut data: Vec<u8>,
    template_hash: u32,
    new_hash: u32,
    row_size: usize,
    row_class: u32,
    description: &str,
) -> AuthoringResult<(Vec<u8>, u16)> {
    let (count, header, rows, class) = array_at(&data, 8)?;
    let rows_end = rows
        .checked_add(
            count
                .checked_mul(row_size)
                .ok_or_else(|| invalid(format!("{description} row size overflowed")))?,
        )
        .ok_or_else(|| invalid(format!("{description} row range overflowed")))?;
    if class != row_class || rows_end != data.len() {
        return Err(invalid(format!(
            "{description} is not the expected terminal fixed-row array"
        )));
    }
    let template_index = find_u32_row_key(&data, rows, count, row_size, template_hash)?
        .ok_or_else(|| {
            invalid(format!(
                "The gameplay donor is missing from the {description}"
            ))
        })?;
    if contains_u32_row_key(&data, rows, count, row_size, new_hash)? {
        return Err(invalid(format!(
            "The authored item already exists in the {description}"
        )));
    }
    let new_index = u16::try_from(count)
        .map_err(|_| invalid(format!("The new {description} index does not fit 16 bits")))?;
    let template_row = rows + template_index * row_size;
    let template = data[template_row..template_row + row_size].to_vec();
    data.extend_from_slice(&template);
    write_u32(&mut data, rows_end, new_hash)?;
    set_array_count(&mut data, 8, header, count + 1)?;
    Ok((data, new_index))
}

pub(super) fn append_keyed_auxiliary_pair(
    mut data: Vec<u8>,
    index: Vec<u8>,
    donor_item_hash: u32,
    authored_item_hash: u32,
    layout: KeyedAuxiliaryLayout,
) -> AuthoringResult<(Vec<u8>, Vec<u8>)> {
    let arrays = validate_keyed_auxiliary_alignment(&data, &index, layout)?;
    let count = arrays.count;
    let header = arrays.header;
    let rows = arrays.rows;
    let rows_end = rows
        .checked_add(
            count
                .checked_mul(layout.row_size)
                .ok_or_else(|| invalid(format!("{} row size overflowed", layout.description)))?,
        )
        .ok_or_else(|| invalid(format!("{} row range overflowed", layout.description)))?;
    if rows_end > data.len() {
        return Err(invalid(format!(
            "{} fixed rows extend beyond the table",
            layout.description
        )));
    }
    let template_index = match classify_keyed_auxiliary_donor(
        &data,
        &index,
        donor_item_hash,
        authored_item_hash,
        layout,
    )? {
        KeyedAuxiliaryDonorPresence::Present(index) => index,
        KeyedAuxiliaryDonorPresence::Absent => {
            return Err(invalid(format!(
                "Donor item 0x{donor_item_hash:08X} is missing from the {} table",
                layout.description,
            )));
        }
    };

    let mut nested_targets = Vec::with_capacity(count);
    for row_index in 0..count {
        let descriptor = rows + row_index * layout.row_size + layout.nested_offset;
        let nested_count = read_u64(&data, descriptor)?;
        if nested_count == 0 {
            nested_targets.push(None);
            continue;
        }
        let (_, nested_header, _, nested_class) = array_at(&data, descriptor)?;
        if nested_class != layout.nested_class || nested_header < rows_end {
            return Err(invalid(format!(
                "{} row {row_index} has an incompatible nested array",
                layout.description
            )));
        }
        nested_targets.push(Some(nested_header));
    }
    let template_target = nested_targets[template_index];
    let secondary_header = layout
        .secondary_class
        .map(|expected_class| {
            let (_, secondary_header, _, secondary_class) = array_at(&data, 0x18)?;
            if secondary_class != expected_class || secondary_header < rows_end {
                return Err(invalid(format!(
                    "{} secondary array is incompatible",
                    layout.description
                )));
            }
            Ok(secondary_header)
        })
        .transpose()?;

    let template_row = rows + template_index * layout.row_size;
    let template = data[template_row..template_row + layout.row_size].to_vec();
    data.splice(rows_end..rows_end, std::iter::repeat_n(0, layout.row_size));
    for (row_index, target) in nested_targets.into_iter().enumerate() {
        if let Some(target) = target {
            let pointer = rows + row_index * layout.row_size + layout.nested_offset + 8;
            write_relative_pointer(&mut data, pointer, target + layout.row_size)?;
        }
    }
    data[rows_end..rows_end + layout.row_size].copy_from_slice(&template);
    write_u32(&mut data, rows_end, authored_item_hash)?;
    if let Some(template_target) = template_target {
        write_relative_pointer(
            &mut data,
            rows_end + layout.nested_offset + 8,
            template_target + layout.row_size,
        )?;
    }
    set_array_count(&mut data, 8, header, count + 1)?;
    if let Some(secondary_header) = secondary_header {
        write_relative_pointer(&mut data, 0x20, secondary_header + layout.row_size)?;
    }

    let (index, new_index) = append_terminal_keyed_row(
        index,
        donor_item_hash,
        authored_item_hash,
        layout.index_row_size,
        layout.index_row_class,
        &format!("{} companion index", layout.description),
    )?;
    if usize::from(new_index) != count {
        return Err(invalid(format!(
            "{} and its companion index were not aligned before authoring",
            layout.description
        )));
    }
    Ok((data, index))
}

pub(super) fn validate_keyed_auxiliary_alignment(
    data: &[u8],
    index: &[u8],
    layout: KeyedAuxiliaryLayout,
) -> AuthoringResult<KeyedAuxiliaryArrays> {
    let (count, header, rows, class) = array_at(data, 8)?;
    let (index_count, _, index_rows, index_class) = array_at(index, 8)?;
    let rows_end = rows
        .checked_add(
            count
                .checked_mul(layout.row_size)
                .ok_or_else(|| invalid(format!("{} row size overflowed", layout.description)))?,
        )
        .ok_or_else(|| invalid(format!("{} row range overflowed", layout.description)))?;
    let index_rows_end = index_rows
        .checked_add(
            index_count
                .checked_mul(layout.index_row_size)
                .ok_or_else(|| {
                    invalid(format!(
                        "{} companion-index row size overflowed",
                        layout.description
                    ))
                })?,
        )
        .ok_or_else(|| {
            invalid(format!(
                "{} companion-index row range overflowed",
                layout.description
            ))
        })?;
    if class != layout.row_class
        || index_class != layout.index_row_class
        || count != index_count
        || rows_end > data.len()
        || index_rows_end != index.len()
    {
        return Err(invalid(format!(
            "{} table and companion index are not aligned fixed-row arrays",
            layout.description
        )));
    }

    let mut keys = BTreeSet::new();
    for row_index in 0..count {
        let primary_key = read_u32(data, rows + row_index * layout.row_size)?;
        let index_key = read_u32(index, index_rows + row_index * layout.index_row_size)?;
        if primary_key != index_key || !keys.insert(primary_key) {
            return Err(invalid(format!(
                "{} primary/index key mismatch or duplicate at row {row_index}",
                layout.description
            )));
        }
    }
    Ok(KeyedAuxiliaryArrays {
        count,
        header,
        rows,
    })
}

pub(super) fn validate_keyed_auxiliary_structure(
    data: &[u8],
    index: &[u8],
    layout: KeyedAuxiliaryLayout,
) -> AuthoringResult<KeyedAuxiliaryArrays> {
    let arrays = validate_keyed_auxiliary_alignment(data, index, layout)?;
    let rows_end = arrays
        .rows
        .checked_add(
            arrays
                .count
                .checked_mul(layout.row_size)
                .ok_or_else(|| validation("Keyed auxiliary row extent overflowed"))?,
        )
        .ok_or_else(|| validation("Keyed auxiliary row extent overflowed"))?;
    for row_index in 0..arrays.count {
        let descriptor = arrays.rows + row_index * layout.row_size + layout.nested_offset;
        if read_u64(data, descriptor)? == 0 {
            continue;
        }
        let (_, nested_header, nested_rows, nested_class) = array_at(data, descriptor)?;
        if nested_class != layout.nested_class
            || nested_header < rows_end
            || nested_rows < nested_header
            || nested_rows > data.len()
        {
            return Err(validation(format!(
                "{} row {row_index} has an invalid nested array after authoring",
                layout.description
            )));
        }
    }
    if let Some(expected_class) = layout.secondary_class {
        let (_, secondary_header, secondary_rows, secondary_class) = array_at(data, 0x18)?;
        if secondary_class != expected_class
            || secondary_header < rows_end
            || secondary_rows < secondary_header
            || secondary_rows > data.len()
        {
            return Err(validation(format!(
                "{} secondary array is invalid after authoring",
                layout.description
            )));
        }
    }
    Ok(arrays)
}

pub(super) fn classify_keyed_auxiliary_donor(
    data: &[u8],
    index: &[u8],
    donor_item_hash: u32,
    authored_item_hash: u32,
    layout: KeyedAuxiliaryLayout,
) -> AuthoringResult<KeyedAuxiliaryDonorPresence> {
    let arrays = validate_keyed_auxiliary_alignment(data, index, layout)?;
    let mut donor_position = None;
    for row_index in 0..arrays.count {
        let key = read_u32(data, arrays.rows + row_index * layout.row_size)?;
        if key == donor_item_hash && donor_position.replace(row_index).is_some() {
            return Err(invalid(format!(
                "Donor item 0x{donor_item_hash:08X} is duplicated in the {} pair",
                layout.description
            )));
        }
        if key == authored_item_hash {
            return Err(invalid(format!(
                "Item 0x{authored_item_hash:08X} already exists in the {} pair",
                layout.description
            )));
        }
    }
    Ok(donor_position.map_or(
        KeyedAuxiliaryDonorPresence::Absent,
        KeyedAuxiliaryDonorPresence::Present,
    ))
}

pub(super) fn append_authored_weapon_icon_row(
    mut icons: Vec<u8>,
    donor_index: u16,
    authored_item_hash: u32,
    authored_container: TagHash,
) -> AuthoringResult<(Vec<u8>, u16)> {
    let (count, header, rows, class) = array_at(&icons, 8)?;
    let rows_end = rows
        .checked_add(
            count
                .checked_mul(ITEM_ICON_ROW_SIZE)
                .ok_or_else(|| invalid("Item-icon row size overflowed"))?,
        )
        .ok_or_else(|| invalid("Item-icon row range overflowed"))?;
    let donor_index = usize::from(donor_index);
    let authored_index = u16::try_from(count)
        .map_err(|_| invalid("Authored weapon icon index does not fit 16 bits"))?;
    if class != ITEM_ICON_ROW_CLASS
        || count <= STOCK_ITEM_ICON_COUNT
        || rows_end != icons.len()
        || donor_index >= STOCK_ITEM_ICON_COUNT
        || contains_u32_row_key(&icons, rows, count, ITEM_ICON_ROW_SIZE, authored_item_hash)?
    {
        return Err(invalid(
            "Authored weapon icon row cannot be appended to the project item-icon table",
        ));
    }
    let donor_row = rows + donor_index * ITEM_ICON_ROW_SIZE;
    let authored_row = item_icon_row_with_container(
        &icons[donor_row..donor_row + ITEM_ICON_ROW_SIZE],
        authored_item_hash,
        authored_container,
    )?;
    icons.extend_from_slice(&authored_row);
    set_array_count(&mut icons, 8, header, count + 1)?;
    Ok((icons, authored_index))
}

pub(super) fn validate_authored_item_icon(
    icons: &[u8],
    strings: &[u8],
    authored_item_hash: u32,
    authored_index: u16,
    authored_container: TagHash,
) -> AuthoringResult<()> {
    let (count, _, rows, class) = array_at(icons, 8)?;
    let authored_index_usize = usize::from(authored_index);
    let row = rows
        .checked_add(
            authored_index_usize
                .checked_mul(ITEM_ICON_ROW_SIZE)
                .ok_or_else(|| validation("Authored item-icon row size overflowed"))?,
        )
        .ok_or_else(|| validation("Authored item-icon row offset overflowed"))?;
    if class != ITEM_ICON_ROW_CLASS
        || authored_index_usize <= STOCK_ITEM_ICON_COUNT
        || authored_index_usize >= count
        || read_u16(strings, ITEM_STRING_ICON_INDEX_OFFSET)? != authored_index
        || read_u32(icons, row)? != authored_item_hash
        || read_u32(icons, row + ITEM_ICON_CONTAINER_OFFSET)? != u32::from(authored_container)
    {
        return Err(validation(
            "Authored item-string and item-icon row do not select the Sunrise container",
        ));
    }
    Ok(())
}

pub(super) fn validate_reused_stock_item_icon(
    icons: &[u8],
    strings: &[u8],
    donor_index: u16,
) -> AuthoringResult<()> {
    stock_item_icon_container(icons, donor_index)?;
    if read_u16(strings, ITEM_STRING_ICON_INDEX_OFFSET)? != donor_index {
        return Err(validation(
            "The authored item's string does not retain a valid stock donor icon row",
        ));
    }
    Ok(())
}

pub(super) fn stock_item_icon_container(icons: &[u8], icon_index: u16) -> AuthoringResult<TagHash> {
    let (count, _, rows, class) = array_at(icons, 8)?;
    let icon_index = usize::from(icon_index);
    if class != ITEM_ICON_ROW_CLASS || count != STOCK_ITEM_ICON_COUNT || icon_index >= count {
        return Err(validation("Stock item icon row is unavailable"));
    }
    let container = TagHash(read_u32(
        icons,
        rows + icon_index * ITEM_ICON_ROW_SIZE + ITEM_ICON_CONTAINER_OFFSET,
    )?);
    if !is_valid_package_tag(container) {
        return Err(validation(
            "Stock item icon row does not reference a valid container tag",
        ));
    }
    Ok(container)
}

pub(super) fn item_hash_index_arrays(data: &[u8]) -> AuthoringResult<[ItemHashIndexArray; 2]> {
    let mut arrays = Vec::with_capacity(ITEM_HASH_INDEX_DESCRIPTORS.len());
    for descriptor in ITEM_HASH_INDEX_DESCRIPTORS {
        let (count, header, rows, class) = array_at(data, descriptor)?;
        let rows_end = rows
            .checked_add(
                count
                    .checked_mul(ITEM_HASH_INDEX_ROW_SIZE)
                    .ok_or_else(|| invalid("Item hash-index row extent overflowed"))?,
            )
            .ok_or_else(|| invalid("Item hash-index row extent overflowed"))?;
        if class != ITEM_HASH_INDEX_ROW_CLASS || rows_end > data.len() {
            return Err(invalid(
                "Item hash-index table has an incompatible row array",
            ));
        }
        arrays.push(ItemHashIndexArray {
            descriptor,
            count,
            header,
            rows,
            rows_end,
        });
    }
    let arrays: [ItemHashIndexArray; 2] = arrays
        .try_into()
        .map_err(|_| invalid("Item hash-index table does not have two arrays"))?;
    if arrays[0].rows_end > arrays[1].header || arrays[1].rows_end != data.len() {
        return Err(invalid(
            "Item hash-index table arrays overlap or do not end at the payload boundary",
        ));
    }
    Ok(arrays)
}

pub(super) fn append_item_hash_index_row(
    mut data: Vec<u8>,
    source_hash: u32,
    source_item_index: u16,
    authored_hash: u32,
    authored_item_index: u16,
) -> AuthoringResult<Vec<u8>> {
    if source_hash == authored_hash || source_item_index == authored_item_index {
        return Err(invalid(
            "Item hash-index authoring requires distinct donor and authored identities",
        ));
    }
    if authored_item_index > i16::MAX as u16 {
        return Err(invalid(
            "Authored item index exceeds the investment registry's signed 16-bit range",
        ));
    }
    let arrays = item_hash_index_arrays(&data)?;
    let mut source = Vec::new();
    for (array_index, array) in arrays.iter().enumerate() {
        for row_index in 0..array.count {
            let row = array.rows + row_index * ITEM_HASH_INDEX_ROW_SIZE;
            let hash = read_u32(&data, row)?;
            let item_index = read_u16(&data, row + ITEM_HASH_INDEX_ITEM_INDEX_OFFSET)?;
            if hash == authored_hash || item_index == authored_item_index {
                return Err(invalid(format!(
                    "Authored item hash-index identity already exists in array {array_index} row {row_index}"
                )));
            }
            if hash == source_hash {
                if item_index != source_item_index {
                    return Err(invalid(format!(
                        "Item hash-index donor 0x{source_hash:08X} points to item {item_index}, not {source_item_index}"
                    )));
                }
                source.push((array_index, row));
            }
        }
    }
    let [(target_index, donor_row)] = source.as_slice() else {
        return Err(invalid(format!(
            "Item hash-index donor 0x{source_hash:08X} occurs {} times; exactly one is required",
            source.len()
        )));
    };
    let target_index = *target_index;
    let target = arrays[target_index];
    let donor = data
        .get(*donor_row..*donor_row + ITEM_HASH_INDEX_ROW_SIZE)
        .ok_or_else(|| invalid("Item hash-index donor row is truncated"))?
        .to_vec();
    let insert_at = target.rows_end;
    data.splice(insert_at..insert_at, donor);
    let new_row = insert_at;
    write_u32(&mut data, new_row, authored_hash)?;
    write_u16(
        &mut data,
        new_row + ITEM_HASH_INDEX_ITEM_INDEX_OFFSET,
        authored_item_index,
    )?;

    for (array_index, array) in arrays.iter().enumerate() {
        let shifted_header =
            array.header + usize::from(array.header >= insert_at) * ITEM_HASH_INDEX_ROW_SIZE;
        write_relative_pointer(&mut data, array.descriptor + 8, shifted_header)?;
        set_array_count(
            &mut data,
            array.descriptor,
            shifted_header,
            array.count + usize::from(array_index == target_index),
        )?;
    }
    let payload_size =
        u64::try_from(data.len()).map_err(|_| invalid("Item hash-index size is too large"))?;
    write_u64(&mut data, 0, payload_size)?;

    let authored = item_hash_index_arrays(&data)?;
    let target = authored[target_index];
    let row = target.rows + (target.count - 1) * ITEM_HASH_INDEX_ROW_SIZE;
    if target.count != arrays[target_index].count + 1
        || read_u32(&data, row)? != authored_hash
        || read_u16(&data, row + ITEM_HASH_INDEX_ITEM_INDEX_OFFSET)? != authored_item_index
    {
        return Err(validation(
            "Authored item hash-index row did not round-trip",
        ));
    }
    Ok(data)
}

pub(super) fn terminal_index_table_layout(
    data: &[u8],
    expected_class: u32,
    description: &str,
) -> AuthoringResult<(usize, usize, usize)> {
    let (count, header, rows, class) = array_at(data, 8)?;
    let rows_end = rows
        .checked_add(
            count
                .checked_mul(ITEM_ROW_SIZE)
                .ok_or_else(|| invalid(format!("{description} row extent overflowed")))?,
        )
        .ok_or_else(|| invalid(format!("{description} row extent overflowed")))?;
    if class != expected_class || rows_end != data.len() {
        return Err(invalid(format!(
            "{description} is not a terminal native fixed-row array"
        )));
    }
    Ok((count, header, rows))
}

pub(super) fn append_index_row(
    mut data: Vec<u8>,
    template_index: usize,
    new_hash: u32,
    new_tag: TagHash,
    expected_class: u32,
    description: &str,
) -> AuthoringResult<Vec<u8>> {
    let (count, header, rows) = terminal_index_table_layout(&data, expected_class, description)?;
    if template_index >= count {
        return Err(invalid(format!(
            "{description} donor template is outside the table"
        )));
    }
    if contains_u32_row_key(&data, rows, count, ITEM_ROW_SIZE, new_hash)? {
        return Err(invalid(format!(
            "Authored hash 0x{new_hash:08X} already exists in the {description}"
        )));
    }
    let end = data.len();
    let template = data
        [rows + template_index * ITEM_ROW_SIZE..rows + (template_index + 1) * ITEM_ROW_SIZE]
        .to_vec();
    data.extend_from_slice(&template);
    write_u32(&mut data, end, new_hash)?;
    write_u32(&mut data, end + 16, new_tag.0)?;
    set_array_count(&mut data, 8, header, count + 1)?;
    let (authored_count, _, authored_rows) =
        terminal_index_table_layout(&data, expected_class, description)?;
    if authored_count != count + 1
        || read_u32(&data, authored_rows + count * ITEM_ROW_SIZE)? != new_hash
        || read_u32(&data, authored_rows + count * ITEM_ROW_SIZE + 16)? != new_tag.0
    {
        return Err(validation(format!(
            "Authored {description} row is inconsistent"
        )));
    }
    Ok(data)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn validate_authored_tables(
    items: &[u8],
    strings: &[u8],
    collectibles: &[u8],
    collectible_displays: &[u8],
    unlocks: &[u8],
    displays: &[u8],
    identity: WeaponCloneIdentity,
    definition_tag: TagHash,
    string_tag: TagHash,
    item_index: u16,
    collectible_index: u16,
    unlock_index: u16,
    unlock_slot: u16,
    collectible_icon_index: u16,
    localization_table_index: u32,
    expected_material_set: u16,
    expected_presentation_parents: &[u16],
    collection_requirement_hash: Option<u32>,
) -> AuthoringResult<()> {
    let (item_count, _, item_rows) = terminal_index_table_layout(
        items,
        ITEM_DEFINITION_INDEX_ROW_CLASS,
        "item-definition index",
    )?;
    let (string_count, _, string_rows) =
        terminal_index_table_layout(strings, ITEM_STRING_INDEX_ROW_CLASS, "item-string index")?;
    if item_count != string_count || item_count.checked_sub(1) != Some(usize::from(item_index)) {
        return Err(validation(
            "Authored item and string tables are not aligned",
        ));
    }
    let item_row = item_rows + usize::from(item_index) * ITEM_ROW_SIZE;
    let string_row = string_rows + usize::from(item_index) * ITEM_ROW_SIZE;
    if read_u32(items, item_row)? != identity.item_hash
        || read_u32(items, item_row + 16)? != definition_tag.0
        || read_u32(strings, string_row)? != identity.item_hash
        || read_u32(strings, string_row + 16)? != string_tag.0
    {
        return Err(validation(
            "Authored item index rows do not point to the new tags",
        ));
    }
    let (collectible_count, _, collectible_rows, collectible_class) = array_at(collectibles, 8)?;
    if collectible_class != COLLECTIBLE_DEFINITION_ROW_CLASS
        || collectible_count.checked_sub(1) != Some(usize::from(collectible_index))
    {
        return Err(validation("Authored collectible count is incorrect"));
    }
    let collectible_row = collectible_rows + usize::from(collectible_index) * COLLECTIBLE_ROW_SIZE;
    let (parent_count, _, parent_rows, parent_class) = array_at(
        collectibles,
        collectible_row + COLLECTIBLE_PRESENTATION_NODE_PARENTS_OFFSET,
    )?;
    let authored_parents = (0..parent_count)
        .map(|position| read_u16(collectibles, parent_rows + position * size_of::<u16>()))
        .collect::<AuthoringResult<Vec<_>>>()?;
    if read_u32(collectibles, collectible_row + COLLECTIBLE_HASH_OFFSET)?
        != identity.collectible_hash
        || collectibles[collectible_row + COLLECTIBLE_CURATED_ACQUISITION_FLAG_OFFSET]
            != COLLECTIBLE_CURATED_ACQUISITION_FLAG
        || read_u16(
            collectibles,
            collectible_row + COLLECTIBLE_REACQUISITION_STATE_OFFSET,
        )? != COLLECTIBLE_REACQUISITION_ENABLED
        || read_u16(
            collectibles,
            collectible_row + COLLECTIBLE_ITEM_INDEX_OFFSET,
        )? != item_index
        || read_u16(
            collectibles,
            collectible_row + COLLECTIBLE_MATERIAL_SET_OFFSET,
        )? != expected_material_set
        || collection_unlock_index(collectibles, collectible_row)? != usize::from(unlock_index)
        || parent_class != PRESENTATION_NODE_INDEX_ROW_CLASS
        || authored_parents != expected_presentation_parents
    {
        return Err(validation(
            "Authored collectible does not reach the item, unlock, and intended Collections pages",
        ));
    }
    let (collectible_display_count, _, collectible_display_rows, collectible_display_class) =
        array_at(collectible_displays, 8)?;
    let collectible_display_row =
        collectible_display_rows + usize::from(collectible_index) * COLLECTIBLE_DISPLAY_ROW_SIZE;
    if collectible_display_class != COLLECTIBLE_DISPLAY_ROW_CLASS
        || collectible_display_count != collectible_count
        || read_u32(collectible_displays, collectible_display_row)? != identity.collectible_hash
        || read_u32(
            collectible_displays,
            collectible_display_row + COLLECTIBLE_DISPLAY_ICON_INDEX_OFFSET,
        )? != u32::from(collectible_icon_index)
        || read_u32(
            collectible_displays,
            collectible_display_row + COLLECTIBLE_DISPLAY_NAME_REFERENCE_OFFSET,
        )? != localization_table_index
        || read_u32(
            collectible_displays,
            collectible_display_row + COLLECTIBLE_DISPLAY_NAME_REFERENCE_OFFSET + 4,
        )? != identity.name_hash
        || read_u32(
            collectible_displays,
            collectible_display_row + COLLECTIBLE_DISPLAY_DESCRIPTION_REFERENCE_OFFSET,
        )? != localization_table_index
        || read_u32(
            collectible_displays,
            collectible_display_row + COLLECTIBLE_DISPLAY_DESCRIPTION_REFERENCE_OFFSET + 4,
        )? != identity.flavor_hash
        || read_u32(
            collectible_displays,
            collectible_display_row + COLLECTIBLE_DISPLAY_SOURCE_REFERENCE_OFFSET,
        )? != localization_table_index
        || read_u32(
            collectible_displays,
            collectible_display_row + COLLECTIBLE_DISPLAY_SOURCE_REFERENCE_OFFSET + 4,
        )? != identity.source_hash
        || if let Some(hash) = collection_requirement_hash {
            read_u32(
                collectible_displays,
                collectible_display_row + COLLECTIBLE_DISPLAY_REQUIREMENT_REFERENCE_OFFSET,
            )? != localization_table_index
                || read_u32(
                    collectible_displays,
                    collectible_display_row + COLLECTIBLE_DISPLAY_REQUIREMENT_REFERENCE_OFFSET + 4,
                )? != hash
        } else {
            read_u32(
                collectible_displays,
                collectible_display_row + COLLECTIBLE_DISPLAY_REQUIREMENT_REFERENCE_OFFSET,
            )? != BLANK_LOCALIZED_REFERENCE_TABLE_INDEX
                || read_u32(
                    collectible_displays,
                    collectible_display_row + COLLECTIBLE_DISPLAY_REQUIREMENT_REFERENCE_OFFSET + 4,
                )? != BLANK_LOCALIZED_REFERENCE_HASH
        }
    {
        return Err(validation(
            "Authored collectible display is not aligned with collectible definitions",
        ));
    }
    let (unlock_count, _, unlock_rows, unlock_class) = array_at(unlocks, 8)?;
    if unlock_class != UNLOCK_FLAG_DEFINITION_ROW_CLASS
        || unlock_count.checked_sub(1) != Some(usize::from(unlock_index))
    {
        return Err(validation("Authored unlock count is incorrect"));
    }
    let unlock_row = unlock_rows + usize::from(unlock_index) * UNLOCK_ROW_SIZE;
    if read_u32(unlocks, unlock_row)? != identity.unlock_hash
        || read_u16(unlocks, unlock_row + 4)? != u16::from(ACCOUNT_UNLOCK_BANK)
        || read_u16(unlocks, unlock_row + 6)? != unlock_slot
    {
        return Err(validation(
            "Authored unlock definition has incorrect storage",
        ));
    }
    let (display_count, _, display_rows, display_class) = array_at(displays, 8)?;
    let display_row = display_rows + usize::from(unlock_index) * UNLOCK_DISPLAY_ROW_SIZE;
    if display_class != UNLOCK_FLAG_DISPLAY_ROW_CLASS
        || display_count != unlock_count
        || read_u32(displays, display_row)? != identity.unlock_hash
    {
        return Err(validation(
            "Authored unlock display is not aligned with definitions",
        ));
    }
    Ok(())
}

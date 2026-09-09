use super::*;

pub(crate) fn collection_unlock_index(data: &[u8], row: usize) -> AuthoringResult<usize> {
    let layout = numeric_program_layout(data, row + COLLECTIBLE_CONDITION_OFFSET)?;
    let flags = layout
        .tokens
        .iter()
        .filter_map(|(opcode, operand)| (*opcode == NUMERIC_FLAG_INSTRUCTION).then_some(*operand))
        .collect::<BTreeSet<_>>();
    let flags = flags.into_iter().collect::<Vec<_>>();
    let [flag] = flags.as_slice() else {
        return Err(AuthoringError::InvalidInput(
            "Donor collectible acquisition condition does not identify exactly one unlock FLAG"
                .into(),
        ));
    };
    Ok(usize::from(*flag))
}

pub(crate) fn template_presentation_parents(
    nodes: &[u8],
    collectibles: &[u8],
    template_collectible_index: usize,
) -> AuthoringResult<Vec<u16>> {
    let (collectible_count, _, collectible_rows, _) = array_at(collectibles, 8)?;
    if template_collectible_index >= collectible_count {
        return Err(invalid(
            "The gameplay-donor collectible index is outside the collectible table",
        ));
    }
    let template_row = collectible_rows + template_collectible_index * COLLECTIBLE_ROW_SIZE;
    let (parent_count, _, parent_rows, parent_class) = array_at(collectibles, template_row + 0x18)?;
    let (node_count, _, node_rows, _) = array_at(nodes, 8)?;
    if parent_class != PRESENTATION_NODE_INDEX_ROW_CLASS || parent_count == 0 {
        return Err(invalid(
            "The gameplay donor has no compatible presentation-node parents",
        ));
    }
    let template_index = u16::try_from(template_collectible_index)
        .map_err(|_| invalid("The gameplay-donor collectible index does not fit 16 bits"))?;
    let mut presentation_parents = Vec::new();
    for position in 0..parent_count {
        let parent = read_u16(collectibles, parent_rows + position * 2)?;
        if usize::from(parent) >= node_count {
            return Err(invalid(format!(
                "Gameplay-donor presentation parent {parent} is outside the node table"
            )));
        }
        let descriptor = node_rows
            + usize::from(parent) * PRESENTATION_NODE_ROW_SIZE
            + PRESENTATION_NODE_COLLECTIBLES_OFFSET;
        let (child_count, _, child_rows, child_class) = array_at(nodes, descriptor)?;
        if child_class != PRESENTATION_NODE_COLLECTIBLE_ROW_CLASS {
            return Err(invalid(format!(
                "Gameplay-donor presentation parent {parent} has incompatible children"
            )));
        }
        let donor_rows = (0..child_count)
            .filter(|child_position| {
                read_u16(
                    nodes,
                    child_rows + child_position * PRESENTATION_NODE_COLLECTIBLE_ROW_SIZE,
                )
                .ok()
                    == Some(template_index)
            })
            .collect::<Vec<_>>();
        let [donor_position] = donor_rows.as_slice() else {
            return Err(invalid(format!(
                "Gameplay-donor presentation parent {parent} does not contain exactly one donor child"
            )));
        };
        let _sort = read_u16(
            nodes,
            child_rows + *donor_position * PRESENTATION_NODE_COLLECTIBLE_ROW_SIZE + 2,
        )?;
        presentation_parents.push(parent);
    }
    Ok(presentation_parents)
}

pub(crate) fn donor_weapon_collection_page(
    nodes: &[u8],
    donor_parents: &[u16],
) -> AuthoringResult<u16> {
    let (node_count, _, node_rows, node_class) = array_at(nodes, 8)?;
    if node_class != PRESENTATION_NODE_DEFINITION_ROW_CLASS
        || node_count != STOCK_PRESENTATION_NODE_COUNT
        || read_u32(
            nodes,
            node_rows
                + BADGES_ROOT_NODE_INDEX * PRESENTATION_NODE_ROW_SIZE
                + PRESENTATION_NODE_HASH_OFFSET,
        )? != BADGES_ROOT_NODE_HASH
    {
        return Err(invalid("The stock Badges presentation root is unavailable"));
    }
    let mut badge_nodes = BTreeSet::new();
    let mut pending = vec![BADGES_ROOT_NODE_INDEX];
    while let Some(node_index) = pending.pop() {
        if !badge_nodes.insert(node_index) {
            continue;
        }
        let descriptor = node_rows
            + node_index * PRESENTATION_NODE_ROW_SIZE
            + PRESENTATION_NODE_CHILD_NODES_OFFSET;
        let count = read_u64(nodes, descriptor)? as usize;
        if count == 0 {
            continue;
        }
        let (count, _, children, class) = array_at(nodes, descriptor)?;
        if class != PRESENTATION_NODE_CHILD_NODE_ROW_CLASS {
            return Err(invalid(format!(
                "Badge presentation node {node_index} has an incompatible child-node array"
            )));
        }
        for position in 0..count {
            let child = usize::from(read_u16(
                nodes,
                children + position * PRESENTATION_NODE_CHILD_NODE_ROW_SIZE,
            )?);
            if child >= node_count {
                return Err(invalid(
                    "The Badges hierarchy contains an invalid child node",
                ));
            }
            pending.push(child);
        }
    }
    let weapon_pages = donor_parents
        .iter()
        .copied()
        .filter(|parent| !badge_nodes.contains(&usize::from(*parent)))
        .collect::<Vec<_>>();
    let [weapon_page] = weapon_pages.as_slice() else {
        return Err(invalid(format!(
            "The donor has {} non-Badges presentation parents instead of one",
            weapon_pages.len()
        )));
    };
    Ok(*weapon_page)
}

pub(crate) fn validate_weapon_material_sets(data: &[u8]) -> AuthoringResult<()> {
    let (set_count, _, set_rows, set_class) = array_at(data, 8)?;
    if set_class != MATERIAL_REQUIREMENT_SET_ROW_CLASS {
        return Err(invalid(
            "The weapon Collections material-set table has an unexpected shape",
        ));
    }
    validate_weapon_material_set(
        data,
        set_count,
        set_rows,
        COLLECTIBLE_CURATED_WEAPON_MATERIAL_SET,
        CURATED_WEAPON_MATERIAL_SET_HASH,
        &CURATED_WEAPON_MATERIAL_REQUIREMENTS,
        "curated",
    )?;
    validate_weapon_material_set(
        data,
        set_count,
        set_rows,
        COLLECTIBLE_EXOTIC_WEAPON_MATERIAL_SET,
        EXOTIC_WEAPON_MATERIAL_SET_HASH,
        &EXOTIC_WEAPON_MATERIAL_REQUIREMENTS,
        "Exotic",
    )
}

pub(super) fn validate_weapon_material_set(
    data: &[u8],
    set_count: usize,
    set_rows: usize,
    set_index: u16,
    expected_hash: u32,
    expected_requirements: &[(u32, u32)],
    description: &str,
) -> AuthoringResult<()> {
    let set_index = usize::from(set_index);
    if set_index >= set_count {
        return Err(invalid(format!(
            "The {description}-weapon Collections material set is unavailable"
        )));
    }
    let set_row = set_rows + set_index * MATERIAL_REQUIREMENT_SET_ROW_SIZE;
    let definition = relative_target(data, set_row + 8)?;
    let (requirement_count, _, requirement_rows, requirement_class) = array_at(data, definition)?;
    if read_u32(data, set_row)? != expected_hash
        || requirement_class != MATERIAL_REQUIREMENT_ROW_CLASS
        || requirement_count != expected_requirements.len()
    {
        return Err(invalid(format!(
            "The {description}-weapon Collections material set has an unexpected shape"
        )));
    }
    for (position, (item_index, quantity)) in expected_requirements.iter().copied().enumerate() {
        let row = requirement_rows + position * MATERIAL_REQUIREMENT_ROW_SIZE;
        if read_u32(data, row)? != item_index
            || read_u32(data, row + 4)? != quantity
            || data.get(row + 8) != Some(&1)
            || data.get(row + 9) != Some(&1)
            || read_u16(data, row + 10)? != u16::MAX
        {
            return Err(invalid(format!(
                "The {description}-weapon Collections material row {position} is incompatible"
            )));
        }
    }
    Ok(())
}

pub(super) fn collectible_nested_shape(field: usize, class: u32) -> AuthoringResult<usize> {
    match (field, class) {
        (COLLECTIBLE_PRESENTATION_NODE_PARENTS_OFFSET, PRESENTATION_NODE_INDEX_ROW_CLASS) => {
            Ok(size_of::<u16>())
        }
        (0x30 | 0x40 | 0x50 | 0x60 | COLLECTIBLE_CONDITION_OFFSET, NUMERIC_PROGRAM_ROW_CLASS) => {
            Ok(NUMERIC_INSTRUCTION_ROW_SIZE)
        }
        (COLLECTIBLE_SOCKET_OVERRIDES_OFFSET, COLLECTIBLE_SOCKET_OVERRIDE_ROW_CLASS) => {
            Ok(COLLECTIBLE_SOCKET_OVERRIDE_ROW_SIZE)
        }
        _ => Err(invalid(format!(
            "Collectible nested field 0x{field:X} has unsupported row class 0x{class:08X}"
        ))),
    }
}

pub(super) fn flat_collectible_nested_segment(
    data: &[u8],
    header: usize,
    count: usize,
    class: u32,
    stride: usize,
) -> AuthoringResult<Vec<u8>> {
    let row_bytes = count
        .checked_mul(stride)
        .ok_or_else(|| invalid("Collectible nested-row size overflowed"))?;
    let rows_end_from_header = 16usize
        .checked_add(row_bytes)
        .ok_or_else(|| invalid("Collectible nested-row range overflowed"))?;
    let rows_end = header
        .checked_add(rows_end_from_header)
        .ok_or_else(|| invalid("Collectible nested-row offset overflowed"))?;
    // The serialized array terminator is the four-byte 0x80809FBD marker. Any zero word before it
    // is alignment padding, not a fixed part of the marker. Treating that word as mandatory skips
    // an entire 16-byte boundary when the header plus rows is already 12 bytes into an alignment
    // unit (for example, five 12-byte collectible socket overrides).
    let segment_length = rows_end_from_header
        .checked_add(NESTED_ARRAY_MARKER.len())
        .and_then(|end| end.checked_add(15))
        .map(|end| end & !15)
        .ok_or_else(|| invalid("Collectible nested-array trailer overflowed"))?;
    let segment_end = header
        .checked_add(segment_length)
        .ok_or_else(|| invalid("Collectible nested-array range overflowed"))?;
    let trailer = segment_end - NESTED_ARRAY_MARKER.len();
    let actual_count = read_u64(data, header)? as usize;
    let actual_class = read_u32(data, header + 8)?;
    let reserved_valid = data
        .get(header + 12..header + 16)
        .is_some_and(|reserved| reserved.iter().all(|byte| *byte == 0));
    let padding_valid = data
        .get(rows_end..trailer)
        .is_some_and(|padding| padding.iter().all(|byte| *byte == 0));
    let trailer_valid = data.get(trailer..segment_end) == Some(&NESTED_ARRAY_MARKER);
    if actual_count != count
        || actual_class != class
        || !reserved_valid
        || !padding_valid
        || !trailer_valid
    {
        return Err(invalid(format!(
            "Collectible nested array has an unexpected flat serialized layout (header=0x{header:X}, count={actual_count}/{count}, class=0x{actual_class:08X}/0x{class:08X}, reserved={reserved_valid}, padding={padding_valid}, trailer={trailer_valid}, end=0x{segment_end:X})"
        )));
    }
    data.get(header..segment_end)
        .ok_or_else(|| invalid("Collectible nested array is truncated"))
        .map(ToOwned::to_owned)
}

pub(super) fn collectible_nested_clones(
    data: &[u8],
    rows: usize,
    count: usize,
    rows_end: usize,
    template_row: usize,
    source_unlock_index: usize,
    unlock_index: u16,
) -> AuthoringResult<Vec<CollectibleNestedClone>> {
    let source_unlock_index = u16::try_from(source_unlock_index)
        .map_err(|_| invalid("Donor acquired-flag index does not fit 16 bits"))?;
    let mut target_owners = BTreeMap::<usize, Vec<(usize, usize)>>::new();
    for collectible_index in 0..count {
        let row = rows + collectible_index * COLLECTIBLE_ROW_SIZE;
        for (field, target) in pointer_targets(data, row, rows_end)? {
            target_owners
                .entry(target)
                .or_default()
                .push((collectible_index, field));
        }
    }
    if let Some((target, owners)) = target_owners.iter().find(|(_, owners)| owners.len() != 1) {
        return Err(invalid(format!(
            "Installed collectible nested target 0x{target:X} is shared by {owners:?}"
        )));
    }

    let mut clones = Vec::new();
    for (field, target) in pointer_targets(data, template_row, rows_end)? {
        if matches!(
            field,
            COLLECTIBLE_PRESENTATION_NODE_PARENTS_OFFSET | COLLECTIBLE_SOCKET_OVERRIDES_OFFSET
        ) {
            continue;
        }
        let descriptor = template_row + field;
        let nested_count = usize::try_from(read_u64(data, descriptor)?)
            .map_err(|_| invalid("Collectible nested count does not fit usize"))?;
        let class = read_u32(data, target + 8)?;
        let stride = collectible_nested_shape(field, class)?;
        let mut bytes = flat_collectible_nested_segment(data, target, nested_count, class, stride)
            .map_err(|error| {
                invalid(format!(
                    "Donor collectible field 0x{field:X} at 0x{target:X} is incompatible: {error}"
                ))
            })?;
        let mut retargeted_source_flags = 0usize;
        if class == NUMERIC_PROGRAM_ROW_CLASS {
            if field == COLLECTIBLE_CONDITION_OFFSET {
                bytes = canonical_single_flag_program(
                    &bytes,
                    nested_count,
                    source_unlock_index,
                    unlock_index,
                )?;
                retargeted_source_flags = 1;
            } else {
                for index in 0..nested_count {
                    let row = 16 + index * NUMERIC_INSTRUCTION_ROW_SIZE;
                    let instruction = NumericInstruction::read(&bytes, row)?;
                    if instruction.opcode == NUMERIC_FLAG_INSTRUCTION
                        && instruction.operand == source_unlock_index
                    {
                        write_u16(&mut bytes, row + 4, unlock_index)?;
                        retargeted_source_flags += 1;
                    }
                }
            }
        }
        if field == COLLECTIBLE_CONDITION_OFFSET
            && (class != NUMERIC_PROGRAM_ROW_CLASS || retargeted_source_flags != 1)
        {
            return Err(invalid(
                "Donor collectible has an unexpected acquired-flag condition",
            ));
        }
        clones.push(CollectibleNestedClone {
            field,
            count: if field == COLLECTIBLE_CONDITION_OFFSET {
                1
            } else {
                nested_count
            },
            class,
            bytes,
            retargeted_source_flags,
        });
    }
    Ok(clones)
}

pub(crate) fn validate_authored_collectible_nested_isolation(
    data: &[u8],
    authored_collectible_index: usize,
    source_unlock_index: usize,
    unlock_index: u16,
) -> AuthoringResult<()> {
    let (count, _, rows, class) = array_at(data, 8)?;
    if class != COLLECTIBLE_DEFINITION_ROW_CLASS || authored_collectible_index >= count {
        return Err(validation(
            "Authored collectible is outside its final table",
        ));
    }
    let rows_end = rows
        .checked_add(
            count
                .checked_mul(COLLECTIBLE_ROW_SIZE)
                .ok_or_else(|| validation("Collectible fixed-row size overflowed"))?,
        )
        .ok_or_else(|| validation("Collectible fixed-row range overflowed"))?;
    let authored_row = rows + authored_collectible_index * COLLECTIBLE_ROW_SIZE;
    if data.get(
        authored_row + COLLECTIBLE_SOCKET_OVERRIDES_OFFSET
            ..authored_row + COLLECTIBLE_SOCKET_OVERRIDES_OFFSET + 16,
    ) != Some(&[0; 16])
    {
        return Err(validation(
            "Authored collectible retains donor socket overrides",
        ));
    }
    let authored_targets = pointer_targets(data, authored_row, rows_end)?;
    let mut target_owners = BTreeMap::<usize, Vec<(usize, usize)>>::new();
    for collectible_index in 0..count {
        let row = rows + collectible_index * COLLECTIBLE_ROW_SIZE;
        for (field, target) in pointer_targets(data, row, rows_end)? {
            target_owners
                .entry(target)
                .or_default()
                .push((collectible_index, field));
        }
    }
    let source_unlock_index = u16::try_from(source_unlock_index)
        .map_err(|_| validation("Donor acquired-flag index does not fit 16 bits"))?;
    let mut acquired_flag_matches = 0usize;
    for (field, target) in authored_targets {
        if target_owners
            .get(&target)
            .is_none_or(|owners| owners.as_slice() != [(authored_collectible_index, field)])
        {
            return Err(validation(format!(
                "Authored collectible field 0x{field:X} shares nested target 0x{target:X}"
            )));
        }
        let class = read_u32(data, target + 8)?;
        if class != NUMERIC_PROGRAM_ROW_CLASS {
            continue;
        }
        let layout = numeric_program_layout(data, authored_row + field)?;
        if layout
            .tokens
            .contains(&(NUMERIC_FLAG_INSTRUCTION, source_unlock_index))
        {
            return Err(validation(format!(
                "Authored collectible field 0x{field:X} retains the donor acquired flag"
            )));
        }
        if field == COLLECTIBLE_CONDITION_OFFSET {
            acquired_flag_matches = layout
                .tokens
                .iter()
                .filter(|token| **token == (NUMERIC_FLAG_INSTRUCTION, unlock_index))
                .count();
        }
    }
    if acquired_flag_matches != 1 {
        return Err(validation(
            "Authored collectible acquired condition does not contain exactly one authored flag",
        ));
    }
    Ok(())
}

pub(crate) fn append_collectible(
    mut data: Vec<u8>,
    template_index: usize,
    spec: AuthoredCollectibleSpec<'_>,
) -> AuthoringResult<Vec<u8>> {
    let AuthoredCollectibleSpec {
        collectible_hash,
        item_index,
        unlock,
        material_set_index,
        presentation_parents,
        require_donor_parent_subset,
    } = spec;
    let CollectibleUnlockClone {
        source_index: source_unlock_index,
        authored_index: unlock_index,
    } = unlock;
    let (count, header, rows, class) = array_at(&data, 8)?;
    if class != COLLECTIBLE_DEFINITION_ROW_CLASS || template_index >= count {
        return Err(invalid("Collectible template index is outside the table"));
    }
    let rows_end = rows
        .checked_add(
            count
                .checked_mul(COLLECTIBLE_ROW_SIZE)
                .ok_or_else(|| invalid("Collectible fixed-row extent overflowed"))?,
        )
        .ok_or_else(|| invalid("Collectible fixed-row extent overflowed"))?;
    if rows_end > data.len() {
        return Err(invalid("Collectible fixed rows extend beyond the table"));
    }
    let template_row = rows
        .checked_add(
            template_index
                .checked_mul(COLLECTIBLE_ROW_SIZE)
                .ok_or_else(|| invalid("Collectible template offset overflowed"))?,
        )
        .ok_or_else(|| invalid("Collectible template offset overflowed"))?;
    let template = data[template_row..template_row + COLLECTIBLE_ROW_SIZE].to_vec();
    // The authored row receives Parhelion's validated rarity-appropriate reacquisition recipe.
    // Donors may be already reacquirable or may carry no material recipe; those source scalars do
    // not affect the nested collectible topology cloned below.
    let (source_parent_count, source_parent_header, source_parent_rows, source_parent_class) =
        array_at(&data, template_row + 0x18)?;
    if source_parent_class != PRESENTATION_NODE_INDEX_ROW_CLASS
        || presentation_parents.is_empty()
        || (require_donor_parent_subset && presentation_parents.len() > source_parent_count)
    {
        return Err(invalid(
            "The authored presentation parents are incompatible with the gameplay donor",
        ));
    }
    let source_parents = (0..source_parent_count)
        .map(|position| read_u16(&data, source_parent_rows + position * 2))
        .collect::<AuthoringResult<Vec<_>>>()?;
    if (require_donor_parent_subset
        && presentation_parents
            .iter()
            .any(|parent| !source_parents.contains(parent)))
        || presentation_parents
            .windows(2)
            .any(|pair| pair[0] == pair[1])
    {
        return Err(invalid(
            "Selected presentation parents are not a unique gameplay-donor parent subset",
        ));
    }
    let source_parent_header_bytes = data
        .get(source_parent_header..source_parent_rows)
        .ok_or_else(|| invalid("The gameplay-donor parent-array header is truncated"))?
        .to_vec();
    let nested_clones = collectible_nested_clones(
        &data,
        rows,
        count,
        rows_end,
        template_row,
        source_unlock_index,
        unlock_index,
    )?;
    let retargeted_source_flags = nested_clones
        .iter()
        .map(|nested| nested.retargeted_source_flags)
        .sum::<usize>();
    if retargeted_source_flags == 0 {
        return Err(invalid(
            "Donor collectible conditions do not reference its acquired flag",
        ));
    }

    data.splice(
        rows_end..rows_end,
        std::iter::repeat_n(0, COLLECTIBLE_ROW_SIZE),
    );
    for index in 0..count {
        let row = rows + index * COLLECTIBLE_ROW_SIZE;
        rebase_row_pointers(&mut data, row, rows_end, COLLECTIBLE_ROW_SIZE)?;
    }
    data[rows_end..rows_end + COLLECTIBLE_ROW_SIZE].copy_from_slice(&template);
    // Random-roll donors can force placeholder perks/shaders in Collections. Those overrides
    // apply by socket type (including both trait columns), not by authored lane or selected plug.
    // Fixed authored rolls must instead use their item-definition defaults, as stock fixed rolls do.
    data[rows_end + COLLECTIBLE_SOCKET_OVERRIDES_OFFSET
        ..rows_end + COLLECTIBLE_SOCKET_OVERRIDES_OFFSET + 16]
        .fill(0);
    write_u32(
        &mut data,
        rows_end + COLLECTIBLE_HASH_OFFSET,
        collectible_hash,
    )?;
    write_u16(
        &mut data,
        rows_end + COLLECTIBLE_ITEM_INDEX_OFFSET,
        item_index,
    )?;
    data[rows_end + COLLECTIBLE_CURATED_ACQUISITION_FLAG_OFFSET] =
        COLLECTIBLE_CURATED_ACQUISITION_FLAG;
    write_u16(
        &mut data,
        rows_end + COLLECTIBLE_REACQUISITION_STATE_OFFSET,
        COLLECTIBLE_REACQUISITION_ENABLED,
    )?;
    write_u16(
        &mut data,
        rows_end + COLLECTIBLE_MATERIAL_SET_OFFSET,
        material_set_index,
    )?;

    while data.len() % 16 != 0 {
        data.push(0);
    }
    let new_parent_target = data.len();
    let mut parent_block = source_parent_header_bytes;
    write_u64(&mut parent_block, 0, presentation_parents.len() as u64)?;
    for parent in presentation_parents {
        parent_block.extend_from_slice(&parent.to_le_bytes());
    }
    while (parent_block.len() + NESTED_ARRAY_TRAILER.len()) % 16 != 0 {
        parent_block.push(0);
    }
    parent_block.extend_from_slice(&NESTED_ARRAY_TRAILER);
    data.extend_from_slice(&parent_block);
    write_u64(
        &mut data,
        rows_end + COLLECTIBLE_PRESENTATION_NODE_PARENTS_OFFSET,
        presentation_parents.len() as u64,
    )?;
    write_relative_pointer(
        &mut data,
        rows_end + COLLECTIBLE_PRESENTATION_NODE_PARENTS_OFFSET + 8,
        new_parent_target,
    )?;

    for nested in nested_clones {
        while data.len() % 16 != 0 {
            data.push(0);
        }
        let new_target = data.len();
        data.extend_from_slice(&nested.bytes);
        write_u64(&mut data, rows_end + nested.field, nested.count as u64)?;
        write_relative_pointer(&mut data, rows_end + nested.field + 8, new_target)?;
        let (authored_count, authored_header, _, authored_class) =
            array_at(&data, rows_end + nested.field)?;
        if authored_count != nested.count
            || authored_header != new_target
            || authored_class != nested.class
            || data.get(new_target..new_target + nested.bytes.len())
                != Some(nested.bytes.as_slice())
        {
            return Err(validation(
                "An authored collectible nested array was not independently serialized",
            ));
        }
    }
    set_array_count(&mut data, 8, header, count + 1)?;
    validate_authored_collectible_nested_isolation(
        &data,
        count,
        source_unlock_index,
        unlock_index,
    )?;
    Ok(data)
}

pub(crate) fn append_collectible_display(
    mut data: Vec<u8>,
    template_index: usize,
    identity: WeaponCloneIdentity,
    collectible_icon_index: u16,
    localization_table_index: u32,
) -> AuthoringResult<Vec<u8>> {
    let (count, header, rows, class) = array_at(&data, 8)?;
    if class != COLLECTIBLE_DISPLAY_ROW_CLASS || template_index >= count {
        return Err(invalid(
            "Collectible-display table or gameplay-donor template is incompatible",
        ));
    }
    let rows_end = rows
        .checked_add(
            count
                .checked_mul(COLLECTIBLE_DISPLAY_ROW_SIZE)
                .ok_or_else(|| invalid("Collectible-display row size overflowed"))?,
        )
        .ok_or_else(|| invalid("Collectible-display row range overflowed"))?;
    if rows_end > data.len() {
        return Err(invalid(
            "Collectible-display fixed rows extend beyond the table",
        ));
    }
    let template_row = rows + template_index * COLLECTIBLE_DISPLAY_ROW_SIZE;
    let (condition_count, condition_header, _, condition_class) =
        array_at(&data, template_row + COLLECTIBLE_DISPLAY_CONDITION_OFFSET)?;
    if condition_count != 0
        || condition_header != template_row + COLLECTIBLE_DISPLAY_CONDITION_OFFSET + 8
        || condition_class != 0x0000_FFFF
    {
        return Err(invalid(
            "The gameplay-donor collectible display has an unexpected condition layout",
        ));
    }
    let template = data[template_row..template_row + COLLECTIBLE_DISPLAY_ROW_SIZE].to_vec();

    data.splice(
        rows_end..rows_end,
        std::iter::repeat_n(0, COLLECTIBLE_DISPLAY_ROW_SIZE),
    );
    for index in 0..count {
        let row = rows + index * COLLECTIBLE_DISPLAY_ROW_SIZE;
        let descriptor = row + COLLECTIBLE_DISPLAY_CONDITION_OFFSET;
        if read_u64(&data, descriptor)? == 0 {
            continue;
        }
        let target = relative_target(&data, descriptor + 8)?;
        if target < rows_end {
            return Err(invalid(
                "Collectible-display condition unexpectedly targets fixed rows",
            ));
        }
        write_relative_pointer(
            &mut data,
            descriptor + 8,
            target + COLLECTIBLE_DISPLAY_ROW_SIZE,
        )?;
    }
    data[rows_end..rows_end + COLLECTIBLE_DISPLAY_ROW_SIZE].copy_from_slice(&template);
    write_u32(&mut data, rows_end, identity.collectible_hash)?;
    write_u32(
        &mut data,
        rows_end + COLLECTIBLE_DISPLAY_ICON_INDEX_OFFSET,
        u32::from(collectible_icon_index),
    )?;
    write_localized_reference(
        &mut data,
        rows_end + COLLECTIBLE_DISPLAY_NAME_REFERENCE_OFFSET,
        localization_table_index,
        identity.name_hash,
    )?;
    write_localized_reference(
        &mut data,
        rows_end + COLLECTIBLE_DISPLAY_DESCRIPTION_REFERENCE_OFFSET,
        localization_table_index,
        identity.flavor_hash,
    )?;
    write_localized_reference(
        &mut data,
        rows_end + COLLECTIBLE_DISPLAY_SOURCE_REFERENCE_OFFSET,
        localization_table_index,
        identity.source_hash,
    )?;
    // Authored curated weapons are reacquirable, so do not inherit a randomized-donor
    // requirementDescription such as "Cannot reacquire randomized gear." Native curated
    // weapon displays encode the absent string with this sentinel pair.
    write_localized_reference(
        &mut data,
        rows_end + COLLECTIBLE_DISPLAY_REQUIREMENT_REFERENCE_OFFSET,
        BLANK_LOCALIZED_REFERENCE_TABLE_INDEX,
        BLANK_LOCALIZED_REFERENCE_HASH,
    )?;
    set_array_count(&mut data, 8, header, count + 1)?;
    Ok(data)
}

pub(crate) fn validate_authored_collectible_display_row(
    data: &[u8],
    collectible_index: u16,
) -> AuthoringResult<()> {
    let (count, _, rows, class) = array_at(data, 8)?;
    let row_index = usize::from(collectible_index);
    if class != COLLECTIBLE_DISPLAY_ROW_CLASS || row_index >= count {
        return Err(validation(
            "Authored collectible display is outside its final table",
        ));
    }
    let row = rows
        .checked_add(
            row_index
                .checked_mul(COLLECTIBLE_DISPLAY_ROW_SIZE)
                .ok_or_else(|| validation("Collectible-display row overflowed"))?,
        )
        .ok_or_else(|| validation("Collectible-display row overflowed"))?;
    let descriptor = row + COLLECTIBLE_DISPLAY_CONDITION_OFFSET;
    let (condition_count, condition_header, _, condition_class) = array_at(data, descriptor)?;
    if condition_count != 0 || condition_header != descriptor + 8 || condition_class != 0x0000_FFFF
    {
        return Err(validation(
            "Authored collectible-display condition is not the native empty sentinel",
        ));
    }
    Ok(())
}

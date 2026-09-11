//! Checked native table and presentation-tree append operations.
use super::*;
use crate::tag_payload::contains_u32_at_offset;
pub(crate) const PRESENTATION_CHILD_ARRAY_SENTINEL: u32 = 0x8080_9FBD;

#[allow(clippy::too_many_arguments)]
pub(crate) fn append_fixed_rows_without_donor_dependencies(
    mut data: Vec<u8>,
    expected_count: usize,
    row_size: usize,
    row_class: u32,
    pointer_fields: &[usize],
    allowed_template_pointers: &[usize],
    template_indices: &[usize],
    template_hashes: &[u32],
    authored_hashes: &[u32],
    hash_offset: usize,
    description: &str,
) -> AuthoringResult<Vec<u8>> {
    if template_indices.len() != template_hashes.len()
        || template_indices.len() != authored_hashes.len()
    {
        return Err(invalid(format!(
            "{description} clone metadata is inconsistent"
        )));
    }
    let (count, main_header, rows, class) = array_at(&data, 8)?;
    let rows_end = rows
        .checked_add(
            count
                .checked_mul(row_size)
                .ok_or_else(|| invalid(format!("{description} fixed-row size overflowed")))?,
        )
        .ok_or_else(|| invalid(format!("{description} fixed-row range overflowed")))?;
    if class != row_class || count != expected_count || rows_end > data.len() {
        return Err(invalid(format!(
            "Sunrise badge authoring requires the audited {expected_count}-row {description}"
        )));
    }
    let mut templates = Vec::with_capacity(template_indices.len());
    for (position, &template_index) in template_indices.iter().enumerate() {
        if template_index >= count
            || read_u32(&data, rows + template_index * row_size + hash_offset)?
                != template_hashes[position]
        {
            return Err(invalid(format!(
                "Sunrise badge {description} template identity changed"
            )));
        }
        let row = rows + template_index * row_size;
        for &field in pointer_fields {
            if read_u64(&data, row + field)? != 0 && !allowed_template_pointers.contains(&field) {
                return Err(invalid(format!(
                    "Sunrise badge {description} template acquired an unexpected dependency at 0x{field:X}"
                )));
            }
        }
        templates.push(data[row..row + row_size].to_vec());
    }
    for &hash in authored_hashes {
        if contains_u32_at_offset(&data, rows, count, row_size, hash_offset, hash)? {
            return Err(invalid(format!(
                "Sunrise badge {description} hash 0x{hash:08X} already exists"
            )));
        }
    }

    let fixed_shift = row_size
        .checked_mul(templates.len())
        .ok_or_else(|| invalid(format!("{description} fixed-row append overflowed")))?;
    data.splice(rows_end..rows_end, std::iter::repeat_n(0, fixed_shift));
    for index in 0..count {
        let row = rows + index * row_size;
        for &field in pointer_fields {
            if read_u64(&data, row + field)? == 0 {
                continue;
            }
            let target = relative_target(&data, row + field + 8)?;
            if target < rows_end {
                return Err(invalid(format!(
                    "{description} nested array unexpectedly targets fixed rows"
                )));
            }
            write_relative_pointer(&mut data, row + field + 8, target + fixed_shift)?;
        }
    }
    for (position, template) in templates.iter().enumerate() {
        let row = rows_end + position * row_size;
        data[row..row + row_size].copy_from_slice(template);
        for &field in pointer_fields {
            data[row + field..row + field + 16].fill(0);
        }
        write_u32(&mut data, row + hash_offset, authored_hashes[position])?;
    }
    set_array_count(
        &mut data,
        8,
        main_header,
        expected_count + authored_hashes.len(),
    )?;
    Ok(data)
}

pub(crate) fn append_numeric_program(
    data: &mut Vec<u8>,
    descriptor: usize,
    instructions: &[[u8; 8]],
    description: &str,
) -> AuthoringResult<()> {
    if instructions.is_empty() {
        return Err(invalid(format!("{description} cannot be empty")));
    }
    let tokens = instructions
        .iter()
        .map(|instruction| {
            (
                instruction[0],
                u16::from_le_bytes([instruction[4], instruction[5]]),
            )
        })
        .collect::<Vec<_>>();
    if numeric_program_stack_depth(&tokens)? != 1 {
        return Err(validation(format!(
            "{description} does not reduce to one value"
        )));
    }
    while data.len() % 16 != 0 {
        data.push(0);
    }
    let program_header = data.len();
    data.extend_from_slice(
        &u64::try_from(instructions.len())
            .map_err(|_| invalid(format!("{description} is too large")))?
            .to_le_bytes(),
    );
    data.extend_from_slice(&NUMERIC_PROGRAM_ROW_CLASS.to_le_bytes());
    data.extend_from_slice(&0_u32.to_le_bytes());
    for instruction in instructions {
        data.extend_from_slice(instruction);
    }
    while (data.len() + NESTED_ARRAY_TRAILER.len()) % 16 != 0 {
        data.push(0);
    }
    data.extend_from_slice(&NESTED_ARRAY_TRAILER);
    write_u64(data, descriptor, instructions.len() as u64)?;
    write_relative_pointer(data, descriptor + 8, program_header)?;
    let layout = numeric_program_layout(data, descriptor)?;
    if layout.tokens != tokens || numeric_program_stack_depth(&layout.tokens)? != 1 {
        return Err(validation(format!(
            "{description} did not serialize faithfully"
        )));
    }
    Ok(())
}

pub(crate) fn append_single_u16_array(
    data: &mut Vec<u8>,
    descriptor: usize,
    row_class: u32,
    value: u16,
) -> AuthoringResult<()> {
    // Sunrise reads the type marker immediately before the array header.
    // Reserve it even when the preceding payload already ends on a boundary.
    let header = data
        .len()
        .checked_add(19)
        .map(|end| end & !15)
        .ok_or_else(|| invalid("Record objective array alignment overflowed"))?;
    data.resize(header - 4, 0);
    data.extend_from_slice(&PRESENTATION_CHILD_ARRAY_SENTINEL.to_le_bytes());
    let header = data.len();
    data.extend_from_slice(&1_u64.to_le_bytes());
    data.extend_from_slice(&row_class.to_le_bytes());
    data.extend_from_slice(&0_u32.to_le_bytes());
    data.extend_from_slice(&value.to_le_bytes());
    while (data.len() + NESTED_ARRAY_TRAILER.len()) % 16 != 0 {
        data.push(0);
    }
    data.extend_from_slice(&NESTED_ARRAY_TRAILER);
    write_u64(data, descriptor, 1)?;
    write_relative_pointer(data, descriptor + 8, header)
}

pub(crate) fn append_presentation_node_child(
    nodes: &mut Vec<u8>,
    parent_index: usize,
    donor_child_index: usize,
    authored_child_index: usize,
) -> AuthoringResult<()> {
    let (node_count, _, node_rows, _) = array_at(nodes, 8)?;
    if parent_index >= node_count || authored_child_index >= node_count {
        return Err(invalid(
            "Presentation child-node append index is outside the table",
        ));
    }
    let descriptor = node_rows
        + parent_index * PRESENTATION_NODE_ROW_SIZE
        + PRESENTATION_NODE_CHILD_NODES_OFFSET;
    let (child_count, child_header, child_rows, child_class) = array_at(nodes, descriptor)?;
    if child_class != PRESENTATION_NODE_CHILD_NODE_ROW_CLASS || child_count == 0 {
        return Err(invalid(
            "Presentation parent has no compatible child-node array",
        ));
    }
    let donor_positions = (0..child_count)
        .filter(|position| {
            read_u16(
                nodes,
                child_rows + position * PRESENTATION_NODE_CHILD_NODE_ROW_SIZE,
            )
            .ok()
                == u16::try_from(donor_child_index).ok()
        })
        .collect::<Vec<_>>();
    let [donor_position] = donor_positions.as_slice() else {
        return Err(invalid(
            "Presentation parent does not contain one donor child node",
        ));
    };
    let source_rows = (0..child_count)
        .map(|position| {
            let row = child_rows + position * PRESENTATION_NODE_CHILD_NODE_ROW_SIZE;
            Ok(nodes[row..row + PRESENTATION_NODE_CHILD_NODE_ROW_SIZE].to_vec())
        })
        .collect::<AuthoringResult<Vec<_>>>()?;
    let header_bytes = nodes
        .get(child_header..child_rows)
        .ok_or_else(|| invalid("Presentation child-node header is truncated"))?
        .to_vec();
    let mut authored = source_rows[*donor_position].clone();
    write_u16(
        &mut authored,
        0,
        u16::try_from(authored_child_index)
            .map_err(|_| invalid("Authored presentation node index does not fit 16 bits"))?,
    )?;
    write_u64(&mut authored, 8, 0)?;
    write_u64(&mut authored, 16, 0)?;
    let mut authored_rows = Vec::with_capacity(source_rows.len() + 1);
    authored_rows.push(authored);
    authored_rows.extend(source_rows.iter().cloned());

    while nodes.len() % 16 != 0 {
        nodes.push(0);
    }
    let new_header = nodes.len();
    nodes.extend_from_slice(&header_bytes);
    let new_rows = nodes.len();
    for row in &authored_rows {
        nodes.extend_from_slice(row);
    }
    while (nodes.len() + NESTED_ARRAY_TRAILER.len()) % 16 != 0 {
        nodes.push(0);
    }
    nodes.extend_from_slice(&NESTED_ARRAY_TRAILER);
    write_u64(nodes, new_header, authored_rows.len() as u64)?;
    for (position, source_row) in source_rows.iter().enumerate() {
        let authored_position = position + 1;
        if read_u64(source_row, 8)? == 0 {
            write_u64(
                nodes,
                new_rows + authored_position * PRESENTATION_NODE_CHILD_NODE_ROW_SIZE + 16,
                0,
            )?;
            continue;
        }
        let source_descriptor = child_rows + position * PRESENTATION_NODE_CHILD_NODE_ROW_SIZE + 8;
        let layout = numeric_program_layout(nodes, source_descriptor)?;
        while nodes.len() % 16 != 0 {
            nodes.push(0);
        }
        let target = nodes.len();
        let program = nodes
            .get(layout.header..layout.segment_end)
            .ok_or_else(|| invalid("Presentation child condition is truncated"))?
            .to_vec();
        nodes.extend_from_slice(&program);
        let authored_descriptor =
            new_rows + authored_position * PRESENTATION_NODE_CHILD_NODE_ROW_SIZE + 8;
        write_relative_pointer(nodes, authored_descriptor + 8, target)?;
    }
    write_u64(nodes, descriptor, authored_rows.len() as u64)?;
    write_relative_pointer(nodes, descriptor + 8, new_header)
}

pub(crate) fn append_collectible_child_to_node(
    nodes: &mut Vec<u8>,
    parent_index: usize,
    donor_collectible_index: usize,
    authored_collectible_index: usize,
) -> AuthoringResult<()> {
    let (node_count, _, node_rows, _) = array_at(nodes, 8)?;
    if parent_index >= node_count {
        return Err(invalid(
            "Weapon-page presentation node is outside the table",
        ));
    }
    let descriptor = node_rows
        + parent_index * PRESENTATION_NODE_ROW_SIZE
        + PRESENTATION_NODE_COLLECTIBLES_OFFSET;
    let (count, header, rows, class) = array_at(nodes, descriptor)?;
    if class != PRESENTATION_NODE_COLLECTIBLE_ROW_CLASS || count == 0 {
        return Err(invalid(
            "Weapon-page presentation node has no collectible children",
        ));
    }
    let donor_index = u16::try_from(donor_collectible_index)
        .map_err(|_| invalid("Donor collectible index does not fit 16 bits"))?;
    let donor_positions = (0..count)
        .filter(|position| {
            read_u16(
                nodes,
                rows + position * PRESENTATION_NODE_COLLECTIBLE_ROW_SIZE,
            )
            .ok()
                == Some(donor_index)
        })
        .collect::<Vec<_>>();
    let [donor_position] = donor_positions.as_slice() else {
        return Err(invalid(
            "Weapon page does not contain exactly one donor collectible",
        ));
    };
    let child_end = rows
        .checked_add(
            count
                .checked_mul(PRESENTATION_NODE_COLLECTIBLE_ROW_SIZE)
                .ok_or_else(|| invalid("Presentation child row size overflowed"))?,
        )
        .ok_or_else(|| invalid("Presentation child row range overflowed"))?;
    validate_presentation_child_array_terminator(nodes, child_end)?;
    let insertion = rows + (*donor_position + 1) * PRESENTATION_NODE_COLLECTIBLE_ROW_SIZE;
    let donor = rows + *donor_position * PRESENTATION_NODE_COLLECTIBLE_ROW_SIZE;
    let mut authored = nodes[header..insertion].to_vec();
    authored.extend_from_slice(&nodes[donor..donor + PRESENTATION_NODE_COLLECTIBLE_ROW_SIZE]);
    authored.extend_from_slice(&nodes[insertion..child_end]);
    append_presentation_child_array_terminator(&mut authored)?;
    write_u64(&mut authored, 0, (count + 1) as u64)?;
    write_u16(
        &mut authored,
        16 + (*donor_position + 1) * PRESENTATION_NODE_COLLECTIBLE_ROW_SIZE,
        u16::try_from(authored_collectible_index)
            .map_err(|_| invalid("Authored collectible index does not fit 16 bits"))?,
    )?;
    while nodes.len() % 16 != 0 {
        nodes.push(0);
    }
    let new_header = nodes.len();
    nodes.extend_from_slice(&authored);
    write_u64(nodes, descriptor, (count + 1) as u64)?;
    write_relative_pointer(nodes, descriptor + 8, new_header)
}

pub(crate) fn replace_with_single_collectible_child(
    nodes: &mut Vec<u8>,
    descriptor: usize,
    collectible_index: usize,
) -> AuthoringResult<()> {
    let (source_count, source_header, source_rows, source_class) = array_at(nodes, descriptor)?;
    if source_count == 0 || source_class != PRESENTATION_NODE_COLLECTIBLE_ROW_CLASS {
        return Err(invalid(
            "Badge class template has no collectible child template",
        ));
    }
    let template = nodes
        .get(source_rows..source_rows + PRESENTATION_NODE_COLLECTIBLE_ROW_SIZE)
        .ok_or_else(|| invalid("Badge collectible-child template is truncated"))?
        .to_vec();
    while nodes.len() % 16 != 0 {
        nodes.push(0);
    }
    let header = nodes.len();
    let header_bytes = nodes
        .get(source_header..source_rows)
        .ok_or_else(|| invalid("Badge collectible-child header is truncated"))?
        .to_vec();
    nodes.extend_from_slice(&header_bytes);
    nodes.extend_from_slice(&template);
    append_presentation_child_array_terminator(nodes)?;
    write_u64(nodes, header, 1)?;
    write_u16(
        nodes,
        header + 16,
        u16::try_from(collectible_index)
            .map_err(|_| invalid("Authored collectible index does not fit 16 bits"))?,
    )?;
    write_u64(nodes, descriptor, 1)?;
    write_relative_pointer(nodes, descriptor + 8, header)
}

pub(crate) fn append_presentation_child_array_terminator(
    data: &mut Vec<u8>,
) -> AuthoringResult<()> {
    let segment_end = presentation_child_array_end(data.len())?;
    data.resize(segment_end - size_of::<u32>(), 0);
    data.extend_from_slice(&PRESENTATION_CHILD_ARRAY_SENTINEL.to_le_bytes());
    Ok(())
}

pub(crate) fn presentation_child_array_end(child_end: usize) -> AuthoringResult<usize> {
    child_end
        .checked_add(size_of::<u32>())
        .and_then(|end| end.checked_add(15))
        .map(|end| end & !15)
        .ok_or_else(|| invalid("Presentation-node child trailer overflowed"))
}

pub(crate) fn validate_presentation_child_array_terminator(
    data: &[u8],
    child_end: usize,
) -> AuthoringResult<usize> {
    let segment_end = presentation_child_array_end(child_end)?;
    let sentinel_start = segment_end - size_of::<u32>();
    if data
        .get(child_end..sentinel_start)
        .is_none_or(|padding| padding.iter().any(|byte| *byte != 0))
        || read_u32(data, sentinel_start)? != PRESENTATION_CHILD_ARRAY_SENTINEL
    {
        return Err(invalid(
            "Presentation-node child array has an unexpected terminator",
        ));
    }
    Ok(segment_end)
}

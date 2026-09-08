//! Native socket columns operations with independent validation.
use super::*;

// The middle descriptor is an auxiliary native array (for example class 0x80803149
// on Flash and Thunder's masterwork row). Its payload remains opaque during relocation.
const SOCKET_ROW_ARRAY_DESCRIPTOR_OFFSETS: [usize; 3] = [
    ITEM_ORDINARY_SOCKET_RANDOMIZED_SELECTION_PROGRAM_OFFSET,
    0x28,
    ITEM_ORDINARY_SOCKET_EMBEDDED_PLUGS_OFFSET,
];

pub(super) fn resolve_socket_column_indices(
    item_rows_by_hash: &BTreeMap<u32, Vec<usize>>,
    donor_definition: &[u8],
    columns: &[Option<WeaponSocketColumnOverride>],
) -> AuthoringResult<Vec<Option<ResolvedSocketColumn>>> {
    let socket_resource = relative_target(donor_definition, ITEM_ORDINARY_SOCKET_POINTER_OFFSET)?;
    let (socket_count, _, _, socket_class) = array_at(donor_definition, socket_resource)?;
    if socket_class != ITEM_ORDINARY_SOCKET_ROW_CLASS {
        return Err(invalid(
            "The donor has an unrecognized ordinary-socket row class",
        ));
    }
    if socket_count > sundial::investment::MAX_WEAPON_SOCKETS
        || (!columns.is_empty() && columns.len() < socket_count)
        || columns.len() > sundial::investment::MAX_WEAPON_SOCKETS
    {
        return Err(invalid(format!(
            "The recipe has {} socket columns but its donor has {socket_count}, with at most {} ordinary sockets supported",
            columns.len(),
            sundial::investment::MAX_WEAPON_SOCKETS,
        )));
    }

    let mut resolved_choices = vec![None; socket_count.max(columns.len())];
    for (lane, column) in columns.iter().enumerate() {
        if lane >= socket_count {
            validate_added_socket_column(
                lane,
                column.as_ref().and_then(|column| column.socket_type),
                column.as_ref().map_or(0, |column| column.choices.len()),
            )?;
        }
        let Some(column) = column else {
            continue;
        };
        let mut indices = Vec::with_capacity(column.choices.len());
        for &hash in &column.choices {
            let matches = item_rows_by_hash
                .get(&hash)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            let [index] = matches else {
                return Err(invalid(format!(
                    "Socket column {lane} plug 0x{hash:08X} resolved to {} item rows",
                    matches.len()
                )));
            };
            indices.push(u16::try_from(*index).map_err(|_| {
                invalid(format!(
                    "Socket column {lane} plug index does not fit 16 bits"
                ))
            })?);
        }
        resolved_choices[lane] = Some(indices);
    }

    normalize_inherited_randomized_socket_columns(donor_definition, &mut resolved_choices)?;
    if resolved_choices.iter().all(Option::is_none) {
        return Ok(Vec::new());
    }
    Ok(resolved_choices
        .into_iter()
        .enumerate()
        .map(|(lane, choices)| {
            choices.map(|choices| {
                let authored = columns.get(lane).and_then(Option::as_ref);
                ResolvedSocketColumn {
                    choices,
                    socket_type: authored.and_then(|column| column.socket_type),
                    choice_weight_bits: authored
                        .map_or_else(Vec::new, |column| column.choice_weight_bits.clone()),
                    choice_conditions: authored
                        .map_or_else(Vec::new, |column| column.choice_conditions.clone()),
                    reusable_plug_set_index: authored
                        .and_then(|column| column.reusable_plug_set_index),
                    randomized_plug_set_index: authored
                        .and_then(|column| column.randomized_plug_set_index),
                    randomized_selection_program: authored.map_or_else(Vec::new, |column| {
                        column.randomized_selection_program.clone()
                    }),
                }
            })
        })
        .collect())
}

pub(super) fn replace_socket_choice_item_index(
    data: &mut [u8],
    socket_index: usize,
    choice_index: usize,
    source_item_index: u16,
    authored_item_index: u16,
) -> AuthoringResult<()> {
    if source_item_index == authored_item_index || authored_item_index == u16::MAX {
        return Err(invalid(
            "Private socket-plug replacement requires two distinct active item indices",
        ));
    }
    let resource = relative_target(data, ITEM_ORDINARY_SOCKET_POINTER_OFFSET)?;
    let (socket_count, _, socket_rows, socket_class) = array_at(data, resource)?;
    if socket_class != ITEM_ORDINARY_SOCKET_ROW_CLASS || socket_index >= socket_count {
        return Err(invalid(format!(
            "Private socket-plug replacement selects socket {socket_index} of {socket_count}"
        )));
    }
    let socket_row = socket_rows + socket_index * ITEM_ORDINARY_SOCKET_ROW_SIZE;
    let default_offset = socket_row + ITEM_ORDINARY_SOCKET_DEFAULT_PLUG_OFFSET;
    let default_item_index = read_u16(data, default_offset)?;
    let embedded_descriptor = socket_row + ITEM_ORDINARY_SOCKET_EMBEDDED_PLUGS_OFFSET;
    let embedded_count = usize::try_from(read_u64(data, embedded_descriptor)?)
        .map_err(|_| invalid("Private socket-plug member count does not fit memory"))?;
    let mut logical_choices = vec![(default_item_index, None)];
    if embedded_count != 0 {
        let (count, _, member_rows, member_class) = array_at(data, embedded_descriptor)?;
        if count != embedded_count || member_class != ITEM_ORDINARY_SOCKET_PLUG_MEMBER_ROW_CLASS {
            return Err(invalid(
                "Private socket-plug donor has an incompatible embedded choice array",
            ));
        }
        validate_socket_member_segment(data, member_rows, count)?;
        for member_index in 0..count {
            let member = member_rows + member_index * ITEM_ORDINARY_SOCKET_PLUG_MEMBER_ROW_SIZE;
            let item_index = read_u16(data, member)?;
            if item_index != u16::MAX
                && !logical_choices
                    .iter()
                    .any(|(existing, _)| *existing == item_index)
            {
                logical_choices.push((item_index, Some(member)));
            }
        }
    }
    let Some(&(selected_item_index, selected_member)) = logical_choices.get(choice_index) else {
        return Err(invalid(format!(
            "Private socket-plug replacement selects choice {choice_index} of {} on socket {socket_index}",
            logical_choices.len()
        )));
    };
    if selected_item_index != source_item_index {
        return Err(invalid(format!(
            "Private socket {socket_index} choice {choice_index} resolves to item index {selected_item_index}, not source index {source_item_index}"
        )));
    }
    if choice_index == 0 {
        write_u16(data, default_offset, authored_item_index)?;
        if embedded_count != 0 {
            let (_, _, member_rows, _) = array_at(data, embedded_descriptor)?;
            for member_index in 0..embedded_count {
                let member = member_rows + member_index * ITEM_ORDINARY_SOCKET_PLUG_MEMBER_ROW_SIZE;
                if read_u16(data, member)? == source_item_index {
                    write_u16(data, member, authored_item_index)?;
                }
            }
        }
    } else {
        let member = selected_member.ok_or_else(|| {
            validation("A non-default private socket choice has no native member row")
        })?;
        write_u16(data, member, authored_item_index)?;
    }
    Ok(())
}

pub(super) fn index_item_rows_by_hash(
    item_table: &[u8],
    rows: usize,
    count: usize,
) -> AuthoringResult<BTreeMap<u32, Vec<usize>>> {
    let mut item_rows_by_hash = BTreeMap::<u32, Vec<usize>>::new();
    for index in 0..count {
        let hash = read_u32(item_table, rows + index * ITEM_ROW_SIZE)?;
        item_rows_by_hash.entry(hash).or_default().push(index);
    }
    Ok(item_rows_by_hash)
}

pub(super) fn normalize_inherited_randomized_socket_columns(
    donor_definition: &[u8],
    columns: &mut [Option<Vec<u16>>],
) -> AuthoringResult<()> {
    let resource = relative_target(donor_definition, ITEM_ORDINARY_SOCKET_POINTER_OFFSET)?;
    let (count, _, rows, class) = array_at(donor_definition, resource)?;
    if class != ITEM_ORDINARY_SOCKET_ROW_CLASS
        || count > columns.len()
        || columns.len() > sundial::investment::MAX_WEAPON_SOCKETS
    {
        return Err(invalid(
            "The donor ordinary-socket rows cannot be normalized for curated authoring",
        ));
    }

    for (lane, column) in columns.iter_mut().take(count).enumerate() {
        let row = rows + lane * ITEM_ORDINARY_SOCKET_ROW_SIZE;
        let randomized_set = read_u16(
            donor_definition,
            row + ITEM_ORDINARY_SOCKET_RANDOMIZED_PLUG_SET_OFFSET,
        )?;
        let selection_program = row + ITEM_ORDINARY_SOCKET_RANDOMIZED_SELECTION_PROGRAM_OFFSET;
        let has_selection_program =
            socket_has_randomized_selection_program(donor_definition, selection_program, lane)?;
        if randomized_set == u16::MAX {
            if has_selection_program && column.is_some() {
                return Err(invalid(format!(
                    "Socket lane {lane} has a program-only selection topology that curated authoring does not support"
                )));
            }
            continue;
        }
        if !has_selection_program {
            return Err(invalid(format!(
                "Socket lane {lane} has a randomized plug set without its required selection program"
            )));
        }
        if column.is_some() {
            continue;
        }

        let default = read_u16(
            donor_definition,
            row + ITEM_ORDINARY_SOCKET_DEFAULT_PLUG_OFFSET,
        )?;
        if default == u16::MAX {
            return Err(invalid(format!(
                "Randomized socket lane {lane} has no native default plug"
            )));
        }
        let embedded_descriptor = row + ITEM_ORDINARY_SOCKET_EMBEDDED_PLUGS_OFFSET;
        let embedded_count = usize::try_from(read_u64(donor_definition, embedded_descriptor)?)
            .map_err(|_| invalid("Socket plug-member count is too large"))?;
        let socket_type = read_u16(donor_definition, row)?;
        let maximum = authored_socket_choice_limit(socket_type);
        if maximum == 0 {
            return Err(invalid(format!(
                "Randomized socket lane {lane} cannot be represented as a curated column"
            )));
        }

        let mut choices = vec![default];
        if embedded_count != 0 {
            let (member_count, _, member_rows, member_class) =
                array_at(donor_definition, embedded_descriptor)?;
            if member_count != embedded_count
                || member_class != ITEM_ORDINARY_SOCKET_PLUG_MEMBER_ROW_CLASS
            {
                return Err(invalid(format!(
                    "Randomized socket lane {lane} has an invalid embedded default list"
                )));
            }
            validate_socket_member_segment(donor_definition, member_rows, member_count)?;
            for index in 0..member_count {
                let member = member_rows + index * ITEM_ORDINARY_SOCKET_PLUG_MEMBER_ROW_SIZE;
                if canonical_socket_member_template(donor_definition, member).is_none() {
                    return Err(invalid(format!(
                        "Randomized socket lane {lane} has a conditional embedded default"
                    )));
                }
                let plug = read_u16(donor_definition, member)?;
                if plug != u16::MAX && !choices.contains(&plug) && choices.len() < maximum {
                    choices.push(plug);
                }
            }
        }
        *column = Some(choices);
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn weapon_default_plug_indices(data: &[u8]) -> AuthoringResult<Vec<u16>> {
    let resource = relative_target(data, ITEM_ORDINARY_SOCKET_POINTER_OFFSET)?;
    let (count, _, rows, class) = array_at(data, resource)?;
    if class != ITEM_ORDINARY_SOCKET_ROW_CLASS || count > sundial::investment::MAX_WEAPON_SOCKETS {
        return Err(invalid("Weapon ordinary-socket rows are incompatible"));
    }
    (0..count)
        .map(|index| {
            read_u16(
                data,
                rows + index * ITEM_ORDINARY_SOCKET_ROW_SIZE
                    + ITEM_ORDINARY_SOCKET_DEFAULT_PLUG_OFFSET,
            )
        })
        .collect()
}

pub(super) fn socket_has_randomized_selection_program(
    data: &[u8],
    descriptor: usize,
    lane: usize,
) -> AuthoringResult<bool> {
    let count = read_u64(data, descriptor)?;
    let pointer = read_i64(data, descriptor + 8)?;
    if count == 0 {
        if pointer != 0 {
            return Err(invalid(format!(
                "Socket lane {lane} has an empty randomized-selection program with a nonzero pointer"
            )));
        }
        return Ok(false);
    }
    numeric_program_layout(data, descriptor).map_err(|error| {
        invalid(format!(
            "Socket lane {lane} has an invalid randomized-selection program: {error}"
        ))
    })?;
    Ok(true)
}

pub(super) fn socket_member_segment_end(rows: usize, count: usize) -> AuthoringResult<usize> {
    rows.checked_add(
        count
            .checked_mul(ITEM_ORDINARY_SOCKET_PLUG_MEMBER_ROW_SIZE)
            .ok_or_else(|| invalid("Socket plug-member row extent overflowed"))?,
    )
    .and_then(|end| end.checked_add(NESTED_ARRAY_TRAILER.len()))
    .and_then(|end| end.checked_add(15))
    .map(|end| end & !15)
    .ok_or_else(|| invalid("Socket plug-member segment overflowed"))
}

pub(super) fn validate_socket_member_segment(
    data: &[u8],
    rows: usize,
    count: usize,
) -> AuthoringResult<usize> {
    let marker = rows
        .checked_sub(16 + NESTED_ARRAY_TRAILER.len())
        .and_then(|start| data.get(start..rows - 16));
    if marker != Some(NESTED_ARRAY_TRAILER.as_slice()) {
        return Err(invalid(
            "Socket member array is missing its native relocation marker",
        ));
    }
    let rows_end = rows
        .checked_add(
            count
                .checked_mul(ITEM_ORDINARY_SOCKET_PLUG_MEMBER_ROW_SIZE)
                .ok_or_else(|| invalid("Socket plug-member row extent overflowed"))?,
        )
        .ok_or_else(|| invalid("Socket plug-member rows overflowed"))?;
    let segment_end = socket_member_segment_end(rows, count)?;
    let trailer_start = segment_end - NESTED_ARRAY_TRAILER.len();
    if data
        .get(rows_end..trailer_start)
        .is_none_or(|padding| padding.iter().any(|byte| *byte != 0))
        || data.get(trailer_start..segment_end) != Some(&NESTED_ARRAY_TRAILER)
    {
        return Err(invalid(
            "The donor socket member list has noncanonical padding or trailer",
        ));
    }
    Ok(segment_end)
}

pub(super) fn canonical_socket_member_template(data: &[u8], rows: usize) -> Option<Vec<u8>> {
    let row = data.get(rows..rows + ITEM_ORDINARY_SOCKET_PLUG_MEMBER_ROW_SIZE)?;
    let expression_count = read_u64(row, 8).ok()?;
    let expression_pointer = read_i64(row, 16).ok()?;
    let leading_padding_is_zero = row.get(2..8)?.iter().all(|byte| *byte == 0);
    let trailing_padding_is_zero = row.get(28..32)?.iter().all(|byte| *byte == 0);
    (expression_count == 0
        && expression_pointer == 0
        && leading_padding_is_zero
        && trailing_padding_is_zero)
        .then(|| row.to_vec())
}

pub(super) fn write_numeric_program(
    data: &mut Vec<u8>,
    descriptor: usize,
    instructions: &[WeaponNumericInstruction],
) -> AuthoringResult<()> {
    if instructions.is_empty() {
        write_u64(data, descriptor, 0)?;
        write_i64(data, descriptor + 8, 0)?;
        return Ok(());
    }
    while data.len() % 16 != 0 {
        data.push(0);
    }
    data.extend_from_slice(&[0; 8]);
    data.extend_from_slice(&NESTED_ARRAY_TRAILER);
    let header = data.len();
    let mut segment = vec![0_u8; 16];
    write_u64(&mut segment, 0, instructions.len() as u64)?;
    write_u32(&mut segment, 8, NUMERIC_PROGRAM_ROW_CLASS)?;
    for instruction in instructions {
        let mut row = [0_u8; NUMERIC_INSTRUCTION_ROW_SIZE];
        row[0] = instruction.opcode;
        row[4..6].copy_from_slice(&instruction.operand.to_le_bytes());
        segment.extend_from_slice(&row);
    }
    while (segment.len() + NESTED_ARRAY_TRAILER.len()) % 16 != 0 {
        segment.push(0);
    }
    segment.extend_from_slice(&NESTED_ARRAY_TRAILER);
    data.extend_from_slice(&segment);
    write_u64(data, descriptor, instructions.len() as u64)?;
    write_relative_pointer(data, descriptor + 8, header)?;
    Ok(())
}

pub(super) fn read_numeric_program(
    data: &[u8],
    descriptor: usize,
) -> AuthoringResult<Vec<WeaponNumericInstruction>> {
    let count = read_u64(data, descriptor)?;
    let pointer = read_i64(data, descriptor + 8)?;
    if count == 0 {
        if pointer != 0 {
            return Err(invalid(
                "Empty numeric program has a nonzero relative pointer",
            ));
        }
        return Ok(Vec::new());
    }
    Ok(numeric_program_layout(data, descriptor)?
        .tokens
        .into_iter()
        .map(|(opcode, operand)| WeaponNumericInstruction { opcode, operand })
        .collect())
}

fn validate_added_socket_column(
    lane: usize,
    socket_type: Option<u16>,
    choice_count: usize,
) -> AuthoringResult<()> {
    if socket_type.is_none_or(|socket_type| authored_socket_choice_limit(socket_type) == 0)
        || choice_count == 0
    {
        return Err(invalid(format!(
            "Added socket lane {lane} requires an explicit active socket type and at least one plug choice"
        )));
    }
    Ok(())
}

/// Appends a larger row array while keeping every nested payload at its original address.
/// Relative pointers belong to their row's location, so copied descriptors are rebased.
fn grow_socket_rows(
    data: &mut Vec<u8>,
    descriptor: usize,
    rows: usize,
    count: usize,
    new_count: usize,
) -> AuthoringResult<usize> {
    if count == new_count {
        return Ok(rows);
    }
    let source_end = rows
        .checked_add(count * ITEM_ORDINARY_SOCKET_ROW_SIZE)
        .ok_or_else(|| invalid("The donor socket row extent overflowed"))?;
    let original_rows = data
        .get(rows..source_end)
        .ok_or_else(|| invalid("The donor socket rows are truncated"))?
        .to_vec();
    let mut nested_targets = Vec::new();
    for lane in 0..count {
        for offset in SOCKET_ROW_ARRAY_DESCRIPTOR_OFFSETS {
            let source_descriptor = rows + lane * ITEM_ORDINARY_SOCKET_ROW_SIZE + offset;
            let nested_count = read_u64(data, source_descriptor)?;
            let relative = read_i64(data, source_descriptor + 8)?;
            if nested_count == 0 && relative == 0 {
                continue;
            }
            if relative == 0 {
                return Err(invalid(format!(
                    "Socket lane {lane} has an inconsistent nested array descriptor"
                )));
            }
            let (_, target, _, _) = array_at(data, source_descriptor)?;
            nested_targets.push((lane, offset + 8, target));
        }
    }

    while data.len() % 16 != 0 {
        data.push(0);
    }
    data.extend_from_slice(&[0; 8]);
    data.extend_from_slice(&NESTED_ARRAY_TRAILER);
    let header = data.len();
    let new_rows = header + 16;
    let mut segment = vec![0_u8; 16];
    write_u64(&mut segment, 0, new_count as u64)?;
    write_u32(&mut segment, 8, ITEM_ORDINARY_SOCKET_ROW_CLASS)?;
    segment.extend_from_slice(&original_rows);
    for _ in count..new_count {
        let mut row = [0_u8; ITEM_ORDINARY_SOCKET_ROW_SIZE];
        for offset in [
            0,
            ITEM_ORDINARY_SOCKET_DEFAULT_PLUG_OFFSET,
            ITEM_ORDINARY_SOCKET_REUSABLE_PLUG_SET_OFFSET,
            ITEM_ORDINARY_SOCKET_RANDOMIZED_PLUG_SET_OFFSET,
        ] {
            write_u16(&mut row, offset, u16::MAX)?;
        }
        // The two additional 16-bit scalar indices use the same disabled sentinel
        // in stock empty and ordinary trait rows. Zero would select native row zero.
        write_u32(&mut row, 4, u32::MAX)?;
        segment.extend_from_slice(&row);
    }
    while (segment.len() + NESTED_ARRAY_TRAILER.len()) % 16 != 0 {
        segment.push(0);
    }
    segment.extend_from_slice(&NESTED_ARRAY_TRAILER);
    data.extend_from_slice(&segment);
    for (lane, offset, target) in nested_targets {
        write_relative_pointer(
            data,
            new_rows + lane * ITEM_ORDINARY_SOCKET_ROW_SIZE + offset,
            target,
        )?;
    }
    write_u64(data, descriptor, new_count as u64)?;
    write_relative_pointer(data, descriptor + 8, header)?;
    Ok(new_rows)
}

pub(super) fn set_weapon_socket_columns(
    data: &mut Vec<u8>,
    columns: &[Option<ResolvedSocketColumn>],
) -> AuthoringResult<()> {
    let resource = relative_target(data, ITEM_ORDINARY_SOCKET_POINTER_OFFSET)?;
    let (count, _, rows, class) = array_at(data, resource)?;
    if class != ITEM_ORDINARY_SOCKET_ROW_CLASS
        || count > columns.len()
        || columns.len() > sundial::investment::MAX_WEAPON_SOCKETS
    {
        return Err(invalid(format!(
            "The donor has {count} ordinary sockets and cannot be authored with {} sockets, with at most {} supported",
            columns.len(),
            sundial::investment::MAX_WEAPON_SOCKETS,
        )));
    }
    for (lane, column) in columns.iter().enumerate().skip(count) {
        validate_added_socket_column(
            lane,
            column.as_ref().and_then(|column| column.socket_type),
            column.as_ref().map_or(0, |column| column.choices.len()),
        )?;
    }
    if columns.iter().all(Option::is_none) {
        return Ok(());
    }
    let native_socket_types = (0..count)
        .map(|lane| read_u16(data, rows + lane * ITEM_ORDINARY_SOCKET_ROW_SIZE))
        .collect::<AuthoringResult<Vec<_>>>()?;
    let authored_socket_types = columns
        .iter()
        .enumerate()
        .map(|(lane, column)| {
            column
                .as_ref()
                .and_then(|column| column.socket_type)
                .or_else(|| native_socket_types.get(lane).copied())
                .ok_or_else(|| invalid(format!("Added socket lane {lane} has no socket type")))
        })
        .collect::<AuthoringResult<Vec<_>>>()?;

    for (lane, column) in columns.iter().enumerate() {
        let Some(column) = column else {
            continue;
        };
        let maximum = authored_socket_choice_limit(authored_socket_types[lane]);
        if !(1..=maximum).contains(&column.choices.len()) {
            return Err(invalid(format!(
                "Socket lane {lane} accepts at most {maximum} authored choices, but {} were requested",
                column.choices.len()
            )));
        }
        if column.choices.contains(&u16::MAX)
            || column
                .choices
                .iter()
                .copied()
                .collect::<BTreeSet<_>>()
                .len()
                != column.choices.len()
        {
            return Err(invalid(format!(
                "Socket lane {lane} contains disabled or duplicate plug choices"
            )));
        }
        if lane >= count {
            continue;
        }
        let row = rows + lane * ITEM_ORDINARY_SOCKET_ROW_SIZE;
        let randomized_set = read_u16(data, row + ITEM_ORDINARY_SOCKET_RANDOMIZED_PLUG_SET_OFFSET)?;
        let randomized_selection = row + ITEM_ORDINARY_SOCKET_RANDOMIZED_SELECTION_PROGRAM_OFFSET;
        let has_randomized_selection =
            socket_has_randomized_selection_program(data, randomized_selection, lane)?;
        if (randomized_set != u16::MAX) != has_randomized_selection {
            return Err(invalid(format!(
                "Socket lane {lane} has inconsistent donor randomized-selection metadata"
            )));
        }
    }

    let rows = grow_socket_rows(data, resource, rows, count, columns.len())?;
    let preserved_rows = columns
        .iter()
        .enumerate()
        .filter_map(|(lane, column)| column.is_none().then_some(lane))
        .map(|lane| {
            let row = rows + lane * ITEM_ORDINARY_SOCKET_ROW_SIZE;
            Ok((
                lane,
                data.get(row..row + ITEM_ORDINARY_SOCKET_ROW_SIZE)
                    .ok_or_else(|| invalid("The donor socket row is truncated"))?
                    .to_vec(),
            ))
        })
        .collect::<AuthoringResult<Vec<_>>>()?;

    for (lane, column) in columns.iter().enumerate() {
        let Some(column) = column else {
            continue;
        };
        let row = rows + lane * ITEM_ORDINARY_SOCKET_ROW_SIZE;
        write_u16(data, row, authored_socket_types[lane])?;
        write_u16(
            data,
            row + ITEM_ORDINARY_SOCKET_DEFAULT_PLUG_OFFSET,
            column.choices[0],
        )?;
        write_u16(
            data,
            row + ITEM_ORDINARY_SOCKET_REUSABLE_PLUG_SET_OFFSET,
            column.reusable_plug_set_index.unwrap_or(u16::MAX),
        )?;
        let randomized_selection = row + ITEM_ORDINARY_SOCKET_RANDOMIZED_SELECTION_PROGRAM_OFFSET;
        write_numeric_program(
            data,
            randomized_selection,
            &column.randomized_selection_program,
        )?;
        write_u16(
            data,
            row + ITEM_ORDINARY_SOCKET_RANDOMIZED_PLUG_SET_OFFSET,
            column.randomized_plug_set_index.unwrap_or(u16::MAX),
        )?;

        while data.len() % 16 != 0 {
            data.push(0);
        }
        // A trailer after this array does not identify its own header to the native loader.
        // Always emit the leading marker, including the first appended socket or condition.
        data.extend_from_slice(&[0; 8]);
        data.extend_from_slice(&NESTED_ARRAY_TRAILER);
        let member_header = data.len();
        let mut member_segment = vec![0_u8; 16];
        write_u64(&mut member_segment, 0, column.choices.len() as u64)?;
        write_u32(
            &mut member_segment,
            8,
            ITEM_ORDINARY_SOCKET_PLUG_MEMBER_ROW_CLASS,
        )?;
        for (choice, &plug_index) in column.choices.iter().enumerate() {
            let mut member = [0_u8; ITEM_ORDINARY_SOCKET_PLUG_MEMBER_ROW_SIZE];
            write_u16(&mut member, 0, plug_index)?;
            let weight_bits = column
                .choice_weight_bits
                .get(choice)
                .copied()
                .unwrap_or_else(|| 1.0_f32.to_bits());
            write_u32(
                &mut member,
                ITEM_ORDINARY_SOCKET_PLUG_MEMBER_WEIGHT_OFFSET,
                weight_bits,
            )?;
            member_segment.extend_from_slice(&member);
        }
        while (member_segment.len() + NESTED_ARRAY_TRAILER.len()) % 16 != 0 {
            member_segment.push(0);
        }
        member_segment.extend_from_slice(&NESTED_ARRAY_TRAILER);
        data.extend_from_slice(&member_segment);
        let embedded_descriptor = row + ITEM_ORDINARY_SOCKET_EMBEDDED_PLUGS_OFFSET;
        write_u64(data, embedded_descriptor, column.choices.len() as u64)?;
        write_relative_pointer(data, embedded_descriptor + 8, member_header)?;
        let member_rows = member_header + 16;
        for choice in 0..column.choices.len() {
            let condition = column
                .choice_conditions
                .get(choice)
                .map_or(&[][..], Vec::as_slice);
            write_numeric_program(
                data,
                member_rows + choice * ITEM_ORDINARY_SOCKET_PLUG_MEMBER_ROW_SIZE + 8,
                condition,
            )?;
        }
    }
    for (lane, original) in preserved_rows {
        let row = rows + lane * ITEM_ORDINARY_SOCKET_ROW_SIZE;
        if data.get(row..row + ITEM_ORDINARY_SOCKET_ROW_SIZE) != Some(original.as_slice()) {
            return Err(validation(format!(
                "Authoring a socket column changed untouched socket lane {lane}"
            )));
        }
    }
    validate_weapon_socket_columns(data, columns, &authored_socket_types)?;
    Ok(())
}

pub(super) fn validate_weapon_socket_columns(
    data: &[u8],
    columns: &[Option<ResolvedSocketColumn>],
    expected_socket_types: &[u16],
) -> AuthoringResult<()> {
    let resource = relative_target(data, ITEM_ORDINARY_SOCKET_POINTER_OFFSET)?;
    let (count, _, rows, class) = array_at(data, resource)?;
    if class != ITEM_ORDINARY_SOCKET_ROW_CLASS
        || count != columns.len()
        || count != expected_socket_types.len()
        || count > sundial::investment::MAX_WEAPON_SOCKETS
    {
        return Err(validation(
            "Authored weapon has an invalid ordinary-socket array",
        ));
    }
    for (lane, column) in columns.iter().enumerate() {
        let Some(column) = column else {
            continue;
        };
        let row = rows + lane * ITEM_ORDINARY_SOCKET_ROW_SIZE;
        if read_u16(data, row)? != column.socket_type.unwrap_or(expected_socket_types[lane])
            || read_u16(data, row + ITEM_ORDINARY_SOCKET_DEFAULT_PLUG_OFFSET)? != column.choices[0]
            || read_u16(data, row + ITEM_ORDINARY_SOCKET_REUSABLE_PLUG_SET_OFFSET)?
                != column.reusable_plug_set_index.unwrap_or(u16::MAX)
            || read_u16(data, row + ITEM_ORDINARY_SOCKET_RANDOMIZED_PLUG_SET_OFFSET)?
                != column.randomized_plug_set_index.unwrap_or(u16::MAX)
        {
            return Err(validation(format!(
                "Authored weapon socket lane {lane} has inconsistent scalar metadata"
            )));
        }
        let randomized_selection = row + ITEM_ORDINARY_SOCKET_RANDOMIZED_SELECTION_PROGRAM_OFFSET;
        if read_numeric_program(data, randomized_selection)? != column.randomized_selection_program
        {
            return Err(validation(format!(
                "Authored weapon socket lane {lane} has inconsistent randomized-selection metadata"
            )));
        }
        let embedded_descriptor = row + ITEM_ORDINARY_SOCKET_EMBEDDED_PLUGS_OFFSET;
        let (member_count, _, member_rows, member_class) = array_at(data, embedded_descriptor)?;
        if member_count != column.choices.len()
            || member_class != ITEM_ORDINARY_SOCKET_PLUG_MEMBER_ROW_CLASS
        {
            return Err(validation(format!(
                "Authored weapon socket lane {lane} has an inconsistent embedded choice array"
            )));
        }
        validate_socket_member_segment(data, member_rows, member_count)
            .map_err(|error| validation(error.to_string()))?;
        for (choice, expected_plug) in column.choices.iter().copied().enumerate() {
            let member = member_rows + choice * ITEM_ORDINARY_SOCKET_PLUG_MEMBER_ROW_SIZE;
            let expected_weight = column
                .choice_weight_bits
                .get(choice)
                .copied()
                .unwrap_or_else(|| 1.0_f32.to_bits());
            let expected_condition = column
                .choice_conditions
                .get(choice)
                .map_or(&[][..], Vec::as_slice);
            if read_u16(data, member)? != expected_plug
                || data
                    .get(member + 2..member + 8)
                    .is_none_or(|padding| padding.iter().any(|byte| *byte != 0))
                || read_u32(
                    data,
                    member + ITEM_ORDINARY_SOCKET_PLUG_MEMBER_WEIGHT_OFFSET,
                )? != expected_weight
                || data
                    .get(member + 28..member + 32)
                    .is_none_or(|padding| padding.iter().any(|byte| *byte != 0))
                || read_numeric_program(data, member + 8)? != expected_condition
            {
                return Err(validation(format!(
                    "Authored weapon socket lane {lane} choice {choice} is inconsistent"
                )));
            }
        }
    }
    Ok(())
}

use super::*;

pub(super) fn canonical_single_flag_program(
    template: &[u8],
    count: usize,
    source_flag: u16,
    authored_flag: u16,
) -> AuthoringResult<Vec<u8>> {
    let flag_template = (0..count)
        .map(|index| NumericInstruction::read(template, 16 + index * NUMERIC_INSTRUCTION_ROW_SIZE))
        .collect::<AuthoringResult<Vec<_>>>()?
        .into_iter()
        .find(|instruction| {
            instruction.opcode == NUMERIC_FLAG_INSTRUCTION && instruction.operand == source_flag
        })
        .ok_or_else(|| {
            invalid("Donor collectible acquisition condition has no matching FLAG instruction")
        })?;
    let mut program = template
        .get(..16)
        .ok_or_else(|| invalid("Donor collectible acquisition header is truncated"))?
        .to_vec();
    write_u64(&mut program, 0, 1)?;
    program.extend_from_slice(
        &flag_template
            .with_semantics(NUMERIC_FLAG_INSTRUCTION, authored_flag)
            .serialized,
    );
    while (program.len() + NESTED_ARRAY_TRAILER.len()) % 16 != 0 {
        program.push(0);
    }
    program.extend_from_slice(&NESTED_ARRAY_TRAILER);
    Ok(program)
}

pub(crate) fn patch_project_collection_objectives(
    mut objectives: Vec<u8>,
    nodes: &[u8],
    page_counts: &BTreeMap<u16, usize>,
    weapon_count: usize,
) -> AuthoringResult<Vec<u8>> {
    let (objective_count, _, objective_rows, objective_class) = array_at(&objectives, 8)?;
    let (node_count, _, node_rows, _) = array_at(nodes, 8)?;
    if objective_class != OBJECTIVE_ROW_CLASS {
        return Err(invalid("Objective table has an unexpected row class"));
    }
    let mut patched = BTreeSet::new();
    for (&page, &increment) in page_counts {
        let page = usize::from(page);
        if page >= node_count || increment == 0 {
            return Err(invalid("Project weapon page is outside its node table"));
        }
        let node = node_rows + page * PRESENTATION_NODE_ROW_SIZE;
        let objective = usize::from(read_u16(
            nodes,
            node + PRESENTATION_NODE_OBJECTIVE_INDEX_OFFSET,
        )?);
        let (child_count, _, _, child_class) =
            array_at(nodes, node + PRESENTATION_NODE_COLLECTIBLES_OFFSET)?;
        if objective >= objective_count
            || !patched.insert(objective)
            || child_class != PRESENTATION_NODE_COLLECTIBLE_ROW_CLASS
            || child_count < increment
        {
            return Err(invalid(
                "Project weapon page has no unique compatible count objective",
            ));
        }
        let completion =
            objective_rows + objective * OBJECTIVE_ROW_SIZE + OBJECTIVE_COMPLETION_VALUE_OFFSET;
        let stock = read_i32(&objectives, completion)?;
        let increment = i32::try_from(increment)
            .map_err(|_| invalid("Project page increment does not fit i32"))?;
        let final_value = stock
            .checked_add(increment)
            .ok_or_else(|| invalid("Project page objective overflowed"))?;
        if usize::try_from(final_value).ok() != Some(child_count) {
            return Err(invalid(
                "Project page objective does not match its final child count",
            ));
        }
        write_i32(&mut objectives, completion, final_value)?;
    }

    if page_counts.values().sum::<usize>() != weapon_count {
        return Err(invalid(
            "Collection page counts disagree with the authored weapon count",
        ));
    }
    // Exotic and ordinary weapons have different ancestors. Update only ancestors
    // reached by each destination page, not the ordinary Weapons total unconditionally.
    let mut ancestor_pages = BTreeMap::<usize, BTreeSet<u16>>::new();
    for &page in page_counts.keys() {
        for ancestor in presentation_ancestor_nodes(nodes, &[page])? {
            if ancestor == usize::from(page) {
                continue;
            }
            let objective = usize::from(read_u16(
                nodes,
                node_rows
                    + ancestor * PRESENTATION_NODE_ROW_SIZE
                    + PRESENTATION_NODE_OBJECTIVE_INDEX_OFFSET,
            )?);
            if objective == usize::from(u16::MAX) {
                continue;
            }
            if objective >= objective_count {
                return Err(invalid(
                    "Collection ancestor objective is outside its table",
                ));
            }
            if patched.contains(&objective) {
                return Err(invalid(
                    "A collection page shares its count objective with an ancestor",
                ));
            }
            ancestor_pages.entry(objective).or_default().insert(page);
        }
    }
    for (objective, pages) in ancestor_pages {
        let completion =
            objective_rows + objective * OBJECTIVE_ROW_SIZE + OBJECTIVE_COMPLETION_VALUE_OFFSET;
        let stock = read_i32(&objectives, completion)?;
        // Zero is the native disabled/uncounted completion target.
        if stock == 0 {
            continue;
        }
        let increment = pages.iter().map(|page| page_counts[page]).sum::<usize>();
        let increment = i32::try_from(increment)
            .map_err(|_| invalid("Aggregate increment does not fit i32"))?;
        write_i32(
            &mut objectives,
            completion,
            stock
                .checked_add(increment)
                .ok_or_else(|| invalid("Aggregate objective overflowed"))?,
        )?;
    }
    Ok(objectives)
}

pub(crate) fn numeric_program_layout(
    data: &[u8],
    descriptor: usize,
) -> AuthoringResult<NumericProgramLayout> {
    let (count, header, rows, class) = array_at(data, descriptor)?;
    let rows_end = rows
        .checked_add(
            count
                .checked_mul(NUMERIC_INSTRUCTION_ROW_SIZE)
                .ok_or_else(|| invalid("Numeric-program instruction size overflowed"))?,
        )
        .ok_or_else(|| invalid("Numeric-program instruction range overflowed"))?;
    // Appending an odd number of 184-byte collectible rows shifts these
    // segments by eight bytes. Their padding stays relative to the header.
    let segment_end = rows_end
        .checked_sub(header)
        .ok_or_else(|| invalid("Numeric-program rows precede its header"))?
        .checked_add(NESTED_ARRAY_TRAILER.len())
        .and_then(|end| end.checked_add(15))
        .map(|end| end & !15)
        .and_then(|length| header.checked_add(length))
        .ok_or_else(|| invalid("Numeric-program trailer range overflowed"))?;
    let trailer_start = segment_end - NESTED_ARRAY_TRAILER.len();
    let reserved_valid = data
        .get(header + 12..header + 16)
        .is_some_and(|reserved| reserved.iter().all(|byte| *byte == 0));
    let padding_valid = data
        .get(rows_end..trailer_start)
        .is_some_and(|padding| padding.iter().all(|byte| *byte == 0));
    let trailer_valid = data.get(trailer_start..segment_end) == Some(&NESTED_ARRAY_TRAILER);
    if class != NUMERIC_PROGRAM_ROW_CLASS
        || rows != header + 16
        || !reserved_valid
        || !padding_valid
        || !trailer_valid
    {
        return Err(invalid(format!(
            "Numeric-expression program has an unexpected serialized layout (header=0x{header:X}, rows=0x{rows:X}, class=0x{class:08X}, reserved={reserved_valid}, padding={padding_valid}, trailer={trailer_valid})"
        )));
    }
    let instructions = (0..count)
        .map(|index| {
            let row = rows + index * NUMERIC_INSTRUCTION_ROW_SIZE;
            NumericInstruction::read(data, row)
        })
        .collect::<AuthoringResult<Vec<_>>>()?;
    let tokens = instructions
        .iter()
        .map(|instruction| (instruction.opcode, instruction.operand))
        .collect();
    Ok(NumericProgramLayout {
        count,
        header,
        rows_end,
        segment_end,
        instructions,
        tokens,
    })
}

pub(crate) fn numeric_program_stack_depth(tokens: &[(u8, u16)]) -> AuthoringResult<usize> {
    let mut depth = 0usize;
    for (position, (opcode, operand)) in tokens.iter().copied().enumerate() {
        match opcode {
            // Flag, value, literal, and shared-pool instructions each push one numeric value.
            1 | 10 | 11 | 12 => {
                depth = depth
                    .checked_add(1)
                    .ok_or_else(|| invalid("Numeric-program stack depth overflowed"))?;
            }
            // Logical NOT and the numeric coercion instruction are unary.
            2 | 22 => {
                if depth < 1 {
                    return Err(invalid(format!(
                        "Numeric program underflows at unary instruction {position}"
                    )));
                }
            }
            // Logical, comparison, arithmetic, FNV-combine, and bitwise instructions
            // consume two values and push one.
            3..=9 | 13..=21 | 24..=27 => {
                if depth < 2 {
                    return Err(invalid(format!(
                        "Numeric program underflows at binary instruction {position}"
                    )));
                }
                if opcode == NUMERIC_ADD_INSTRUCTION && operand != u16::MAX {
                    return Err(invalid(format!(
                        "Numeric ADD instruction {position} has operand 0x{operand:04X}"
                    )));
                }
                depth -= 1;
            }
            _ => {
                return Err(invalid(format!(
                    "Numeric program uses unsupported opcode {opcode} at instruction {position}"
                )));
            }
        }
    }
    Ok(depth)
}

pub(crate) fn validate_shared_expression_table(
    data: &[u8],
    require_terminal_parallel_array: bool,
) -> AuthoringResult<(usize, usize)> {
    let (count, _, rows, class) = array_at(data, 8)?;
    let (parallel_count, _, parallel_rows, parallel_class) = array_at(data, 0x18)?;
    let rows_end = rows
        .checked_add(
            count
                .checked_mul(SHARED_EXPRESSION_POOL_ROW_SIZE)
                .ok_or_else(|| invalid("Shared-expression pool row size overflowed"))?,
        )
        .ok_or_else(|| invalid("Shared-expression pool row range overflowed"))?;
    let parallel_end = parallel_rows
        .checked_add(
            parallel_count
                .checked_mul(size_of::<u16>())
                .ok_or_else(|| invalid("Shared-expression parallel row size overflowed"))?,
        )
        .ok_or_else(|| invalid("Shared-expression parallel row range overflowed"))?;
    if count != SHARED_EXPRESSION_POOL_COUNT
        || parallel_count != count
        || class != SHARED_EXPRESSION_POOL_ROW_CLASS
        || parallel_class != SHARED_EXPRESSION_PARALLEL_ROW_CLASS
        || rows_end > data.len()
        || parallel_end > data.len()
        || (require_terminal_parallel_array && parallel_end != data.len())
    {
        return Err(invalid(
            "Shared numeric-expression pool table has an unexpected top-level layout",
        ));
    }
    Ok((rows, parallel_end))
}

pub(super) fn presentation_ancestor_nodes(
    nodes: &[u8],
    starts: &[u16],
) -> AuthoringResult<BTreeSet<usize>> {
    let (node_count, _, node_rows, _) = array_at(nodes, 8)?;
    let mut visited = BTreeSet::new();
    let mut pending = starts.iter().copied().map(usize::from).collect::<Vec<_>>();
    while let Some(index) = pending.pop() {
        if index >= node_count || !visited.insert(index) {
            continue;
        }
        let descriptor = node_rows + index * PRESENTATION_NODE_ROW_SIZE + 0x18;
        if read_u64(nodes, descriptor)? == 0 {
            continue;
        }
        let (count, _, parents, class) = array_at(nodes, descriptor)?;
        if class != PRESENTATION_NODE_INDEX_ROW_CLASS {
            return Err(invalid(
                "Presentation ancestor array has an unexpected class",
            ));
        }
        for position in 0..count {
            pending.push(usize::from(read_u16(nodes, parents + position * 2)?));
        }
    }
    Ok(visited)
}

pub(super) fn pool_dependency_closure(
    pools: &[u8],
    pool_rows: usize,
    root: usize,
) -> AuthoringResult<BTreeSet<usize>> {
    fn visit(
        pools: &[u8],
        pool_rows: usize,
        index: usize,
        visited: &mut BTreeSet<usize>,
        active: &mut BTreeSet<usize>,
    ) -> AuthoringResult<()> {
        if index >= SHARED_EXPRESSION_POOL_COUNT {
            return Err(invalid(
                "Objective references an out-of-range expression pool",
            ));
        }
        if visited.contains(&index) {
            return Ok(());
        }
        if !active.insert(index) {
            return Err(invalid("Expression-pool dependency graph contains a cycle"));
        }
        let descriptor = pool_rows
            + index * SHARED_EXPRESSION_POOL_ROW_SIZE
            + SHARED_EXPRESSION_DESCRIPTOR_OFFSET;
        let layout = numeric_program_layout(pools, descriptor)?;
        for (opcode, operand) in layout.tokens {
            if opcode == NUMERIC_POOL_INSTRUCTION {
                visit(pools, pool_rows, usize::from(operand), visited, active)?;
            }
        }
        active.remove(&index);
        visited.insert(index);
        Ok(())
    }
    let mut visited = BTreeSet::new();
    visit(pools, pool_rows, root, &mut visited, &mut BTreeSet::new())?;
    Ok(visited)
}

pub(super) fn objective_pool_roots(
    objectives: &[u8],
    nodes: &[u8],
    node_indices: &BTreeSet<usize>,
) -> AuthoringResult<BTreeSet<usize>> {
    let (objective_count, _, objective_rows, objective_class) = array_at(objectives, 8)?;
    let (node_count, _, node_rows, _) = array_at(nodes, 8)?;
    if objective_class != OBJECTIVE_ROW_CLASS {
        return Err(invalid("Objective table has an unexpected class"));
    }
    let mut roots = BTreeSet::new();
    for &node_index in node_indices {
        if node_index >= node_count {
            return Err(invalid("Presentation ancestor is outside its table"));
        }
        let objective = read_u16(
            nodes,
            node_rows
                + node_index * PRESENTATION_NODE_ROW_SIZE
                + PRESENTATION_NODE_OBJECTIVE_INDEX_OFFSET,
        )?;
        if objective == u16::MAX {
            continue;
        }
        let objective = usize::from(objective);
        if objective >= objective_count {
            return Err(invalid("Presentation node references an invalid objective"));
        }
        let row = objective_rows + objective * OBJECTIVE_ROW_SIZE;
        for field in [0x08usize, 0x38] {
            if read_u64(objectives, row + field)? == 0 {
                continue;
            }
            let layout = numeric_program_layout(objectives, row + field)?;
            roots.extend(layout.tokens.into_iter().filter_map(|(opcode, operand)| {
                (opcode == NUMERIC_POOL_INSTRUCTION).then_some(operand as usize)
            }));
        }
    }
    Ok(roots)
}

mod objectives;
pub(super) use objectives::pools_for_objective_roots;

pub(crate) fn classify_sunrise_count_pools(
    pools: &[u8],
    nodes: &[u8],
    _collectibles: &[u8],
    objectives: &[u8],
    donor_parents: &[u16],
    weapon_page: u16,
    source_acquired_flag_index: u16,
) -> AuthoringResult<SunriseAcquiredPoolSelection> {
    let (pool_rows, _) = validate_shared_expression_table(pools, true)?;
    let badge_parents = donor_parents
        .iter()
        .copied()
        .filter(|parent| *parent != weapon_page)
        .collect::<Vec<_>>();
    let allowed_nodes = presentation_ancestor_nodes(nodes, &[weapon_page])?;
    let badge_nodes = presentation_ancestor_nodes(nodes, &badge_parents)?;
    let allowed_roots = objective_pool_roots(objectives, nodes, &allowed_nodes)?;
    let badge_roots = objective_pool_roots(objectives, nodes, &badge_nodes)?;
    let selected =
        pools_for_objective_roots(pools, pool_rows, &allowed_roots, source_acquired_flag_index)?;
    let excluded_badge =
        pools_for_objective_roots(pools, pool_rows, &badge_roots, source_acquired_flag_index)?;
    if selected.is_empty() || !selected.is_disjoint(&excluded_badge) {
        return Err(invalid(
            "Retained collection and removed badge acquired-count pool closures are empty or ambiguous",
        ));
    }
    Ok(SunriseAcquiredPoolSelection {
        selected,
        excluded_badge,
    })
}

pub(super) fn direct_acquired_count_pool_links(
    data: &[u8],
    pool_rows: usize,
    pool_count: usize,
    source_acquired_flag_index: u16,
) -> AuthoringResult<Vec<AcquiredCountPoolLink>> {
    let mut links = Vec::new();
    for pool_index in 0..pool_count {
        let descriptor = pool_rows
            + pool_index * SHARED_EXPRESSION_POOL_ROW_SIZE
            + SHARED_EXPRESSION_DESCRIPTOR_OFFSET;
        let layout = numeric_program_layout(data, descriptor)?;
        let direct_count = layout
            .tokens
            .iter()
            .filter(|token| **token == (NUMERIC_FLAG_INSTRUCTION, source_acquired_flag_index))
            .count();
        let additive = direct_count == 1
            && direct_flag_reaches_root_through_add(&layout.tokens, source_acquired_flag_index)?;
        if direct_count != 1 || !additive {
            continue;
        }
        links.push(AcquiredCountPoolLink {
            pool_index,
            source_count: layout.count,
            source_opcode: NUMERIC_FLAG_INSTRUCTION,
            source_operand: source_acquired_flag_index,
            direct_additive_flag: true,
        });
    }
    Ok(links)
}

pub(super) fn direct_flag_reaches_root_through_add(
    tokens: &[(u8, u16)],
    source_acquired_flag_index: u16,
) -> AuthoringResult<bool> {
    if tokens
        .iter()
        .filter(|token| **token == (NUMERIC_FLAG_INSTRUCTION, source_acquired_flag_index))
        .count()
        != 1
    {
        return Ok(false);
    }

    let mut stack = Vec::new();
    for (position, (opcode, operand)) in tokens.iter().copied().enumerate() {
        match opcode {
            1 | 10 | 11 | 12 => stack.push(NumericStackNode {
                contains_target: opcode == NUMERIC_FLAG_INSTRUCTION
                    && operand == source_acquired_flag_index,
                additive_target_path: true,
            }),
            2 | 22 => {
                let child = stack.pop().ok_or_else(|| {
                    invalid(format!(
                        "Numeric program underflows at unary instruction {position}"
                    ))
                })?;
                stack.push(NumericStackNode {
                    contains_target: child.contains_target,
                    additive_target_path: !child.contains_target,
                });
            }
            3 | 4 | 8 | 9 | 13 | 14 | 17 => {
                let right = stack.pop().ok_or_else(|| {
                    invalid(format!(
                        "Numeric program underflows at binary instruction {position}"
                    ))
                })?;
                let left = stack.pop().ok_or_else(|| {
                    invalid(format!(
                        "Numeric program underflows at binary instruction {position}"
                    ))
                })?;
                if opcode == NUMERIC_ADD_INSTRUCTION && operand != u16::MAX {
                    return Err(invalid(format!(
                        "Numeric ADD instruction {position} has operand 0x{operand:04X}"
                    )));
                }
                let contains_target = left.contains_target || right.contains_target;
                stack.push(NumericStackNode {
                    contains_target,
                    additive_target_path: (!left.contains_target || left.additive_target_path)
                        && (!right.contains_target || right.additive_target_path)
                        && (!contains_target || opcode == NUMERIC_ADD_INSTRUCTION),
                });
            }
            // Unknown expression operators cannot prove an additive path, so leave that
            // direct occurrence untouched instead of guessing at its semantics.
            _ => return Ok(false),
        }
    }
    let [root] = stack.as_slice() else {
        return Err(invalid(format!(
            "Numeric program leaves {} values on its stack",
            stack.len()
        )));
    };
    Ok(root.contains_target && root.additive_target_path)
}

pub(crate) fn shared_numeric_instruction_template(
    data: &[u8],
    pool_rows: usize,
    opcode: u8,
    operand: Option<u16>,
) -> AuthoringResult<NumericInstruction> {
    for pool_index in 0..SHARED_EXPRESSION_POOL_COUNT {
        let descriptor = pool_rows
            + pool_index * SHARED_EXPRESSION_POOL_ROW_SIZE
            + SHARED_EXPRESSION_DESCRIPTOR_OFFSET;
        let layout = numeric_program_layout(data, descriptor)?;
        if let Some(instruction) = layout.instructions.into_iter().find(|instruction| {
            instruction.opcode == opcode
                && operand.is_none_or(|operand| instruction.operand == operand)
        }) {
            return Ok(instruction);
        }
    }
    Err(invalid(format!(
        "Shared expression table has no template for opcode {opcode}"
    )))
}

pub(crate) fn patch_project_acquired_count_programs(
    stock: Vec<u8>,
    rows: &[ProjectAuthoredRow],
) -> AuthoringResult<Vec<u8>> {
    let original = stock;
    let mut data = original.clone();
    let (pool_rows, _) = validate_shared_expression_table(&original, true)?;
    let mut additions = BTreeMap::<usize, Vec<(u16, u16)>>::new();
    for row in rows {
        let direct = direct_acquired_count_pool_links(
            &original,
            pool_rows,
            SHARED_EXPRESSION_POOL_COUNT,
            row.source_acquired_flag,
        )?;
        let selected = direct
            .into_iter()
            .filter(|link| row.count_selection.selected.contains(&link.pool_index))
            .collect::<Vec<_>>();
        if selected.is_empty()
            || selected.iter().any(|link| {
                row.count_selection
                    .excluded_badge
                    .contains(&link.pool_index)
            })
        {
            return Err(invalid(
                "Project donor has no retained additive acquired-count programs",
            ));
        }
        for link in selected {
            additions
                .entry(link.pool_index)
                .or_default()
                .push((row.authored_unlock_index, link.source_operand));
        }
    }
    let add_template = shared_numeric_instruction_template(
        &original,
        pool_rows,
        NUMERIC_ADD_INSTRUCTION,
        Some(u16::MAX),
    )?;
    for (&pool_index, authored_flags) in &additions {
        let unique = authored_flags
            .iter()
            .map(|(authored, _)| *authored)
            .collect::<BTreeSet<_>>();
        if unique.len() != authored_flags.len() {
            return Err(invalid(
                "Project acquired-count pool received a duplicate authored flag",
            ));
        }
        let descriptor = pool_rows
            + pool_index * SHARED_EXPRESSION_POOL_ROW_SIZE
            + SHARED_EXPRESSION_DESCRIPTOR_OFFSET;
        let layout = numeric_program_layout(&original, descriptor)?;
        if numeric_program_stack_depth(&layout.tokens)? != 1 {
            return Err(invalid("Project acquired-count source program is invalid"));
        }
        let mut authored = original[layout.header..layout.segment_end].to_vec();
        let insertion = layout.rows_end - layout.header;
        let mut terms = Vec::with_capacity(authored_flags.len() * 16);
        for (authored_flag, source_flag) in authored_flags {
            let flag_template = layout
                .instructions
                .iter()
                .copied()
                .find(|instruction| {
                    instruction.opcode == NUMERIC_FLAG_INSTRUCTION
                        && instruction.operand == *source_flag
                })
                .ok_or_else(|| {
                    invalid("Project acquired-count source FLAG template is unavailable")
                })?;
            terms.extend_from_slice(
                &flag_template
                    .with_semantics(NUMERIC_FLAG_INSTRUCTION, *authored_flag)
                    .serialized,
            );
            terms.extend_from_slice(&add_template.serialized);
        }
        authored.splice(insertion..insertion, terms);
        write_u64(
            &mut authored,
            0,
            u64::try_from(layout.count + authored_flags.len() * 2)
                .map_err(|_| invalid("Project acquired-count length does not fit u64"))?,
        )?;
        while data.len() % 16 != 0 {
            data.push(0);
        }
        let new_header = data.len();
        data.extend_from_slice(&authored);
        write_u64(
            &mut data,
            descriptor,
            u64::try_from(layout.count + authored_flags.len() * 2)
                .map_err(|_| invalid("Project acquired-count length does not fit u64"))?,
        )?;
        write_relative_pointer(&mut data, descriptor + 8, new_header)?;
        let final_layout = numeric_program_layout(&data, descriptor)?;
        if final_layout.count != layout.count + authored_flags.len() * 2
            || numeric_program_stack_depth(&final_layout.tokens)? != 1
        {
            return Err(validation(
                "Project acquired-count program did not retain a valid stack",
            ));
        }
    }
    validate_shared_expression_table(&data, false)?;
    Ok(data)
}

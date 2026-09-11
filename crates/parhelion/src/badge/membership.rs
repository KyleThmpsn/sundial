//! Change Sunrise membership without changing normal or custom Collections placement.
use super::*;

pub(crate) fn set_sunrise_members(
    graph: &mut SunriseBadgeGraph,
    members: &[SunriseBadgePlacement],
    pools: &[u8],
) -> AuthoringResult<()> {
    let keep = members
        .iter()
        .map(|member| member.authored_collectible_index)
        .collect::<BTreeSet<_>>();
    let (_, _, rows, _) = array_at(&graph.nodes, 8)?;
    for leaf in 1..=3 {
        let descriptor = rows
            + (STOCK_PRESENTATION_NODE_COUNT + leaf) * PRESENTATION_NODE_ROW_SIZE
            + PRESENTATION_NODE_COLLECTIBLES_OFFSET;
        let (count, header, children, class) = array_at(&graph.nodes, descriptor)?;
        if class != PRESENTATION_NODE_COLLECTIBLE_ROW_CLASS {
            return Err(invalid("Sunrise badge members have an unsupported layout"));
        }
        let mut data = graph.nodes[header..children].to_vec();
        let mut retained = 0;
        for child in 0..count {
            let offset = children + child * PRESENTATION_NODE_COLLECTIBLE_ROW_SIZE;
            if keep.contains(&usize::from(read_u16(&graph.nodes, offset)?)) {
                data.extend_from_slice(
                    &graph.nodes[offset..offset + PRESENTATION_NODE_COLLECTIBLE_ROW_SIZE],
                );
                retained += 1;
            }
        }
        if retained != members.len() {
            return Err(invalid(
                "Sunrise badge members do not match the authored collectibles",
            ));
        }
        write_u64(&mut data, 0, retained as u64)?;
        append_presentation_child_array_terminator(&mut data)?;
        while graph.nodes.len() % 16 != 0 {
            graph.nodes.push(0);
        }
        let header = graph.nodes.len();
        graph.nodes.extend(data);
        write_u64(&mut graph.nodes, descriptor, retained as u64)?;
        write_relative_pointer(&mut graph.nodes, descriptor + 8, header)?;
    }
    let (_, _, rows, _) = array_at(&graph.objectives, 8)?;
    let objective = rows + STOCK_OBJECTIVE_COUNT * OBJECTIVE_ROW_SIZE;
    write_i32(
        &mut graph.objectives,
        objective + OBJECTIVE_COMPLETION_VALUE_OFFSET,
        members.len() as i32,
    )?;
    let (pool_rows, _) = validate_shared_expression_table(pools, true)?;
    let flag =
        shared_numeric_instruction_template(pools, pool_rows, NUMERIC_FLAG_INSTRUCTION, None)?;
    let add = shared_numeric_instruction_template(
        pools,
        pool_rows,
        NUMERIC_ADD_INSTRUCTION,
        Some(u16::MAX),
    )?;
    let mut instructions = Vec::new();
    for (position, member) in members.iter().enumerate() {
        instructions.push(
            flag.with_semantics(NUMERIC_FLAG_INSTRUCTION, member.authored_unlock_index)
                .serialized,
        );
        if position > 0 {
            instructions.push(add.serialized);
        }
    }
    if instructions.is_empty() {
        let constant = shared_numeric_instruction_template(pools, pool_rows, 11, None)?;
        instructions.push(constant.with_semantics(11, 0).serialized);
    }
    append_numeric_program(
        &mut graph.objectives,
        objective + 0x08,
        &instructions,
        "Sunrise badge membership",
    )
}

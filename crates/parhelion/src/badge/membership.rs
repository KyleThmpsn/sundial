//! Change Sunrise membership without changing normal or custom Collections placement.
use super::*;

pub(crate) fn set_sunrise_members(
    graph: &mut SunriseBadgeGraph,
    members: &[SunriseBadgePlacement],
    pools: &[u8],
) -> AuthoringResult<()> {
    set_members(graph, members, pools, &Layout::sunrise())
}

/// Keep leaf members, their collectible parents and each backing record's counter aligned.
pub(super) fn set_members(
    graph: &mut SunriseBadgeGraph,
    members: &[SunriseBadgePlacement],
    pools: &[u8],
    layout: &Layout,
) -> AuthoringResult<()> {
    let (_, _, rows, _) = array_at(&graph.nodes, 8)?;
    for leaf in 1..=3 {
        let class_members = members
            .iter()
            .copied()
            .filter(|member| member.classes.supports((leaf - 1) as u8))
            .collect::<Vec<_>>();
        let keep = class_members
            .iter()
            .map(|member| member.authored_collectible_index)
            .collect::<BTreeSet<_>>();
        let descriptor = rows
            + (layout.node_start + leaf) * PRESENTATION_NODE_ROW_SIZE
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
        if retained != class_members.len() {
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
        let (_, _, record_rows, _) = array_at(&graph.records, 8)?;
        let record = record_rows + (layout.record_start + leaf) * RECORD_ROW_SIZE;
        let (count, _, links, class) =
            array_at(&graph.records, record + RECORD_OBJECTIVE_DESCRIPTOR_OFFSET)?;
        if count != 1 || class != RECORD_OBJECTIVE_ROW_CLASS {
            return Err(invalid(
                "Badge class record has an incompatible objective link",
            ));
        }
        let mut objective_index = usize::from(read_u16(&graph.records, links)?);
        if objective_index == layout.objective_start
            && members
                .iter()
                .any(|member| member.classes != crate::collection::Classes::ALL)
        {
            // Ordinary weapons still share the original objective. Class armor needs a
            // private leaf counter so another class's armor never blocks completion.
            let class_layout = Layout {
                objective_start: array_at(&graph.objectives, 8)?.0,
                objective_hash: crate::presentation::text_hash(
                    &format!("badge/{:08X}/class/{leaf}", layout.node_hashes[0]),
                    "objective",
                ),
                ..*layout
            };
            let unlocks = class_members
                .iter()
                .map(|member| member.authored_unlock_index)
                .collect::<Vec<_>>();
            let (objectives, strings, index) = append_objective(
                std::mem::take(&mut graph.objectives),
                std::mem::take(&mut graph.objective_strings),
                pools,
                &unlocks,
                crate::package_profile::LOCALIZATION_DONOR_TABLE_INDEX as u32,
                &class_layout,
            )?;
            graph.objectives = objectives;
            graph.objective_strings = strings;
            objective_index = usize::from(index);
            append_single_u16_array(
                &mut graph.records,
                record + RECORD_OBJECTIVE_DESCRIPTOR_OFFSET,
                RECORD_OBJECTIVE_ROW_CLASS,
                index,
            )?;
        }
        if objective_index != layout.objective_start {
            set_objective(graph, &class_members, pools, objective_index)?;
        }
    }
    set_objective(graph, members, pools, layout.objective_start)
}

fn set_objective(
    graph: &mut SunriseBadgeGraph,
    members: &[SunriseBadgePlacement],
    pools: &[u8],
    index: usize,
) -> AuthoringResult<()> {
    let (_, _, rows, _) = array_at(&graph.objectives, 8)?;
    let objective = rows + index * OBJECTIVE_ROW_SIZE;
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

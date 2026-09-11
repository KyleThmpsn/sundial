//! Add shared weapon-type leaves and their own acquired-count objectives.
use super::*;
use crate::{badge::SunriseBadgeGraph, progression::presentation::*, progression::*};
use sundial::package_authoring::investment_schema::{
    OBJECTIVE_STRING_ROW_CLASS, OBJECTIVE_STRING_ROW_SIZE,
};

pub(super) fn append(
    graph: &mut SunriseBadgeGraph,
    plan: &placements::Plan,
    rows: &[ProjectAuthoredRow],
    pools: &[u8],
) -> AuthoringResult<()> {
    for page in plan
        .pages
        .values()
        .filter(|page| page.index != page.template)
    {
        let members = rows
            .iter()
            .filter(|row| row.weapon_page == page.index)
            .collect::<Vec<_>>();
        let Some(_) = members.first() else {
            return Err(invalid("Added Collections page has no members"));
        };
        let (count, _, node_rows, _) = array_at(&graph.nodes, 8)?;
        if count != usize::from(page.index) {
            return Err(invalid("Collections page allocation changed during build"));
        }
        let source = node_rows + usize::from(page.template) * PRESENTATION_NODE_ROW_SIZE;
        let template_hash = read_u32(&graph.nodes, source + PRESENTATION_NODE_HASH_OFFSET)?;
        let template_objective = usize::from(read_u16(
            &graph.nodes,
            source + PRESENTATION_NODE_OBJECTIVE_INDEX_OFFSET,
        )?);
        let (child_count, _, child_rows, child_class) =
            array_at(&graph.nodes, source + PRESENTATION_NODE_COLLECTIBLES_OFFSET)?;
        if child_class != PRESENTATION_NODE_COLLECTIBLE_ROW_CLASS {
            return Err(invalid("Collections child template has the wrong class"));
        }
        let child = (0..child_count)
            .find(|i| {
                read_u16(
                    &graph.nodes,
                    child_rows + i * PRESENTATION_NODE_COLLECTIBLE_ROW_SIZE,
                )
                .ok()
                    == u16::try_from(page.donor).ok()
            })
            .ok_or_else(|| invalid("Collections page has no matching child template"))?;
        let template_child = crate::tag_payload::read_array::<4>(
            &graph.nodes,
            child_rows + child * PRESENTATION_NODE_COLLECTIBLE_ROW_SIZE,
        )?;
        let objective = append_objective(graph, template_objective, page.destination, pools)?;
        graph.nodes = append_fixed_rows_without_donor_dependencies(
            std::mem::take(&mut graph.nodes),
            count,
            PRESENTATION_NODE_ROW_SIZE,
            PRESENTATION_NODE_DEFINITION_ROW_CLASS,
            &PRESENTATION_NODE_POINTER_FIELDS,
            &PRESENTATION_NODE_POINTER_FIELDS,
            &[usize::from(page.template)],
            &[template_hash],
            &[page.destination.hash("node")],
            PRESENTATION_NODE_HASH_OFFSET,
            "Collections nodes",
        )?;
        let row = node_rows + count * PRESENTATION_NODE_ROW_SIZE;
        write_u16(
            &mut graph.nodes,
            row + PRESENTATION_NODE_OBJECTIVE_INDEX_OFFSET,
            objective,
        )?;
        write_u16(
            &mut graph.nodes,
            row + PRESENTATION_NODE_RECORD_INDEX_OFFSET,
            u16::MAX,
        )?;
        append_single_u16_array(
            &mut graph.nodes,
            row + 0x18,
            PRESENTATION_NODE_INDEX_ROW_CLASS,
            page.parent,
        )?;
        append_members(
            &mut graph.nodes,
            row + PRESENTATION_NODE_COLLECTIBLES_OFFSET,
            &members,
            template_child,
        )?;
        append_presentation_node_child(
            &mut graph.nodes,
            usize::from(page.parent),
            usize::from(page.sibling),
            count,
        )?;
        graph.node_strings = append_fixed_rows_without_donor_dependencies(
            std::mem::take(&mut graph.node_strings),
            count,
            PRESENTATION_NODE_STRING_ROW_SIZE,
            PRESENTATION_NODE_STRING_ROW_CLASS,
            &[],
            &[],
            &[usize::from(page.template)],
            &[template_hash],
            &[page.destination.hash("node")],
            0,
            "Collections names and icons",
        )?;
    }
    Ok(())
}

fn append_members(
    nodes: &mut Vec<u8>,
    descriptor: usize,
    members: &[&ProjectAuthoredRow],
    template: [u8; 4],
) -> AuthoringResult<()> {
    while nodes.len() % 16 != 0 {
        nodes.push(0);
    }
    let header = nodes.len();
    nodes.extend_from_slice(&(members.len() as u64).to_le_bytes());
    nodes.extend_from_slice(&PRESENTATION_NODE_COLLECTIBLE_ROW_CLASS.to_le_bytes());
    nodes.extend_from_slice(&0u32.to_le_bytes());
    for member in members {
        nodes.extend_from_slice(
            &u16::try_from(member.authored_collectible_index)
                .map_err(|_| invalid("Collections item index exceeds capacity"))?
                .to_le_bytes(),
        );
        nodes.extend_from_slice(&template[2..]);
    }
    append_presentation_child_array_terminator(nodes)?;
    write_u64(nodes, descriptor, members.len() as u64)?;
    write_relative_pointer(nodes, descriptor + 8, header)
}

fn append_objective(
    graph: &mut SunriseBadgeGraph,
    template: usize,
    destination: crate::collection::Destination,
    pools: &[u8],
) -> AuthoringResult<u16> {
    let (count, _, rows, _) = array_at(&graph.objectives, 8)?;
    let source = rows + template * OBJECTIVE_ROW_SIZE;
    let hash = read_u32(&graph.objectives, source)?;
    let secondary = read_u64(&graph.objectives, source + 0x38)? != 0;
    let fields = [0x08, 0x38, 0x48];
    graph.objectives = append_fixed_rows_without_donor_dependencies(
        std::mem::take(&mut graph.objectives),
        count,
        OBJECTIVE_ROW_SIZE,
        OBJECTIVE_ROW_CLASS,
        &fields,
        &fields,
        &[template],
        &[hash],
        &[destination.hash("objective")],
        0,
        "Collections objectives",
    )?;
    graph.objective_strings = append_fixed_rows_without_donor_dependencies(
        std::mem::take(&mut graph.objective_strings),
        count,
        OBJECTIVE_STRING_ROW_SIZE,
        OBJECTIVE_STRING_ROW_CLASS,
        &[],
        &[],
        &[template],
        &[hash],
        &[destination.hash("objective")],
        0,
        "Collections objective text",
    )?;
    let row = rows + count * OBJECTIVE_ROW_SIZE;
    write_i32(
        &mut graph.objectives,
        row + OBJECTIVE_COMPLETION_VALUE_OFFSET,
        0,
    )?;
    let (pool_rows, _) = validate_shared_expression_table(pools, true)?;
    let zero = shared_numeric_instruction_template(pools, pool_rows, 11, Some(0))?.serialized;
    append_numeric_program(
        &mut graph.objectives,
        row + 8,
        &[zero],
        "New Collections counter",
    )?;
    if secondary {
        append_numeric_program(
            &mut graph.objectives,
            row + 0x38,
            &[zero],
            "New Collections excluded-item count",
        )?;
    }
    u16::try_from(count).map_err(|_| invalid("Collections objective index exceeds capacity"))
}

/// Explicit placement adds flags to the destination objectives directly, keeping every
/// stock donor pool intact even when the destination is in another ammo branch.
pub(super) fn add_counts(
    objectives: &mut Vec<u8>,
    nodes: &[u8],
    rows: &[ProjectAuthoredRow],
    plan: &placements::Plan,
    pools: &[u8],
) -> AuthoringResult<()> {
    let (pool_rows, _) = validate_shared_expression_table(pools, true)?;
    let flag =
        shared_numeric_instruction_template(pools, pool_rows, NUMERIC_FLAG_INSTRUCTION, None)?;
    let add = shared_numeric_instruction_template(
        pools,
        pool_rows,
        NUMERIC_ADD_INSTRUCTION,
        Some(u16::MAX),
    )?;
    let (_, _, node_rows, _) = array_at(nodes, 8)?;
    let (objective_count, _, objective_rows, _) = array_at(objectives, 8)?;
    let explicit = plan
        .pages
        .values()
        .map(|page| page.index)
        .collect::<BTreeSet<_>>();
    let mut additions = BTreeMap::<u16, BTreeSet<u16>>::new();
    for row in rows
        .iter()
        .filter(|row| explicit.contains(&row.weapon_page) && row.count_selection.is_direct())
    {
        for node in presentation_ancestor_nodes(nodes, &[row.weapon_page])? {
            let objective = read_u16(
                nodes,
                node_rows
                    + node * PRESENTATION_NODE_ROW_SIZE
                    + PRESENTATION_NODE_OBJECTIVE_INDEX_OFFSET,
            )?;
            if objective != u16::MAX {
                additions
                    .entry(objective)
                    .or_default()
                    .insert(row.authored_unlock_index);
            }
        }
    }
    for (objective, flags) in additions {
        if usize::from(objective) >= objective_count {
            return Err(invalid("Collections objective is out of range"));
        }
        let row = objective_rows + usize::from(objective) * OBJECTIVE_ROW_SIZE;
        // +08 counts acquisitions. +38 counts hidden, unacquired entries which the
        // client subtracts from the completion target. Authored entries are always
        // visible, so acquiring one must never reduce the displayed denominator.
        let field = 0x08;
        if read_u64(objectives, row + field)? == 0 {
            return Err(invalid(
                "Collections objective has no acquired-count program",
            ));
        }
        let mut instructions = numeric_program_layout(objectives, row + field)?
            .instructions
            .into_iter()
            .map(|i| i.serialized)
            .collect::<Vec<_>>();
        for &index in &flags {
            instructions.push(
                flag.with_semantics(NUMERIC_FLAG_INSTRUCTION, index)
                    .serialized,
            );
            instructions.push(add.serialized);
        }
        append_numeric_program(
            objectives,
            row + field,
            &instructions,
            "Collections acquired count",
        )?;
    }
    Ok(())
}

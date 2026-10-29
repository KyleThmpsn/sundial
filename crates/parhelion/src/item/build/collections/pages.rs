//! Add shared weapon-type leaves, branded gear pages, weapon types' Weapon Ornaments pages and
//! their own acquired-count objectives.
use super::*;
use crate::{badge::SunriseBadgeGraph, progression::presentation::*, progression::*};

/// One page the build adds, cloned from a stock page.
struct NewPage {
    index: u16,
    template: u16,
    parent: u16,
    sibling: u16,
    donor: usize,
    node_hash: u32,
    objective_hash: u32,
    name_hash: Option<u32>,
    /// Named, described and drawn like the runtime's badge instead of its template page.
    branded: bool,
    children: Vec<u16>,
    /// A set's members in tile order. Other pages list theirs in build order.
    members: Vec<u32>,
    /// Whether the page joins its parent's children as it is added. An armor set is already its
    /// category's.
    attach: bool,
    /// An ornament set, which joins its class's stock Universal Ornaments once every set exists.
    ornament_set: bool,
    /// A weapon type's Weapon Ornaments page, which its weapons' linked ornaments join during
    /// emission, so the page starts without members.
    ornaments: bool,
}
use sundial::package_authoring::investment_schema::{
    OBJECTIVE_STRING_ROW_CLASS, OBJECTIVE_STRING_ROW_SIZE,
};

pub(super) fn append(
    graph: &mut SunriseBadgeGraph,
    plan: &placements::Plan,
    rows: &[ProjectAuthoredRow],
    weapons: &[WeaponCloneSpec],
    pools: &[u8],
    badge_icon_index: u16,
) -> AuthoringResult<()> {
    if rows.len() != weapons.len() {
        return Err(invalid("Collections items and rows have different counts"));
    }
    let rows_by_hash = weapons
        .iter()
        .zip(rows)
        .map(|(weapon, row)| (weapon.identity.item_hash, row))
        .collect::<BTreeMap<_, _>>();
    // Indices were allocated weapon pages first, then gear pages, then the Weapon Ornaments pages
    // of weapon types without a stock one, each in key order.
    let weapon_pages = plan
        .pages
        .values()
        .filter(|page| page.index != page.template)
        .map(|page| NewPage {
            index: page.index,
            template: page.template,
            parent: page.parent,
            sibling: page.sibling,
            donor: page.donor,
            node_hash: page.destination.hash("node"),
            objective_hash: page.destination.hash("objective"),
            name_hash: page.destination.name_hash(),
            branded: false,
            children: Vec::new(),
            members: Vec::new(),
            attach: true,
            ornament_set: false,
            ornaments: false,
        });
    let gear_pages = plan.gear_pages.values().map(|page| NewPage {
        index: page.index,
        template: page.template,
        parent: page.parent,
        sibling: page.sibling,
        donor: page.donor,
        node_hash: page.page.hash("node"),
        objective_hash: page.page.hash("objective"),
        name_hash: page.page.set().map(|_| page.page.hash("name")),
        branded: true,
        children: page.children.clone(),
        members: page.members.clone(),
        attach: page.page.set().is_none(),
        ornament_set: page.page.is_ornament_set(),
        ornaments: false,
    });
    let ornament_pages = plan
        .ornament_pages
        .values()
        .filter(|page| page.added)
        .map(|page| NewPage {
            index: page.index,
            template: page.template,
            parent: page.parent,
            sibling: page.sibling,
            donor: page.donor,
            node_hash: page.family.ornament_page_hash("node"),
            objective_hash: page.family.ornament_page_hash("objective"),
            name_hash: Some(page.family.ornament_page_hash("name")),
            branded: false,
            children: Vec::new(),
            members: Vec::new(),
            attach: true,
            ornament_set: false,
            ornaments: true,
        });
    let mut ornament_sets = Vec::new();
    for page in weapon_pages.chain(gear_pages).chain(ornament_pages) {
        let members = page_members(&page, rows, &rows_by_hash)?;
        if members.is_empty() && page.children.is_empty() && !page.ornaments {
            return Err(invalid("Added Collections page has no members or sets"));
        }
        if !page.members.is_empty() && members.len() > crate::collection::ARMOR_SET_SIZE {
            return Err(invalid("Collections set exceeds five items"));
        }
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
        let template_child = if page.children.is_empty() {
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
            Some(crate::tag_payload::read_array::<4>(
                &graph.nodes,
                child_rows + child * PRESENTATION_NODE_COLLECTIBLE_ROW_SIZE,
            )?)
        } else {
            None
        };
        let set_template = if !page.children.is_empty() {
            let (count, _, child_rows, class) =
                array_at(&graph.nodes, source + PRESENTATION_NODE_CHILD_NODES_OFFSET)?;
            if count == 0 || class != PRESENTATION_NODE_CHILD_NODE_ROW_CLASS {
                return Err(invalid("Armor category has no set-row template"));
            }
            Some(crate::tag_payload::read_array::<24>(
                &graph.nodes,
                child_rows,
            )?)
        } else {
            None
        };
        let objective = append_objective(graph, template_objective, page.objective_hash, pools)?;
        graph.nodes = append_fixed_rows_without_donor_dependencies(
            std::mem::take(&mut graph.nodes),
            count,
            PRESENTATION_NODE_ROW_SIZE,
            PRESENTATION_NODE_DEFINITION_ROW_CLASS,
            &PRESENTATION_NODE_POINTER_FIELDS,
            &PRESENTATION_NODE_POINTER_FIELDS,
            &[usize::from(page.template)],
            &[template_hash],
            &[page.node_hash],
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
        if let Some(template) = template_child {
            append_members(
                &mut graph.nodes,
                row + PRESENTATION_NODE_COLLECTIBLES_OFFSET,
                &members,
                template,
            )?;
        }
        if let Some(template) = set_template {
            append_sets(
                &mut graph.nodes,
                row + PRESENTATION_NODE_CHILD_NODES_OFFSET,
                &page.children,
                template,
            )?;
        }
        if page.ornament_set {
            ornament_sets.push((page.parent, page.sibling, count));
        } else if page.attach {
            append_presentation_node_child(
                &mut graph.nodes,
                usize::from(page.parent),
                usize::from(page.sibling),
                count,
                crate::collection::first(None),
            )?;
        } else {
            let descriptor = node_rows
                + usize::from(page.parent) * PRESENTATION_NODE_ROW_SIZE
                + PRESENTATION_NODE_CHILD_NODES_OFFSET;
            let (count, _, children, _) = array_at(&graph.nodes, descriptor)?;
            if (0..count)
                .filter(|i| {
                    read_u16(
                        &graph.nodes,
                        children + i * PRESENTATION_NODE_CHILD_NODE_ROW_SIZE,
                    )
                    .ok()
                        == Some(page.index)
                })
                .count()
                != 1
            {
                return Err(invalid("Armor category is missing its authored set"));
            }
        }
        graph.node_strings = append_fixed_rows_without_donor_dependencies(
            std::mem::take(&mut graph.node_strings),
            count,
            PRESENTATION_NODE_STRING_ROW_SIZE,
            PRESENTATION_NODE_STRING_ROW_CLASS,
            &[],
            &[],
            &[usize::from(page.template)],
            &[template_hash],
            &[page.node_hash],
            0,
            "Collections names and icons",
        )?;
        if page.branded {
            brand_page(graph, usize::from(page.index), badge_icon_index)?;
        }
        if let Some(hash) = page.name_hash {
            let (_, _, rows, _) = array_at(&graph.node_strings, 8)?;
            write_localized_reference(
                &mut graph.node_strings,
                rows + usize::from(page.index) * PRESENTATION_NODE_STRING_ROW_SIZE
                    + PRESENTATION_NODE_STRING_NAME_REFERENCE_OFFSET,
                LOCALIZATION_DONOR_TABLE_INDEX as u32,
                hash,
            )?;
        }
    }
    attach_ornament_sets(&mut graph.nodes, ornament_sets)
}

/// Adds each ornament set to its class's stock Universal Ornaments in set order. A set joins
/// before the stock sets or after them, so leading sets go in last to first.
fn attach_ornament_sets(
    nodes: &mut Vec<u8>,
    mut sets: Vec<(u16, u16, usize)>,
) -> AuthoringResult<()> {
    let first = crate::collection::first(None);
    if first {
        sets.reverse();
    }
    for (parent, sibling, index) in sets {
        append_presentation_node_child(
            nodes,
            usize::from(parent),
            usize::from(sibling),
            index,
            first,
        )?;
    }
    Ok(())
}

fn page_members<'a>(
    page: &NewPage,
    rows: &'a [ProjectAuthoredRow],
    rows_by_hash: &BTreeMap<u32, &'a ProjectAuthoredRow>,
) -> AuthoringResult<Vec<&'a ProjectAuthoredRow>> {
    let members = rows
        .iter()
        .filter(|row| row.pages.contains(page.index))
        .collect::<Vec<_>>();
    if page.members.is_empty() {
        return Ok(members);
    }
    if page.members.len() != members.len()
        || page.members.iter().collect::<BTreeSet<_>>().len() != members.len()
    {
        return Err(invalid("Collections set membership changed during build"));
    }
    page.members
        .iter()
        .map(|hash| {
            rows_by_hash
                .get(hash)
                .copied()
                .filter(|row| row.pages.contains(page.index))
                .ok_or_else(|| invalid("Collections set member is missing from its planned page"))
        })
        .collect()
}

/// Names, describes and draws a gear page with the strings and artwork of the runtime's badge.
fn brand_page(
    graph: &mut SunriseBadgeGraph,
    index: usize,
    badge_icon_index: u16,
) -> AuthoringResult<()> {
    let (_, _, rows, _) = array_at(&graph.node_strings, 8)?;
    let row = rows + index * PRESENTATION_NODE_STRING_ROW_SIZE;
    write_u16(
        &mut graph.node_strings,
        row + PRESENTATION_NODE_STRING_ICON_OFFSET,
        badge_icon_index,
    )?;
    for (offset, hash) in [
        (
            PRESENTATION_NODE_STRING_NAME_REFERENCE_OFFSET,
            crate::badge::SUNRISE_BADGE_NAME_HASH,
        ),
        (
            PRESENTATION_NODE_STRING_DESCRIPTION_REFERENCE_OFFSET,
            crate::badge::SUNRISE_BADGE_DESCRIPTION_HASH,
        ),
    ] {
        write_localized_reference(
            &mut graph.node_strings,
            row + offset,
            LOCALIZATION_DONOR_TABLE_INDEX as u32,
            hash,
        )?;
    }
    Ok(())
}

/// Replace stock category children with ordered authored armor-set rows.
fn append_sets(
    nodes: &mut Vec<u8>,
    descriptor: usize,
    sets: &[u16],
    template: [u8; 24],
) -> AuthoringResult<()> {
    let header = (nodes.len() + 19) & !15;
    nodes.resize(header - 4, 0);
    nodes.extend_from_slice(&PRESENTATION_CHILD_ARRAY_SENTINEL.to_le_bytes());
    nodes.extend_from_slice(&(sets.len() as u64).to_le_bytes());
    nodes.extend_from_slice(&PRESENTATION_NODE_CHILD_NODE_ROW_CLASS.to_le_bytes());
    nodes.extend_from_slice(&0u32.to_le_bytes());
    for &set in sets {
        let mut child = template;
        write_u16(&mut child, 0, set)?;
        child[8..].fill(0);
        nodes.extend_from_slice(&child);
    }
    append_presentation_child_array_terminator(nodes)?;
    write_u64(nodes, descriptor, sets.len() as u64)?;
    write_relative_pointer(nodes, descriptor + 8, header)
}

fn append_members(
    nodes: &mut Vec<u8>,
    descriptor: usize,
    members: &[&ProjectAuthoredRow],
    template: [u8; 4],
) -> AuthoringResult<()> {
    let header = (nodes.len() + 19) & !15;
    nodes.resize(header - 4, 0);
    nodes.extend_from_slice(&PRESENTATION_CHILD_ARRAY_SENTINEL.to_le_bytes());
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
    objective_hash: u32,
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
        &[objective_hash],
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
        &[objective_hash],
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

use super::*;
use crate::weapon::WeaponCloneSpec;
use std::collections::BTreeMap;

pub(crate) fn append_custom_badges(
    mut graph: SunriseBadgeGraph,
    weapons: &[WeaponCloneSpec],
    placements: &[SunriseBadgePlacement],
    icons: &BTreeMap<String, u16>,
    pools: &[u8],
    collectibles: &mut Vec<u8>,
    localization_table_index: u32,
) -> AuthoringResult<SunriseBadgeGraph> {
    if weapons.len() != placements.len() {
        return Err(invalid(
            "Badge membership does not match the authored weapons",
        ));
    }
    let mut groups = BTreeMap::<&str, Vec<SunriseBadgePlacement>>::new();
    for (weapon, placement) in weapons.iter().zip(placements) {
        if let Some(badge) = &weapon.overrides.badge {
            groups.entry(&badge.name).or_default().push(*placement);
        }
    }
    if groups.len() > crate::presentation::MAX_CUSTOM_BADGES {
        return Err(invalid(
            "A build can contain at most 24 custom badges within Sunrise’s presentation node capacity",
        ));
    }
    for (name, members) in groups {
        let node_start = array_at(&graph.nodes, 8)?.0;
        let record_start = array_at(&graph.records, 8)?.0;
        let objective_start = array_at(&graph.objectives, 8)?.0;
        if node_start + 4 > crate::collection::NODE_CAPACITY
            || record_start + 4 > 4096
            || objective_start + 1 >= u16::MAX as usize
        {
            return Err(invalid(
                "Custom badges exceed the native table index capacity",
            ));
        }
        let hash = |field: &str| crate::presentation::text_hash(name, field);
        let layout = Layout {
            node_start,
            record_start,
            objective_start,
            node_hashes: std::array::from_fn(|i| hash(&format!("badge-node-{i}"))),
            record_hashes: std::array::from_fn(|i| hash(&format!("badge-record-{i}"))),
            objective_hash: hash("badge-objective"),
            name_hash: hash("badge-name"),
            description_hash: hash("badge-description"),
        };
        graph = author_graph(
            SunriseBadgeGraphInput {
                stock_nodes: graph.nodes,
                stock_node_strings: graph.node_strings,
                stock_objectives: graph.objectives,
                stock_objective_strings: graph.objective_strings,
                stock_records: graph.records,
                stock_record_strings: graph.record_strings,
                shared_expression_pools: pools,
                localization_table_index,
                badge_icon_index: *icons
                    .get(name)
                    .ok_or_else(|| invalid("Custom badge icon was not allocated"))?,
                placements: &members,
            },
            &layout,
        )?;
        for member in members {
            append_parents(collectibles, member.authored_collectible_index, node_start)?;
        }
    }
    Ok(graph)
}

fn append_parents(data: &mut Vec<u8>, index: usize, node_start: usize) -> AuthoringResult<()> {
    use crate::progression::{COLLECTIBLE_PRESENTATION_NODE_PARENTS_OFFSET, COLLECTIBLE_ROW_SIZE};
    let (count, _, rows, _) = array_at(data, 8)?;
    if index >= count {
        return Err(invalid("Custom badge collectible is out of range"));
    }
    let descriptor =
        rows + index * COLLECTIBLE_ROW_SIZE + COLLECTIBLE_PRESENTATION_NODE_PARENTS_OFFSET;
    let (count, _, rows, class) = array_at(data, descriptor)?;
    if class != PRESENTATION_NODE_INDEX_ROW_CLASS {
        return Err(invalid(
            "Custom badge collectible parents have an unsupported layout",
        ));
    }
    let mut parents = (0..count)
        .map(|i| read_u16(data, rows + i * 2))
        .collect::<AuthoringResult<Vec<_>>>()?;
    parents.extend((1..=3).map(|i| (node_start + i) as u16));
    if parents.iter().copied().collect::<BTreeSet<_>>().len() != parents.len() {
        return Err(invalid(
            "Custom badge membership duplicates a collectible parent",
        ));
    }
    while data.len() % 16 != 0 {
        data.push(0);
    }
    let header = data.len();
    data.extend_from_slice(&(parents.len() as u64).to_le_bytes());
    data.extend_from_slice(&class.to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());
    for parent in &parents {
        data.extend_from_slice(&parent.to_le_bytes());
    }
    while (data.len() + NESTED_ARRAY_TRAILER.len()) % 16 != 0 {
        data.push(0);
    }
    data.extend_from_slice(&NESTED_ARRAY_TRAILER);
    write_u64(data, descriptor, parents.len() as u64)?;
    write_relative_pointer(data, descriptor + 8, header)
}

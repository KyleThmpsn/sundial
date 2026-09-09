//! Build badge membership and acquired-count programs from completed weapon rows.
use super::*;

pub(super) struct Tables {
    pub nodes: Vec<u8>,
    pub node_strings: Vec<u8>,
    pub objective_strings: Vec<u8>,
    pub records: Vec<u8>,
    pub record_strings: Vec<u8>,
    pub objectives: Vec<u8>,
    pub pools: Vec<u8>,
}

pub(super) fn author(
    sources: &mut sources::ProjectSources,
    project_rows: &[ProjectAuthoredRow],
    badge_icon_index: u16,
    weapon_count: usize,
) -> AuthoringResult<Tables> {
    let badge_placements = project_rows
        .iter()
        .map(|row| SunriseBadgePlacement {
            weapon_page: row.weapon_page,
            donor_collectible_index: row.donor_collectible_index,
            authored_collectible_index: row.authored_collectible_index,
            authored_unlock_index: row.authored_unlock_index,
        })
        .collect::<Vec<_>>();
    let badge = author_sunrise_badge_graph(SunriseBadgeGraphInput {
        stock_nodes: std::mem::take(&mut sources.stock_nodes),
        stock_node_strings: std::mem::take(&mut sources.stock_node_strings),
        stock_objectives: std::mem::take(&mut sources.stock_objectives),
        stock_objective_strings: std::mem::take(&mut sources.stock_objective_strings),
        stock_records: std::mem::take(&mut sources.stock_records),
        stock_record_strings: std::mem::take(&mut sources.stock_record_strings),
        shared_expression_pools: &sources.stock_pools,
        localization_table_index: LOCALIZATION_DONOR_TABLE_INDEX as u32,
        badge_icon_index,
        placements: &badge_placements,
    })?;
    let nodes = badge.nodes;
    let node_strings = badge.node_strings;
    let objective_strings = badge.objective_strings;
    let records = badge.records;
    let record_strings = badge.record_strings;
    let mut page_counts = BTreeMap::new();
    for row in project_rows {
        *page_counts.entry(row.weapon_page).or_insert(0usize) += 1;
    }
    let objectives =
        patch_project_collection_objectives(badge.objectives, &nodes, &page_counts, weapon_count)?;
    let authored_unlock_indices = project_rows
        .iter()
        .map(|row| row.authored_unlock_index)
        .collect::<Vec<_>>();
    let objectives = patch_badges_root_objective(
        objectives,
        &nodes,
        &sources.stock_pools,
        &authored_unlock_indices,
    )?;
    let pools = patch_project_acquired_count_programs(
        std::mem::take(&mut sources.stock_pools),
        project_rows,
    )?;

    Ok(Tables {
        nodes,
        node_strings,
        objective_strings,
        records,
        record_strings,
        objectives,
        pools,
    })
}

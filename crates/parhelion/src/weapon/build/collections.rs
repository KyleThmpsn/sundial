//! Build badge membership and acquired-count programs from completed weapon rows.
use super::*;
mod pages;

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
    plan: &placements::Plan,
    project_rows: &[ProjectAuthoredRow],
    badge_icon_index: u16,
    weapons: &[WeaponCloneSpec],
    custom_icons: &BTreeMap<String, u16>,
    collectibles: &mut Vec<u8>,
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
    let sunrise_members = weapons
        .iter()
        .zip(&badge_placements)
        .filter(|(weapon, _)| !weapon.overrides.exclude_from_sunrise_badge)
        .map(|(_, placement)| *placement)
        .collect::<Vec<_>>();
    let mut badge = crate::badge::append_custom_badges(
        badge,
        weapons,
        &badge_placements,
        custom_icons,
        &sources.stock_pools,
        collectibles,
        LOCALIZATION_DONOR_TABLE_INDEX as u32,
    )?;
    pages::append(&mut badge, plan, project_rows, &sources.stock_pools)?;
    if sunrise_members.len() != badge_placements.len() {
        crate::badge::set_sunrise_members(&mut badge, &sunrise_members, &sources.stock_pools)?;
    }
    let nodes = badge.nodes;
    let node_strings = badge.node_strings;
    let objective_strings = badge.objective_strings;
    let records = badge.records;
    let record_strings = badge.record_strings;
    let mut page_counts = BTreeMap::new();
    for row in project_rows {
        *page_counts.entry(row.weapon_page).or_insert(0usize) += 1;
    }
    let mut objectives =
        patch_project_collection_objectives(badge.objectives, &nodes, &page_counts, weapons.len())?;
    pages::add_counts(
        &mut objectives,
        &nodes,
        project_rows,
        plan,
        &sources.stock_pools,
    )?;
    let authored_unlock_indices = sunrise_members
        .iter()
        .map(|row| row.authored_unlock_index)
        .collect::<Vec<_>>();
    let mut groups = vec![authored_unlock_indices];
    let mut custom_groups = BTreeMap::<&str, Vec<u16>>::new();
    for (weapon, row) in weapons.iter().zip(project_rows) {
        if let Some(badge) = &weapon.overrides.badge {
            custom_groups
                .entry(&badge.name)
                .or_default()
                .push(row.authored_unlock_index);
        }
    }
    groups.extend(custom_groups.into_values());
    let objectives =
        crate::badge::patch_badge_objectives(objectives, &nodes, &sources.stock_pools, &groups)?;
    let inherited_rows = project_rows
        .iter()
        .filter(|row| !row.count_selection.is_direct())
        .cloned()
        .collect::<Vec<_>>();
    let pools = patch_project_acquired_count_programs(
        std::mem::take(&mut sources.stock_pools),
        &inherited_rows,
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

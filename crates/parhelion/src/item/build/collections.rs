//! Build badge membership and acquired-count programs from completed weapon rows, and the pages
//! emission puts linked ornaments on.
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
    /// Where emission puts each ordinary weapon's linked ornaments, by the weapon's item hash.
    pub ornament_targets: BTreeMap<u32, placements::OrnamentTarget>,
}

pub(super) fn author(
    sources: &mut sources::ProjectSources,
    plan: &placements::Plan,
    project_rows: &[ProjectAuthoredRow],
    weapons: &[WeaponCloneSpec],
    icons: &assets::IconRows,
    collectibles: &mut Vec<u8>,
) -> AuthoringResult<Tables> {
    // A project of items without an entry, such as subclasses alone, leaves Collections as
    // they are. A main mod that only hosts its importers' rows still keeps their places in the
    // Badges root, lists the stock badges their badges write over and counts their badges in the
    // root's total, which Sunrise keeps from one mod and the main mod's build writes.
    if project_rows.is_empty() {
        let mut nodes = std::mem::take(&mut sources.stock_nodes);
        let mut objectives = std::mem::take(&mut sources.stock_objectives);
        if crate::package_profile::member().is_none()
            && let Some(family) = crate::mod_family::build::badges().filter(|family| *family > 0)
        {
            crate::badge::arrange_root(&mut nodes)?;
            objectives = crate::badge::patch_badge_objectives(
                objectives,
                &nodes,
                &sources.stock_pools,
                &vec![Vec::new(); family],
                0,
            )?;
        }
        // It also counts the importers' items in the stock pages they join.
        if crate::package_profile::member().is_none() {
            count_constant_entries(
                (&mut objectives, &nodes, &sources.stock_pools),
                &[],
                sources.table_tags.objective_table_tag,
                true,
            )?;
        }
        return Ok(Tables {
            nodes,
            node_strings: std::mem::take(&mut sources.stock_node_strings),
            objective_strings: std::mem::take(&mut sources.stock_objective_strings),
            records: std::mem::take(&mut sources.stock_records),
            record_strings: std::mem::take(&mut sources.stock_record_strings),
            objectives,
            pools: std::mem::take(&mut sources.stock_pools),
            ornament_targets: BTreeMap::new(),
        });
    }
    let badge_placements = project_rows
        .iter()
        .zip(weapons)
        .map(|(row, weapon)| SunriseBadgePlacement {
            pages: row.pages,
            classes: row.classes,
            donor_collectible_index: row.donor_collectible_index,
            authored_collectible_index: row.authored_collectible_index,
            acquisition: row.acquisition,
            position: weapon.overrides.collection_position,
        })
        .collect::<Vec<_>>();
    // An importer's entries placed first take stock rows of the build's table
    // (`collection::front`), so the added pages read their templates' rows from the stock one.
    let stock_nodes = sources.stock_nodes.clone();
    let badge = author_sunrise_badge_graph(SunriseBadgeGraphInput {
        stock_nodes: std::mem::take(&mut sources.stock_nodes),
        stock_node_strings: std::mem::take(&mut sources.stock_node_strings),
        stock_objectives: std::mem::take(&mut sources.stock_objectives),
        stock_objective_strings: std::mem::take(&mut sources.stock_objective_strings),
        stock_records: std::mem::take(&mut sources.stock_records),
        stock_record_strings: std::mem::take(&mut sources.stock_record_strings),
        shared_expression_pools: &sources.stock_pools,
        localization_table_index: crate::package_profile::localization_table() as u32,
        badge_icon_index: icons.badge_index,
        placements: &badge_placements,
    })?;
    let sunrise_members = weapons
        .iter()
        .zip(&badge_placements)
        .filter(|(weapon, _)| crate::badge::joins_own_badge(weapon))
        .map(|(_, placement)| *placement)
        .collect::<Vec<_>>();
    let badge = crate::badge::append_custom_badges(
        badge,
        weapons,
        &badge_placements,
        &icons.custom_badges,
        &sources.stock_pools,
        collectibles,
        crate::package_profile::localization_table() as u32,
    )?;
    let mut badge = crate::badge::append_source_badges(
        badge,
        weapons,
        &badge_placements,
        &icons.source_badges,
        &sources.stock_pools,
        collectibles,
        crate::package_profile::localization_table() as u32,
    )?;
    pages::append(
        &mut badge,
        &stock_nodes,
        plan,
        project_rows,
        weapons,
        &sources.stock_pools,
        icons.badge_index,
    )?;
    if sunrise_members.len() != badge_placements.len() {
        crate::badge::set_sunrise_members(&mut badge, &sunrise_members, &sources.stock_pools)?;
    }
    // The build's badges take their places in the Badges root together.
    crate::badge::arrange_root(&mut badge.nodes)?;
    let nodes = badge.nodes;
    let node_strings = badge.node_strings;
    let objective_strings = badge.objective_strings;
    let records = badge.records;
    let record_strings = badge.record_strings;
    let mut page_members = BTreeMap::<u16, BTreeSet<u16>>::new();
    let (_, _, node_rows, _) = array_at(&nodes, 8)?;
    // An entry acquired unconditionally has no flag, so the pages' counts leave it out.
    for (row, flag) in project_rows
        .iter()
        .filter_map(|row| Some((row, row.acquisition.flag()?)))
    {
        for page in row.pages.iter() {
            if read_u16(
                &nodes,
                node_rows
                    + usize::from(page) * crate::progression::PRESENTATION_NODE_ROW_SIZE
                    + crate::progression::PRESENTATION_NODE_OBJECTIVE_INDEX_OFFSET,
            )? == u16::MAX
            {
                continue;
            }
            page_members.entry(page).or_default().insert(flag);
        }
    }
    // A collectible in more than one class branch or in Exotics counts once at shared ancestors.
    let mut objectives = crate::progression::patch_project_collection_objectives_with_members(
        badge.objectives,
        &nodes,
        &page_members,
    )?;
    crate::progression::add_direct_acquired_counts(
        &mut objectives,
        &nodes,
        project_rows,
        &sources.stock_pools,
    )?;
    count_constant_entries(
        (&mut objectives, &nodes, &sources.stock_pools),
        project_rows,
        sources.table_tags.objective_table_tag,
        true,
    )?;
    // Every build acquires its entries unconditionally, so each badge it adds reads complete and
    // counts one in the Badges root: Project Sunrise, each custom badge and, in a build without
    // mod folders, each importer's badge. Sunrise keeps one mod's value of the root's total, so on
    // its mod loader the main mod's build counts every member's badges there and an importer's
    // build leaves the root as it is.
    let objectives = if crate::package_profile::member().is_some() {
        objectives
    } else {
        let own = crate::badge::count(weapons);
        let family = crate::mod_family::build::badges().unwrap_or(own);
        if family < own {
            return Err(invalid(
                "The family's badge total leaves out badges this build adds",
            ));
        }
        crate::badge::patch_badge_objectives(
            objectives,
            &nodes,
            &sources.stock_pools,
            &vec![Vec::new(); family],
            own,
        )?
    };
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
        ornament_targets: plan.ornament_targets(),
    })
}

/// Counts the entries of `rows` acquired unconditionally, as every built item on Sunrise's mod
/// loader is, in the stock Collections objectives of their pages and those pages' ancestors.
/// `tables` are the objectives, presentation nodes and expression pools the build writes.
///
/// Every family member with items on one page changes that objective's words, and Sunrise keeps
/// one mod's value of each, so the main mod writes every member's counts whenever it builds: an
/// importer records its counts for it, and the main mod adds what the importers recorded when
/// `hosted`. Without a main mod build an importer writes its own counts, and a review that finds
/// two importers writing them has the main mod build to hold them
/// (`mod_family::build::Shared::Collections`).
pub(in crate::item) fn count_constant_entries(
    (objectives, nodes, pools): (&mut Vec<u8>, &[u8], &[u8]),
    rows: &[ProjectAuthoredRow],
    objective_table: TagHash,
    hosted: bool,
) -> AuthoringResult<()> {
    use crate::mod_family::build::{Shared, contribute, contributions, hosts, produced};
    let mut counts = crate::progression::constant_acquired_counts(
        nodes,
        rows,
        crate::badge::STOCK_OBJECTIVE_COUNT,
    )?;
    if crate::package_profile::member().is_some() {
        if !counts.is_empty() {
            contribute(|contributions| {
                for (&objective, &count) in &counts {
                    *contributions.collections.entry(objective).or_default() += count;
                }
            });
        }
        if hosts(Shared::Collections) {
            return Ok(());
        }
    } else if hosted {
        for contributions in contributions() {
            for (objective, count) in contributions.collections {
                *counts.entry(objective).or_default() += count;
            }
        }
    }
    if counts.is_empty() {
        return Ok(());
    }
    produced(Shared::Collections, [objective_table.0]);
    crate::progression::add_constant_acquired_counts(objectives, pools, &counts)
}

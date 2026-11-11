//! What a member's build adds to the stock Collections lists, counted from its recipes and the
//! stock tables without compiling them.
//!
//! The main mod builds first, yet it makes room for the importers' entries at the start of each
//! stock list and writes the stock objectives' counts of their items (`mod_family::build`). Each
//! entry's page comes from its recipe and the stock tables alone, so the family counts them here
//! before the first pass. The count resolves the recipes' Collections places as the build does
//! and follows the build's rules for which entries go first, so a pass whose importers record
//! what was counted needs no other.
use super::*;
use crate::progression::{
    PRESENTATION_NODE_OBJECTIVE_INDEX_OFFSET, PRESENTATION_NODE_ROW_SIZE,
    STOCK_PRESENTATION_NODE_COUNT, presentation_ancestor_nodes,
};

/// Counts what compiling `project` against the stock tables in `package_directory` adds to the
/// stock Collections lists.
pub(crate) fn census(
    package_directory: &Path,
    project: &WeaponProjectSpec,
) -> AuthoringResult<crate::mod_family::build::Census> {
    let mut census = crate::mod_family::build::Census::default();
    if project.weapons.is_empty() {
        return Ok(census);
    }
    let weapons = canonical_project_weapons(project)?;
    let sources = sources::load_project_sources(package_directory)?;
    // The build plans its pages with the subclasses grouped by class, and the first base of each
    // kind names the category its page joins.
    let grouped = resolve::subclass_order(&sources, &weapons)?
        .into_iter()
        .map(|place| weapons[place].clone())
        .collect::<Vec<_>>();
    let plan = placements::Plan::new(&sources, &grouped)?;
    let mut report = |_: Event<'_>| {};
    let mut progress = Progress::new(grouped.len(), &mut report);
    let resolved =
        resolve::resolve_project_weapons_with_progress(&sources, &grouped, &plan, &mut progress)?;
    let parents = added_parents(&plan);
    let mut add = |pages: &[u16], first: bool| -> AuthoringResult<()> {
        if first {
            for &page in pages {
                if usize::from(page) < STOCK_PRESENTATION_NODE_COUNT {
                    *census.fronts.entry(usize::from(page)).or_default() += 1;
                }
            }
        }
        count(&mut census.counts, &sources.stock_nodes, &parents, pages)
    };
    for weapon in &resolved {
        let Some(collection) = &weapon.collection else {
            continue;
        };
        let pages = collection.pages.iter().collect::<Vec<_>>();
        add(
            &pages,
            crate::collection::leads_stock_collectibles(
                weapon.weapon.overrides.collection_position,
            ),
        )?;
    }
    #[cfg(feature = "d2-model-importer")]
    {
        for page in linked_ornament_pages(&sources, &plan, &grouped)? {
            add(&[page], crate::collection::leads_stock_collectibles(None))?;
        }
    }
    Ok(census)
}

/// The parent of each page the build adds, as the plan places it.
fn added_parents(plan: &placements::Plan) -> BTreeMap<u16, u16> {
    plan.pages
        .values()
        .map(|page| (page.index, page.parent))
        .chain(
            plan.gear_pages
                .values()
                .map(|page| (page.index, page.parent)),
        )
        .chain(
            plan.ornament_pages
                .values()
                .map(|page| (page.index, page.parent)),
        )
        .filter(|(index, _)| usize::from(*index) >= STOCK_PRESENTATION_NODE_COUNT)
        .collect()
}

/// Counts one entry on `pages` in the stock objectives of those pages and of their ancestors, an
/// objective they share once, as `progression::constant_acquired_counts` counts the build's
/// entries. A page the build adds has an objective of its own, so it counts at its stock
/// ancestors.
fn count(
    counts: &mut BTreeMap<u16, usize>,
    nodes: &[u8],
    parents: &BTreeMap<u16, u16>,
    pages: &[u16],
) -> AuthoringResult<()> {
    let starts = pages
        .iter()
        .map(|&page| stock_ancestor(parents, page))
        .collect::<AuthoringResult<Vec<_>>>()?;
    let (_, _, node_rows, _) = array_at(nodes, 8)?;
    for node in presentation_ancestor_nodes(nodes, &starts)? {
        let objective = read_u16(
            nodes,
            node_rows
                + node * PRESENTATION_NODE_ROW_SIZE
                + PRESENTATION_NODE_OBJECTIVE_INDEX_OFFSET,
        )?;
        if objective != u16::MAX && usize::from(objective) < crate::badge::STOCK_OBJECTIVE_COUNT {
            *counts.entry(objective).or_default() += 1;
        }
    }
    Ok(())
}

/// `page` when it is a stock page, or the nearest stock ancestor of a page the build adds.
fn stock_ancestor(parents: &BTreeMap<u16, u16>, mut page: u16) -> AuthoringResult<u16> {
    for _ in 0..=parents.len() {
        if usize::from(page) < STOCK_PRESENTATION_NODE_COUNT {
            return Ok(page);
        }
        page = *parents
            .get(&page)
            .ok_or_else(|| invalid("A Collections page the build adds has no planned parent"))?;
    }
    Err(invalid(
        "The Collections pages the build adds name each other as parents",
    ))
}

/// The page each of the imported weapons' own ornaments joins, as emission puts them there
/// (`emission::imported::ornament_collections`): its weapon type's Weapon Ornaments page, or for
/// an Exotic the page of the native ornament it copies. Only a link that names its weapon adds one.
#[cfg(feature = "d2-model-importer")]
fn linked_ornament_pages(
    sources: &sources::ProjectSources,
    plan: &placements::Plan,
    weapons: &[WeaponCloneSpec],
) -> AuthoringResult<Vec<u16>> {
    let word = |value: Option<&serde_json::Value>| {
        value
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
    };
    let targets = plan.ornament_targets();
    let mut pages = Vec::new();
    for reference in weapons
        .iter()
        .filter_map(|weapon| weapon.overrides.imported_graph.as_ref())
    {
        for attachment in &reference.attachments {
            let bytes = std::fs::read(attachment.directory.join("asset-graph.json"))
                .map_err(|error| invalid(format!("Linked ornament graph: {error}")))?;
            let graph: serde_json::Value = serde_json::from_slice(&bytes)
                .map_err(|error| invalid(format!("Linked ornament graph: {error}")))?;
            let Some(link) = graph.get("ornament") else {
                continue;
            };
            let Some(owner) = word(link.get("target_weapon")) else {
                continue;
            };
            if let Some(target) = targets.get(&owner) {
                pages.push(target.page);
                continue;
            }
            let plug = word(link.get("native_plug"))
                .ok_or_else(|| invalid("Linked ornament has no native ornament"))?;
            let item = u16::try_from(placements::item_index(sources, plug, "native ornament")?)
                .map_err(|_| invalid("Native ornament index exceeds 16 bits"))?;
            let donors = (0..sources.stock_collectible_count)
                .filter(|&collectible| {
                    read_u16(
                        &sources.stock_collectibles,
                        sources.collectible_rows
                            + collectible * COLLECTIBLE_ROW_SIZE
                            + COLLECTIBLE_ITEM_INDEX_OFFSET,
                    )
                    .ok()
                        == Some(item)
                })
                .collect::<Vec<_>>();
            let [donor] = donors.as_slice() else {
                return Err(invalid("Native ornament must have one collectible"));
            };
            let parents = template_presentation_parents(
                &sources.stock_nodes,
                &sources.stock_collectibles,
                *donor,
            )?;
            let [leaf] = parents.as_slice() else {
                return Err(invalid("Native ornament must have one weapon-type leaf"));
            };
            pages.push(*leaf);
        }
    }
    Ok(pages)
}

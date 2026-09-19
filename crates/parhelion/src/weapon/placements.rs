//! Resolve semantic collection destinations against the immutable native tree.
use super::*;
use crate::collection::{Ammo, Destination, Family, NodeBudget};
use crate::progression::{
    PRESENTATION_NODE_CHILD_NODE_ROW_SIZE, PRESENTATION_NODE_CHILD_NODES_OFFSET,
    PRESENTATION_NODE_ROW_SIZE,
};

pub(super) struct Page {
    pub destination: Destination,
    pub index: u16,
    pub template: u16,
    pub parent: u16,
    pub sibling: u16,
    pub donor: usize,
}
pub(super) struct Plan {
    pub pages: BTreeMap<Destination, Page>,
}

impl Plan {
    pub fn new(
        sources: &sources::ProjectSources,
        weapons: &[WeaponCloneSpec],
    ) -> AuthoringResult<Self> {
        let mut destinations = BTreeSet::new();
        for weapon in weapons {
            if let Some(destination) = weapon.overrides.collection_destination {
                let rarity = match weapon.overrides.rarity {
                    Some(rarity) => rarity,
                    None => {
                        let item = item_index(sources, weapon.donor_item_hash)?;
                        let tag = TagHash(read_u32(
                            &sources.stock_item_table,
                            sources.item_rows + item * ITEM_ROW_SIZE + 16,
                        )?);
                        weapon_rarity(&read_tag(&sources.manager, tag, "Collections rarity")?)?
                    }
                };
                if rarity != AuthoredWeaponRarity::Exotic {
                    destinations.insert(destination);
                }
            }
        }
        let badges = weapons
            .iter()
            .filter_map(|w| w.overrides.badge.as_ref().map(|b| b.name.as_str()))
            .collect::<BTreeSet<_>>();
        let budget = NodeBudget {
            badges: badges.len(),
            pages: destinations
                .iter()
                .filter(|d| d.stock_exemplar().is_none())
                .count(),
        };
        budget.validate()?;
        let mut next = crate::collection::BASE_NODE_COUNT + badges.len() * 4;
        let mut pages = BTreeMap::new();
        for destination in destinations {
            let template_destination = if destination.stock_exemplar().is_some() {
                destination
            } else {
                destination.family.template()
            };
            let (donor, template, _) = stock_page(sources, template_destination)?;
            let (_, sibling, parent) = stock_page(
                sources,
                Destination {
                    ammo: destination.ammo,
                    family: match destination.ammo {
                        Ammo::Primary => Family::AutoRifles,
                        Ammo::Special => Family::FusionRifles,
                        Ammo::Heavy => Family::Swords,
                    },
                },
            )?;
            let index = if destination.stock_exemplar().is_some() {
                template
            } else {
                let index = next as u16;
                next += 1;
                index
            };
            pages.insert(
                destination,
                Page {
                    destination,
                    index,
                    template,
                    parent,
                    sibling,
                    donor,
                },
            );
        }
        Ok(Self { pages })
    }
}

fn item_index(sources: &sources::ProjectSources, hash: u32) -> AuthoringResult<usize> {
    sources
        .stock_item_rows_by_hash
        .get(&hash)
        .and_then(|rows| rows.first())
        .copied()
        .ok_or_else(|| invalid(format!("Collections exemplar 0x{hash:08X} is missing")))
}

fn stock_page(
    sources: &sources::ProjectSources,
    destination: Destination,
) -> AuthoringResult<(usize, u16, u16)> {
    let hash = destination
        .stock_exemplar()
        .ok_or_else(|| invalid("Collections template has no stock exemplar"))?;
    let item = item_index(sources, hash)?;
    let candidates = (0..sources.stock_collectible_count)
        .filter(|&i| {
            read_u16(
                &sources.stock_collectibles,
                sources.collectible_rows + i * COLLECTIBLE_ROW_SIZE + COLLECTIBLE_ITEM_INDEX_OFFSET,
            )
            .ok()
                == Some(item as u16)
        })
        .collect::<Vec<_>>();
    let [donor] = candidates.as_slice() else {
        return Err(invalid(
            "Collections template must have exactly one collectible",
        ));
    };
    let parents =
        template_presentation_parents(&sources.stock_nodes, &sources.stock_collectibles, *donor)?;
    let page = donor_weapon_collection_page(&sources.stock_nodes, &parents)?;
    let (_, _, rows, _) = array_at(&sources.stock_nodes, 8)?;
    let (count, _, parent_rows, _) = array_at(
        &sources.stock_nodes,
        rows + usize::from(page) * PRESENTATION_NODE_ROW_SIZE + 0x18,
    )?;
    if count != 1 {
        return Err(invalid("Collections weapon page must have one ammo parent"));
    }
    let parent = read_u16(&sources.stock_nodes, parent_rows)?;
    let (count, _, children, _) = array_at(
        &sources.stock_nodes,
        rows + usize::from(parent) * PRESENTATION_NODE_ROW_SIZE
            + PRESENTATION_NODE_CHILD_NODES_OFFSET,
    )?;
    if !(0..count).any(|i| {
        read_u16(
            &sources.stock_nodes,
            children + i * PRESENTATION_NODE_CHILD_NODE_ROW_SIZE,
        )
        .ok()
            == Some(page)
    }) {
        return Err(invalid(
            "Collections ammo parent is missing its weapon page",
        ));
    }
    Ok((*donor, page, parent))
}

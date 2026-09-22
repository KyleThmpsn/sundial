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
    weapon_destinations: Vec<Option<Destination>>,
}

impl Plan {
    pub fn new(
        sources: &sources::ProjectSources,
        weapons: &[WeaponCloneSpec],
    ) -> AuthoringResult<Self> {
        let mut family_hashes = None;
        let mut destinations = BTreeSet::new();
        let mut weapon_destinations = Vec::with_capacity(weapons.len());
        for weapon in weapons {
            let item = item_index(sources, weapon.donor_item_hash)?;
            let definition_tag = TagHash(read_u32(
                &sources.stock_item_table,
                sources.item_rows + item * ITEM_ROW_SIZE + 16,
            )?);
            let definition = read_tag(&sources.manager, definition_tag, "Collections rarity")?;
            let rarity = weapon
                .overrides
                .rarity
                .unwrap_or(weapon_rarity(&definition)?);
            let destination = if rarity == AuthoredWeaponRarity::Exotic {
                None
            } else if let Some(destination) = weapon.overrides.collection_destination {
                Some(destination)
            } else {
                if family_hashes.is_none() {
                    family_hashes = Some(stock_family_hashes(sources)?);
                }
                let Some(family_hashes) = family_hashes.as_ref() else {
                    return Err(invalid("Collections family templates were not initialized"));
                };
                Some(automatic_destination(sources, weapon, item, family_hashes)?)
            };
            if let Some(destination) = destination {
                destinations.insert(destination);
            }
            weapon_destinations.push(destination);
        }
        let badges = weapons
            .iter()
            .filter_map(|w| w.overrides.badge.as_ref().map(|b| b.name.as_str()))
            .collect::<BTreeSet<_>>();
        let budget = NodeBudget::new(weapons.iter().zip(&weapon_destinations).map(
            |(weapon, destination)| {
                (
                    weapon
                        .overrides
                        .badge
                        .as_ref()
                        .map(|badge| badge.name.as_str()),
                    *destination,
                )
            },
        ));
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
        Ok(Self {
            pages,
            weapon_destinations,
        })
    }

    pub fn page_for_weapon(&self, index: usize) -> Option<&Page> {
        self.weapon_destinations
            .get(index)
            .copied()
            .flatten()
            .and_then(|destination| self.pages.get(&destination))
    }
}

fn automatic_destination(
    sources: &sources::ProjectSources,
    weapon: &WeaponCloneSpec,
    item: usize,
    family_hashes: &BTreeMap<u32, Family>,
) -> AuthoringResult<Destination> {
    let string_tag = TagHash(read_u32(
        &sources.stock_item_strings,
        sources.string_rows + item * ITEM_ROW_SIZE + 16,
    )?);
    let strings = read_tag(
        &sources.manager,
        string_tag,
        "Collections weapon classification",
    )?;
    let ammo = weapon
        .overrides
        .ammo_type
        .or(item_string_ammo_type(&strings)?)
        .ok_or_else(|| invalid("Collections weapon has no ammunition classification"))?;
    let family_hash = read_u32(&strings, ITEM_TYPE_REFERENCE_OFFSET + 4)?;
    let family = family_hashes.get(&family_hash).copied().ok_or_else(|| {
        invalid(
            "This weapon family has no non-Exotic Collections page template in this game version",
        )
    })?;
    Ok(Destination {
        ammo: match ammo {
            WeaponAmmoType::Primary => Ammo::Primary,
            WeaponAmmoType::Special => Ammo::Special,
            WeaponAmmoType::Heavy => Ammo::Heavy,
        },
        family,
    })
}

fn stock_family_hashes(
    sources: &sources::ProjectSources,
) -> AuthoringResult<BTreeMap<u32, Family>> {
    let mut hashes = BTreeMap::new();
    for family in Family::ALL {
        let exemplar = family
            .template()
            .stock_exemplar()
            .ok_or_else(|| invalid("Collections family template has no stock exemplar"))?;
        let item = item_index(sources, exemplar)?;
        let string_tag = TagHash(read_u32(
            &sources.stock_item_strings,
            sources.string_rows + item * ITEM_ROW_SIZE + 16,
        )?);
        let strings = read_tag(
            &sources.manager,
            string_tag,
            "Collections family classification",
        )?;
        let hash = read_u32(&strings, ITEM_TYPE_REFERENCE_OFFSET + 4)?;
        if hashes.insert(hash, family).is_some() {
            return Err(invalid(
                "Collections family templates share a weapon-type identity",
            ));
        }
    }
    Ok(hashes)
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

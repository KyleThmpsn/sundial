//! Resolve semantic collection destinations against the immutable native tree.
use super::*;
use crate::collection::{Ammo, Destination, Family, GearPage, NodeBudget};
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
/// A branded page for one kind of gear, added under the category that holds its base's page.
pub(super) struct GearPagePlan {
    pub page: GearPage,
    pub index: u16,
    /// A stock page in the same category, whose rows the new page clones.
    pub template: u16,
    pub parent: u16,
    pub sibling: u16,
    /// A collectible under `template`, whose child row the members copy.
    pub donor: usize,
    pub children: Vec<u16>,
}

/// An armor piece's branded pages: one per supported class, in the set that class has filled
/// to, with the first base of each class's page noted. Exotic armor has none.
fn armor_pages(
    weapon: &WeaponCloneSpec,
    definition: &[u8],
    supported: crate::collection::Classes,
    (armor_counts, gear_bases): (&mut [usize; 3], &mut BTreeMap<GearPage, u32>),
) -> AuthoringResult<Vec<GearPage>> {
    let rarity = weapon
        .overrides
        .rarity
        .unwrap_or(weapon_rarity(definition)?);
    if rarity == AuthoredWeaponRarity::Exotic {
        return Ok(Vec::new());
    }
    Ok(supported
        .iter()
        .map(|class| {
            let count = &mut armor_counts[usize::from(class)];
            let set = *count / crate::collection::ARMOR_SET_SIZE;
            *count += 1;
            gear_bases
                .entry(GearPage::armor(class))
                .or_insert(weapon.donor_item_hash);
            GearPage::armor_set(class, set)
        })
        .collect())
}

pub(super) struct Plan {
    pub pages: BTreeMap<Destination, Page>,
    pub gear_pages: BTreeMap<GearPage, GearPagePlan>,
    weapon_destinations: Vec<Option<Destination>>,
    gear_destinations: Vec<Vec<GearPage>>,
    pub classes: Vec<crate::collection::Classes>,
}

impl Plan {
    pub fn new(
        sources: &sources::ProjectSources,
        weapons: &[WeaponCloneSpec],
    ) -> AuthoringResult<Self> {
        let mut family_hashes = None;
        let mut destinations = BTreeSet::new();
        let mut weapon_destinations = Vec::with_capacity(weapons.len());
        let mut gear_destinations = Vec::with_capacity(weapons.len());
        let mut classes = Vec::with_capacity(weapons.len());
        // The first base of each kind names the category its branded page joins.
        let mut gear_bases = BTreeMap::new();
        let mut armor_counts = [0usize; 3];
        for weapon in weapons {
            let armor_definition = if weapon.kind == ItemKind::Armor {
                let item = item_index(sources, weapon.donor_item_hash)?;
                Some(read_tag(
                    &sources.manager,
                    TagHash(read_u32(
                        &sources.stock_item_table,
                        sources.item_rows + item * ITEM_ROW_SIZE + 16,
                    )?),
                    "Armor Collections class and rarity",
                )?)
            } else {
                None
            };
            let supported = if let Some(definition) = armor_definition.as_deref() {
                super::armor::classes(definition, weapon.overrides.armor_class)
                    .map_err(|error| weapon.in_recipe(error))?
            } else {
                crate::collection::Classes::ALL
            };
            classes.push(supported);
            let gear_pages = match armor_definition.as_deref() {
                Some(definition) => armor_pages(
                    weapon,
                    definition,
                    supported,
                    (&mut armor_counts, &mut gear_bases),
                )?,
                None => GearPage::for_kind(weapon.kind).into_iter().collect(),
            };
            for &page in &gear_pages {
                gear_bases.entry(page).or_insert(weapon.donor_item_hash);
            }
            gear_destinations.push(gear_pages);
            // Gear destinations do not use weapon ammo and family placement.
            if !weapon.kind.is_weapon() {
                weapon_destinations.push(None);
                continue;
            }
            let destination = weapon_destination(sources, weapon, &mut family_hashes)?;
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
        ))
        .with_gear_pages(gear_bases.len());
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
        let mut gear_pages = BTreeMap::new();
        for (page, base) in gear_bases {
            let index = u16::try_from(next)
                .map_err(|_| invalid("Collections page index exceeds capacity"))?;
            next += 1;
            gear_pages.insert(page, gear_page(sources, page, base, index)?);
        }
        // Categories link to the preallocated set rows, preserving their display order.
        let armor_categories = gear_pages
            .keys()
            .copied()
            .filter(|page| page.kind() == ItemKind::Armor && page.set().is_none())
            .collect::<Vec<_>>();
        for category in armor_categories {
            let children = gear_pages
                .values()
                .filter(|page| page.page.class() == category.class() && page.page.set().is_some())
                .map(|page| page.index)
                .collect::<Vec<_>>();
            let parent = gear_pages[&category].index;
            for page in gear_pages
                .values_mut()
                .filter(|page| page.page.class() == category.class() && page.page.set().is_some())
            {
                page.parent = parent;
            }
            gear_pages.get_mut(&category).unwrap().children = children;
        }
        Ok(Self {
            pages,
            gear_pages,
            weapon_destinations,
            gear_destinations,
            classes,
        })
    }

    pub fn page_for_weapon(&self, index: usize) -> Option<&Page> {
        self.weapon_destinations
            .get(index)
            .copied()
            .flatten()
            .and_then(|destination| self.pages.get(&destination))
    }

    pub fn gear_pages_for(&self, index: usize) -> Vec<&GearPagePlan> {
        self.gear_destinations
            .get(index)
            .into_iter()
            .flatten()
            .filter_map(|page| self.gear_pages.get(page))
            .collect()
    }
}

/// Places a gear kind's branded page beside the stock page that holds `base`, under that page's
/// one parent: a season page's parent is its Ships, Sparrows or Ghosts category.
fn gear_page(
    sources: &sources::ProjectSources,
    page: GearPage,
    base: u32,
    index: u16,
) -> AuthoringResult<GearPagePlan> {
    if page.kind() == ItemKind::Armor {
        return super::armor::page(sources, page, index);
    }
    let item = item_index(sources, base)?;
    let own = (0..sources.stock_collectible_count).find(|&collectible| {
        read_u16(
            &sources.stock_collectibles,
            sources.collectible_rows
                + collectible * COLLECTIBLE_ROW_SIZE
                + COLLECTIBLE_ITEM_INDEX_OFFSET,
        )
        .ok()
            == u16::try_from(item).ok()
    });
    let donor = match own {
        Some(donor) => donor,
        None => {
            let strings = read_tag(
                &sources.manager,
                TagHash(read_u32(
                    &sources.stock_item_strings,
                    sources.string_rows + item * ITEM_ROW_SIZE + 16,
                )?),
                "Collections base strings",
            )?;
            super::gear::stand_in_collectible(sources, &strings)?.ok_or_else(|| {
                invalid(
                    "Neither this base item nor another version of it is in Collections. Choose a base from Collections.",
                )
            })?
        }
    };
    let parents =
        template_presentation_parents(&sources.stock_nodes, &sources.stock_collectibles, donor)?;
    let template = crate::progression::gear_collection_page(&sources.stock_nodes, &parents)?;
    let (_, _, rows, _) = array_at(&sources.stock_nodes, 8)?;
    let (count, _, parent_rows, _) = array_at(
        &sources.stock_nodes,
        rows + usize::from(template) * PRESENTATION_NODE_ROW_SIZE + 0x18,
    )?;
    if count != 1 {
        return Err(invalid(format!(
            "The base's Collections page must have one parent to hold a {} page",
            page.kind().label()
        )));
    }
    let parent = read_u16(&sources.stock_nodes, parent_rows)?;
    let (count, _, children, _) = array_at(
        &sources.stock_nodes,
        rows + usize::from(parent) * PRESENTATION_NODE_ROW_SIZE
            + PRESENTATION_NODE_CHILD_NODES_OFFSET,
    )?;
    let sibling = count
        .checked_sub(1)
        .map(|last| {
            read_u16(
                &sources.stock_nodes,
                children + last * PRESENTATION_NODE_CHILD_NODE_ROW_SIZE,
            )
        })
        .transpose()?
        .ok_or_else(|| invalid("The base's Collections category has no pages"))?;
    Ok(GearPagePlan {
        page,
        index,
        template,
        parent,
        sibling,
        donor,
        children: Vec::new(),
    })
}

/// Where a weapon's Collections row goes: none for an Exotic, the recipe's own node, or the
/// node matching its ammo and weapon type. The stock family templates are read once, for the
/// first weapon that needs them.
fn weapon_destination(
    sources: &sources::ProjectSources,
    weapon: &WeaponCloneSpec,
    family_hashes: &mut Option<BTreeMap<u32, Family>>,
) -> AuthoringResult<Option<Destination>> {
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
    if rarity == AuthoredWeaponRarity::Exotic {
        return Ok(None);
    }
    if let Some(destination) = weapon.overrides.collection_destination {
        return Ok(Some(destination));
    }
    if family_hashes.is_none() {
        *family_hashes = Some(stock_family_hashes(sources)?);
    }
    let Some(family_hashes) = family_hashes.as_ref() else {
        return Err(invalid("Collections family templates were not initialized"));
    };
    automatic_destination(sources, weapon, item, family_hashes)
        .map(Some)
        .map_err(|error| weapon.in_recipe(error))
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
    // A donor with no ammo type of its own, such as Rose, files under Primary unless the
    // recipe chooses an Ammo Type.
    let ammo = weapon
        .overrides
        .ammo_type
        .or(item_string_ammo_type(&strings)?)
        .unwrap_or(WeaponAmmoType::Primary);
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
        if family.template().family != family {
            continue;
        }
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

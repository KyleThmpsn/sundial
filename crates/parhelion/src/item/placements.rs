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
    /// Authored item identities in tile order for an armor set. Other pages use build order.
    pub members: Vec<u32>,
}

/// A weapon type's page under Weapons and Weapon Ornaments, where an ordinary weapon's own
/// ornaments go: the stock page, or one the build adds for a type without one.
pub(super) struct OrnamentPage {
    pub family: Family,
    pub index: u16,
    pub added: bool,
    /// The stock page, or the stock page an added one clones.
    pub template: u16,
    pub parent: u16,
    pub sibling: u16,
    /// A stock collectible under `template`, whose child row the ornaments copy.
    pub donor: usize,
}

/// Where emission puts an ordinary weapon's linked ornaments: their page, and the stock child row
/// on `child_page` that names `child_donor`, which their rows copy.
#[derive(Clone, Copy)]
#[cfg_attr(not(feature = "d2-model-importer"), allow(dead_code))]
pub(super) struct OrnamentTarget {
    pub page: u16,
    pub child_page: u16,
    pub child_donor: usize,
}

/// Whether an imported weapon's graph links ornaments of its own, which emission adds to its
/// ornament socket and to Collections.
#[cfg(feature = "d2-model-importer")]
fn links_ornaments(weapon: &WeaponCloneSpec) -> AuthoringResult<bool> {
    let Some(reference) = &weapon.overrides.imported_graph else {
        return Ok(false);
    };
    for attachment in &reference.attachments {
        let bytes = std::fs::read(attachment.directory.join("asset-graph.json"))
            .map_err(|error| invalid(format!("Linked ornament graph: {error}")))?;
        let graph: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|error| invalid(format!("Linked ornament graph: {error}")))?;
        if graph.get("ornament").is_some() {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(not(feature = "d2-model-importer"))]
fn links_ornaments(_: &WeaponCloneSpec) -> AuthoringResult<bool> {
    Ok(false)
}

/// The one parent of a stock page and that parent's last child page.
fn parent_and_last_sibling(nodes: &[u8], page: u16) -> AuthoringResult<(u16, u16)> {
    let (_, _, rows, _) = array_at(nodes, 8)?;
    let (count, _, parent_rows, _) = array_at(
        nodes,
        rows + usize::from(page) * PRESENTATION_NODE_ROW_SIZE + 0x18,
    )?;
    if count != 1 {
        return Err(invalid(
            "A Collections page must have one parent to hold another",
        ));
    }
    let parent = read_u16(nodes, parent_rows)?;
    let (count, _, children, _) = array_at(
        nodes,
        rows + usize::from(parent) * PRESENTATION_NODE_ROW_SIZE
            + PRESENTATION_NODE_CHILD_NODES_OFFSET,
    )?;
    let sibling = count
        .checked_sub(1)
        .map(|last| {
            read_u16(
                nodes,
                children + last * PRESENTATION_NODE_CHILD_NODE_ROW_SIZE,
            )
        })
        .transpose()?
        .ok_or_else(|| invalid("A Collections category has no pages"))?;
    Ok((parent, sibling))
}

/// `family`'s Weapon Ornaments page: the stock page its exemplar ornament is on, or a page added at
/// `next` that clones the Fusion Rifles page for a type without one.
fn ornament_page(
    sources: &sources::ProjectSources,
    family: Family,
    next: &mut usize,
) -> AuthoringResult<OrnamentPage> {
    let (exemplar, added) = match family.ornament_exemplar() {
        Some(exemplar) => (exemplar, false),
        None => (
            Family::FusionRifles
                .ornament_exemplar()
                .ok_or_else(|| invalid("Weapon Ornaments has no page template"))?,
            true,
        ),
    };
    let item = u16::try_from(item_index(sources, exemplar, "weapon ornament page")?)
        .map_err(|_| invalid("Weapon ornament exemplar index exceeds 16 bits"))?;
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
        return Err(invalid(
            "A Weapon Ornaments exemplar must have exactly one collectible",
        ));
    };
    let parents =
        template_presentation_parents(&sources.stock_nodes, &sources.stock_collectibles, *donor)?;
    let &[template] = parents.as_slice() else {
        return Err(invalid(
            "A Weapon Ornaments exemplar must be on one Collections page",
        ));
    };
    let (parent, sibling) = parent_and_last_sibling(&sources.stock_nodes, template)?;
    let index = if added {
        let index =
            u16::try_from(*next).map_err(|_| invalid("Collections page index exceeds capacity"))?;
        *next += 1;
        index
    } else {
        template
    };
    Ok(OrnamentPage {
        family,
        index,
        added,
        template,
        parent,
        sibling,
        donor: *donor,
    })
}

/// Ordinary armor uses equipment buckets. Exotic armor keeps its native flat page.
fn armor_piece(
    sources: &sources::ProjectSources,
    index: usize,
    weapon: &WeaponCloneSpec,
    definition: &[u8],
    supported: crate::collection::Classes,
) -> AuthoringResult<Option<crate::collection::armor::Piece>> {
    let rarity = weapon
        .overrides
        .rarity
        .unwrap_or(weapon_rarity(definition)?);
    if rarity == AuthoredWeaponRarity::Exotic {
        return Ok(None);
    }
    let bucket = *definition
        .get(sundial::package_authoring::investment_schema::ITEM_INVENTORY_SLOT_OFFSET)
        .ok_or_else(|| invalid("Armor Collections base has no inventory slot"))?;
    let slot = crate::collection::armor::Slot::from_native_bucket(bucket)
        .ok_or_else(|| invalid("Armor Collections base has an unsupported inventory slot"))?;
    Ok(Some(crate::collection::armor::Piece {
        index,
        slot: Some(slot),
        classes: supported,
        group: super::armor::group(sources, weapon)?,
    }))
}

/// An imported universal ornament's place in the sets of its class. Its base's plug category names
/// the class and the armor slot it applies to.
fn ornament_piece(
    sources: &sources::ProjectSources,
    index: usize,
    weapon: &WeaponCloneSpec,
) -> AuthoringResult<Option<crate::collection::armor::Piece>> {
    if !weapon.is_imported_ornament() {
        return Ok(None);
    }
    let item = item_index(sources, weapon.donor_item_hash, "universal ornament base")?;
    let definition = read_tag(
        &sources.manager,
        TagHash(read_u32(
            &sources.stock_item_table,
            sources.item_rows + item * ITEM_ROW_SIZE + 16,
        )?),
        "Universal ornament class and slot",
    )?;
    let category = crate::plug_classification::PlugClassification::category(&definition)?;
    let (class, slot) = crate::collection::armor::Slot::of_ornament(category)
        .ok_or_else(|| invalid("An imported ornament's base must be a universal ornament"))?;
    Ok(Some(crate::collection::armor::Piece {
        index,
        slot: Some(slot),
        classes: crate::collection::Classes::one(class),
        group: None,
    }))
}

/// Places armor in its class's branded category, or universal ornaments beside the stock sets of
/// their class's Universal Ornaments, which needs no category of its own.
fn assign_armor_sets(
    weapons: &[WeaponCloneSpec],
    armor: &[crate::collection::armor::Piece],
    page: fn(u8, usize) -> GearPage,
    destinations: &mut [Vec<GearPage>],
    bases: &mut BTreeMap<GearPage, u32>,
) -> BTreeMap<GearPage, Vec<u32>> {
    crate::collection::armor::sets(armor, page)
        .into_iter()
        .map(|set| {
            let base = weapons[set.members[0]].donor_item_hash;
            if !set.page.is_ornament_set() {
                bases
                    .entry(GearPage::armor(set.page.class().unwrap()))
                    .or_insert(base);
            }
            bases.insert(set.page, base);
            let members = set
                .members
                .into_iter()
                .map(|index| {
                    destinations[index].push(set.page);
                    weapons[index].identity.item_hash
                })
                .collect();
            (set.page, members)
        })
        .collect()
}

pub(super) struct Plan {
    pub pages: BTreeMap<Destination, Page>,
    pub gear_pages: BTreeMap<GearPage, GearPagePlan>,
    pub ornament_pages: BTreeMap<Family, OrnamentPage>,
    weapon_destinations: Vec<Option<Destination>>,
    gear_destinations: Vec<Vec<GearPage>>,
    /// The weapon type of each ordinary weapon that links ornaments, by its item hash.
    ornament_owners: BTreeMap<u32, Family>,
    pub classes: Vec<crate::collection::Classes>,
}

impl Plan {
    #[allow(clippy::cognitive_complexity)]
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
        let mut armor = Vec::new();
        let mut ornaments = Vec::new();
        let mut ornament_owners = BTreeMap::new();
        for (index, weapon) in weapons.iter().enumerate() {
            if let Some(piece) =
                ornament_piece(sources, index, weapon).map_err(|error| weapon.in_recipe(error))?
            {
                classes.push(piece.classes);
                ornaments.push(piece);
                gear_destinations.push(Vec::new());
                weapon_destinations.push(None);
                continue;
            }
            let armor_definition = if weapon.kind == ItemKind::Armor {
                let item = item_index(sources, weapon.donor_item_hash, "armor base")
                    .map_err(|error| weapon.in_recipe(error))?;
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
                Some(definition) => {
                    armor.extend(
                        armor_piece(sources, index, weapon, definition, supported)
                            .map_err(|error| weapon.in_recipe(error))?,
                    );
                    Vec::new()
                }
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
            let destination = weapon_destination(sources, weapon, &mut family_hashes)
                .map_err(|error| weapon.in_recipe(error))?;
            if let Some(destination) = destination {
                destinations.insert(destination);
                // An ordinary weapon's own ornaments go to its type's Weapon Ornaments page. An
                // Exotic's stay beside the native ornament they copy, under Exotic.
                if links_ornaments(weapon).map_err(|error| weapon.in_recipe(error))? {
                    ornament_owners.insert(weapon.identity.item_hash, destination.family);
                }
            }
            weapon_destinations.push(destination);
        }
        let mut armor_members = assign_armor_sets(
            weapons,
            &armor,
            GearPage::armor_set,
            &mut gear_destinations,
            &mut gear_bases,
        );
        armor_members.extend(assign_armor_sets(
            weapons,
            &ornaments,
            GearPage::ornament_set,
            &mut gear_destinations,
            &mut gear_bases,
        ));
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
        .with_gear_pages(
            gear_bases.len()
                + ornament_owners
                    .values()
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .filter(|family| family.ornament_exemplar().is_none())
                    .count(),
        );
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
            let mut planned = gear_page(sources, page, base, index)?;
            planned.members = armor_members.remove(&page).unwrap_or_default();
            gear_pages.insert(page, planned);
        }
        let mut ornament_pages = BTreeMap::new();
        for family in ornament_owners.values().copied().collect::<BTreeSet<_>>() {
            ornament_pages.insert(family, ornament_page(sources, family, &mut next)?);
        }
        // Categories link to the preallocated set rows, preserving their display order.
        let armor_categories = gear_pages
            .keys()
            .copied()
            .filter(|page| page.kind() == ItemKind::Armor && page.set().is_none())
            .collect::<Vec<_>>();
        let in_category = |category: GearPage, page: GearPage| {
            page.kind() == ItemKind::Armor
                && page.class() == category.class()
                && page.set().is_some()
        };
        for category in armor_categories {
            let children = gear_pages
                .values()
                .filter(|page| in_category(category, page.page))
                .map(|page| page.index)
                .collect::<Vec<_>>();
            let parent = gear_pages[&category].index;
            for page in gear_pages
                .values_mut()
                .filter(|page| in_category(category, page.page))
            {
                page.parent = parent;
            }
            gear_pages.get_mut(&category).unwrap().children = children;
        }
        Ok(Self {
            pages,
            gear_pages,
            ornament_pages,
            weapon_destinations,
            gear_destinations,
            ornament_owners,
            classes,
        })
    }

    /// Where each ordinary weapon's linked ornaments go, by the weapon's item hash.
    pub fn ornament_targets(&self) -> BTreeMap<u32, OrnamentTarget> {
        self.ornament_owners
            .iter()
            .filter_map(|(&owner, family)| {
                let page = self.ornament_pages.get(family)?;
                Some((
                    owner,
                    OrnamentTarget {
                        page: page.index,
                        child_page: page.template,
                        child_donor: page.donor,
                    },
                ))
            })
            .collect()
    }

    /// The names of the pages the build adds: numbered sets, and the Weapon Ornaments pages of
    /// weapon types without a stock one.
    pub fn page_names(&self, brand: &str) -> Vec<(u32, String)> {
        self.gear_pages
            .keys()
            .filter_map(|page| page.set_name(brand).map(|name| (page.hash("name"), name)))
            .chain(
                self.ornament_pages
                    .values()
                    .filter(|page| page.added)
                    .map(|page| {
                        (
                            page.family.ornament_page_hash("name"),
                            page.family.label().to_owned(),
                        )
                    }),
            )
            .collect()
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
    if page.is_ornament_set() {
        return super::armor::ornament_page(sources, page, index);
    }
    let item = item_index(sources, base, "gear base")?;
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
        members: Vec::new(),
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
    let item = item_index(sources, weapon.donor_item_hash, "weapon base")?;
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
    automatic_destination(sources, weapon, item, family_hashes).map(Some)
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
    // The weapon files under the type it shows: its appearance's, unless it keeps the base's.
    // An appearance type with no page of its own, such as a Glaive, files under the base's.
    let family_of = |strings: &[u8]| -> AuthoringResult<Option<Family>> {
        Ok(family_hashes
            .get(&read_u32(strings, ITEM_TYPE_REFERENCE_OFFSET + 4)?)
            .copied())
    };
    let shown = match &weapon.presentation_donor {
        Some(presentation) if !weapon.overrides.base_type => {
            let item = item_index(sources, presentation.item_hash, "appearance donor")?;
            let string_tag = TagHash(read_u32(
                &sources.stock_item_strings,
                sources.string_rows + item * ITEM_ROW_SIZE + 16,
            )?);
            family_of(&read_tag(
                &sources.manager,
                string_tag,
                "Collections appearance classification",
            )?)?
        }
        _ => None,
    };
    let family = match shown {
        Some(family) => family,
        None => family_of(&strings)?.ok_or_else(|| {
            invalid(
                "This weapon family has no non-Exotic Collections page template in this game version",
            )
        })?,
    };
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
        // Shadowkeep has trace rifles but only Exotic Collections entries for
        // them. Their native type identity still selects a new ordinary page.
        let exemplar = if family == Family::TraceRifles {
            0x0122_48BA // Prometheus Lens supplies the native Trace Rifle type.
        } else {
            if family.template().family != family {
                continue;
            }
            family
                .template()
                .stock_exemplar()
                .ok_or_else(|| invalid("Collections family template has no stock exemplar"))?
        };
        let item = item_index(sources, exemplar, "family template")?;
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

pub(super) fn item_index(
    sources: &sources::ProjectSources,
    hash: u32,
    role: &str,
) -> AuthoringResult<usize> {
    sources
        .stock_item_rows_by_hash
        .get(&hash)
        .and_then(|rows| rows.first())
        .copied()
        .ok_or_else(|| {
            invalid(format!(
                "Collections {role} item 0x{hash:08X} is missing from the stock game data"
            ))
        })
}

fn stock_page(
    sources: &sources::ProjectSources,
    destination: Destination,
) -> AuthoringResult<(usize, u16, u16)> {
    let hash = destination
        .stock_exemplar()
        .ok_or_else(|| invalid("Collections template has no stock exemplar"))?;
    let item = item_index(sources, hash, "page template")?;
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

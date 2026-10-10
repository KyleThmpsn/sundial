//! Shadowkeep subclass ability and attunement decoding.
//!
//! Every list with a super lane is a subclass's, stock or authored. Stock lists share one layout,
//! and an authored list keeps it, taking some entries' pools from other stock subclasses. So a
//! list's class, its middle attunement's super and its attunement names all come from the data.

use std::collections::{BTreeMap, HashMap};

use crate::package_runtime::reader::PackageManager;
use serde::{Deserialize, Serialize};
use tiger_pkg::TagHash;

use crate::{
    investment::localization::{LocalizedStringCache, resolve_localized_hash, resolve_string},
    investment::schema::{GLOBALS_SUBCLASS_DISPLAY_TABLE_SLOT, ROOT_SOCKET_ENTRY_LIST_TABLE_SLOT},
    package_payload::{array_at, i32_at, i64_at, relative_offset, u32_at},
};

use super::ItemDef;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct AbilityOptions {
    pub movement: Vec<AbilityChoice>,
    pub grenade: Vec<AbilityChoice>,
    pub super_ability: Vec<AbilityChoice>,
    pub melee: Vec<AbilityChoice>,
    pub class_ability: Vec<AbilityChoice>,
    #[serde(default)]
    pub attunements: Vec<AttunementChoice>,
    /// Each entry's sandbox perks, from its pool's active variant.
    #[serde(default)]
    pub entry_perks: BTreeMap<u64, Vec<u16>>,
    /// Each entry's icon container and description, from its node display record.
    #[serde(default)]
    pub entry_icons: BTreeMap<u64, u32>,
    #[serde(default)]
    pub entry_descriptions: BTreeMap<u64, String>,
    /// Each entry's ability entity, which its pool's active records name through the entity
    /// assignment table.
    #[serde(default)]
    pub entry_entities: BTreeMap<u64, u32>,
    /// Each entry's ability row, the row its active pool's own record equips.
    #[serde(default)]
    pub entry_rows: BTreeMap<u64, u8>,
    /// Each key an entry's active pool applies to an ability, with the row it applies it to.
    #[serde(default)]
    pub entry_modifiers: BTreeMap<u64, Vec<(u32, u8)>>,
    /// Whether the list is one a build added rather than one of the stock nine. Equip eligibility
    /// comes from the item's equipment conditions, separately from the grid's base class.
    #[serde(default)]
    pub authored: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AbilityChoice {
    pub entry: u64,
    pub name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct AttunementChoice {
    pub name: String,
    pub super_abilities: Vec<AbilityChoice>,
    pub melee: AbilityChoice,
    pub perks: Vec<AbilityChoice>,
}

#[derive(Default)]
pub(in crate::catalog) struct AbilityDisplayData {
    names: HashMap<u32, String>,
    attunement_names: Vec<String>,
    /// Each attunement's name by its lead plug source, from the lore row the display record
    /// shows it by, as the client reads it. An authored path can name a row of its own.
    path_names: HashMap<u32, String>,
    /// Each node's icon container and description, by display hash.
    icons: HashMap<u32, u32>,
    descriptions: HashMap<u32, String>,
}

#[derive(Clone)]
struct ParsedAbilityEntry {
    choice: AbilityChoice,
    plug_source: u32,
    group: u8,
}

/// The stock lists and their classes: 0 Titan, 1 Hunter, 2 Warlock.
const STOCK_LIST_CLASSES: [(u16, u64); 9] = [
    (1, 1),
    (2, 1),
    (3, 1),
    (5, 0),
    (6, 0),
    (7, 0),
    (9, 2),
    (10, 2),
    (11, 2),
];
/// The entry kind of the super lane, which only a subclass's list carries.
/// The stock subclass socket-entry lists and their classes, by row in the stock table. Any
/// other list, authored or not, takes the class of the stock list whose class-base entry it
/// shares.
pub fn stock_subclass_list_classes() -> impl Iterator<Item = (u16, u8)> {
    STOCK_LIST_CLASSES
        .iter()
        .filter_map(|(list, class)| Some((*list, u8::try_from(*class).ok()?)))
}

const SUPER_LANE_KIND: u8 = 34;
/// The base melee target every attunement's melee links to. Its pool differs by class.
const CLASS_BASE_ENTRY: usize = 0;
/// The middle attunement leads with its own super here when it brings one.
const MIDDLE_SUPER_ENTRY: usize = 20;
/// Attunement entries, top, bottom and middle, in the order their plug sources first appear.
const ATTUNEMENT_ENTRIES: [[usize; 4]; 3] = [[11, 12, 13, 14], [15, 16, 17, 18], [20, 21, 22, 23]];
/// A display record's attunement rows: each path's lead plug source and the lore row that shows
/// the path, whose name reference sits at +12 of a 40-byte row.
const DISPLAY_PATH_ARRAY: usize = 0x30;
const LORE_DISPLAY_TABLE_SLOT: usize = 34;
const LORE_DISPLAY_ROW_SIZE: usize = 40;
const LORE_DISPLAY_NAME: usize = 12;
/// A pool variant's sandbox perks, two bytes each.
const VARIANT_PERK_ARRAY: usize = 0x20;
/// A node display record's name and description references, and its icon's row in the item icon
/// table (globals slot 75), 0xFFFF for a node without one. Every stock node with a name has an
/// icon, and neighbouring nodes take neighbouring rows.
const NODE_DISPLAY_NAME: usize = 160;
const NODE_DISPLAY_DESCRIPTION: usize = 168;
const NODE_DISPLAY_ICON: usize = 184;

/// One socket entry's kind and pool.
struct ListEntry {
    kind: u8,
    pool: u32,
}

fn list_entries(list: &[u8]) -> Vec<ListEntry> {
    let Ok((count, rows, _)) = array_at(list, 16) else {
        return Vec::new();
    };
    (0..count.min(64))
        .map_while(|index| {
            let base = rows + index * 64;
            Some(ListEntry {
                kind: *list.get(base + 13)?,
                pool: u32_at(list, base + 56).ok()?,
            })
        })
        .collect()
}

pub(in crate::catalog) fn build_subclass_choices(
    manager: &PackageManager,
    root: &[u8],
    (ability_displays, ability_entities): (&HashMap<u16, AbilityDisplayData>, &[Option<u32>]),
    item_socket_lists: Vec<(usize, u16, Option<u8>)>,
    items: &mut [ItemDef],
) -> Result<(), String> {
    let list_table = manager
        .read_tag(TagHash(u32_at(
            root,
            8 + ROOT_SOCKET_ENTRY_LIST_TABLE_SLOT * 16,
        )?))
        .map_err(|error| format!("Could not read subclass ability table: {error}"))?;
    let (list_count, list_rows, _) = array_at(&list_table, 8)?;
    // Every list with a super lane, in table order, so the stock lists come first.
    let mut lists = BTreeMap::new();
    for index in 0..list_count {
        let Ok(list_index) = u16::try_from(index) else {
            break;
        };
        let tag = TagHash(u32_at(&list_table, list_rows + index * 24 + 16)?);
        let Ok(list) = manager.read_tag(tag) else {
            continue;
        };
        let entries = list_entries(&list);
        if entries.iter().any(|entry| entry.kind == SUPER_LANE_KIND) {
            lists.insert(list_index, (list, entries));
        }
    }
    // Another list's class is the class of the stock list that shares its base melee target.
    let class_by_base = STOCK_LIST_CLASSES
        .iter()
        .filter_map(|(index, class)| {
            Some((lists.get(index)?.1.get(CLASS_BASE_ENTRY)?.pool, *class))
        })
        .collect::<HashMap<_, _>>();
    // An attunement keeps the name of the stock subclass it comes from, which its pools identify.
    let mut attunement_names = HashMap::<Vec<u32>, String>::new();
    let mut options = HashMap::new();
    for (&index, (list, entries)) in &lists {
        let Some(display) = ability_displays.get(&index) else {
            continue;
        };
        // The middle attunement's super replaces the shared one only when it declares its own
        // bucket kind. Otherwise it adds a hash to the shared super and entry 10 stays selected.
        let middle_super = if declares_kind(manager, list, MIDDLE_SUPER_ENTRY) {
            MIDDLE_SUPER_ENTRY as u64
        } else {
            10
        };
        let mut abilities = parse_abilities(list, display, middle_super);
        abilities.entry_perks = (0..entries.len())
            .map(|entry| (entry as u64, entry_perks(manager, list, entry)))
            .filter(|(_, perks)| !perks.is_empty())
            .collect();
        abilities.entry_entities = (0..entries.len())
            .filter_map(|entry| {
                Some((
                    entry as u64,
                    entry_entity(manager, list, entry, ability_entities)?,
                ))
            })
            .collect();
        abilities.entry_rows = (0..entries.len())
            .filter_map(|entry| Some((entry as u64, entry_row(manager, list, entry)?)))
            .collect();
        abilities.entry_modifiers = (0..entries.len())
            .map(|entry| (entry as u64, entry_modifiers(manager, list, entry)))
            .filter(|(_, modifiers)| !modifiers.is_empty())
            .collect();
        // Without lore rows to read, a path takes the name of the stock one with its pools.
        if display.path_names.is_empty() {
            for (attunement, positions) in abilities.attunements.iter_mut().zip(ATTUNEMENT_ENTRIES)
            {
                let pools = positions
                    .iter()
                    .filter_map(|&position| entries.get(position).map(|entry| entry.pool))
                    .collect::<Vec<_>>();
                let name = attunement_names
                    .entry(pools)
                    .or_insert_with(|| attunement.name.clone())
                    .clone();
                attunement.name = name;
            }
        }
        let class = STOCK_LIST_CLASSES
            .iter()
            .find(|(list, _)| *list == index)
            .map(|(_, class)| *class)
            .or_else(|| {
                entries
                    .get(CLASS_BASE_ENTRY)
                    .and_then(|entry| class_by_base.get(&entry.pool).copied())
            })
            .unwrap_or(3);
        abilities.authored = !STOCK_LIST_CLASSES.iter().any(|(list, _)| *list == index);
        options.insert(index, (abilities, class));
    }
    for (item_index, list_index, equipment_class) in item_socket_lists {
        let Some(item) = items.get_mut(item_index) else {
            continue;
        };
        if item.bucket_hash != super::SUBCLASS_BUCKET_HASH {
            continue;
        }
        if let Some((abilities, class)) = options.get(&list_index) {
            item.abilities = abilities.clone();
            item.class_type = equipment_class.map(u64::from).unwrap_or(*class);
        }
    }
    Ok(())
}

/// An entry's pool and the offset of its active variant. Follows official Sunrise's ability pool
/// reader: the last variant is active unless a single-group pool's entry carries a selector.
fn active_variant(
    manager: &PackageManager,
    list: &[u8],
    entry: usize,
) -> Result<Option<(Vec<u8>, usize)>, String> {
    let (_, rows, _) = array_at(list, 16)?;
    let base = rows + entry * 64;
    let pool = manager.read_tag(TagHash(u32_at(list, base + 56)?))?;
    let group = relative_offset(16, 0, i64_at(&pool, 16)?)? + 16;
    let variants = i32_at(&pool, group)?;
    if variants <= 0 {
        return Ok(None);
    }
    let selector_rel = i64_at(list, base + 32)?;
    let selector = if selector_rel == 0 {
        255
    } else {
        *list
            .get(relative_offset(base + 32, 0, selector_rel)?)
            .ok_or("selector outside the list")?
    };
    let variant = if i64_at(&pool, 8)? == 1 && selector != 255 {
        18 % variants
    } else {
        variants - 1
    };
    let variant = relative_offset(group + 8, 0, i64_at(&pool, group + 8)?)?
        + 16
        + usize::try_from(variant).map_err(|error| error.to_string())? * 88;
    Ok(Some((pool, variant)))
}

/// Whether an entry's active pool record declares a bucket kind of its own.
fn declares_kind(manager: &PackageManager, list: &[u8], entry: usize) -> bool {
    (|| -> Result<bool, String> {
        let Some((pool, variant)) = active_variant(manager, list, entry)? else {
            return Ok(false);
        };
        if i32_at(&pool, variant)? <= 0 {
            return Ok(false);
        }
        let records = relative_offset(variant + 8, 0, i64_at(&pool, variant + 8)?)? + 16;
        Ok(pool.get(records + 11).is_some_and(|kind| *kind != 255))
    })()
    .unwrap_or(false)
}

/// Each ability row's entity: the definition table's pattern hash for the row, through the entity
/// assignment table. A pool record names its ability by row.
pub(in crate::catalog) fn scan_ability_entities(
    manager: &PackageManager,
    globals: &[u8],
) -> Vec<Option<u32>> {
    (|| -> Result<Vec<Option<u32>>, String> {
        let definitions = manager.read_tag(TagHash(u32_at(
            globals,
            16 + crate::ability::definition::DEFINITION_TABLE_SLOT * 16,
        )?))?;
        let assignments = manager.read_tag(TagHash(
            crate::entity::SANDBOX_PATTERN_ENTITY_ASSIGNMENT_TAG,
        ))?;
        crate::ability::definition::patterns(&definitions)?
            .into_iter()
            .map(|pattern| crate::entity::weapon_entity_assignment(&assignments, pattern))
            .collect()
    })()
    .unwrap_or_default()
}

/// A pool record's hash, the row it applies the hash to, and the row it equips.
const RECORD_KEY_ROW: usize = 4;
const RECORD_EQUIPPED_ROW: usize = 11;
const NO_ROW: u8 = 0xFF;
/// The hash a record holds when it files no key, FNV-1's offset basis.
const NO_KEY: u32 = 0x811C_9DC5;

/// An entry's active pool variant's records, sixteen bytes each.
fn active_records(
    manager: &PackageManager,
    list: &[u8],
    entry: usize,
) -> Result<Vec<[u8; 16]>, String> {
    let Some((pool, variant)) = active_variant(manager, list, entry)? else {
        return Ok(Vec::new());
    };
    let count = i32_at(&pool, variant)?;
    if count <= 0 {
        return Ok(Vec::new());
    }
    let records = relative_offset(variant + 8, 0, i64_at(&pool, variant + 8)?)? + 16;
    (0..usize::try_from(count).map_err(|error| error.to_string())?)
        .map(|index| {
            let at = records + index * 16;
            pool.get(at..at + 16)
                .and_then(|record| record.try_into().ok())
                .ok_or_else(|| "record outside its pool".to_owned())
        })
        .collect()
}

/// An entry's ability entity: the entity of the first row its active pool equips (+0xB) that has
/// one, as authoring picks it.
fn entry_entity(
    manager: &PackageManager,
    list: &[u8],
    entry: usize,
    ability_entities: &[Option<u32>],
) -> Option<u32> {
    active_records(manager, list, entry)
        .ok()?
        .iter()
        .find_map(|record| {
            ability_entities
                .get(usize::from(record[RECORD_EQUIPPED_ROW]))
                .copied()
                .flatten()
        })
}

/// An entry's ability row: the first row its active pool equips.
fn entry_row(manager: &PackageManager, list: &[u8], entry: usize) -> Option<u8> {
    active_records(manager, list, entry)
        .ok()?
        .iter()
        .map(|record| record[RECORD_EQUIPPED_ROW])
        .find(|row| *row != NO_ROW)
}

/// Each key an entry's active pool applies to an ability without equipping one, with the row
/// it applies it to, as Dawn files it into that ability's bucket.
fn entry_modifiers(manager: &PackageManager, list: &[u8], entry: usize) -> Vec<(u32, u8)> {
    let mut modifiers = Vec::new();
    for record in active_records(manager, list, entry).unwrap_or_default() {
        let key = u32::from_le_bytes([record[0], record[1], record[2], record[3]]);
        let row = record[RECORD_KEY_ROW];
        if record[RECORD_EQUIPPED_ROW] == NO_ROW
            && row != NO_ROW
            && !matches!(key, 0 | u32::MAX | NO_KEY)
            && !modifiers.contains(&(key, row))
        {
            modifiers.push((key, row));
        }
    }
    modifiers
}

/// Each ability row's bank as modifiers see it: whether it takes extra charges, its script
/// parameters and the keys of its property rows. A row without an entity or a bank has none.
pub(in crate::catalog) fn scan_ability_rows(
    manager: &PackageManager,
    ability_entities: &[Option<u32>],
) -> Vec<crate::investment::AbilityRowSummary> {
    use crate::ability::bank::{self, Modifier};
    use crate::investment::{AbilityKey, AbilityParameter, AbilityRowSummary};
    let parameter = |parameter: &bank::Parameter| AbilityParameter {
        name: parameter.name,
        reset: parameter.reset,
        applied: parameter.applied,
        add: parameter.add,
    };
    ability_entities
        .iter()
        .enumerate()
        .filter_map(|(row, entity)| {
            let row = u8::try_from(row).ok()?;
            let mut summary = AbilityRowSummary {
                row,
                entity: *entity,
                ..AbilityRowSummary::default()
            };
            let bank_tag = entity
                .and_then(|entity| manager.read_tag(TagHash(entity)).ok())
                .and_then(|payload| {
                    crate::ability::modifier::entity_bank(&payload)
                        .ok()
                        .flatten()
                });
            if let Some(bank_tag) = bank_tag
                && let Ok(payload) = manager.read_tag(TagHash(bank_tag))
            {
                summary.bank = Some(bank_tag);
                summary.slot = crate::ability::modifier::bank_slot(bank_tag);
                summary.charges = bank::handler_slot(&payload, Modifier::Charges(1))
                    .ok()
                    .flatten()
                    .is_some();
                summary.recharge = crate::ability::modifier::takes_recharge(&payload);
                summary.parameters = crate::ability::modifier::settable_parameters(&payload)
                    .unwrap_or_default()
                    .iter()
                    .map(parameter)
                    .collect();
                summary.keys = bank::property_rows(&payload)
                    .unwrap_or_default()
                    .iter()
                    .map(|row| AbilityKey {
                        key: row.key,
                        charges: row.charge,
                        parameters: row.parameters.iter().map(parameter).collect(),
                    })
                    .collect();
            }
            Some(summary)
        })
        .collect()
}

/// The sandbox perks an entry's active pool variant grants.
fn entry_perks(manager: &PackageManager, list: &[u8], entry: usize) -> Vec<u16> {
    (|| -> Result<Vec<u16>, String> {
        let Some((pool, variant)) = active_variant(manager, list, entry)? else {
            return Ok(Vec::new());
        };
        let descriptor = variant + VARIANT_PERK_ARRAY;
        let count = i64_at(&pool, descriptor)?;
        if count <= 0 {
            return Ok(Vec::new());
        }
        let rows = relative_offset(descriptor + 8, 0, i64_at(&pool, descriptor + 8)?)? + 16;
        (0..usize::try_from(count).map_err(|error| error.to_string())?)
            .map(|index| {
                pool.get(rows + index * 2..rows + index * 2 + 2)
                    .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
                    .ok_or_else(|| "perk outside its pool".to_owned())
            })
            .collect()
    })()
    .unwrap_or_default()
}

fn parse_abilities(list: &[u8], display: &AbilityDisplayData, middle_super: u64) -> AbilityOptions {
    let Ok((count, rows, _)) = array_at(list, 16) else {
        return AbilityOptions::default();
    };
    let mut entries = Vec::new();
    let mut entry_icons = BTreeMap::new();
    let mut entry_descriptions = BTreeMap::new();
    for index in 0..count.min(64) {
        let base = rows + index * 64;
        let Ok(display_hash) = u32_at(list, base) else {
            break;
        };
        if let Some(&icon) = display.icons.get(&display_hash) {
            entry_icons.insert(index as u64, icon);
        }
        if let Some(description) = display.descriptions.get(&display_hash) {
            entry_descriptions.insert(index as u64, description.clone());
        }
        let Ok(plug_source) = u32_at(list, base + 8) else {
            break;
        };
        let Some(&group) = list.get(base + 12) else {
            break;
        };
        let name = display
            .names
            .get(&display_hash)
            .cloned()
            .unwrap_or_else(|| format!("Unknown ability (0x{display_hash:08X})"));
        entries.push(ParsedAbilityEntry {
            choice: AbilityChoice {
                entry: index as u64,
                name,
            },
            plug_source,
            group,
        });
    }
    let choices = |indices: &[usize]| -> Vec<AbilityChoice> {
        indices
            .iter()
            .filter_map(|&index| entries.get(index).map(|entry| entry.choice.clone()))
            .collect()
    };
    let attunements = parse_attunements(
        &entries,
        (&display.attunement_names, &display.path_names),
        middle_super,
    );
    let mut super_ability = attunements
        .iter()
        .flat_map(|attunement| attunement.super_abilities.iter().cloned())
        .collect::<Vec<_>>();
    let mut seen_super_entries = Vec::new();
    super_ability.retain(|choice| {
        if seen_super_entries.contains(&choice.entry) {
            false
        } else {
            seen_super_entries.push(choice.entry);
            true
        }
    });
    let melee = attunements
        .iter()
        .map(|attunement| attunement.melee.clone())
        .collect();
    AbilityOptions {
        class_ability: choices(&[2, 3]),
        movement: choices(&[4, 5, 6]),
        grenade: choices(&[7, 8, 9]),
        super_ability,
        melee,
        attunements,
        entry_perks: BTreeMap::new(),
        entry_entities: BTreeMap::new(),
        entry_rows: BTreeMap::new(),
        entry_modifiers: BTreeMap::new(),
        entry_icons,
        entry_descriptions,
        authored: false,
    }
}

fn parse_attunements(
    entries: &[ParsedAbilityEntry],
    (names, path_names): (&[String], &HashMap<u32, String>),
    middle_super: u64,
) -> Vec<AttunementChoice> {
    let mut sources = Vec::<u32>::new();
    for entry in entries {
        if entry.group == 3
            && entry.plug_source != sundial_account::NO_DEFINITION_HASH.get()
            && !sources.contains(&entry.plug_source)
        {
            sources.push(entry.plug_source);
        }
    }
    sources
        .into_iter()
        .enumerate()
        .filter_map(|(path_index, source)| {
            let perks = entries
                .iter()
                .filter(|entry| entry.group == 3 && entry.plug_source == source)
                .map(|entry| entry.choice.clone())
                .collect::<Vec<_>>();
            let melee = if perks.first().is_some_and(|choice| choice.entry == 20) {
                perks.get(1)
            } else {
                perks.first()
            }?
            .clone();
            // The middle-tree display is entry 20 even when the native bucket
            // selection must remain at entry 10 (Sentinel and Arcstrider).
            let super_ability = if path_index == 2 {
                entries
                    .get(20)
                    .filter(|entry| entry.plug_source == source)
                    .map(|entry| AbilityChoice {
                        entry: middle_super,
                        name: entry.choice.name.clone(),
                    })
            } else {
                entries.get(10).map(|entry| entry.choice.clone())
            };
            let super_abilities = super_ability.into_iter().collect();
            let name = path_names
                .get(&source)
                .or_else(|| names.get(path_index))
                .cloned()
                .unwrap_or_else(|| match path_index {
                    0 => "Top path".into(),
                    1 => "Bottom path".into(),
                    _ => "Middle path".into(),
                });
            Some(AttunementChoice {
                name,
                super_abilities,
                melee,
                perks,
            })
        })
        .collect()
}

pub(in crate::catalog) fn scan_ability_displays(
    manager: &PackageManager,
    globals: &[u8],
    localized_tags: &[TagHash],
    localized_cache: &mut LocalizedStringCache,
    icon_containers: &[Option<u32>],
) -> Result<HashMap<u16, AbilityDisplayData>, String> {
    // This parallel display catalogue is an investment-globals child. Its row
    // index is the socket-list index, so discovering records by class/hash order
    // loses the package-authored relationship. Stock ships 14 rows, and each
    // authored subclass list adds one.
    const STOCK_ABILITY_DISPLAY_TABLE_COUNT: usize = 14;
    const ABILITY_DISPLAY_TABLE_CLASS: u32 = 0x8080_5C3C;
    const ABILITY_DISPLAY_RECORD_CLASS: u32 = 0x8080_5C42;

    let table_tag = TagHash(u32_at(
        globals,
        16 + GLOBALS_SUBCLASS_DISPLAY_TABLE_SLOT * 16,
    )?);
    let table_entry = manager
        .get_entry(table_tag)
        .ok_or_else(|| format!("Subclass ability display table {table_tag:?} is not live"))?;
    if table_entry.reference != ABILITY_DISPLAY_TABLE_CLASS {
        return Err(format!(
            "Subclass ability display table {table_tag:?} has class 0x{:08X}, expected 0x{ABILITY_DISPLAY_TABLE_CLASS:08X}",
            table_entry.reference
        ));
    }
    let table_index = manager
        .read_tag(table_tag)
        .map_err(|error| format!("Could not read subclass ability display table: {error}"))?;
    let (table_count, table_rows, _) = array_at(&table_index, 8)?;
    if table_count < STOCK_ABILITY_DISPLAY_TABLE_COUNT {
        return Err(format!(
            "Subclass ability display table has {table_count} rows; expected at least {STOCK_ABILITY_DISPLAY_TABLE_COUNT}"
        ));
    }

    // The lore display rows each attunement is shown by. Without them, names fall back to the
    // path titles in the node records' string banks.
    let lore_rows = u32_at(globals, 16 + LORE_DISPLAY_TABLE_SLOT * 16)
        .ok()
        .and_then(|tag| manager.read_tag(TagHash(tag)).ok());
    let mut result = HashMap::new();
    for row in 0..table_count {
        let Ok(list_id) = u16::try_from(row) else {
            break;
        };
        // Slot 61 is a standard 24-byte index table: the display-record tag
        // is at row +0x10, while row +0x00 is the definition/string hash.
        let tag = TagHash(u32_at(&table_index, table_rows + row * 24 + 16)?);
        let entry = manager.get_entry(tag).ok_or_else(|| {
            format!("Subclass socket list {list_id} display record {tag:?} is not live")
        })?;
        // The empty and cut-down lists carry no subclass display record.
        if entry.reference != ABILITY_DISPLAY_RECORD_CLASS {
            continue;
        }
        let mut names = HashMap::new();
        let mut icons = HashMap::new();
        let mut descriptions = HashMap::new();
        let mut localized_indices = Vec::new();
        let table = manager.read_tag(tag).map_err(|error| {
            format!("Could not read subclass socket list {list_id} display record: {error}")
        })?;
        for offset in (16..table.len()).step_by(4) {
            let Ok(raw_tag) = u32_at(&table, offset) else {
                continue;
            };
            let candidate = TagHash(raw_tag);
            if manager
                .get_entry(candidate)
                .is_none_or(|entry| entry.reference != 0x8080_5C49)
            {
                continue;
            }
            let Ok(display_hash) = u32_at(&table, offset - 16) else {
                continue;
            };
            let Ok(display) = manager.read_tag(candidate) else {
                continue;
            };
            if let Ok(index) = u32_at(&display, NODE_DISPLAY_NAME)
                && (index as usize) < localized_tags.len()
                && !localized_indices.contains(&index)
            {
                localized_indices.push(index);
            }
            if let Some(name) = resolve_string(
                manager,
                localized_tags,
                localized_cache,
                &display,
                NODE_DISPLAY_NAME,
            ) {
                names.entry(display_hash).or_insert(name);
            }
            if let Some(description) = resolve_string(
                manager,
                localized_tags,
                localized_cache,
                &display,
                NODE_DISPLAY_DESCRIPTION,
            )
            .filter(|description| !description.trim().is_empty())
            {
                descriptions.entry(display_hash).or_insert(description);
            }
            if let Ok(row) = u32_at(&display, NODE_DISPLAY_ICON)
                && let Some(container) = usize::try_from(row)
                    .ok()
                    .and_then(|row| icon_containers.get(row).copied().flatten())
            {
                icons.entry(display_hash).or_insert(container);
            }
        }
        // These three hashes are the native localized titles for the top,
        // bottom and Forsaken middle subclass paths. Their string banks are
        // identified by the entry display records above, so no game text is
        // embedded in Sundial.
        let attunement_names = [0xDF41_7340, 0x7308_73A5, 0x761A_F51A]
            .into_iter()
            .filter_map(|hash| {
                resolve_localized_hash(
                    manager,
                    localized_tags,
                    localized_cache,
                    &localized_indices,
                    hash,
                )
            })
            .collect();
        let path_names = lore_rows
            .as_deref()
            .map(|lore| path_names(manager, localized_tags, localized_cache, &table, lore))
            .unwrap_or_default();
        result.insert(
            list_id,
            AbilityDisplayData {
                names,
                attunement_names,
                path_names,
                icons,
                descriptions,
            },
        );
    }
    Ok(result)
}

/// Each attunement's name by its lead plug source, from the lore row the display record names.
fn path_names(
    manager: &PackageManager,
    localized_tags: &[TagHash],
    localized_cache: &mut LocalizedStringCache,
    display: &[u8],
    lore: &[u8],
) -> HashMap<u32, String> {
    let Ok((count, rows, _)) = array_at(display, DISPLAY_PATH_ARRAY) else {
        return HashMap::new();
    };
    let Ok((lore_count, lore_rows, _)) = array_at(lore, 8) else {
        return HashMap::new();
    };
    (0..count.min(8))
        .filter_map(|index| {
            let row = rows + index * 8;
            let source = u32_at(display, row).ok()?;
            let lore_row = usize::try_from(u32_at(display, row + 4).ok()?).ok()?;
            if lore_row >= lore_count {
                return None;
            }
            let lore_row = lore_rows + lore_row * LORE_DISPLAY_ROW_SIZE;
            let name = resolve_string(
                manager,
                localized_tags,
                localized_cache,
                lore,
                lore_row + LORE_DISPLAY_NAME,
            )?;
            Some((source, name))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires SUNDIAL_INSTALL pointing to the supported Shadowkeep build"]
    fn native_subclass_super_lanes() {
        let install = std::path::PathBuf::from(std::env::var("SUNDIAL_INSTALL").unwrap());
        let manager = crate::package_runtime::open_shadowkeep_packages(&install).unwrap();
        let globals =
            crate::package_runtime::resolve_live_named_tag(&manager, "investment_globals", None)
                .unwrap();
        let globals = manager.read_tag(globals).unwrap();
        let root = manager
            .read_tag(TagHash(u32_at(&globals, 16).unwrap()))
            .unwrap();
        let table = manager
            .read_tag(TagHash(u32_at(&root, 8 + 97 * 16).unwrap()))
            .unwrap();
        let (_, rows, _) = array_at(&table, 8).unwrap();
        for index in [1, 2, 3, 5, 6, 7, 9, 10, 11] {
            let list = manager
                .read_tag(TagHash(u32_at(&table, rows + index * 24 + 16).unwrap()))
                .unwrap();
            let (_, rows, _) = array_at(&list, 16).unwrap();
            let middle_super = if matches!(index, 1 | 6) { 10 } else { 20 };
            let choices = parse_abilities(&list, &AbilityDisplayData::default(), middle_super);
            assert_eq!(choices.attunements.len(), 3);
            let pairs = choices
                .attunements
                .iter()
                .map(|path| (path.super_abilities[0].entry, path.melee.entry))
                .collect::<Vec<_>>();
            assert_eq!(pairs, [(10, 11), (10, 15), (middle_super, 21)]);
            for entry in [10, 20] {
                let base = rows + entry * 64;
                // Match official Sunrise 0.3.2 ability_pool_reader.cpp's active
                // variant selection. A hash-only record cannot assign a bucket.
                use crate::package_payload::{i32_at, i64_at, relative_offset};
                let pool = manager
                    .read_tag(TagHash(u32_at(&list, base + 56).unwrap()))
                    .unwrap();
                let group = relative_offset(16, 0, i64_at(&pool, 16).unwrap()).unwrap() + 16;
                let variants = i32_at(&pool, group).unwrap();
                let selector_rel = i64_at(&list, base + 32).unwrap();
                let selector = if selector_rel != 0 {
                    list[relative_offset(base + 32, 0, selector_rel).unwrap()]
                } else {
                    255
                };
                let variant = if i64_at(&pool, 8).unwrap() == 1 && selector != 255 {
                    18 % variants
                } else {
                    variants - 1
                };
                let variant_field =
                    relative_offset(group + 8, 0, i64_at(&pool, group + 8).unwrap()).unwrap()
                        + 24
                        + variant as usize * 88;
                let records =
                    relative_offset(variant_field, 0, i64_at(&pool, variant_field).unwrap())
                        .unwrap()
                        + 16;
                assert!(i32_at(&pool, variant_field - 8).unwrap() > 0);
                if entry == 20 && middle_super == 10 {
                    assert_eq!(
                        pool[records + 11],
                        255,
                        "guard entry must be hash-only: list {index}"
                    );
                } else {
                    assert_ne!(
                        pool[records + 11],
                        255,
                        "selected entry must declare a kind: list {index}"
                    );
                    assert!(
                        pool[records + 12] == 1 || pool[records + 8] == 10,
                        "super must reach bucket 1: list {index}"
                    );
                }
            }
        }
    }
}

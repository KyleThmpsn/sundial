//! Shadowkeep subclass ability and attunement decoding.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tiger_pkg::{PackageManager, TagHash};

use crate::{
    investment_localization::{LocalizedStringCache, resolve_localized_hash, resolve_string},
    package_payload::{array_at, u32_at},
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
}

#[derive(Clone)]
struct ParsedAbilityEntry {
    choice: AbilityChoice,
    plug_source: u32,
    group: u8,
}

pub(in crate::catalog) fn build_subclass_choices(
    manager: &PackageManager,
    root: &[u8],
    ability_displays: &HashMap<u16, AbilityDisplayData>,
    item_socket_lists: Vec<(usize, u16)>,
    items: &mut [ItemDef],
) -> Result<(), String> {
    let list_table = manager
        .read_tag(TagHash(u32_at(root, 8 + 97 * 16)?))
        .map_err(|error| format!("Could not read subclass ability table: {error}"))?;
    let (list_count, list_rows, _) = array_at(&list_table, 8)?;
    for (item_index, list_index) in item_socket_lists {
        let Some(item) = items.get_mut(item_index) else {
            continue;
        };
        if item.bucket_hash != 3_284_755_031 {
            continue;
        }
        if usize::from(list_index) >= list_count {
            continue;
        }
        let list_tag = TagHash(u32_at(
            &list_table,
            list_rows + usize::from(list_index) * 24 + 16,
        )?);
        if let Ok(list) = manager.read_tag(list_tag)
            && let Some(display) = ability_displays.get(&list_index)
        {
            let middle_super = crate::subclass::shadowkeep_subclass_rules(item.hash)
                .map_or(20, |(_, entry)| entry);
            item.abilities = parse_abilities(&list, display, middle_super);
            item.class_type = match list_index {
                1..=3 => 1,  // Hunter
                5..=7 => 0,  // Titan
                9..=11 => 2, // Warlock
                _ => 3,
            };
        }
    }
    Ok(())
}

fn parse_abilities(list: &[u8], display: &AbilityDisplayData, middle_super: u64) -> AbilityOptions {
    let Ok((count, rows, _)) = array_at(list, 16) else {
        return AbilityOptions::default();
    };
    let mut entries = Vec::new();
    for index in 0..count.min(64) {
        let base = rows + index * 64;
        let Ok(display_hash) = u32_at(list, base) else {
            break;
        };
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
    let attunements = parse_attunements(&entries, &display.attunement_names, middle_super);
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
    }
}

fn parse_attunements(
    entries: &[ParsedAbilityEntry],
    names: &[String],
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
            let name = names
                .get(path_index)
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
) -> Result<HashMap<u16, AbilityDisplayData>, String> {
    // This parallel display catalogue is an investment-globals child. Its row
    // index is the socket-list index, so discovering records by class/hash order
    // loses the package-authored relationship.
    const SUBCLASS_LIST_IDS: [u16; 9] = [1, 2, 3, 5, 6, 7, 9, 10, 11];
    const ABILITY_DISPLAY_TABLE_COUNT: usize = 14;
    const ABILITY_DISPLAY_TABLE_CLASS: u32 = 0x8080_5C3C;
    const ABILITY_DISPLAY_RECORD_CLASS: u32 = 0x8080_5C42;

    let table_tag = TagHash(u32_at(globals, 16 + 61 * 16)?);
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
    if table_count != ABILITY_DISPLAY_TABLE_COUNT {
        return Err(format!(
            "Subclass ability display table has {table_count} rows; expected {ABILITY_DISPLAY_TABLE_COUNT}"
        ));
    }

    let mut result = HashMap::new();
    for list_id in SUBCLASS_LIST_IDS {
        // Slot 61 is a standard 24-byte index table: the display-record tag
        // is at row +0x10, while row +0x00 is the definition/string hash.
        let tag = TagHash(u32_at(
            &table_index,
            table_rows + usize::from(list_id) * 24 + 16,
        )?);
        let entry = manager.get_entry(tag).ok_or_else(|| {
            format!("Subclass socket list {list_id} display record {tag:?} is not live")
        })?;
        if entry.reference != ABILITY_DISPLAY_RECORD_CLASS {
            return Err(format!(
                "Subclass socket list {list_id} display record {tag:?} has class 0x{:08X}, expected 0x{ABILITY_DISPLAY_RECORD_CLASS:08X}",
                entry.reference
            ));
        }
        let mut names = HashMap::new();
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
            if let Ok(index) = u32_at(&display, 160)
                && (index as usize) < localized_tags.len()
                && !localized_indices.contains(&index)
            {
                localized_indices.push(index);
            }
            if let Some(name) =
                resolve_string(manager, localized_tags, localized_cache, &display, 160)
            {
                names.entry(display_hash).or_insert(name);
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
        result.insert(
            list_id,
            AbilityDisplayData {
                names,
                attunement_names,
            },
        );
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires SUNDIAL_TEST_INSTALL pointing to the supported Shadowkeep build"]
    fn native_subclass_super_lanes() {
        let install = std::path::PathBuf::from(std::env::var("SUNDIAL_TEST_INSTALL").unwrap());
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

    fn entries() -> Vec<ParsedAbilityEntry> {
        let mut entries = (0..24)
            .map(|entry| ParsedAbilityEntry {
                choice: AbilityChoice {
                    entry,
                    name: format!("Entry {entry}"),
                },
                plug_source: sundial_account::NO_DEFINITION_HASH.get(),
                group: u8::MAX,
            })
            .collect::<Vec<_>>();
        for (range, source) in [(11..15, 1), (15..19, 2), (20..24, 3)] {
            for index in range {
                entries[index].plug_source = source;
                entries[index].group = 3;
            }
        }
        entries
    }

    #[test]
    fn attunements_keep_super_and_melee_in_the_same_native_path() {
        let paths = parse_attunements(
            &entries(),
            &["Sky".into(), "Flame".into(), "Grace".into()],
            20,
        );
        assert_eq!(paths.len(), 3);
        assert_eq!(paths[0].melee.entry, 11);
        assert_eq!(paths[1].melee.entry, 15);
        assert_eq!(paths[2].melee.entry, 21);
        assert_eq!(
            paths[1]
                .super_abilities
                .iter()
                .map(|choice| choice.entry)
                .collect::<Vec<_>>(),
            vec![10]
        );
        assert_eq!(paths[1].super_abilities[0].name, "Entry 10");
        assert_eq!(paths[2].super_abilities[0].entry, 20);
    }

    #[test]
    fn distinct_middle_super_attunements_use_the_native_entries() {
        let paths = parse_attunements(
            &entries(),
            &["Top".into(), "Bottom".into(), "Middle".into()],
            20,
        );
        let pairs = paths
            .iter()
            .map(|path| (path.super_abilities[0].entry, path.melee.entry))
            .collect::<Vec<_>>();

        assert_eq!(pairs, vec![(10, 11), (10, 15), (20, 21)]);
    }

    #[test]
    fn guard_attunements_keep_the_base_super_with_the_middle_tree_display() {
        let paths = parse_attunements(&entries(), &[], 10);
        let pairs = paths
            .iter()
            .map(|path| (path.super_abilities[0].entry, path.melee.entry))
            .collect::<Vec<_>>();
        assert_eq!(pairs, vec![(10, 11), (10, 15), (10, 21)]);
        assert_eq!(paths[2].super_abilities[0].name, "Entry 20");
    }
}

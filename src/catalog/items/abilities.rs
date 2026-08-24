//! Shadowkeep subclass ability and attunement decoding.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tiger_pkg::{PackageManager, TagHash};

use super::{
    super::{
        localization::{LocalizedStringCache, resolve_localized_hash, resolve_string},
        package::{array_at, u32_at},
    },
    ItemDef,
};

const NO_PLUG_SOURCE: u32 = 0x811C_9DC5;

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
            item.abilities = parse_abilities(&list, display, list_index);
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

fn parse_abilities(list: &[u8], display: &AbilityDisplayData, list_index: u16) -> AbilityOptions {
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
    let attunements = parse_attunements(&entries, &display.attunement_names, list_index);
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
    list_index: u16,
) -> Vec<AttunementChoice> {
    let mut sources = Vec::<u32>::new();
    for entry in entries {
        if entry.group == 3
            && entry.plug_source != NO_PLUG_SOURCE
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
            let matching_super = super_entry_indices(list_index).iter().find_map(|&index| {
                let entry = entries.get(index)?;
                (entry.plug_source == source).then(|| entry.choice.clone())
            });
            // The top and bottom paths select the base super lane at entry 10.
            // Most Forsaken middle paths carry a distinct super at entry 20,
            // but Arcstrider and Sentinel route their guard super through the
            // path selected by the melee entry and keep the base super lane.
            let super_ability = if path_index == 2 && !middle_path_uses_base_super(list_index) {
                matching_super
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

const fn middle_path_uses_base_super(list_index: u16) -> bool {
    matches!(list_index, 1 | 6)
}

const fn super_entry_indices(list_index: u16) -> &'static [usize] {
    match list_index {
        // Arcstrider
        1 => &[10, 14, 20],
        // Gunslinger: Golden Gun, Deadshot/Six-Shooter and precision-tree
        // modifiers, plus Blade Barrage.
        2 => &[10, 13, 14, 17, 18, 20],
        // Nightstalker
        3 => &[10, 13, 18, 20],
        // Striker, Sentinel, and Voidwalker
        5 | 6 | 10 => &[10, 14, 18, 20],
        // Sunbreaker
        7 => &[10, 13, 14, 18, 20],
        // Dawnblade
        9 => &[10, 16, 17, 18, 20],
        // Stormcaller
        11 => &[10, 12, 14, 16, 20],
        _ => &[10, 20],
    }
}

pub(in crate::catalog) fn scan_ability_displays(
    manager: &PackageManager,
    localized_tags: &[TagHash],
    localized_cache: &mut LocalizedStringCache,
) -> HashMap<u16, AbilityDisplayData> {
    // Shadowkeep's nine subclass socket lists are sparse. The display tables
    // are stored in descending socket-list order; list IDs 4 and 8 are not
    // subclass definitions.
    const SUBCLASS_LIST_IDS: [u16; 9] = [11, 10, 9, 7, 6, 5, 3, 2, 1];

    let mut tables: Vec<TagHash> = manager
        .get_all_by_reference(0x8080_5C42)
        .into_iter()
        .map(|(tag, _)| tag)
        .filter(|tag| manager.read_tag(*tag).is_ok_and(|data| data.len() > 700))
        .collect();
    tables.sort_by_key(|tag| tag.0);
    if tables.len() > 9 {
        tables = tables.split_off(tables.len() - 9);
    }
    let mut result = HashMap::new();
    for (list_id, tag) in SUBCLASS_LIST_IDS.into_iter().zip(tables) {
        let mut names = HashMap::new();
        let mut localized_indices = Vec::new();
        let Ok(table) = manager.read_tag(tag) else {
            continue;
        };
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
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries() -> Vec<ParsedAbilityEntry> {
        let mut entries = (0..24)
            .map(|entry| ParsedAbilityEntry {
                choice: AbilityChoice {
                    entry,
                    name: format!("Entry {entry}"),
                },
                plug_source: NO_PLUG_SOURCE,
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
    fn super_choices_include_gunslinger_and_dawnblade_alternates() {
        assert!(super_entry_indices(2).contains(&13)); // Deadshot
        assert!(super_entry_indices(9).contains(&20)); // Well of Radiance
    }

    #[test]
    fn attunements_keep_super_and_melee_in_the_same_native_path() {
        let paths = parse_attunements(
            &entries(),
            &["Sky".into(), "Flame".into(), "Grace".into()],
            9,
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
    fn all_shadowkeep_attunements_use_the_native_super_and_melee_entries() {
        let entries = entries();
        for (list_index, middle_super) in [
            (1, 10),
            (2, 20),
            (3, 20),
            (5, 20),
            (6, 10),
            (7, 20),
            (9, 20),
            (10, 20),
            (11, 20),
        ] {
            let paths = parse_attunements(
                &entries,
                &["Top".into(), "Bottom".into(), "Middle".into()],
                list_index,
            );
            let pairs = paths
                .iter()
                .map(|path| (path.super_abilities[0].entry, path.melee.entry))
                .collect::<Vec<_>>();
            assert_eq!(
                pairs,
                vec![(10, 11), (10, 15), (middle_super, 21)],
                "socket list {list_index}"
            );
        }
    }
}

//! Investment-stat definitions and item stat allocation decoding.

use serde::{Deserialize, Serialize};
use tiger_pkg::{PackageManager, TagHash};

use super::super::{
    Catalog,
    localization::{LocalizedStringCache, resolve_string},
    package::{array_at, i32_at, i64_at, relative_offset, u16_at, u32_at},
};

const INVESTMENT_STAT_CLASS: u32 = 0x8080_3033;
const INVESTMENT_STAT_RESOURCE_CLASS: u32 = 0x8080_77B9;
const INVESTMENT_STAT_DEFINITION_CLASS: u32 = 0x8080_7D09;
const STAT_STRING_MAP_CLASS: u32 = 0x8080_5CC9;
const INVESTMENT_STAT_DESCRIPTOR: usize = 0x2C0;
const INVESTMENT_STAT_ROW_SIZE: usize = 40;
const STAT_STRING_MAP_INDEX: usize = 59;
const STAT_STRING_ROW_SIZE: usize = 36;
const STAT_ICON_INDEX_OFFSET: usize = 20;
const STAT_DEFINITION_TABLE_SLOT: usize = 95;
const STAT_DEFINITION_ROW_SIZE: usize = 32;
const ITEM_INVESTMENT_STAT_POINTER_OFFSET: usize = 0x70;
const ARMOR_STAT_NAMES: [&str; 6] = [
    "Mobility",
    "Resilience",
    "Recovery",
    "Discipline",
    "Intellect",
    "Strength",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ItemInvestmentStat {
    pub definition_index: u16,
    pub value: i32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct ItemStatDefinition {
    pub definition_index: u16,
    pub hash: u64,
    pub name: String,
    #[serde(default)]
    pub icon_container: Option<u32>,
}

impl Catalog {
    pub(crate) fn item_rarity(&self, hash: u64) -> super::ItemRarity {
        self.item_package_metadata
            .get(&hash)
            .map_or(super::ItemRarity::Unknown, |metadata| metadata.rarity)
    }

    pub(crate) fn item_stat_definition(
        &self,
        definition_index: u16,
    ) -> Option<&ItemStatDefinition> {
        self.item_stat_definitions
            .get(usize::from(definition_index))
            .filter(|definition| definition.definition_index == definition_index)
    }

    pub(crate) fn item_stat_definition_by_hash(&self, hash: u64) -> Option<&ItemStatDefinition> {
        self.item_stat_definitions
            .iter()
            .find(|definition| definition.hash == hash)
    }

    pub(crate) fn item_investment_stat_references(
        &self,
        hash: u64,
    ) -> Vec<(u64, &ItemInvestmentStat)> {
        let Some(definition_index) = self
            .item_stat_definition_by_hash(hash)
            .map(|definition| definition.definition_index)
        else {
            return Vec::new();
        };
        let mut references = self
            .item_package_metadata
            .iter()
            .flat_map(|(&item_hash, metadata)| {
                metadata
                    .investment_stats
                    .iter()
                    .filter(move |stat| stat.definition_index == definition_index)
                    .map(move |stat| (item_hash, stat))
            })
            .collect::<Vec<_>>();
        references.sort_by(|(first_hash, _), (second_hash, _)| {
            self.package_item_name(*first_hash)
                .unwrap_or("")
                .to_lowercase()
                .cmp(
                    &self
                        .package_item_name(*second_hash)
                        .unwrap_or("")
                        .to_lowercase(),
                )
                .then_with(|| first_hash.cmp(second_hash))
        });
        references
    }

    /// Returns the six armor-stat contributions authored on an item or plug.
    /// Values come from the installed package definitions and may be negative.
    pub(crate) fn armor_stat_values(&self, hash: u64) -> [i32; 6] {
        let mut values = [0_i32; 6];
        let Some(metadata) = self.item_package_metadata.get(&hash) else {
            return values;
        };
        for stat in &metadata.investment_stats {
            let Some(definition) = self.item_stat_definition(stat.definition_index) else {
                continue;
            };
            let Some(index) = ARMOR_STAT_NAMES
                .iter()
                .position(|name| name.eq_ignore_ascii_case(definition.name.trim()))
            else {
                continue;
            };
            values[index] = values[index].saturating_add(stat.value);
        }
        values
    }
}

pub(in crate::catalog) fn scan_stat_definitions(
    manager: &PackageManager,
    root: &[u8],
    globals: &[u8],
    localized_tags: &[TagHash],
    localized_cache: &mut LocalizedStringCache,
    icon_containers_by_index: &[Option<u32>],
) -> Vec<ItemStatDefinition> {
    let Ok(definition_tag) = u32_at(root, 8 + STAT_DEFINITION_TABLE_SLOT * 16) else {
        return Vec::new();
    };
    let Ok(definition_table) = manager.read_tag(TagHash(definition_tag)) else {
        return Vec::new();
    };
    let Ok((definition_count, definition_rows, definition_class)) = array_at(&definition_table, 8)
    else {
        return Vec::new();
    };
    let Ok(string_tag) = u32_at(globals, 16 + STAT_STRING_MAP_INDEX * 16) else {
        return Vec::new();
    };
    let Ok(string_table) = manager.read_tag(TagHash(string_tag)) else {
        return Vec::new();
    };
    let Ok((string_count, string_rows, string_class)) = array_at(&string_table, 8) else {
        return Vec::new();
    };
    if definition_class != INVESTMENT_STAT_DEFINITION_CLASS
        || string_class != STAT_STRING_MAP_CLASS
        || definition_count != string_count
        || definition_count > 256
    {
        return Vec::new();
    }
    (0..definition_count)
        .map(|index| {
            let definition_row = definition_rows + index * STAT_DEFINITION_ROW_SIZE;
            let string_row = string_rows + index * STAT_STRING_ROW_SIZE;
            let definition_hash = u32_at(&definition_table, definition_row).ok()?;
            if u32_at(&string_table, string_row).ok()? != definition_hash {
                return None;
            }
            let icon_index = u16_at(&string_table, string_row + STAT_ICON_INDEX_OFFSET).ok()?;
            Some(ItemStatDefinition {
                definition_index: u16::try_from(index).ok()?,
                hash: u64::from(definition_hash),
                name: resolve_string(
                    manager,
                    localized_tags,
                    localized_cache,
                    &string_table,
                    string_row + 4,
                )
                .unwrap_or_default(),
                icon_container: (icon_index != u16::MAX)
                    .then(|| {
                        icon_containers_by_index
                            .get(usize::from(icon_index))
                            .copied()
                            .flatten()
                    })
                    .flatten(),
            })
        })
        .collect::<Option<Vec<_>>>()
        .unwrap_or_default()
}

pub(in crate::catalog) fn item_investment_stats(
    item: &[u8],
    definitions: &[ItemStatDefinition],
) -> Vec<ItemInvestmentStat> {
    let Some(resource) = item_investment_resource(item) else {
        return Vec::new();
    };
    let Ok((count, rows, class)) = array_at(item, resource) else {
        return Vec::new();
    };
    if class != INVESTMENT_STAT_CLASS || count > 256 {
        return Vec::new();
    }
    (0..count)
        .filter_map(|row_index| {
            let row = rows.checked_add(row_index.checked_mul(INVESTMENT_STAT_ROW_SIZE)?)?;
            let definition_index = u16_at(item, row).ok()?;
            definitions.get(usize::from(definition_index))?;
            Some(ItemInvestmentStat {
                definition_index,
                value: i32_at(item, row + 4).ok()?,
            })
        })
        .collect()
}

pub(super) fn item_investment_resource(item: &[u8]) -> Option<usize> {
    let Ok(relative) = i64_at(item, ITEM_INVESTMENT_STAT_POINTER_OFFSET) else {
        return None;
    };
    if relative == 0 {
        return None;
    }
    let Ok(resource) = relative_offset(ITEM_INVESTMENT_STAT_POINTER_OFFSET, 0, relative) else {
        return None;
    };
    if resource < 4 || u32_at(item, resource - 4).ok() != Some(INVESTMENT_STAT_RESOURCE_CLASS) {
        return None;
    }
    Some(resource)
}

pub(in crate::catalog) fn stat_allocation_labels(
    item: &[u8],
    stat_names: &[String],
) -> Option<(String, &'static str)> {
    let (count, rows, class) = array_at(item, INVESTMENT_STAT_DESCRIPTOR).ok()?;
    if class != INVESTMENT_STAT_CLASS || count != 3 {
        return None;
    }

    let mut stats = [(0_usize, 0_u32); 3];
    for (row_index, stat) in stats.iter_mut().enumerate() {
        let row = rows.checked_add(row_index.checked_mul(INVESTMENT_STAT_ROW_SIZE)?)?;
        *stat = (
            usize::from(u16_at(item, row).ok()?),
            u32_at(item, row + 4).ok()?,
        );
    }

    let (expected_indexes, type_name) = if stats.iter().all(|(index, _)| (3..=5).contains(index)) {
        ([3, 4, 5], "Top Stat Allocation")
    } else if stats.iter().all(|(index, _)| (6..=8).contains(index)) {
        ([6, 7, 8], "Bottom Stat Allocation")
    } else {
        return None;
    };

    let mut parts = Vec::with_capacity(3);
    for expected_index in expected_indexes {
        let value = stats
            .iter()
            .find_map(|(index, value)| (*index == expected_index).then_some(*value))?;
        if value == 0 || value > 100 {
            return None;
        }
        let stat_name = stat_names.get(expected_index)?.trim();
        if stat_name.is_empty() {
            return None;
        }
        parts.push(format!("{value} {stat_name}"));
    }

    Some((parts.join(" / "), type_name))
}

pub(in crate::catalog) fn masterwork_label(
    item: &[u8],
    stat_names: &[String],
    current_name: &str,
) -> Option<String> {
    let is_full_masterwork = current_name == "Masterwork";
    let is_item_tier = current_name.starts_with("Tier ")
        && (current_name.ends_with(" Weapon") || current_name.ends_with(" Armor"));
    if !is_full_masterwork && !is_item_tier {
        return None;
    }

    let (count, rows, class) = array_at(item, INVESTMENT_STAT_DESCRIPTOR).ok()?;
    if class != INVESTMENT_STAT_CLASS || !(1..=4).contains(&count) {
        return None;
    }
    let primary_stat = usize::from(u16_at(item, rows).ok()?);
    let name = stat_names.get(primary_stat)?.trim();
    if name.is_empty() {
        return None;
    }
    Some(if is_full_masterwork {
        format!("Masterwork: {name}")
    } else {
        format!("{current_name}: {name}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unnamed_armor_stat_plugs_use_their_local_investment_values() {
        let mut item = vec![0_u8; 0x300 + INVESTMENT_STAT_ROW_SIZE * 3];
        let count = 3_u64;
        item[INVESTMENT_STAT_DESCRIPTOR..INVESTMENT_STAT_DESCRIPTOR + 8]
            .copy_from_slice(&count.to_le_bytes());
        item[INVESTMENT_STAT_DESCRIPTOR + 8..INVESTMENT_STAT_DESCRIPTOR + 16]
            .copy_from_slice(&(0x28_i64).to_le_bytes());
        item[0x2F0..0x2F8].copy_from_slice(&count.to_le_bytes());
        item[0x2F8..0x2FC].copy_from_slice(&INVESTMENT_STAT_CLASS.to_le_bytes());

        for (row, stat_index, value) in [(0, 5_u16, 7_u32), (1, 3, 13), (2, 4, 1)] {
            let offset = 0x300 + row * INVESTMENT_STAT_ROW_SIZE;
            item[offset..offset + 2].copy_from_slice(&stat_index.to_le_bytes());
            item[offset + 4..offset + 8].copy_from_slice(&value.to_le_bytes());
        }
        let stat_names = vec![
            String::new(),
            String::new(),
            String::new(),
            "Mobility".into(),
            "Resilience".into(),
            "Recovery".into(),
        ];

        assert_eq!(
            stat_allocation_labels(&item, &stat_names),
            Some((
                "13 Mobility / 1 Resilience / 7 Recovery".into(),
                "Top Stat Allocation"
            ))
        );
    }

    #[test]
    fn item_stats_follow_the_typed_item_resource() {
        let mut item = vec![0_u8; 0x1A0];
        item[ITEM_INVESTMENT_STAT_POINTER_OFFSET..ITEM_INVESTMENT_STAT_POINTER_OFFSET + 8]
            .copy_from_slice(&(0x90_i64).to_le_bytes());
        item[0xFC..0x100].copy_from_slice(&INVESTMENT_STAT_RESOURCE_CLASS.to_le_bytes());
        item[0x100..0x108].copy_from_slice(&2_u64.to_le_bytes());
        item[0x108..0x110].copy_from_slice(&(0x38_i64).to_le_bytes());
        item[0x140..0x148].copy_from_slice(&2_u64.to_le_bytes());
        item[0x148..0x14C].copy_from_slice(&INVESTMENT_STAT_CLASS.to_le_bytes());
        item[0x150..0x152].copy_from_slice(&1_u16.to_le_bytes());
        item[0x154..0x158].copy_from_slice(&67_i32.to_le_bytes());
        item[0x178..0x17A].copy_from_slice(&0_u16.to_le_bytes());
        item[0x17C..0x180].copy_from_slice(&(-12_i32).to_le_bytes());
        let definitions = vec![
            ItemStatDefinition {
                definition_index: 0,
                hash: 10,
                name: "Impact".into(),
                icon_container: None,
            },
            ItemStatDefinition {
                definition_index: 1,
                hash: 20,
                name: "Range".into(),
                icon_container: None,
            },
        ];

        assert_eq!(
            item_investment_stats(&item, &definitions),
            vec![
                ItemInvestmentStat {
                    definition_index: 1,
                    value: 67,
                },
                ItemInvestmentStat {
                    definition_index: 0,
                    value: -12,
                },
            ]
        );

        item[0xFC..0x100].copy_from_slice(&0_u32.to_le_bytes());
        assert!(item_investment_stats(&item, &definitions).is_empty());
    }

    #[test]
    fn masterwork_labels_use_the_primary_local_stat_name() {
        const ROW_SIZE: usize = 48;
        let mut item = vec![0_u8; 0x300 + ROW_SIZE * 2];
        let count = 2_u64;
        item[INVESTMENT_STAT_DESCRIPTOR..INVESTMENT_STAT_DESCRIPTOR + 8]
            .copy_from_slice(&count.to_le_bytes());
        item[INVESTMENT_STAT_DESCRIPTOR + 8..INVESTMENT_STAT_DESCRIPTOR + 16]
            .copy_from_slice(&(0x28_i64).to_le_bytes());
        item[0x2F0..0x2F8].copy_from_slice(&count.to_le_bytes());
        item[0x2F8..0x2FC].copy_from_slice(&INVESTMENT_STAT_CLASS.to_le_bytes());
        item[0x300..0x302].copy_from_slice(&2_u16.to_le_bytes());
        item[0x300 + ROW_SIZE..0x302 + ROW_SIZE].copy_from_slice(&1_u16.to_le_bytes());
        let stat_names = vec![String::new(), "Impact".into(), "Charge Time".into()];

        assert_eq!(
            masterwork_label(&item, &stat_names, "Masterwork").as_deref(),
            Some("Masterwork: Charge Time")
        );
        assert_eq!(
            masterwork_label(&item, &stat_names, "Tier 7 Weapon").as_deref(),
            Some("Tier 7 Weapon: Charge Time")
        );
        let armor_stat_names = vec![
            String::new(),
            "Heroic Resistance".into(),
            "Arc Damage Resistance".into(),
        ];
        assert_eq!(
            masterwork_label(&item, &armor_stat_names, "Tier 4 Armor").as_deref(),
            Some("Tier 4 Armor: Arc Damage Resistance")
        );
        assert_eq!(
            masterwork_label(&item, &stat_names, "Masterwork Weapon"),
            None
        );
        assert_eq!(
            masterwork_label(&item, &stat_names[..2], "Masterwork"),
            None
        );
    }
}

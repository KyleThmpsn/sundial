//! Investment-stat definitions and item stat allocation decoding.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use tiger_pkg::{PackageManager, TagHash};

use crate::{
    investment_localization::{LocalizedStringCache, resolve_string},
    investment_schema::{
        ITEM_INVESTMENT_STAT_POINTER_OFFSET, ITEM_INVESTMENT_STAT_RESOURCE_CLASS,
        ITEM_INVESTMENT_STAT_ROW_CLASS, ITEM_INVESTMENT_STAT_ROW_SIZE,
        ITEM_STRING_STAT_GROUP_INDEX_OFFSET, ITEM_STRING_STAT_GROUP_POINTER_OFFSET,
        ITEM_STRING_STAT_GROUP_RESOURCE_CLASS,
    },
    package_payload::{array_at, i32_at, i64_at, relative_offset, u16_at, u32_at},
};

use super::super::Catalog;

const INVESTMENT_STAT_DEFINITION_CLASS: u32 = 0x8080_7D09;
const STAT_STRING_MAP_CLASS: u32 = 0x8080_5CC9;
const INVESTMENT_STAT_DESCRIPTOR: usize = 0x2C0;
const STAT_STRING_MAP_INDEX: usize = 59;
const STAT_GROUP_MAP_INDEX: usize = 60;
const STAT_STRING_ROW_SIZE: usize = 36;
const STAT_ICON_INDEX_OFFSET: usize = 20;
const STAT_DEFINITION_TABLE_SLOT: usize = 95;
const STAT_DEFINITION_ROW_SIZE: usize = 32;
const STAT_GROUP_CLASS: u32 = 0x8080_5D02;
const STAT_GROUP_ROW_SIZE: usize = 0x38;
const STAT_GROUP_SCALED_STATS_OFFSET: usize = 0x10;
const STAT_GROUP_MAXIMUM_VALUE_OFFSET: usize = 0x30;
const SCALED_STAT_CLASS: u32 = 0x8080_5D06;
const SCALED_STAT_ROW_SIZE: usize = 0x18;
const SCALED_STAT_INTERPOLATION_OFFSET: usize = 0x08;
const STAT_INTERPOLATION_CLASS: u32 = 0x8080_7D1A;
const STAT_INTERPOLATION_ROW_SIZE: usize = 0x08;
const ARMOR_STAT_NAMES: [&str; 6] = [
    "Mobility",
    "Resilience",
    "Recovery",
    "Discipline",
    "Intellect",
    "Strength",
];
const ROUNDS_PER_MINUTE_HASH: u64 = 0xFF66_4809;

pub(crate) trait InvestmentStatDisplayPoint {
    fn investment_value(&self) -> i32;
    fn display_value(&self) -> i32;
}

/// Applies the installed stat group's historical display curve to a stored investment value.
///
/// This is the shared Sundial implementation used by both the Inspector and Parhelion. Exact
/// authored points take precedence, linear groups preserve unlisted values, and non-linear groups
/// interpolate inside their decoded domain while clamping to its endpoints outside it.
pub(crate) fn interpolate_investment_stat_display<T: InvestmentStatDisplayPoint>(
    points: &[T],
    is_linear: bool,
    investment_value: i32,
) -> Option<i32> {
    if let Some(point) = points
        .iter()
        .find(|point| point.investment_value() == investment_value)
    {
        return Some(point.display_value());
    }
    if is_linear {
        return Some(investment_value);
    }
    let first = points.first()?;
    if investment_value < first.investment_value() {
        return Some(first.display_value());
    }
    let last = points.last()?;
    if investment_value > last.investment_value() {
        return Some(last.display_value());
    }
    for pair in points.windows(2) {
        let [left, right] = pair else {
            continue;
        };
        if investment_value < left.investment_value() || investment_value > right.investment_value()
        {
            continue;
        }
        let investment_span = right.investment_value() - left.investment_value();
        if investment_span == 0 {
            return Some(right.display_value());
        }
        let position =
            f64::from(investment_value - left.investment_value()) / f64::from(investment_span);
        let display = f64::from(left.display_value())
            + position * f64::from(right.display_value() - left.display_value());
        return Some(display.round_ties_even() as i32);
    }
    None
}

pub(crate) fn format_in_game_investment_stat(
    definition_hash: Option<u64>,
    display_value: i32,
) -> String {
    if definition_hash == Some(ROUNDS_PER_MINUTE_HASH) {
        format!("{display_value} RPM")
    } else {
        display_value.to_string()
    }
}

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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ItemStatDisplayPoint {
    pub investment_value: i32,
    pub display_value: i32,
}

impl InvestmentStatDisplayPoint for ItemStatDisplayPoint {
    fn investment_value(&self) -> i32 {
        self.investment_value
    }

    fn display_value(&self) -> i32 {
        self.display_value
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ItemScaledStat {
    pub definition_index: u16,
    pub display_as_numeric: bool,
    pub is_linear: bool,
    pub display_interpolation: Vec<ItemStatDisplayPoint>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ItemStatGroup {
    pub hash: u64,
    pub maximum_value: i32,
    pub scaled_stats: Vec<ItemScaledStat>,
}

impl ItemStatGroup {
    pub(crate) fn minimum_value(&self, definition_index: u16) -> Option<i32> {
        self.scaled_stats
            .iter()
            .find(|stat| stat.definition_index == definition_index)?
            .display_interpolation
            .iter()
            .map(|point| point.investment_value)
            .min()
    }
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

    pub(crate) fn item_scaled_stat(
        &self,
        item_hash: u64,
        definition_index: u16,
    ) -> Option<&ItemScaledStat> {
        self.item_stat_group(item_hash)?
            .scaled_stats
            .iter()
            .find(|stat| stat.definition_index == definition_index)
    }

    pub(crate) fn item_stat_group(&self, item_hash: u64) -> Option<&ItemStatGroup> {
        let group_index = self
            .item_package_metadata
            .get(&item_hash)?
            .stat_group_index?;
        self.item_stat_group_by_index(group_index)
    }

    pub(crate) fn item_stat_group_by_index(&self, group_index: u16) -> Option<&ItemStatGroup> {
        stat_group_by_index(&self.item_stat_groups, group_index)
    }

    pub(crate) fn item_in_game_stat_display(
        &self,
        item_hash: u64,
        stat: &ItemInvestmentStat,
    ) -> String {
        let definition_hash = self
            .item_stat_definition(stat.definition_index)
            .map(|definition| definition.hash);
        let display_value = self
            .item_scaled_stat(item_hash, stat.definition_index)
            .and_then(|scaled| {
                interpolate_investment_stat_display(
                    &scaled.display_interpolation,
                    scaled.is_linear,
                    stat.value,
                )
            })
            .unwrap_or(stat.value);
        format_in_game_investment_stat(definition_hash, display_value)
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

fn stat_group_by_index(groups: &[ItemStatGroup], group_index: u16) -> Option<&ItemStatGroup> {
    groups.get(usize::from(group_index))
}

pub(in crate::catalog) fn scan_stat_definitions(
    manager: &PackageManager,
    root: &[u8],
    globals: &[u8],
    localized_tags: &[TagHash],
    localized_cache: &mut LocalizedStringCache,
    icon_containers_by_index: &[Option<u32>],
) -> Result<Vec<ItemStatDefinition>, String> {
    let definition_tag = u32_at(root, 8 + STAT_DEFINITION_TABLE_SLOT * 16)
        .map_err(|error| format!("Could not read the investment stat-definition tag: {error}"))?;
    let definition_table = manager
        .read_tag(TagHash(definition_tag))
        .map_err(|error| format!("Could not read the investment stat-definition table: {error}"))?;
    let (definition_count, definition_rows, definition_class) = array_at(&definition_table, 8)
        .map_err(|error| {
            format!("Could not decode the investment stat-definition table: {error}")
        })?;
    let string_tag = u32_at(globals, 16 + STAT_STRING_MAP_INDEX * 16)
        .map_err(|error| format!("Could not read the investment stat-string tag: {error}"))?;
    let string_table = manager
        .read_tag(TagHash(string_tag))
        .map_err(|error| format!("Could not read the investment stat-string table: {error}"))?;
    let (string_count, string_rows, string_class) = array_at(&string_table, 8)
        .map_err(|error| format!("Could not decode the investment stat-string table: {error}"))?;
    if definition_class != INVESTMENT_STAT_DEFINITION_CLASS
        || string_class != STAT_STRING_MAP_CLASS
        || definition_count != string_count
        || definition_count > 256
    {
        return Err(format!(
            "Investment stat tables have incompatible layouts (definitions: {definition_count} rows, class 0x{definition_class:08X}; strings: {string_count} rows, class 0x{string_class:08X})"
        ));
    }

    let mut definitions = Vec::with_capacity(definition_count);
    for index in 0..definition_count {
        let definition_row = definition_rows
            .checked_add(
                index
                    .checked_mul(STAT_DEFINITION_ROW_SIZE)
                    .ok_or("Investment stat-definition row overflowed")?,
            )
            .ok_or("Investment stat-definition row overflowed")?;
        let string_row = string_rows
            .checked_add(
                index
                    .checked_mul(STAT_STRING_ROW_SIZE)
                    .ok_or("Investment stat-string row overflowed")?,
            )
            .ok_or("Investment stat-string row overflowed")?;
        let definition_hash = u32_at(&definition_table, definition_row).map_err(|error| {
            format!("Investment stat-definition row {index} is malformed: {error}")
        })?;
        let string_hash = u32_at(&string_table, string_row)
            .map_err(|error| format!("Investment stat-string row {index} is malformed: {error}"))?;
        if string_hash != definition_hash {
            return Err(format!(
                "Investment stat row {index} maps definition 0x{definition_hash:08X} to string 0x{string_hash:08X}"
            ));
        }
        let icon_offset = string_row
            .checked_add(STAT_ICON_INDEX_OFFSET)
            .ok_or("Investment stat icon offset overflowed")?;
        let localized_offset = string_row
            .checked_add(4)
            .ok_or("Investment stat localization offset overflowed")?;
        let icon_index = u16_at(&string_table, icon_offset)
            .map_err(|error| format!("Investment stat-string row {index} is malformed: {error}"))?;
        definitions.push(ItemStatDefinition {
            definition_index: u16::try_from(index)
                .map_err(|_| "Investment stat-definition index exceeds u16")?,
            hash: u64::from(definition_hash),
            name: resolve_string(
                manager,
                localized_tags,
                localized_cache,
                &string_table,
                localized_offset,
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
        });
    }
    Ok(definitions)
}

pub(in crate::catalog) fn scan_stat_groups(
    manager: &PackageManager,
    globals: &[u8],
) -> Result<Vec<ItemStatGroup>, String> {
    let group_tag = u32_at(globals, 16 + STAT_GROUP_MAP_INDEX * 16)
        .map_err(|error| format!("Could not read the investment stat-group tag: {error}"))?;
    let group_table = manager
        .read_tag(TagHash(group_tag))
        .map_err(|error| format!("Could not read the investment stat-group table: {error}"))?;
    decode_stat_groups(&group_table)
        .map_err(|error| format!("Could not decode the investment stat-group table: {error}"))
}

fn decode_stat_groups(table: &[u8]) -> Result<Vec<ItemStatGroup>, String> {
    let (group_count, group_rows, group_class) = array_at(table, 8)?;
    if group_class != STAT_GROUP_CLASS || group_count > 256 {
        return Err("Invalid investment stat-group table".into());
    }

    let mut groups = Vec::with_capacity(group_count);
    for group_index in 0..group_count {
        let group_row = group_rows
            .checked_add(
                group_index
                    .checked_mul(STAT_GROUP_ROW_SIZE)
                    .ok_or("Investment stat-group row overflowed")?,
            )
            .ok_or("Investment stat-group row overflowed")?;
        let (scaled_count, scaled_rows, scaled_class) = array_at(
            table,
            group_row
                .checked_add(STAT_GROUP_SCALED_STATS_OFFSET)
                .ok_or("Investment scaled-stat descriptor overflowed")?,
        )?;
        if scaled_count > 256 || (scaled_count != 0 && scaled_class != SCALED_STAT_CLASS) {
            return Err("Invalid scaled-stat table".into());
        }

        let mut scaled_stats = Vec::with_capacity(scaled_count);
        for scaled_index in 0..scaled_count {
            let scaled_row = scaled_rows
                .checked_add(
                    scaled_index
                        .checked_mul(SCALED_STAT_ROW_SIZE)
                        .ok_or("Scaled-stat row overflowed")?,
                )
                .ok_or("Scaled-stat row overflowed")?;
            let definition_index = u16::from(
                *table
                    .get(scaled_row)
                    .ok_or("Scaled-stat definition index is truncated")?,
            );
            let display_as_numeric = *table
                .get(scaled_row + 1)
                .ok_or("Scaled-stat numeric flag is truncated")?
                == 1;
            let is_linear = *table
                .get(scaled_row + 3)
                .ok_or("Scaled-stat linear flag is truncated")?
                == 1;
            let (point_count, point_rows, point_class) = array_at(
                table,
                scaled_row
                    .checked_add(SCALED_STAT_INTERPOLATION_OFFSET)
                    .ok_or("Stat interpolation descriptor overflowed")?,
            )?;
            if point_count > 256 || (point_count != 0 && point_class != STAT_INTERPOLATION_CLASS) {
                return Err("Invalid stat interpolation table".into());
            }
            let mut display_interpolation = Vec::with_capacity(point_count);
            for point_index in 0..point_count {
                let point_row = point_rows
                    .checked_add(
                        point_index
                            .checked_mul(STAT_INTERPOLATION_ROW_SIZE)
                            .ok_or("Stat interpolation row overflowed")?,
                    )
                    .ok_or("Stat interpolation row overflowed")?;
                display_interpolation.push(ItemStatDisplayPoint {
                    // The curve compares the first value against the stored investment input and
                    // returns the second value for display. For RPM this is 0 -> 360 through
                    // 100 -> 720; reversing these axes incorrectly makes 360 the minimum raw RPM.
                    investment_value: i32_at(table, point_row)?,
                    display_value: i32_at(table, point_row + 4)?,
                });
            }
            scaled_stats.push(ItemScaledStat {
                definition_index,
                display_as_numeric,
                is_linear,
                display_interpolation,
            });
        }
        groups.push(ItemStatGroup {
            hash: u64::from(u32_at(table, group_row)?),
            maximum_value: i32_at(table, group_row + STAT_GROUP_MAXIMUM_VALUE_OFFSET)?,
            scaled_stats,
        });
    }
    Ok(groups)
}

pub(in crate::catalog) fn item_stat_group_index(string_definition: &[u8]) -> Option<u16> {
    let relative = i64_at(string_definition, ITEM_STRING_STAT_GROUP_POINTER_OFFSET)
        .ok()
        .filter(|relative| *relative != 0)?;
    let resource = relative_offset(ITEM_STRING_STAT_GROUP_POINTER_OFFSET, 0, relative).ok()?;
    if resource < 4
        || u32_at(string_definition, resource - 4).ok()
            != Some(ITEM_STRING_STAT_GROUP_RESOURCE_CLASS)
    {
        return None;
    }
    let index = i32_at(
        string_definition,
        resource.checked_add(ITEM_STRING_STAT_GROUP_INDEX_OFFSET)?,
    )
    .ok()?;
    u16::try_from(index).ok()
}

pub(in crate::catalog) fn item_investment_stats(
    item: &[u8],
    definitions: &[ItemStatDefinition],
) -> Result<Vec<ItemInvestmentStat>, ()> {
    let Some(resource) = checked_item_investment_resource(item)? else {
        return Ok(Vec::new());
    };
    // Native perk-only items omit the stat array with an all-zero descriptor.
    // Following its null pointer would read the adjacent perk count as a row class.
    if item.get(resource..resource.checked_add(16).ok_or(())?) == Some(&[0; 16]) {
        return Ok(Vec::new());
    }
    let (count, rows, class) = array_at(item, resource).map_err(|_| ())?;
    if class != ITEM_INVESTMENT_STAT_ROW_CLASS || count > 256 {
        return Err(());
    }
    let mut seen = BTreeSet::new();
    let mut stats = Vec::with_capacity(count);
    for row_index in 0..count {
        let row = rows
            .checked_add(
                row_index
                    .checked_mul(ITEM_INVESTMENT_STAT_ROW_SIZE)
                    .ok_or(())?,
            )
            .ok_or(())?;
        let definition_index = u16::from(*item.get(row).ok_or(())?);
        // The native row stores an 8-bit stat index followed by a reserved byte. Do not
        // reinterpret the pair as a u16 index; malformed rows are not safe authoring inputs.
        if *item.get(row.checked_add(1).ok_or(())?).ok_or(())? != 0
            || definitions.get(usize::from(definition_index)).is_none()
            || !seen.insert(definition_index)
        {
            return Err(());
        }
        stats.push(ItemInvestmentStat {
            definition_index,
            value: i32_at(item, row.checked_add(4).ok_or(())?).map_err(|_| ())?,
        });
    }
    Ok(stats)
}

pub(super) fn item_investment_resource(item: &[u8]) -> Option<usize> {
    checked_item_investment_resource(item).ok().flatten()
}

fn checked_item_investment_resource(item: &[u8]) -> Result<Option<usize>, ()> {
    let relative = i64_at(item, ITEM_INVESTMENT_STAT_POINTER_OFFSET).map_err(|_| ())?;
    if relative == 0 {
        return Ok(None);
    }
    let resource =
        relative_offset(ITEM_INVESTMENT_STAT_POINTER_OFFSET, 0, relative).map_err(|_| ())?;
    if resource < 4
        || u32_at(item, resource - 4).map_err(|_| ())? != ITEM_INVESTMENT_STAT_RESOURCE_CLASS
    {
        return Err(());
    }
    Ok(Some(resource))
}

pub(in crate::catalog) fn stat_allocation_labels(
    item: &[u8],
    stat_names: &[String],
) -> Option<(String, &'static str)> {
    let (count, rows, class) = array_at(item, INVESTMENT_STAT_DESCRIPTOR).ok()?;
    if class != ITEM_INVESTMENT_STAT_ROW_CLASS || count != 3 {
        return None;
    }

    let mut stats = [(0_usize, 0_u32); 3];
    for (row_index, stat) in stats.iter_mut().enumerate() {
        let row = rows.checked_add(row_index.checked_mul(ITEM_INVESTMENT_STAT_ROW_SIZE)?)?;
        if *item.get(row.checked_add(1)?)? != 0 {
            return None;
        }
        *stat = (usize::from(*item.get(row)?), u32_at(item, row + 4).ok()?);
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
    if class != ITEM_INVESTMENT_STAT_ROW_CLASS || !(1..=4).contains(&count) {
        return None;
    }
    if *item.get(rows.checked_add(1)?)? != 0 {
        return None;
    }
    let primary_stat = usize::from(*item.get(rows)?);
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
        let mut item = vec![0_u8; 0x300 + ITEM_INVESTMENT_STAT_ROW_SIZE * 3];
        let count = 3_u64;
        item[INVESTMENT_STAT_DESCRIPTOR..INVESTMENT_STAT_DESCRIPTOR + 8]
            .copy_from_slice(&count.to_le_bytes());
        item[INVESTMENT_STAT_DESCRIPTOR + 8..INVESTMENT_STAT_DESCRIPTOR + 16]
            .copy_from_slice(&(0x28_i64).to_le_bytes());
        item[0x2F0..0x2F8].copy_from_slice(&count.to_le_bytes());
        item[0x2F8..0x2FC].copy_from_slice(&ITEM_INVESTMENT_STAT_ROW_CLASS.to_le_bytes());

        for (row, stat_index, value) in [(0, 5_u16, 7_u32), (1, 3, 13), (2, 4, 1)] {
            let offset = 0x300 + row * ITEM_INVESTMENT_STAT_ROW_SIZE;
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

        item[0x301] = 1;
        assert_eq!(stat_allocation_labels(&item, &stat_names), None);
    }

    #[test]
    fn item_stats_follow_the_typed_item_resource() {
        let mut item = vec![0_u8; 0x1A0];
        item[ITEM_INVESTMENT_STAT_POINTER_OFFSET..ITEM_INVESTMENT_STAT_POINTER_OFFSET + 8]
            .copy_from_slice(&(0x90_i64).to_le_bytes());
        item[0xFC..0x100].copy_from_slice(&ITEM_INVESTMENT_STAT_RESOURCE_CLASS.to_le_bytes());
        item[0x100..0x108].copy_from_slice(&2_u64.to_le_bytes());
        item[0x108..0x110].copy_from_slice(&(0x38_i64).to_le_bytes());
        item[0x140..0x148].copy_from_slice(&2_u64.to_le_bytes());
        item[0x148..0x14C].copy_from_slice(&ITEM_INVESTMENT_STAT_ROW_CLASS.to_le_bytes());
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
            Ok(vec![
                ItemInvestmentStat {
                    definition_index: 1,
                    value: 67,
                },
                ItemInvestmentStat {
                    definition_index: 0,
                    value: -12,
                },
            ])
        );

        item[0xFC..0x100].copy_from_slice(&0_u32.to_le_bytes());
        assert!(item_investment_stats(&item, &definitions).is_err());

        item[ITEM_INVESTMENT_STAT_POINTER_OFFSET..ITEM_INVESTMENT_STAT_POINTER_OFFSET + 8].fill(0);
        assert_eq!(item_investment_stats(&item, &definitions), Ok(Vec::new()));
    }

    #[test]
    fn perk_only_items_can_omit_the_stat_array_without_being_malformed() {
        let mut item = vec![0_u8; 0x140];
        item[ITEM_INVESTMENT_STAT_POINTER_OFFSET..ITEM_INVESTMENT_STAT_POINTER_OFFSET + 8]
            .copy_from_slice(&0x90_i64.to_le_bytes());
        item[0xFC..0x100].copy_from_slice(&ITEM_INVESTMENT_STAT_RESOURCE_CLASS.to_le_bytes());
        for perk_count in 1_u64..=4 {
            item[0x110..0x118].copy_from_slice(&perk_count.to_le_bytes());
            assert_eq!(item_investment_stats(&item, &[]), Ok(Vec::new()));
        }
        item[0x100] = 1; // A nonempty array with a null pointer is still malformed.
        assert!(item_investment_stats(&item, &[]).is_err());
        item[0x100] = 0;
        item[0xFC..0x100].fill(0);
        assert!(item_investment_stats(&item, &[]).is_err());
    }

    #[test]
    fn item_stat_indices_are_u8_and_require_a_zero_reserved_byte() {
        let mut item = vec![0_u8; 0x180];
        item[ITEM_INVESTMENT_STAT_POINTER_OFFSET..ITEM_INVESTMENT_STAT_POINTER_OFFSET + 8]
            .copy_from_slice(&(0x90_i64).to_le_bytes());
        item[0xFC..0x100].copy_from_slice(&ITEM_INVESTMENT_STAT_RESOURCE_CLASS.to_le_bytes());
        item[0x100..0x108].copy_from_slice(&1_u64.to_le_bytes());
        item[0x108..0x110].copy_from_slice(&(0x18_i64).to_le_bytes());
        item[0x120..0x128].copy_from_slice(&1_u64.to_le_bytes());
        item[0x128..0x12C].copy_from_slice(&ITEM_INVESTMENT_STAT_ROW_CLASS.to_le_bytes());
        item[0x130] = 1;
        item[0x131] = 0xA5;
        item[0x134..0x138].copy_from_slice(&25_i32.to_le_bytes());
        let definitions = vec![
            ItemStatDefinition::default(),
            ItemStatDefinition {
                definition_index: 1,
                ..ItemStatDefinition::default()
            },
        ];

        assert!(item_investment_stats(&item, &definitions).is_err());
        item[0x131] = 0;
        assert_eq!(
            item_investment_stats(&item, &definitions),
            Ok(vec![ItemInvestmentStat {
                definition_index: 1,
                value: 25,
            }])
        );

        item[0x130] = 9;
        assert!(item_investment_stats(&item, &definitions).is_err());
    }

    #[test]
    fn stat_group_minimum_is_scoped_to_the_selected_stat() {
        let group = ItemStatGroup {
            hash: 0,
            maximum_value: 100,
            scaled_stats: vec![
                ItemScaledStat {
                    definition_index: 14,
                    display_interpolation: vec![ItemStatDisplayPoint {
                        investment_value: 10,
                        display_value: 360,
                    }],
                    ..ItemScaledStat::default()
                },
                ItemScaledStat {
                    definition_index: 15,
                    display_interpolation: vec![ItemStatDisplayPoint {
                        investment_value: -20,
                        display_value: 0,
                    }],
                    ..ItemScaledStat::default()
                },
            ],
        };

        assert_eq!(group.minimum_value(14), Some(10));
        assert_eq!(group.minimum_value(15), Some(-20));
        assert_eq!(group.minimum_value(16), None);
    }

    #[test]
    fn item_stat_group_index_follows_the_typed_string_resource() {
        let mut definition = vec![0_u8; 0x130];
        definition
            [ITEM_STRING_STAT_GROUP_POINTER_OFFSET..ITEM_STRING_STAT_GROUP_POINTER_OFFSET + 8]
            .copy_from_slice(&(0x70_i64).to_le_bytes());
        definition[0xDC..0xE0]
            .copy_from_slice(&ITEM_STRING_STAT_GROUP_RESOURCE_CLASS.to_le_bytes());
        definition[0xF4..0xF8].copy_from_slice(&21_i32.to_le_bytes());

        assert_eq!(item_stat_group_index(&definition), Some(21));

        definition[0xDC..0xE0].copy_from_slice(&0_u32.to_le_bytes());
        assert_eq!(item_stat_group_index(&definition), None);
    }

    #[test]
    fn stat_groups_decode_native_numeric_and_interpolation_fields() {
        let mut table = vec![0_u8; 0xC0];
        write_array_descriptor(&mut table, 8, 1, 0x20, STAT_GROUP_CLASS);
        table[0x30..0x34].copy_from_slice(&0xEAEF_4EA1_u32.to_le_bytes());
        table[0x60..0x64].copy_from_slice(&100_i32.to_le_bytes());
        write_array_descriptor(&mut table, 0x40, 1, 0x70, SCALED_STAT_CLASS);
        table[0x80] = 14;
        table[0x81] = 1;
        table[0x83] = 0;
        write_array_descriptor(&mut table, 0x88, 2, 0xA0, STAT_INTERPOLATION_CLASS);
        table[0xB0..0xB4].copy_from_slice(&20_i32.to_le_bytes());
        table[0xB4..0xB8].copy_from_slice(&450_i32.to_le_bytes());
        table[0xB8..0xBC].copy_from_slice(&80_i32.to_le_bytes());
        table[0xBC..0xC0].copy_from_slice(&600_i32.to_le_bytes());

        assert_eq!(
            decode_stat_groups(&table).unwrap(),
            vec![ItemStatGroup {
                hash: 0xEAEF_4EA1,
                maximum_value: 100,
                scaled_stats: vec![ItemScaledStat {
                    definition_index: 14,
                    display_as_numeric: true,
                    is_linear: false,
                    display_interpolation: vec![
                        ItemStatDisplayPoint {
                            investment_value: 20,
                            display_value: 450,
                        },
                        ItemStatDisplayPoint {
                            investment_value: 80,
                            display_value: 600,
                        },
                    ],
                }],
            }]
        );
    }

    #[test]
    fn stat_groups_accept_native_null_arrays() {
        let mut table = vec![0_u8; 0x68];
        write_array_descriptor(&mut table, 8, 1, 0x20, STAT_GROUP_CLASS);
        table[0x30..0x34].copy_from_slice(&0xC3C9_6257_u32.to_le_bytes());
        table[0x60..0x64].copy_from_slice(&10_i32.to_le_bytes());

        assert_eq!(
            decode_stat_groups(&table).unwrap(),
            vec![ItemStatGroup {
                hash: 0xC3C9_6257,
                maximum_value: 10,
                scaled_stats: Vec::new(),
            }]
        );
    }

    #[test]
    fn stat_groups_resolve_only_decoded_indices() {
        let groups = vec![
            ItemStatGroup {
                hash: 1,
                maximum_value: 10,
                scaled_stats: Vec::new(),
            },
            ItemStatGroup {
                hash: 2,
                maximum_value: 100,
                scaled_stats: Vec::new(),
            },
        ];

        assert_eq!(stat_group_by_index(&groups, 1), groups.get(1));
        assert_eq!(stat_group_by_index(&groups, 2), None);
    }

    #[test]
    fn non_linear_stat_display_curves_clamp_to_their_decoded_endpoints() {
        let points = [
            ItemStatDisplayPoint {
                investment_value: 20,
                display_value: 450,
            },
            ItemStatDisplayPoint {
                investment_value: 80,
                display_value: 600,
            },
        ];

        assert_eq!(
            interpolate_investment_stat_display(&points, false, -100),
            Some(450)
        );
        assert_eq!(
            interpolate_investment_stat_display(&points, false, 50),
            Some(525)
        );
        assert_eq!(
            interpolate_investment_stat_display(&points, false, 100),
            Some(600)
        );
        assert_eq!(
            interpolate_investment_stat_display(&points, true, -5),
            Some(-5)
        );
    }

    #[test]
    fn masterwork_labels_use_the_primary_local_stat_name() {
        let mut item = vec![0_u8; 0x300 + ITEM_INVESTMENT_STAT_ROW_SIZE * 2];
        let count = 2_u64;
        item[INVESTMENT_STAT_DESCRIPTOR..INVESTMENT_STAT_DESCRIPTOR + 8]
            .copy_from_slice(&count.to_le_bytes());
        item[INVESTMENT_STAT_DESCRIPTOR + 8..INVESTMENT_STAT_DESCRIPTOR + 16]
            .copy_from_slice(&(0x28_i64).to_le_bytes());
        item[0x2F0..0x2F8].copy_from_slice(&count.to_le_bytes());
        item[0x2F8..0x2FC].copy_from_slice(&ITEM_INVESTMENT_STAT_ROW_CLASS.to_le_bytes());
        item[0x300..0x302].copy_from_slice(&2_u16.to_le_bytes());
        item[0x300 + ITEM_INVESTMENT_STAT_ROW_SIZE..0x302 + ITEM_INVESTMENT_STAT_ROW_SIZE]
            .copy_from_slice(&1_u16.to_le_bytes());
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

        item[0x301] = 1;
        assert_eq!(masterwork_label(&item, &stat_names, "Masterwork"), None);
    }

    fn write_array_descriptor(
        data: &mut [u8],
        descriptor: usize,
        count: u64,
        header: usize,
        class: u32,
    ) {
        data[descriptor..descriptor + 8].copy_from_slice(&count.to_le_bytes());
        let pointer = descriptor + 8;
        data[pointer..pointer + 8]
            .copy_from_slice(&i64::try_from(header - pointer).unwrap().to_le_bytes());
        data[header..header + 8].copy_from_slice(&count.to_le_bytes());
        data[header + 8..header + 12].copy_from_slice(&class.to_le_bytes());
    }
}

//! Investment-stat definitions and item stat allocation decoding.

use std::collections::BTreeSet;

use crate::package_runtime::reader::PackageManager;
use serde::{Deserialize, Serialize};
use tiger_pkg::TagHash;

use crate::{
    investment::localization::{LocalizedStringCache, resolve_string},
    investment::schema::{
        ITEM_INVESTMENT_STAT_POINTER_OFFSET, ITEM_INVESTMENT_STAT_RESOURCE_CLASS,
        ITEM_INVESTMENT_STAT_ROW_CLASS, ITEM_INVESTMENT_STAT_ROW_SIZE,
        ITEM_STRING_DAMAGE_TYPE_OFFSET, ITEM_STRING_STAT_GROUP_INDEX_OFFSET,
        ITEM_STRING_STAT_GROUP_POINTER_OFFSET, ITEM_STRING_STAT_GROUP_RESOURCE_CLASS,
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
/// The investment root holds the constants blob at this slot.
const INVESTMENT_CONSTANTS_SLOT: usize = 11;
/// The constants blob's own 8-byte prefix comes before every offset the client quotes.
const INVESTMENT_CONSTANTS_PREFIX: usize = 8;
/// Client offsets of the six character stat rows, in the two runs the blob stores them in.
const CHARACTER_STAT_ROW_OFFSETS: [usize; 6] = [593, 594, 595, 622, 623, 624];
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

    /// Energy spent when socketing a mod, separate from its artifact unlock cost.
    pub(crate) fn mod_energy_cost(&self, hash: u64) -> Option<(i32, &'static str)> {
        let metadata = self.item_package_metadata.get(&hash)?;
        let cost = metadata.investment_stats.iter().find_map(|stat| {
            let label = match self.item_stat_definition(stat.definition_index)?.hash {
                3_779_394_102 => "Arc Energy Cost",
                3_344_745_325 => "Solar Energy Cost",
                2_399_985_800 => "Void Energy Cost",
                3_578_062_600 => "Energy Cost",
                _ => return None,
            };
            Some((stat.value, label))
        });
        cost.or_else(|| {
            // The artifact's weapon mods have no energy-cost stat and cost zero.
            self.seasonal()?
                .mods
                .iter()
                .any(|entry| entry.item_hash == hash)
                .then_some((0, "Energy Cost"))
        })
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

    /// The six character stat rows in character screen order, when the install names them.
    pub(crate) const fn character_stat_rows(&self) -> Option<[u16; 6]> {
        self.character_stat_rows
    }

    /// Which of the six character stats one investment stat row is, in character screen order.
    ///
    /// The rows are the ones the client's investment constants name, so a stat is matched by
    /// row, the way the client reads it, rather than by its definition's display name.
    fn armor_stat_index(&self, definition_index: u16) -> Option<usize> {
        self.character_stat_rows?
            .iter()
            .position(|row| *row == definition_index)
    }

    /// Returns the six armor-stat contributions authored on an item or plug.
    /// Values come from the installed package definitions and may be negative.
    pub(crate) fn armor_stat_values(&self, hash: u64) -> [i32; 6] {
        let mut values = [0_i32; 6];
        let Some(metadata) = self.item_package_metadata.get(&hash) else {
            return values;
        };
        for stat in &metadata.investment_stats {
            let Some(index) = self.armor_stat_index(stat.definition_index) else {
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

/// Reads the six character stat rows the client searches a character's stat table by, ordered
/// as the character screen lists them.
///
/// The investment constants blob names the rows the client itself uses, so a plug's stat can be
/// matched to a character stat by row rather than by its definition's display name. The blob
/// keeps its own order: each row takes the place its definition's name has in the character
/// screen, and a row whose name the bank does not resolve takes whichever place is left.
pub(in crate::catalog) fn scan_character_stat_rows(
    manager: &PackageManager,
    root: &[u8],
    definitions: &[ItemStatDefinition],
) -> Result<[u16; 6], String> {
    let tag = u32_at(root, 8 + INVESTMENT_CONSTANTS_SLOT * 16)
        .map_err(|error| format!("Could not read the investment constants tag: {error}"))?;
    let blob = manager
        .read_tag(TagHash(tag))
        .map_err(|error| format!("Could not read the investment constants blob: {error}"))?;
    let mut stored = [0_u16; 6];
    for (row, offset) in stored.iter_mut().zip(CHARACTER_STAT_ROW_OFFSETS) {
        let byte = blob
            .get(INVESTMENT_CONSTANTS_PREFIX + offset)
            .ok_or("The investment constants blob is too short for the character stat rows")?;
        *row = u16::from(*byte);
        match definitions.get(usize::from(*row)) {
            Some(definition) if definition.definition_index == *row => {}
            _ => {
                return Err(format!(
                    "Character stat row {row} has no investment stat definition"
                ));
            }
        }
    }
    let mut ordered = [None; 6];
    let mut taken = [false; 6];
    for (position, name) in ARMOR_STAT_NAMES.iter().enumerate() {
        let found = stored.iter().enumerate().position(|(index, row)| {
            !taken[index]
                && definitions[usize::from(*row)]
                    .name
                    .trim()
                    .eq_ignore_ascii_case(name)
        });
        if let Some(index) = found {
            ordered[position] = Some(stored[index]);
            taken[index] = true;
        }
    }
    let mut spare = stored
        .iter()
        .zip(taken)
        .filter(|(_, taken)| !taken)
        .map(|(row, _)| *row);
    let mut rows = [0_u16; 6];
    for (position, row) in rows.iter_mut().enumerate() {
        *row = ordered[position]
            .or_else(|| spare.next())
            .unwrap_or(stored[position]);
    }
    Ok(rows)
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

/// Where an item's strings keep the resource their stat group pointer names, when they have one.
fn string_stat_resource(string_definition: &[u8]) -> Option<usize> {
    let relative = i64_at(string_definition, ITEM_STRING_STAT_GROUP_POINTER_OFFSET)
        .ok()
        .filter(|relative| *relative != 0)?;
    let resource = relative_offset(ITEM_STRING_STAT_GROUP_POINTER_OFFSET, 0, relative).ok()?;
    (resource >= 4
        && u32_at(string_definition, resource - 4).ok()
            == Some(ITEM_STRING_STAT_GROUP_RESOURCE_CLASS))
    .then_some(resource)
}

pub(in crate::catalog) fn item_stat_group_index(string_definition: &[u8]) -> Option<u16> {
    let resource = string_stat_resource(string_definition)?;
    let index = i32_at(
        string_definition,
        resource.checked_add(ITEM_STRING_STAT_GROUP_INDEX_OFFSET)?,
    )
    .ok()?;
    u16::try_from(index).ok()
}

/// The damage type an item's strings give it, in the native damage enum, when it is one of the
/// four.
pub(in crate::catalog) fn item_display_damage_type(string_definition: &[u8]) -> Option<u8> {
    let resource = string_stat_resource(string_definition)?;
    let code = u32_at(
        string_definition,
        resource.checked_add(ITEM_STRING_DAMAGE_TYPE_OFFSET)?,
    )
    .ok()?;
    u8::try_from(code).ok().filter(|code| *code <= 3)
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

/// Names an unnamed armor stat plug from its three stat rows, such as
/// "13 Mobility / 1 Resilience / 7 Recovery".
///
/// `character_stat_rows` are the six rows in character screen order, as
/// `scan_character_stat_rows` reads them; the plug qualifies when its rows are the top three or
/// the bottom three of them.
pub(in crate::catalog) fn stat_allocation_labels(
    item: &[u8],
    character_stat_rows: &[u16; 6],
    stat_names: &[String],
) -> Option<(String, &'static str)> {
    let character_stat_rows = character_stat_rows.map(usize::from);
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

    let (expected_indexes, type_name) = if stats
        .iter()
        .all(|(index, _)| character_stat_rows[..3].contains(index))
    {
        (
            [
                character_stat_rows[0],
                character_stat_rows[1],
                character_stat_rows[2],
            ],
            "Top Stat Allocation",
        )
    } else if stats
        .iter()
        .all(|(index, _)| character_stat_rows[3..].contains(index))
    {
        (
            [
                character_stat_rows[3],
                character_stat_rows[4],
                character_stat_rows[5],
            ],
            "Bottom Stat Allocation",
        )
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
mod tests;

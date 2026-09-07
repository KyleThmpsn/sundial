//! Power caps read from the installed investment root, indexed by item version rows.

use serde::{Deserialize, Serialize};
use tiger_pkg::{PackageManager, TagHash};

use super::super::Catalog;
use crate::{
    investment_schema::{
        POWER_CAP_ROW_CLASS, POWER_CAP_ROW_SIZE, POWER_CAP_TABLE_CLASS, ROOT_POWER_CAP_TABLE_SLOT,
        investment_root_table_tag, item_version_array,
    },
    package_payload::{array_at, u32_at},
};

/// One native table row. Its position is the index stored in an item's version array.
/// Keep every row, including duplicate caps and the large native limits; neither the
/// row count nor an index's meaning is inferred from season numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PowerCapDefinition {
    pub hash: u32,
    pub power_cap: u32,
}

pub(in crate::catalog) fn scan_power_cap_definitions(
    manager: &PackageManager,
    root: &[u8],
) -> Result<Vec<PowerCapDefinition>, String> {
    let tag = TagHash(investment_root_table_tag(root, ROOT_POWER_CAP_TABLE_SLOT)?);
    let entry = manager
        .get_entry(tag)
        .ok_or_else(|| format!("Power-cap table {tag:?} is not live"))?;
    if entry.reference != POWER_CAP_TABLE_CLASS {
        return Err(format!(
            "Power-cap table {tag:?} has class 0x{:08X}, expected 0x{POWER_CAP_TABLE_CLASS:08X}",
            entry.reference,
        ));
    }
    let data = manager
        .read_tag(tag)
        .map_err(|error| format!("Could not read power-cap table {tag:?}: {error}"))?;
    decode_power_cap_definitions(&data).map_err(|error| format!("Power-cap table {tag:?}: {error}"))
}

fn decode_power_cap_definitions(data: &[u8]) -> Result<Vec<PowerCapDefinition>, String> {
    let (count, rows, class) = array_at(data, 8)?;
    // 0xFFFF is the native unassigned index, not a cap-definition row.
    if count == 0 || count > usize::from(u16::MAX) || class != POWER_CAP_ROW_CLASS {
        return Err(format!(
            "Unexpected cap array: {count} rows, class 0x{class:08X}"
        ));
    }
    let end = count
        .checked_mul(POWER_CAP_ROW_SIZE)
        .and_then(|size| rows.checked_add(size))
        .ok_or("Power-cap array size overflowed")?;
    if end != data.len() {
        return Err("Power-cap rows do not match the table's payload size".into());
    }
    (0..count)
        .map(|index| {
            let row = rows + index * POWER_CAP_ROW_SIZE;
            let native = f32::from_bits(u32_at(data, row + 4)?);
            // Native investment levels use one tenth of displayed Power.
            let power = f64::from(native) * 10.0;
            if !power.is_finite()
                || power <= 0.0
                || power > f64::from(u32::MAX)
                || (power - power.round()).abs() > 0.001
            {
                return Err(format!("Row {index} has invalid native power cap {native}"));
            }
            Ok(PowerCapDefinition {
                hash: u32_at(data, row)?,
                power_cap: power.round() as u32,
            })
        })
        .collect()
}

impl Catalog {
    pub(crate) fn power_cap_definitions(&self) -> &[PowerCapDefinition] {
        &self.power_cap_definitions
    }

    pub(crate) fn power_cap_for_version_group(&self, index: u16) -> Option<u32> {
        self.power_cap_definitions
            .get(usize::from(index))
            .map(|row| row.power_cap)
    }

    /// Highest cap across an item's versions, only when all indices resolve.
    pub(crate) fn item_power_cap(&self, hash: u64) -> Option<i64> {
        self.item_package_metadata
            .get(&hash)
            .and_then(|metadata| metadata.power_cap)
            .map(i64::from)
    }
}

pub(in crate::catalog) fn item_power_cap(
    groups: &[u16],
    definitions: &[PowerCapDefinition],
) -> Option<u32> {
    // An unresolved version may have a higher limit. Do not claim a lower known
    // row is the complete item's cap when another row is missing or unassigned.
    groups
        .iter()
        .map(|index| {
            definitions
                .get(usize::from(*index))
                .map(|row| row.power_cap)
        })
        .collect::<Option<Vec<_>>>()?
        .into_iter()
        .max()
}

pub(in crate::catalog) fn item_power_cap_groups(item: &[u8]) -> Vec<u16> {
    item_version_array(item)
        .ok()
        .flatten()
        .map_or_else(Vec::new, |version| version.groups)
}

#[cfg(test)]
mod tests;

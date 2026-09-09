use serde::{Deserialize, Serialize};
use tiger_pkg::{PackageManager, TagHash};

use crate::{
    investment_schema::{
        GLOBALS_FINISHED_SANDBOX_PERK_TABLE_SLOT, ITEM_SANDBOX_PERK_DESCRIPTOR_OFFSET,
        ITEM_SANDBOX_PERK_ROW_CLASS, ITEM_SANDBOX_PERK_ROW_SIZE, investment_globals_table_tag,
    },
    package_payload::{array_at, i64_at, relative_offset, u16_at},
    package_runtime::is_valid_package_tag,
    sandbox_perk::{
        FINISHED_SANDBOX_PERK_CATALOG_CLASS, FINISHED_SANDBOX_PERK_ROW_CLASS,
        FINISHED_SANDBOX_PERK_ROW_SIZE,
    },
};

use super::investment::item_investment_resource;

const SANDBOX_PERK_CATALOG_STOCK_COUNT: usize = 2_481;
const SANDBOX_PERK_CATALOG_ACTIVE_OFFSET: usize = 6;

/// An index into the finished sandbox-perk catalog referenced by an item's
/// `stats_item_block`. The remaining 22 bytes in the package row are not yet
/// assigned semantics here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ItemSandboxPerk {
    pub perk_index: u16,
}

/// Reads the native liveness bit for every row in the finished sandbox-perk
/// catalog. Item definitions carry indices into this table, but the client
/// emits an index only when the referenced row's active byte is nonzero.
pub(in crate::catalog) fn scan_sandbox_perk_catalog(
    manager: &PackageManager,
    globals: &[u8],
) -> Result<Vec<bool>, String> {
    let tag = investment_globals_table_tag(globals, GLOBALS_FINISHED_SANDBOX_PERK_TABLE_SLOT)
        .map_err(|error| format!("Could not read the sandbox-perk catalog tag: {error}"))?;
    let tag = TagHash(tag);
    if !is_valid_package_tag(tag) {
        return Err(format!(
            "Investment globals contains an invalid sandbox-perk catalog tag {tag}"
        ));
    }
    let entry = manager
        .get_entry(tag)
        .ok_or_else(|| format!("Sandbox-perk catalog tag {tag} is not registered"))?;
    if entry.reference != FINISHED_SANDBOX_PERK_CATALOG_CLASS {
        return Err(format!(
            "Sandbox-perk catalog tag {tag} has class 0x{:08X}; expected 0x{FINISHED_SANDBOX_PERK_CATALOG_CLASS:08X}",
            entry.reference
        ));
    }
    let table = manager
        .read_tag(tag)
        .map_err(|error| format!("Could not read the sandbox-perk catalog: {error}"))?;
    validated_sandbox_perk_catalog(&table).ok_or_else(|| {
        format!(
            "Sandbox-perk catalog has fewer than {SANDBOX_PERK_CATALOG_STOCK_COUNT} rows or an invalid native row layout"
        )
    })
}

fn validated_sandbox_perk_catalog(table: &[u8]) -> Option<Vec<bool>> {
    let (count, rows, class) = array_at(table, 8).ok()?;
    if count < SANDBOX_PERK_CATALOG_STOCK_COUNT || class != FINISHED_SANDBOX_PERK_ROW_CLASS {
        return None;
    }
    (0..count)
        .map(|index| {
            let row = rows.checked_add(index.checked_mul(FINISHED_SANDBOX_PERK_ROW_SIZE)?)?;
            let record = relative_offset(row, 8, i64_at(table, row.checked_add(8)?).ok()?).ok()?;
            table
                .get(record.checked_add(SANDBOX_PERK_CATALOG_ACTIVE_OFFSET)?)
                .map(|active| *active != 0)
        })
        .collect()
}

pub(in crate::catalog) fn item_sandbox_perks(
    item: &[u8],
    active_catalog_rows: Option<&[bool]>,
) -> Vec<ItemSandboxPerk> {
    let Some(active_catalog_rows) = active_catalog_rows else {
        return Vec::new();
    };
    let Some(resource) = item_investment_resource(item) else {
        return Vec::new();
    };
    let Some(descriptor) = resource.checked_add(ITEM_SANDBOX_PERK_DESCRIPTOR_OFFSET) else {
        return Vec::new();
    };
    let Ok((count, rows, class)) = array_at(item, descriptor) else {
        return Vec::new();
    };
    if class != ITEM_SANDBOX_PERK_ROW_CLASS || count > 64 {
        return Vec::new();
    }

    (0..count)
        .filter_map(|index| {
            let row = rows.checked_add(index.checked_mul(ITEM_SANDBOX_PERK_ROW_SIZE)?)?;
            let perk_index = u16_at(item, row).ok()?;
            if !active_catalog_rows
                .get(usize::from(perk_index))
                .copied()
                .unwrap_or(false)
            {
                return None;
            }
            Some(ItemSandboxPerk { perk_index })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::package_runtime;

    use super::*;

    fn item_with_sandbox_perks(perks: &[(u16, [u8; 22])]) -> Vec<u8> {
        let mut item = vec![0_u8; 0xD0 + perks.len() * ITEM_SANDBOX_PERK_ROW_SIZE];
        item[0x70..0x78].copy_from_slice(&0x20_i64.to_le_bytes());
        item[0x8C..0x90].copy_from_slice(&0x8080_77B9_u32.to_le_bytes());
        item[0xA0..0xA8].copy_from_slice(&(perks.len() as u64).to_le_bytes());
        item[0xA8..0xB0].copy_from_slice(&0x18_i64.to_le_bytes());
        item[0xC0..0xC8].copy_from_slice(&(perks.len() as u64).to_le_bytes());
        item[0xC8..0xCC].copy_from_slice(&ITEM_SANDBOX_PERK_ROW_CLASS.to_le_bytes());
        for (row_index, (perk_index, tail)) in perks.iter().enumerate() {
            let row = 0xD0 + row_index * ITEM_SANDBOX_PERK_ROW_SIZE;
            item[row..row + 2].copy_from_slice(&perk_index.to_le_bytes());
            item[row + 2..row + ITEM_SANDBOX_PERK_ROW_SIZE].copy_from_slice(tail);
        }
        item
    }

    #[test]
    fn item_perks_use_two_byte_indices_and_twenty_four_byte_rows() {
        let item =
            item_with_sandbox_perks(&[(449, [0xFF; 22]), (84, [0xA5; 22]), (2_480, [0x5A; 22])]);
        let mut active = vec![false; SANDBOX_PERK_CATALOG_STOCK_COUNT];
        active[449] = true;
        active[84] = true;
        active[2_480] = true;

        assert_eq!(
            item_sandbox_perks(&item, Some(&active)),
            vec![
                ItemSandboxPerk { perk_index: 449 },
                ItemSandboxPerk { perk_index: 84 },
                ItemSandboxPerk { perk_index: 2_480 },
            ]
        );
    }

    #[test]
    fn item_perks_reject_inactive_and_out_of_range_catalog_rows() {
        let item = item_with_sandbox_perks(&[(84, [0; 22]), (2_480, [0; 22]), (2_481, [0; 22])]);
        let mut active = vec![false; SANDBOX_PERK_CATALOG_STOCK_COUNT];
        active[2_480] = true;

        assert_eq!(
            item_sandbox_perks(&item, Some(&active)),
            vec![ItemSandboxPerk { perk_index: 2_480 }]
        );
        assert!(item_sandbox_perks(&item, None).is_empty());
    }

    #[test]
    fn finished_catalog_requires_the_established_population_and_reads_active_rows() {
        fn table_with_count(count: usize) -> Vec<u8> {
            let records = 0x40 + count * FINISHED_SANDBOX_PERK_ROW_SIZE;
            let mut table = vec![0_u8; records + count * 8];
            table[8..16].copy_from_slice(&(count as u64).to_le_bytes());
            table[16..24].copy_from_slice(&0x20_i64.to_le_bytes());
            table[0x30..0x38].copy_from_slice(&(count as u64).to_le_bytes());
            table[0x38..0x3C].copy_from_slice(&FINISHED_SANDBOX_PERK_ROW_CLASS.to_le_bytes());
            for index in 0..count {
                let row = 0x40 + index * FINISHED_SANDBOX_PERK_ROW_SIZE;
                let record = records + index * 8;
                let relative = i64::try_from(record).unwrap() - i64::try_from(row + 8).unwrap();
                table[row + 8..row + 16].copy_from_slice(&relative.to_le_bytes());
            }
            table
        }

        let mut table = table_with_count(SANDBOX_PERK_CATALOG_STOCK_COUNT);
        let records = 0x40 + SANDBOX_PERK_CATALOG_STOCK_COUNT * FINISHED_SANDBOX_PERK_ROW_SIZE;
        table[records + 449 * 8 + SANDBOX_PERK_CATALOG_ACTIVE_OFFSET] = 1;
        table[records + 450 * 8 + SANDBOX_PERK_CATALOG_ACTIVE_OFFSET] = 2;
        let active =
            validated_sandbox_perk_catalog(&table).expect("finished catalog should decode");

        assert!(active[449]);
        assert!(active[450]);
        assert!(!active[451]);
        assert!(validated_sandbox_perk_catalog(&table_with_count(979)).is_none());
        assert!(
            validated_sandbox_perk_catalog(&table_with_count(SANDBOX_PERK_CATALOG_STOCK_COUNT + 1))
                .is_some(),
            "authored finished-perk rows must remain scannable"
        );
    }

    #[test]
    #[ignore = "requires SUNDIAL_TEST_INSTALL pointing to the supported Shadowkeep build"]
    fn supported_package_catalog_exposes_known_live_runtime_perk_rows() {
        let install = PathBuf::from(std::env::var("SUNDIAL_TEST_INSTALL").unwrap());
        let manager = package_runtime::open_shadowkeep_packages(&install).unwrap();
        let globals_tag =
            package_runtime::resolve_live_named_tag(&manager, "investment_globals", None).unwrap();
        let globals = manager.read_tag(globals_tag).unwrap();
        let active = scan_sandbox_perk_catalog(&manager, &globals).unwrap();

        assert!(active.len() >= SANDBOX_PERK_CATALOG_STOCK_COUNT);
        // Hydraulic Boosters, Headseeker, Outlaw, the modern elemental
        // markers, and Trench Barrel are package-proved live rows.
        for index in [159, 380, 421, 449, 450, 451, 1_189] {
            assert!(active[index], "sandbox-perk row {index} should be live");
        }
    }
}

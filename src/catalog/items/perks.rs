use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tiger_pkg::{PackageManager, TagHash};

use super::{
    super::{
        Catalog,
        package::{array_at, i64_at, relative_offset, u16_at, u32_at, u64_at},
    },
    ItemPackageMetadata,
};

const INTRINSIC_PERK_CLASS: u32 = 0x8080_2C50;
const SANDBOX_PERK_DEFINITION_CLASS: u32 = 0x8080_748C;
const SANDBOX_PERK_RUNTIME_TABLE_CLASS: u32 = 0x8080_5961;
const SANDBOX_PERK_RUNTIME_DATA_CLASS: u32 = 0x8080_5963;
const SANDBOX_PERK_TABLE_SLOT: usize = 88;
const SANDBOX_PERK_RUNTIME_TABLE_INDEX: usize = 53;
const SANDBOX_PERK_ROW_SIZE: usize = 24;
const SANDBOX_PERK_RUNTIME_HEADER_SIZE: usize = 16;
const ITEM_INTRINSIC_PERK_DESCRIPTOR: usize = 0xE0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ItemIntrinsicPerk {
    pub definition_index: u16,
    pub hash: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SandboxPerkDefinition {
    pub definition_index: u16,
    pub hash: u64,
    pub category: u64,
    pub runtime_category: u64,
    pub runtime_offset: Option<u32>,
    pub runtime_class: Option<u32>,
    pub runtime_data: Option<Vec<u8>>,
}

impl Catalog {
    pub(crate) fn sandbox_perk_definition_by_hash(
        &self,
        hash: u64,
    ) -> Option<&SandboxPerkDefinition> {
        self.sandbox_perk_definitions
            .iter()
            .find(|definition| definition.hash == hash)
    }

    pub(crate) fn intrinsic_perk_references(&self, hash: u64) -> &[u64] {
        self.intrinsic_perk_items
            .get(&hash)
            .map_or(&[], Vec::as_slice)
    }
}

pub(in crate::catalog) fn index_intrinsic_perk_references(
    metadata_by_item: &HashMap<u64, ItemPackageMetadata>,
) -> HashMap<u64, Vec<u64>> {
    let mut item_hashes_by_perk = HashMap::<u64, Vec<u64>>::new();
    for (&item_hash, metadata) in metadata_by_item {
        for perk in &metadata.intrinsic_perks {
            item_hashes_by_perk
                .entry(perk.hash)
                .or_default()
                .push(item_hash);
        }
    }
    for item_hashes in item_hashes_by_perk.values_mut() {
        item_hashes.sort_unstable();
        item_hashes.dedup();
    }
    item_hashes_by_perk
}

pub(in crate::catalog) fn scan_sandbox_perk_hashes(
    manager: &PackageManager,
    root: &[u8],
) -> Vec<u64> {
    let Ok(tag) = u32_at(root, 8 + SANDBOX_PERK_TABLE_SLOT * 16) else {
        return Vec::new();
    };
    let Ok(table) = manager.read_tag(TagHash(tag)) else {
        return Vec::new();
    };
    let Ok((count, rows, class)) = array_at(&table, 8) else {
        return Vec::new();
    };
    if class != SANDBOX_PERK_DEFINITION_CLASS || count > 4096 {
        return Vec::new();
    }
    (0..count)
        .map(|index| {
            u32_at(&table, rows + index * SANDBOX_PERK_ROW_SIZE)
                .map(u64::from)
                .ok()
        })
        .collect::<Option<Vec<_>>>()
        .unwrap_or_default()
}

pub(in crate::catalog) fn scan_sandbox_perk_runtime_definitions(
    manager: &PackageManager,
    root: &[u8],
    globals: &[u8],
    hashes: &[u64],
) -> Vec<SandboxPerkDefinition> {
    let result = (|| {
        let definition_tag = u32_at(root, 8 + SANDBOX_PERK_TABLE_SLOT * 16)?;
        let definition_table = manager
            .read_tag(TagHash(definition_tag))
            .map_err(|error| error.to_string())?;
        let runtime_tag = u32_at(globals, 16 + SANDBOX_PERK_RUNTIME_TABLE_INDEX * 16)?;
        let runtime_table = manager
            .read_tag(TagHash(runtime_tag))
            .map_err(|error| error.to_string())?;
        materialize_sandbox_perk_runtime_definitions(&definition_table, &runtime_table, hashes)
    })();
    result.unwrap_or_default()
}

fn materialize_sandbox_perk_runtime_definitions(
    definition_table: &[u8],
    runtime_table: &[u8],
    hashes: &[u64],
) -> Result<Vec<SandboxPerkDefinition>, String> {
    let (definition_count, definition_rows, definition_class) = array_at(definition_table, 8)?;
    let (runtime_count, runtime_rows, runtime_table_class) = array_at(runtime_table, 8)?;
    if definition_class != SANDBOX_PERK_DEFINITION_CLASS {
        return Err(format!(
            "Unexpected sandbox perk definition class 0x{definition_class:08X}"
        ));
    }
    if runtime_table_class != SANDBOX_PERK_RUNTIME_TABLE_CLASS {
        return Err(format!(
            "Unexpected sandbox perk runtime class 0x{runtime_table_class:08X}"
        ));
    }
    if definition_count != runtime_count || definition_count != hashes.len() {
        return Err("Sandbox perk definition and runtime tables do not align".into());
    }
    if definition_count > 4096 {
        return Err("Sandbox perk runtime table is too large".into());
    }

    let mut runtime_starts = Vec::with_capacity(runtime_count);
    let mut row_metadata = Vec::with_capacity(runtime_count);
    for (index, expected_hash) in hashes.iter().copied().enumerate() {
        let definition_row = definition_rows
            .checked_add(
                index
                    .checked_mul(SANDBOX_PERK_ROW_SIZE)
                    .ok_or("Sandbox perk definition row overflowed")?,
            )
            .ok_or("Sandbox perk definition row overflowed")?;
        let runtime_row = runtime_rows
            .checked_add(
                index
                    .checked_mul(SANDBOX_PERK_ROW_SIZE)
                    .ok_or("Sandbox perk runtime row overflowed")?,
            )
            .ok_or("Sandbox perk runtime row overflowed")?;
        let definition_hash = u64::from(u32_at(definition_table, definition_row)?);
        let runtime_hash = u64::from(u32_at(runtime_table, runtime_row)?);
        if definition_hash != expected_hash || runtime_hash != expected_hash {
            return Err(format!("Sandbox perk hash mismatch at definition {index}"));
        }
        let category = u64_at(definition_table, definition_row + 8)?;
        let runtime_category = u64_at(runtime_table, runtime_row + 8)?;
        let runtime_pointer = i64_at(runtime_table, runtime_row + 16)?;
        if runtime_pointer == 0 {
            runtime_starts.push(None);
            row_metadata.push((category, runtime_category, None));
            continue;
        }
        let runtime_start = relative_offset(runtime_row + 16, 0, runtime_pointer)?;
        let runtime_class_offset = runtime_start
            .checked_add(8)
            .ok_or("Sandbox perk runtime header overflowed")?;
        let runtime_data_start = runtime_start
            .checked_add(SANDBOX_PERK_RUNTIME_HEADER_SIZE)
            .ok_or("Sandbox perk runtime header overflowed")?;
        if runtime_data_start > runtime_table.len()
            || u64_at(runtime_table, runtime_start)? != runtime_category
        {
            return Err(format!(
                "Sandbox perk runtime header mismatch at definition {index}"
            ));
        }
        let runtime_class = u32_at(runtime_table, runtime_class_offset)?;
        if runtime_class != SANDBOX_PERK_RUNTIME_DATA_CLASS {
            return Err(format!(
                "Unexpected sandbox perk data class 0x{runtime_class:08X} at definition {index}"
            ));
        }
        runtime_starts.push(Some(runtime_start));
        row_metadata.push((category, runtime_category, Some(runtime_class)));
    }

    let mut sorted_starts = runtime_starts.iter().flatten().copied().collect::<Vec<_>>();
    sorted_starts.sort_unstable();
    sorted_starts.dedup();
    let mut definitions = Vec::with_capacity(runtime_count);
    for (index, ((runtime_start, &(category, runtime_category, runtime_class)), hash)) in
        runtime_starts
            .iter()
            .copied()
            .zip(&row_metadata)
            .zip(hashes.iter().copied())
            .enumerate()
    {
        let runtime_data = runtime_start
            .map(|runtime_start| {
                let next_index =
                    sorted_starts.partition_point(|candidate| *candidate <= runtime_start);
                let runtime_end = sorted_starts
                    .get(next_index)
                    .copied()
                    .unwrap_or(runtime_table.len());
                let runtime_data_start = runtime_start + SANDBOX_PERK_RUNTIME_HEADER_SIZE;
                if runtime_end < runtime_data_start || runtime_end > runtime_table.len() {
                    return Err(format!(
                        "Sandbox perk runtime data is invalid at definition {index}"
                    ));
                }
                Ok(runtime_table[runtime_data_start..runtime_end].to_vec())
            })
            .transpose()?;
        definitions.push(SandboxPerkDefinition {
            definition_index: u16::try_from(index)
                .map_err(|_| "Sandbox perk definition index is too large")?,
            hash,
            category,
            runtime_category,
            runtime_offset: runtime_start
                .map(u32::try_from)
                .transpose()
                .map_err(|_| "Sandbox perk runtime offset is too large")?,
            runtime_class,
            runtime_data,
        });
    }
    Ok(definitions)
}

pub(in crate::catalog) fn item_intrinsic_perk_hashes(
    item: &[u8],
    sandbox_perk_hashes: &[u64],
) -> Vec<ItemIntrinsicPerk> {
    let Ok((count, rows, class)) = array_at(item, ITEM_INTRINSIC_PERK_DESCRIPTOR) else {
        return Vec::new();
    };
    if class != INTRINSIC_PERK_CLASS || count > 64 {
        return Vec::new();
    }
    (0..count)
        .filter_map(|index| {
            let definition_index = u16_at(item, rows + index * 2).ok()?;
            let hash = sandbox_perk_hashes
                .get(usize::from(definition_index))
                .copied()?;
            Some(ItemIntrinsicPerk {
                definition_index,
                hash,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::package_runtime;

    use super::*;

    #[test]
    #[ignore = "requires SUNDIAL_TEST_INSTALL pointing to the supported Shadowkeep build"]
    fn supported_shadowkeep_build_has_aligned_sandbox_runtime_definitions() {
        let install = PathBuf::from(std::env::var("SUNDIAL_TEST_INSTALL").unwrap());
        let manager = package_runtime::open_shadowkeep_packages(&install).unwrap();
        let globals = manager
            .lookup
            .named_tags
            .iter()
            .find(|entry| entry.name == "investment_globals")
            .unwrap();
        let globals_data = manager.read_tag(globals.hash).unwrap();
        let root = manager
            .read_tag(TagHash(u32_at(&globals_data, 16).unwrap()))
            .unwrap();
        let hashes = scan_sandbox_perk_hashes(&manager, &root);
        let definition_tag = u32_at(&root, 8 + SANDBOX_PERK_TABLE_SLOT * 16).unwrap();
        let definition_table = manager.read_tag(TagHash(definition_tag)).unwrap();
        let runtime_tag =
            u32_at(&globals_data, 16 + SANDBOX_PERK_RUNTIME_TABLE_INDEX * 16).unwrap();
        let runtime_table = manager.read_tag(TagHash(runtime_tag)).unwrap();
        let definitions = materialize_sandbox_perk_runtime_definitions(
            &definition_table,
            &runtime_table,
            &hashes,
        )
        .unwrap();

        assert_eq!(hashes.len(), 979);
        assert_eq!(definitions.len(), hashes.len());
        assert!(definitions.iter().all(|definition| {
            definition.runtime_class.is_none()
                || definition.runtime_class == Some(SANDBOX_PERK_RUNTIME_DATA_CLASS)
        }));
        let missing = definitions
            .iter()
            .filter(|definition| definition.runtime_data.is_none())
            .count();
        let category_variants = definitions
            .iter()
            .filter(|definition| definition.category != definition.runtime_category)
            .count();
        assert_eq!(missing, 11);
        assert_eq!(category_variants, 18);
    }

    #[test]
    fn intrinsic_perks_resolve_only_valid_package_indices() {
        let mut item = vec![0_u8; 0x160];
        item[ITEM_INTRINSIC_PERK_DESCRIPTOR..ITEM_INTRINSIC_PERK_DESCRIPTOR + 8]
            .copy_from_slice(&3_u64.to_le_bytes());
        item[ITEM_INTRINSIC_PERK_DESCRIPTOR + 8..ITEM_INTRINSIC_PERK_DESCRIPTOR + 16]
            .copy_from_slice(&(0x38_i64).to_le_bytes());
        item[0x120..0x128].copy_from_slice(&3_u64.to_le_bytes());
        item[0x128..0x12C].copy_from_slice(&INTRINSIC_PERK_CLASS.to_le_bytes());
        item[0x130..0x132].copy_from_slice(&1_u16.to_le_bytes());
        item[0x132..0x134].copy_from_slice(&3_u16.to_le_bytes());
        item[0x134..0x136].copy_from_slice(&u16::MAX.to_le_bytes());

        assert_eq!(
            item_intrinsic_perk_hashes(&item, &[10, 20, 30, 40]),
            vec![
                ItemIntrinsicPerk {
                    definition_index: 1,
                    hash: 20,
                },
                ItemIntrinsicPerk {
                    definition_index: 3,
                    hash: 40,
                },
            ]
        );
    }

    #[test]
    fn sandbox_perk_runtime_rows_require_exact_index_and_hash_alignment() {
        fn write_array_header(data: &mut [u8], count: u64, class: u32) {
            data[8..16].copy_from_slice(&count.to_le_bytes());
            data[16..24].copy_from_slice(&0x20_i64.to_le_bytes());
            data[0x30..0x38].copy_from_slice(&count.to_le_bytes());
            data[0x38..0x3C].copy_from_slice(&class.to_le_bytes());
        }

        let mut definitions = vec![0_u8; 0x70];
        write_array_header(&mut definitions, 2, SANDBOX_PERK_DEFINITION_CLASS);
        definitions[0x40..0x44].copy_from_slice(&10_u32.to_le_bytes());
        definitions[0x48..0x50].copy_from_slice(&7_u64.to_le_bytes());
        definitions[0x58..0x5C].copy_from_slice(&20_u32.to_le_bytes());
        definitions[0x60..0x68].copy_from_slice(&9_u64.to_le_bytes());

        let mut runtime = vec![0_u8; 0xBC];
        write_array_header(&mut runtime, 2, SANDBOX_PERK_RUNTIME_TABLE_CLASS);
        runtime[0x40..0x44].copy_from_slice(&10_u32.to_le_bytes());
        runtime[0x48..0x50].copy_from_slice(&7_u64.to_le_bytes());
        runtime[0x50..0x58].copy_from_slice(&0x30_i64.to_le_bytes());
        runtime[0x58..0x5C].copy_from_slice(&20_u32.to_le_bytes());
        runtime[0x60..0x68].copy_from_slice(&11_u64.to_le_bytes());
        runtime[0x68..0x70].copy_from_slice(&0x38_i64.to_le_bytes());
        runtime[0x80..0x88].copy_from_slice(&7_u64.to_le_bytes());
        runtime[0x88..0x8C].copy_from_slice(&SANDBOX_PERK_RUNTIME_DATA_CLASS.to_le_bytes());
        runtime[0x90..0xA0]
            .copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]);
        runtime[0xA0..0xA8].copy_from_slice(&11_u64.to_le_bytes());
        runtime[0xA8..0xAC].copy_from_slice(&SANDBOX_PERK_RUNTIME_DATA_CLASS.to_le_bytes());
        runtime[0xB0..0xBC].copy_from_slice(&[21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32]);

        let decoded =
            materialize_sandbox_perk_runtime_definitions(&definitions, &runtime, &[10, 20])
                .unwrap();
        assert_eq!(
            decoded,
            vec![
                SandboxPerkDefinition {
                    definition_index: 0,
                    hash: 10,
                    category: 7,
                    runtime_category: 7,
                    runtime_offset: Some(0x80),
                    runtime_class: Some(SANDBOX_PERK_RUNTIME_DATA_CLASS),
                    runtime_data: Some(
                        vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16,]
                    ),
                },
                SandboxPerkDefinition {
                    definition_index: 1,
                    hash: 20,
                    category: 9,
                    runtime_category: 11,
                    runtime_offset: Some(0xA0),
                    runtime_class: Some(SANDBOX_PERK_RUNTIME_DATA_CLASS),
                    runtime_data: Some(vec![21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32,]),
                },
            ]
        );

        let mut bad_runtime = runtime.clone();
        bad_runtime[0x58..0x5C].copy_from_slice(&21_u32.to_le_bytes());
        assert!(
            materialize_sandbox_perk_runtime_definitions(&definitions, &bad_runtime, &[10, 20])
                .is_err()
        );
    }
}

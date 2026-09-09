use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tiger_pkg::{PackageManager, TagHash};

use crate::investment_schema::{
    COLLECTIBLE_CONDITION_OFFSETS, COLLECTIBLE_DEFINITION_ROW_CLASS,
    COLLECTIBLE_DEFINITION_ROW_SIZE, COLLECTIBLE_HASH_OFFSET,
    COLLECTIBLE_INVENTORY_ITEM_INDEX_OFFSET, COLLECTIBLE_MATERIAL_REQUIREMENT_SET_INDEX_OFFSET,
    COLLECTIBLE_PRESENTATION_NODE_PARENTS_OFFSET, CONDITION_EXPRESSION_ROW_CLASS,
    CONDITION_EXPRESSION_ROW_SIZE, MATERIAL_REQUIREMENT_ROW_CLASS, MATERIAL_REQUIREMENT_ROW_SIZE,
    MATERIAL_REQUIREMENT_SET_ROW_CLASS, MATERIAL_REQUIREMENT_SET_ROW_SIZE,
    PRESENTATION_NODE_INDEX_ROW_CLASS, ROOT_COLLECTIBLE_DEFINITION_TABLE_SLOT,
    ROOT_MATERIAL_REQUIREMENT_TABLE_SLOT, ROOT_SHARED_EXPRESSION_POOL_TABLE_SLOT,
    SHARED_EXPRESSION_POOL_COUNT, SHARED_EXPRESSION_POOL_DIRECT_ROW_CLASS,
    SHARED_EXPRESSION_POOL_DIRECT_ROW_SIZE, SHARED_EXPRESSION_POOL_HASHED_EXPRESSION_OFFSET,
    SHARED_EXPRESSION_POOL_HASHED_ROW_CLASS, SHARED_EXPRESSION_POOL_HASHED_ROW_SIZE,
    investment_root_table_tag,
};
use crate::package_payload::{array_at, bool_at, i64_at, relative_offset, u16_at, u32_at, u64_at};

use super::{
    Catalog,
    progression::{PresentationNodeDef, definition_index_list, presentation_paths},
};

const INSERTION_MATERIAL_REQUIREMENT_SET_INDEX_OFFSET: usize = 0x1E8;
const ENABLED_MATERIAL_REQUIREMENT_SET_INDEX_OFFSET: usize = 0x200;

pub(crate) const COLLECTIBLE_ACQUIRED_CONDITION_FIELD: u8 = 4;
const SHARED_EXPRESSION_POOL_TABLE_TAG: u32 = 0x8131_9324;
const MATERIAL_REQUIREMENT_CAPACITY: usize = 6;
const MATERIAL_REQUIREMENT_SET_CAPACITY: usize = 512;
const COLLECTIBLE_DEFINITION_CAPACITY: usize = 1 << 15;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CollectionConditionTokenDef {
    pub kind: u32,
    pub operand: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CollectionConditionDef {
    pub field: u8,
    pub tokens: Vec<CollectionConditionTokenDef>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CollectibleDef {
    pub index: u16,
    pub hash: u64,
    pub item_definition_index: u16,
    pub item_hash: u64,
    pub material_requirement_set_index: Option<u16>,
    pub material_requirement_set_hash: u64,
    pub material_requirements: Vec<MaterialRequirementDef>,
    pub name: String,
    pub type_name: String,
    pub paths: Vec<Vec<String>>,
    pub conditions: Vec<CollectionConditionDef>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MaterialRequirementDef {
    pub item_definition_index: u16,
    pub item_hash: u64,
    pub quantity: u32,
    pub delete_on_action: bool,
    pub omit_from_requirements: bool,
    pub condition: u16,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MaterialRequirementSetDef {
    pub index: u16,
    pub hash: u64,
    pub requirements: Vec<MaterialRequirementDef>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ItemMaterialRequirementSetIndices {
    pub insertion: Option<u16>,
    pub enabled: Option<u16>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PendingMaterialRequirementDef {
    pub item_definition_index: u16,
    pub quantity: u32,
    pub delete_on_action: bool,
    pub omit_from_requirements: bool,
    pub condition: u16,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct PendingMaterialRequirementSet {
    pub hash: u64,
    pub requirements: Vec<PendingMaterialRequirementDef>,
}

pub(super) struct PendingCollectibleDef {
    pub index: u16,
    pub hash: u64,
    pub item_definition_index: u16,
    pub material_requirement_set_index: Option<u16>,
    pub material_requirement_set_hash: u64,
    pub material_requirements: Vec<PendingMaterialRequirementDef>,
    pub paths: Vec<Vec<String>>,
    pub conditions: Vec<CollectionConditionDef>,
}

pub(super) fn item_material_requirement_set_indices_from_data(
    item: &[u8],
) -> Option<ItemMaterialRequirementSetIndices> {
    fn read(item: &[u8], offset: usize) -> Option<u16> {
        let bytes = item.get(offset..offset + 2)?;
        let index = u16::from_le_bytes([bytes[0], bytes[1]]);
        (index != u16::MAX).then_some(index)
    }

    let insertion = read(item, INSERTION_MATERIAL_REQUIREMENT_SET_INDEX_OFFSET);
    let enabled = read(item, ENABLED_MATERIAL_REQUIREMENT_SET_INDEX_OFFSET);
    (insertion.is_some() || enabled.is_some())
        .then_some(ItemMaterialRequirementSetIndices { insertion, enabled })
}

pub(super) fn materialize_collectibles(
    pending: Vec<PendingCollectibleDef>,
    item_hashes: &[u64],
    names: &HashMap<u64, String>,
    type_names: &HashMap<u64, String>,
) -> Result<Vec<CollectibleDef>, String> {
    pending
        .into_iter()
        .map(|collectible| {
            let item_hash = if collectible.item_definition_index == u16::MAX {
                0
            } else {
                item_hashes
                    .get(usize::from(collectible.item_definition_index))
                    .copied()
                    .ok_or_else(|| {
                        format!(
                            "Collectible #{} references item definition index {}, which is outside the package table",
                            collectible.index, collectible.item_definition_index
                        )
                    })?
            };
            let material_requirements = collectible
                .material_requirements
                .into_iter()
                .enumerate()
                .map(|(row, requirement)| {
                    let item_hash = item_hashes
                        .get(usize::from(requirement.item_definition_index))
                        .copied()
                        .ok_or_else(|| {
                            format!(
                                "Collectible #{} material requirement row #{row} references item definition index {}, which is outside the package table",
                                collectible.index, requirement.item_definition_index
                            )
                        })?;
                    Ok(MaterialRequirementDef {
                        item_definition_index: requirement.item_definition_index,
                        item_hash,
                        quantity: requirement.quantity,
                        delete_on_action: requirement.delete_on_action,
                        omit_from_requirements: requirement.omit_from_requirements,
                        condition: requirement.condition,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            Ok(CollectibleDef {
                index: collectible.index,
                hash: collectible.hash,
                item_definition_index: collectible.item_definition_index,
                item_hash,
                material_requirement_set_index: collectible.material_requirement_set_index,
                material_requirement_set_hash: collectible.material_requirement_set_hash,
                material_requirements,
                name: names.get(&item_hash).cloned().unwrap_or_default(),
                type_name: type_names.get(&item_hash).cloned().unwrap_or_default(),
                paths: collectible.paths,
                conditions: collectible.conditions,
            })
        })
        .collect()
}

impl Catalog {
    pub(crate) fn collectibles(&self) -> &[CollectibleDef] {
        &self.collectibles
    }

    pub(crate) fn shared_expression_pool(&self) -> &[Vec<CollectionConditionTokenDef>] {
        &self.shared_expression_pool
    }

    pub(crate) fn shared_expression(&self, index: usize) -> Option<&[CollectionConditionTokenDef]> {
        self.shared_expression_pool.get(index).map(Vec::as_slice)
    }

    pub(crate) fn material_requirement_sets(&self) -> &[MaterialRequirementSetDef] {
        &self.material_requirement_sets
    }

    pub(crate) fn material_requirement_set(
        &self,
        index: usize,
    ) -> Option<&MaterialRequirementSetDef> {
        self.material_requirement_sets.get(index)
    }

    pub(crate) fn item_material_requirement_set_indices(
        &self,
        hash: u64,
    ) -> Option<ItemMaterialRequirementSetIndices> {
        self.item_material_requirement_set_indices
            .get(&hash)
            .copied()
    }
}

pub(super) fn scan_collectibles(
    manager: &PackageManager,
    root: &[u8],
    presentation_nodes: &[PresentationNodeDef],
    material_requirement_sets: Option<&[PendingMaterialRequirementSet]>,
) -> Result<Vec<PendingCollectibleDef>, String> {
    let definitions = manager
        .read_tag(TagHash(investment_root_table_tag(
            root,
            ROOT_COLLECTIBLE_DEFINITION_TABLE_SLOT,
        )?))
        .map_err(|error| format!("Could not read collectible definitions: {error}"))?;
    pending_collectibles_from_data(&definitions, presentation_nodes, material_requirement_sets)
}

fn pending_collectibles_from_data(
    definitions: &[u8],
    presentation_nodes: &[PresentationNodeDef],
    material_requirement_sets: Option<&[PendingMaterialRequirementSet]>,
) -> Result<Vec<PendingCollectibleDef>, String> {
    let (count, rows, row_class) = array_at(definitions, 8)?;
    if row_class != COLLECTIBLE_DEFINITION_ROW_CLASS {
        return Err(format!(
            "Unexpected collectible row class 0x{row_class:08X}"
        ));
    }
    if count == 0 {
        return Err("Collectible definition table is empty".into());
    }
    if count > COLLECTIBLE_DEFINITION_CAPACITY {
        return Err("Collectible definition count exceeds its native capacity".into());
    }
    let mut output = Vec::new();
    for index in 0..count {
        let row = rows
            .checked_add(
                index
                    .checked_mul(COLLECTIBLE_DEFINITION_ROW_SIZE)
                    .ok_or("Collectible definition row offset overflowed")?,
            )
            .ok_or("Collectible definition row offset overflowed")?;
        let item_definition_index =
            u16_at(definitions, row + COLLECTIBLE_INVENTORY_ITEM_INDEX_OFFSET)?;
        let raw_material_requirement_set_index = u16_at(
            definitions,
            row + COLLECTIBLE_MATERIAL_REQUIREMENT_SET_INDEX_OFFSET,
        )?;
        let material_requirement_set_index = (raw_material_requirement_set_index != u16::MAX)
            .then_some(raw_material_requirement_set_index);
        let material_requirement_set = match (
            material_requirement_set_index,
            material_requirement_sets,
        ) {
            (Some(set_index), Some(material_requirement_sets)) => Some(
                material_requirement_sets
                    .get(usize::from(set_index))
                    .ok_or_else(|| {
                        format!(
                            "Collectible #{index} references material requirement set #{raw_material_requirement_set_index}, which is outside the package table"
                        )
                    })?,
            ),
            (None, _) | (Some(_), None) => None,
        };
        let parents = definition_index_list(
            definitions,
            row + COLLECTIBLE_PRESENTATION_NODE_PARENTS_OFFSET,
            PRESENTATION_NODE_INDEX_ROW_CLASS,
            presentation_nodes.len(),
            "collectible presentation-node parent",
        )?;
        let mut conditions = Vec::new();
        for (field, offset) in COLLECTIBLE_CONDITION_OFFSETS.into_iter().enumerate() {
            let tokens = condition_tokens_at(definitions, row + offset)?;
            if !tokens.is_empty() {
                conditions.push(CollectionConditionDef {
                    field: u8::try_from(field).expect("five collectible condition fields fit u8"),
                    tokens,
                });
            }
        }
        output.push(PendingCollectibleDef {
            index: u16::try_from(index).map_err(|_| "Collectible index is too large")?,
            hash: u64::from(u32_at(definitions, row + COLLECTIBLE_HASH_OFFSET)?),
            item_definition_index,
            material_requirement_set_index,
            material_requirement_set_hash: material_requirement_set.map_or(0, |set| set.hash),
            material_requirements: material_requirement_set
                .map_or_else(Vec::new, |set| set.requirements.clone()),
            paths: presentation_paths(presentation_nodes, &parents),
            conditions,
        });
    }
    Ok(output)
}

pub(super) fn scan_shared_expression_pool(
    manager: &PackageManager,
    root: &[u8],
) -> Result<Vec<Vec<CollectionConditionTokenDef>>, String> {
    let table_tag = investment_root_table_tag(root, ROOT_SHARED_EXPRESSION_POOL_TABLE_SLOT)?;
    if table_tag != SHARED_EXPRESSION_POOL_TABLE_TAG {
        return Err(format!(
            "Unexpected shared expression-pool tag 0x{table_tag:08X}"
        ));
    }
    let table = manager
        .read_tag(TagHash(table_tag))
        .map_err(|error| format!("Could not read shared expression pool: {error}"))?;
    shared_expression_pool_from_data(&table)
}

fn shared_expression_pool_from_data(
    table: &[u8],
) -> Result<Vec<Vec<CollectionConditionTokenDef>>, String> {
    let (count, rows, row_class) = array_at(table, 8)?;
    let (row_size, expression_offset) = match row_class {
        SHARED_EXPRESSION_POOL_HASHED_ROW_CLASS => (
            SHARED_EXPRESSION_POOL_HASHED_ROW_SIZE,
            SHARED_EXPRESSION_POOL_HASHED_EXPRESSION_OFFSET,
        ),
        SHARED_EXPRESSION_POOL_DIRECT_ROW_CLASS => (SHARED_EXPRESSION_POOL_DIRECT_ROW_SIZE, 0),
        _ => {
            return Err(format!(
                "Unexpected shared expression-pool row class 0x{row_class:08X}"
            ));
        }
    };
    if count != SHARED_EXPRESSION_POOL_COUNT {
        return Err(format!(
            "Shared expression-pool row count is {count}; expected {SHARED_EXPRESSION_POOL_COUNT}"
        ));
    }
    let byte_count = count
        .checked_mul(row_size)
        .ok_or("Shared expression-pool size overflowed")?;
    if rows
        .checked_add(byte_count)
        .is_none_or(|end| end > table.len())
    {
        return Err("Shared expression-pool rows extend beyond their package data".into());
    }

    (0..count)
        .map(|index| {
            let descriptor = rows + index * row_size + expression_offset;
            condition_tokens_at(table, descriptor)
                .map_err(|error| format!("Shared expression-pool row #{index}: {error}"))
        })
        .collect()
}

pub(super) fn scan_material_requirement_sets(
    manager: &PackageManager,
    root: &[u8],
) -> Result<Vec<PendingMaterialRequirementSet>, String> {
    let table = manager
        .read_tag(TagHash(investment_root_table_tag(
            root,
            ROOT_MATERIAL_REQUIREMENT_TABLE_SLOT,
        )?))
        .map_err(|error| format!("Could not read material requirement sets: {error}"))?;
    material_requirement_sets_from_data(&table)
}

fn material_requirement_sets_from_data(
    table: &[u8],
) -> Result<Vec<PendingMaterialRequirementSet>, String> {
    let (count, rows, row_class) = array_at(table, 8)?;
    if row_class != MATERIAL_REQUIREMENT_SET_ROW_CLASS {
        return Err(format!(
            "Unexpected material requirement set row class 0x{row_class:08X}"
        ));
    }
    if count == 0 {
        return Err("Material requirement set table is empty".into());
    }
    if count > MATERIAL_REQUIREMENT_SET_CAPACITY {
        return Err("Material requirement set count exceeds its native capacity".into());
    }
    let mut output = Vec::with_capacity(count);
    for index in 0..count {
        let row = rows
            .checked_add(
                index
                    .checked_mul(MATERIAL_REQUIREMENT_SET_ROW_SIZE)
                    .ok_or("Material requirement set row offset overflowed")?,
            )
            .ok_or("Material requirement set row offset overflowed")?;
        let hash = u64::from(u32_at(table, row)?);
        if hash == 0 {
            return Err(format!("Material requirement set #{index} has no hash"));
        }
        let pointer = row + 8;
        let descriptor = relative_offset(pointer, 0, i64_at(table, pointer)?)?;
        let descriptor_end = descriptor
            .checked_add(16)
            .ok_or("Material requirement descriptor offset overflowed")?;
        if table
            .get(descriptor..descriptor_end)
            .is_some_and(|bytes| bytes.iter().all(|byte| *byte == 0))
        {
            output.push(PendingMaterialRequirementSet {
                hash,
                requirements: Vec::new(),
            });
            continue;
        }
        let (requirement_count, requirement_rows, requirement_class) = array_at(table, descriptor)?;
        if requirement_class != MATERIAL_REQUIREMENT_ROW_CLASS {
            return Err(format!(
                "Material requirement set #{index} has unexpected row class 0x{requirement_class:08X}"
            ));
        }
        if requirement_count == 0 || requirement_count > MATERIAL_REQUIREMENT_CAPACITY {
            return Err(format!(
                "Material requirement set #{index} has invalid row count {requirement_count}"
            ));
        }
        let mut requirements = Vec::with_capacity(requirement_count);
        for requirement_index in 0..requirement_count {
            let requirement = requirement_rows
                .checked_add(
                    requirement_index
                        .checked_mul(MATERIAL_REQUIREMENT_ROW_SIZE)
                        .ok_or("Material requirement row offset overflowed")?,
                )
                .ok_or("Material requirement row offset overflowed")?;
            let item_definition_index = u32_at(table, requirement)?;
            requirements.push(PendingMaterialRequirementDef {
                item_definition_index: u16::try_from(item_definition_index).map_err(|_| {
                    format!(
                        "Material requirement set #{index} row #{requirement_index} has an invalid item definition index"
                    )
                })?,
                quantity: u32_at(table, requirement + 4)?,
                delete_on_action: bool_at(table, requirement + 8)?,
                omit_from_requirements: bool_at(table, requirement + 9)?,
                condition: u16_at(table, requirement + 10)?,
            });
        }
        output.push(PendingMaterialRequirementSet { hash, requirements });
    }
    Ok(output)
}

pub(super) fn materialize_material_requirement_sets(
    pending: Vec<PendingMaterialRequirementSet>,
    item_hashes: &[u64],
) -> Result<Vec<MaterialRequirementSetDef>, String> {
    pending
        .into_iter()
        .enumerate()
        .map(|(index, set)| {
            let requirements = set
                .requirements
                .into_iter()
                .enumerate()
                .map(|(row, requirement)| {
                    let item_hash = item_hashes
                        .get(usize::from(requirement.item_definition_index))
                        .copied()
                        .ok_or_else(|| {
                            format!(
                                "Material requirement set #{index} row #{row} references item definition index {}, which is outside the package table",
                                requirement.item_definition_index
                            )
                        })?;
                    Ok(MaterialRequirementDef {
                        item_definition_index: requirement.item_definition_index,
                        item_hash,
                        quantity: requirement.quantity,
                        delete_on_action: requirement.delete_on_action,
                        omit_from_requirements: requirement.omit_from_requirements,
                        condition: requirement.condition,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            Ok(MaterialRequirementSetDef {
                index: u16::try_from(index)
                    .map_err(|_| "Material requirement set index is too large")?,
                hash: set.hash,
                requirements,
            })
        })
        .collect()
}

fn condition_tokens_at(
    data: &[u8],
    descriptor: usize,
) -> Result<Vec<CollectionConditionTokenDef>, String> {
    if u64_at(data, descriptor)? == 0 {
        return Ok(Vec::new());
    }
    let (count, rows, row_class) = array_at(data, descriptor)?;
    if row_class != CONDITION_EXPRESSION_ROW_CLASS {
        return Err(format!(
            "Unexpected collectible condition row class 0x{row_class:08X}"
        ));
    }
    let byte_count = count
        .checked_mul(CONDITION_EXPRESSION_ROW_SIZE)
        .ok_or("Collectible condition size overflowed")?;
    if rows
        .checked_add(byte_count)
        .is_none_or(|end| end > data.len())
    {
        return Err("Collectible condition extends beyond its package data".into());
    }
    (0..count)
        .map(|index| {
            let row = rows + index * CONDITION_EXPRESSION_ROW_SIZE;
            let kind = data
                .get(row)
                .copied()
                .ok_or_else(|| format!("Package data ended at {row}"))?;
            Ok(CollectionConditionTokenDef {
                kind: u32::from(kind),
                // The native constant arm loads a dword. Slot and pool arms load a word.
                operand: if kind == 11 {
                    u32_at(data, row + 4)?
                } else {
                    u32::from(u16_at(data, row + 4)?)
                },
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collectible_scan_keeps_all_five_native_condition_fields() {
        assert_eq!(
            COLLECTIBLE_CONDITION_OFFSETS,
            [0x30, 0x40, 0x50, 0x60, 0x70]
        );
    }

    #[test]
    fn collectible_condition_tokens_preserve_package_order_and_operands() {
        let mut data = vec![0_u8; 96];
        data[0..8].copy_from_slice(&2_u64.to_le_bytes());
        data[8..16].copy_from_slice(&24_i64.to_le_bytes());
        data[32..40].copy_from_slice(&2_u64.to_le_bytes());
        data[40..44].copy_from_slice(&CONDITION_EXPRESSION_ROW_CLASS.to_le_bytes());
        data[48] = 1;
        data[49..52].copy_from_slice(&[0xA1, 0xB2, 0xC3]);
        data[52..54].copy_from_slice(&2003_u16.to_le_bytes());
        data[54..56].copy_from_slice(&[0xD4, 0xE5]);
        data[56] = 11;
        data[57..60].copy_from_slice(&[0xF6, 0x17, 0x28]);
        data[60..64].copy_from_slice(&0x4A39_002A_u32.to_le_bytes());

        assert_eq!(
            condition_tokens_at(&data, 0).unwrap(),
            vec![
                CollectionConditionTokenDef {
                    kind: 1,
                    operand: 2003,
                },
                CollectionConditionTokenDef {
                    kind: 11,
                    operand: 0x4A39_002A,
                },
            ]
        );
    }

    #[test]
    fn collectible_scan_survives_unavailable_material_requirement_enrichment() {
        let rows = 48;
        let mut data = vec![0_u8; rows + COLLECTIBLE_DEFINITION_ROW_SIZE];
        data[8..16].copy_from_slice(&1_u64.to_le_bytes());
        data[16..24].copy_from_slice(&16_i64.to_le_bytes());
        data[32..40].copy_from_slice(&1_u64.to_le_bytes());
        data[40..44].copy_from_slice(&COLLECTIBLE_DEFINITION_ROW_CLASS.to_le_bytes());
        data[rows + COLLECTIBLE_HASH_OFFSET..rows + COLLECTIBLE_HASH_OFFSET + 4]
            .copy_from_slice(&0x1234_5678_u32.to_le_bytes());
        data[rows + COLLECTIBLE_INVENTORY_ITEM_INDEX_OFFSET
            ..rows + COLLECTIBLE_INVENTORY_ITEM_INDEX_OFFSET + 2]
            .copy_from_slice(&42_u16.to_le_bytes());
        data[rows + COLLECTIBLE_MATERIAL_REQUIREMENT_SET_INDEX_OFFSET
            ..rows + COLLECTIBLE_MATERIAL_REQUIREMENT_SET_INDEX_OFFSET + 2]
            .copy_from_slice(&7_u16.to_le_bytes());

        let collectibles = pending_collectibles_from_data(&data, &[], None).unwrap();
        assert_eq!(collectibles.len(), 1);
        assert_eq!(collectibles[0].hash, 0x1234_5678);
        assert_eq!(collectibles[0].item_definition_index, 42);
        assert_eq!(collectibles[0].material_requirement_set_index, Some(7));
        assert_eq!(collectibles[0].material_requirement_set_hash, 0);
        assert!(collectibles[0].material_requirements.is_empty());

        let error = match pending_collectibles_from_data(&data, &[], Some(&[])) {
            Ok(_) => panic!("missing material enrichment unexpectedly succeeded"),
            Err(error) => error,
        };
        assert!(error.contains("material requirement set #7"));
    }

    #[test]
    fn shared_expression_pool_reads_nested_expression_descriptors() {
        for (row_class, row_size, expression_offset) in [
            (
                SHARED_EXPRESSION_POOL_HASHED_ROW_CLASS,
                SHARED_EXPRESSION_POOL_HASHED_ROW_SIZE,
                SHARED_EXPRESSION_POOL_HASHED_EXPRESSION_OFFSET,
            ),
            (
                SHARED_EXPRESSION_POOL_DIRECT_ROW_CLASS,
                SHARED_EXPRESSION_POOL_DIRECT_ROW_SIZE,
                0,
            ),
        ] {
            let rows = 48;
            let instruction_header = rows + SHARED_EXPRESSION_POOL_COUNT * row_size + 16;
            let instruction_rows = instruction_header + 16;
            let mut data = vec![0_u8; instruction_rows + CONDITION_EXPRESSION_ROW_SIZE];
            let pool_count = u64::try_from(SHARED_EXPRESSION_POOL_COUNT).unwrap();

            data[8..16].copy_from_slice(&pool_count.to_le_bytes());
            data[16..24].copy_from_slice(&16_i64.to_le_bytes());
            data[32..40].copy_from_slice(&pool_count.to_le_bytes());
            data[40..44].copy_from_slice(&row_class.to_le_bytes());

            // Pool row 0 points to one literal instruction after the outer rows.
            let descriptor = rows + expression_offset;
            data[descriptor..descriptor + 8].copy_from_slice(&1_u64.to_le_bytes());
            let pointer = descriptor + 8;
            let relative = i64::try_from(instruction_header - pointer).unwrap();
            data[pointer..pointer + 8].copy_from_slice(&relative.to_le_bytes());
            data[instruction_header..instruction_header + 8].copy_from_slice(&1_u64.to_le_bytes());
            data[instruction_header + 8..instruction_header + 12]
                .copy_from_slice(&CONDITION_EXPRESSION_ROW_CLASS.to_le_bytes());
            data[instruction_rows] = 11;
            data[instruction_rows + 4..instruction_rows + 6].copy_from_slice(&7_u16.to_le_bytes());

            // Pool row 1 is an empty expression descriptor.
            let pool = shared_expression_pool_from_data(&data).unwrap();
            assert_eq!(pool.len(), SHARED_EXPRESSION_POOL_COUNT);
            assert_eq!(
                pool[0],
                [CollectionConditionTokenDef {
                    kind: 11,
                    operand: 7,
                }]
            );
            assert!(pool[1].is_empty());
        }
    }

    #[test]
    fn material_requirements_preserve_every_sunrise_field() {
        let mut data = vec![0_u8; 124];

        // Top-level array descriptor at 8; its rows begin at 48.
        data[8..16].copy_from_slice(&1_u64.to_le_bytes());
        data[16..24].copy_from_slice(&16_i64.to_le_bytes());
        data[32..40].copy_from_slice(&1_u64.to_le_bytes());
        data[40..44].copy_from_slice(&MATERIAL_REQUIREMENT_SET_ROW_CLASS.to_le_bytes());

        // Material requirement set row. Its nested descriptor begins at 72.
        data[48..52].copy_from_slice(&0x1234_5678_u32.to_le_bytes());
        data[56..64].copy_from_slice(&16_i64.to_le_bytes());
        data[72..80].copy_from_slice(&1_u64.to_le_bytes());
        data[80..88].copy_from_slice(&16_i64.to_le_bytes());
        data[96..104].copy_from_slice(&1_u64.to_le_bytes());
        data[104..108].copy_from_slice(&MATERIAL_REQUIREMENT_ROW_CLASS.to_le_bytes());

        // Item definition index, quantity, flags, and condition.
        data[112..116].copy_from_slice(&321_u32.to_le_bytes());
        data[116..120].copy_from_slice(&7_u32.to_le_bytes());
        data[120] = 1;
        data[121] = 0;
        data[122..124].copy_from_slice(&42_u16.to_le_bytes());

        let sets = material_requirement_sets_from_data(&data).unwrap();
        assert_eq!(sets.len(), 1);
        assert_eq!(sets[0].hash, 0x1234_5678);
        assert_eq!(
            sets[0].requirements,
            [PendingMaterialRequirementDef {
                item_definition_index: 321,
                quantity: 7,
                delete_on_action: true,
                omit_from_requirements: false,
                condition: 42,
            }]
        );

        let materialized = materialize_material_requirement_sets(sets, &vec![0; 322]).unwrap();
        assert_eq!(materialized[0].index, 0);
        assert_eq!(materialized[0].hash, 0x1234_5678);
        assert_eq!(materialized[0].requirements[0].item_definition_index, 321);
    }

    #[test]
    fn item_material_requirement_links_use_sunrise_offsets_and_sentinels() {
        let mut item = vec![0_u8; ENABLED_MATERIAL_REQUIREMENT_SET_INDEX_OFFSET + 2];
        item[INSERTION_MATERIAL_REQUIREMENT_SET_INDEX_OFFSET
            ..INSERTION_MATERIAL_REQUIREMENT_SET_INDEX_OFFSET + 2]
            .copy_from_slice(&2_u16.to_le_bytes());
        item[ENABLED_MATERIAL_REQUIREMENT_SET_INDEX_OFFSET
            ..ENABLED_MATERIAL_REQUIREMENT_SET_INDEX_OFFSET + 2]
            .copy_from_slice(&u16::MAX.to_le_bytes());

        assert_eq!(
            item_material_requirement_set_indices_from_data(&item),
            Some(ItemMaterialRequirementSetIndices {
                insertion: Some(2),
                enabled: None,
            })
        );
        item[INSERTION_MATERIAL_REQUIREMENT_SET_INDEX_OFFSET
            ..INSERTION_MATERIAL_REQUIREMENT_SET_INDEX_OFFSET + 2]
            .copy_from_slice(&60_000_u16.to_le_bytes());
        assert_eq!(
            item_material_requirement_set_indices_from_data(&item),
            Some(ItemMaterialRequirementSetIndices {
                insertion: Some(60_000),
                enabled: None,
            })
        );
        assert_eq!(
            item_material_requirement_set_indices_from_data(&item[..188]),
            None
        );
    }
}

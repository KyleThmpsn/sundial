use serde::{Deserialize, Serialize};
use tiger_pkg::{PackageManager, TagHash};

use super::{
    package::{array_at, bool_at, i64_at, relative_offset, u16_at, u32_at, u64_at},
    progression::{
        PRESENTATION_NODE_INDEX_ROW_CLASS, PresentationNodeDef, definition_index_list,
        presentation_paths,
    },
};

const COLLECTIBLE_DEFINITION_TABLE_SLOT: usize = 19;
const COLLECTIBLE_DEFINITION_ROW_CLASS: u32 = 0x8080_3475;
const COLLECTIBLE_DEFINITION_ROW_SIZE: usize = 0xB8;
const COLLECTIBLE_PRESENTATION_NODE_PARENTS_OFFSET: usize = 0x18;
const COLLECTIBLE_HASH_OFFSET: usize = 0x28;
const COLLECTIBLE_INVENTORY_ITEM_INDEX_OFFSET: usize = 0x2C;
const COLLECTIBLE_MATERIAL_REQUIREMENT_SET_INDEX_OFFSET: usize = 0x9A;
const COLLECTIBLE_CONDITION_OFFSETS: [usize; 4] = [0x30, 0x40, 0x60, 0x70];
const CONDITION_EXPRESSION_ROW_CLASS: u32 = 0x8080_7D31;
const CONDITION_EXPRESSION_ROW_SIZE: usize = 8;
const MATERIAL_REQUIREMENT_TABLE_SLOT: usize = 96;
const MATERIAL_REQUIREMENT_SET_ROW_CLASS: u32 = 0x8080_7AD4;
const MATERIAL_REQUIREMENT_ROW_CLASS: u32 = 0x8080_7AD7;
const MATERIAL_REQUIREMENT_SET_ROW_SIZE: usize = 0x10;
const MATERIAL_REQUIREMENT_ROW_SIZE: usize = 0x0C;
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

pub(super) fn scan_collectibles(
    manager: &PackageManager,
    root: &[u8],
    presentation_nodes: &[PresentationNodeDef],
    material_requirement_sets: &[PendingMaterialRequirementSet],
) -> Result<Vec<PendingCollectibleDef>, String> {
    let definitions = manager
        .read_tag(TagHash(u32_at(
            root,
            8 + COLLECTIBLE_DEFINITION_TABLE_SLOT * 16,
        )?))
        .map_err(|error| format!("Could not read collectible definitions: {error}"))?;
    let (count, rows, row_class) = array_at(&definitions, 8)?;
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
            u16_at(&definitions, row + COLLECTIBLE_INVENTORY_ITEM_INDEX_OFFSET)?;
        let raw_material_requirement_set_index = u16_at(
            &definitions,
            row + COLLECTIBLE_MATERIAL_REQUIREMENT_SET_INDEX_OFFSET,
        )?;
        let material_requirement_set_index = (raw_material_requirement_set_index != u16::MAX)
            .then_some(raw_material_requirement_set_index);
        let material_requirement_set = material_requirement_set_index
            .map(|set_index| {
                material_requirement_sets
                    .get(usize::from(set_index))
                    .ok_or_else(|| {
                        format!(
                            "Collectible #{index} references material requirement set #{raw_material_requirement_set_index}, which is outside the package table"
                        )
                    })
            })
            .transpose()?;
        let parents = definition_index_list(
            &definitions,
            row + COLLECTIBLE_PRESENTATION_NODE_PARENTS_OFFSET,
            PRESENTATION_NODE_INDEX_ROW_CLASS,
            presentation_nodes.len(),
            "collectible presentation-node parent",
        )?;
        let mut conditions = Vec::new();
        for (field, offset) in COLLECTIBLE_CONDITION_OFFSETS.into_iter().enumerate() {
            let tokens = condition_tokens_at(&definitions, row + offset)?;
            if !tokens.is_empty() {
                conditions.push(CollectionConditionDef {
                    field: u8::try_from(field).expect("four collectible condition fields fit u8"),
                    tokens,
                });
            }
        }
        output.push(PendingCollectibleDef {
            index: u16::try_from(index).map_err(|_| "Collectible index is too large")?,
            hash: u64::from(u32_at(&definitions, row + COLLECTIBLE_HASH_OFFSET)?),
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

pub(super) fn scan_material_requirement_sets(
    manager: &PackageManager,
    root: &[u8],
) -> Result<Vec<PendingMaterialRequirementSet>, String> {
    let table = manager
        .read_tag(TagHash(u32_at(
            root,
            8 + MATERIAL_REQUIREMENT_TABLE_SLOT * 16,
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
            Ok(CollectionConditionTokenDef {
                kind: u32_at(data, row)?,
                operand: u32_at(data, row + 4)?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collectible_condition_tokens_preserve_package_order_and_operands() {
        let mut data = vec![0_u8; 96];
        data[0..8].copy_from_slice(&2_u64.to_le_bytes());
        data[8..16].copy_from_slice(&24_i64.to_le_bytes());
        data[32..40].copy_from_slice(&2_u64.to_le_bytes());
        data[40..44].copy_from_slice(&CONDITION_EXPRESSION_ROW_CLASS.to_le_bytes());
        data[48..52].copy_from_slice(&1_u32.to_le_bytes());
        data[52..56].copy_from_slice(&2003_u32.to_le_bytes());
        data[56..60].copy_from_slice(&11_u32.to_le_bytes());
        data[60..64].copy_from_slice(&42_u32.to_le_bytes());

        assert_eq!(
            condition_tokens_at(&data, 0).unwrap(),
            vec![
                CollectionConditionTokenDef {
                    kind: 1,
                    operand: 2003,
                },
                CollectionConditionTokenDef {
                    kind: 11,
                    operand: 42,
                },
            ]
        );
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
}

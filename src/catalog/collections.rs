use serde::{Deserialize, Serialize};
use tiger_pkg::{PackageManager, TagHash};

use super::{
    package::{array_at, u16_at, u32_at, u64_at},
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
const COLLECTIBLE_CONDITION_OFFSETS: [usize; 4] = [0x30, 0x40, 0x60, 0x70];
const CONDITION_EXPRESSION_ROW_CLASS: u32 = 0x8080_7D31;
const CONDITION_EXPRESSION_ROW_SIZE: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollectionConditionTokenDef {
    pub kind: u32,
    pub operand: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollectionConditionDef {
    pub field: u8,
    pub tokens: Vec<CollectionConditionTokenDef>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollectibleDef {
    pub index: u16,
    pub hash: u64,
    pub item_hash: u64,
    pub name: String,
    pub type_name: String,
    pub paths: Vec<Vec<String>>,
    pub conditions: Vec<CollectionConditionDef>,
}

pub(super) struct PendingCollectibleDef {
    pub index: u16,
    pub hash: u64,
    pub item_index: usize,
    pub paths: Vec<Vec<String>>,
    pub conditions: Vec<CollectionConditionDef>,
}

pub(super) fn scan_collectibles(
    manager: &PackageManager,
    root: &[u8],
    presentation_nodes: &[PresentationNodeDef],
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
    if count > usize::from(u16::MAX) + 1 {
        return Err("Collectible definition count exceeds its native index".into());
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
        let item_index = usize::from(u16_at(
            &definitions,
            row + COLLECTIBLE_INVENTORY_ITEM_INDEX_OFFSET,
        )?);
        if item_index == usize::from(u16::MAX) {
            continue;
        }
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
            item_index,
            paths: presentation_paths(presentation_nodes, &parents),
            conditions,
        });
    }
    Ok(output)
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
}

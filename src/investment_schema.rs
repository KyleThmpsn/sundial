//! Fixed-build native investment table shapes shared by readers and package authoring.

use crate::package_payload::{i64_at, relative_offset, u16_at, u32_at, u64_at};

pub const NESTED_ARRAY_TRAILER: [u8; 8] = [0, 0, 0, 0, 0xBD, 0x9F, 0x80, 0x80];

pub const INVESTMENT_ROOT_CLASS: u32 = 0x8080_7D84;
pub const INVESTMENT_ROOT_TABLE_TAGS_OFFSET: usize = 0x08;
pub const INVESTMENT_GLOBALS_TABLE_TAGS_OFFSET: usize = 0x10;
pub const INVESTMENT_TABLE_TAG_STRIDE: usize = 0x10;

pub const ROOT_ITEM_DEFINITION_TABLE_SLOT: usize = 48;
/// Root table that maps selected item hashes back to their signed 16-bit item-definition indices.
/// Its two arrays cover different item families; authored rows must stay in the donor's array.
pub const ROOT_ITEM_HASH_INDEX_TABLE_SLOT: usize = 50;
pub const ROOT_REUSABLE_PLUG_SET_TABLE_SLOT: usize = 51;
/// Native cap-definition array referenced by indices in each item's quality/version rows.
pub const ROOT_POWER_CAP_TABLE_SLOT: usize = 67;
pub const POWER_CAP_TABLE_CLASS: u32 = 0x8080_7797;
pub const POWER_CAP_ROW_CLASS: u32 = 0x8080_7801;
pub const POWER_CAP_ROW_SIZE: usize = 8;
pub const ROOT_SOCKET_ENTRY_LIST_TABLE_SLOT: usize = 97;
pub const GLOBALS_ITEM_STRING_TABLE_SLOT: usize = 33;
pub const GLOBALS_ITEM_METADATA_TABLE_SLOT: usize = 66;
pub const GLOBALS_SANDBOX_PATTERN_TABLE_SLOT: usize = 70;
/// Finished sandbox-perk catalog used by item-definition perk indices.
pub const GLOBALS_FINISHED_SANDBOX_PERK_TABLE_SLOT: usize = 71;
pub const GLOBALS_ITEM_DENSE_PRESENTATION_TABLE_SLOT: usize = 74;
pub const GLOBALS_ITEM_ICON_TABLE_SLOT: usize = 75;
pub const ROOT_ITEM_METADATA_INDEX_TABLE_SLOT: usize = 101;
/// Per-index sandbox-perk hash and damage-type metadata. This table is row-aligned with the
/// finished sandbox-perk catalog and must be extended whenever that catalog is extended.
pub const ROOT_SANDBOX_PERK_INDEX_TABLE_SLOT: usize = 106;
pub const ROOT_SANDBOX_PATTERN_INDEX_TABLE_SLOT: usize = 107;
pub const ROOT_COLLECTIBLE_DEFINITION_TABLE_SLOT: usize = 19;
pub const GLOBALS_COLLECTIBLE_DISPLAY_TABLE_SLOT: usize = 15;
pub const ROOT_OBJECTIVE_DEFINITION_TABLE_SLOT: usize = 58;
pub const GLOBALS_OBJECTIVE_STRING_TABLE_SLOT: usize = 38;
pub const ROOT_RECORD_DEFINITION_TABLE_SLOT: usize = 72;
pub const GLOBALS_RECORD_STRING_TABLE_SLOT: usize = 49;
pub const ROOT_PRESENTATION_NODE_DEFINITION_TABLE_SLOT: usize = 63;
pub const GLOBALS_PRESENTATION_NODE_STRING_TABLE_SLOT: usize = 41;
pub const ROOT_SHARED_EXPRESSION_POOL_TABLE_SLOT: usize = 109;
pub const GLOBALS_LOCALIZED_STRING_INDEX_TABLE_SLOT: usize = 72;
pub const ROOT_MATERIAL_REQUIREMENT_TABLE_SLOT: usize = 96;
pub const ROOT_UNLOCK_FLAG_BANK_TABLE_SLOT: usize = 111;
pub const ROOT_UNLOCK_FLAG_DEFINITION_TABLE_SLOT: usize = 112;
pub const GLOBALS_UNLOCK_FLAG_DISPLAY_TABLE_SLOT: usize = 78;
pub const LOCALIZED_STRING_INDEX_ROW_SIZE: usize = 0x08;
pub const LOCALIZED_STRING_INDEX_ROW_CLASS: u32 = 0x8080_5F9E;

/// Reads one package tag from the fixed investment-root tag array.
pub fn investment_root_table_tag(data: &[u8], slot: usize) -> Result<u32, String> {
    investment_table_tag(data, INVESTMENT_ROOT_TABLE_TAGS_OFFSET, slot)
}

/// Reads one package tag from the fixed investment-globals tag array.
pub fn investment_globals_table_tag(data: &[u8], slot: usize) -> Result<u32, String> {
    investment_table_tag(data, INVESTMENT_GLOBALS_TABLE_TAGS_OFFSET, slot)
}

fn investment_table_tag(data: &[u8], base: usize, slot: usize) -> Result<u32, String> {
    let offset = slot
        .checked_mul(INVESTMENT_TABLE_TAG_STRIDE)
        .and_then(|offset| base.checked_add(offset))
        .ok_or("Investment table-tag slot offset overflowed")?;
    u32_at(data, offset)
}

pub const ITEM_DEFINITION_INDEX_ROW_CLASS: u32 = 0x8080_7BE8;
pub const ITEM_STRING_INDEX_ROW_CLASS: u32 = 0x8080_5CDF;
pub const ITEM_INDEX_ROW_SIZE: usize = 0x18;
pub const ITEM_HASH_INDEX_TABLE_CLASS: u32 = 0x8080_7BD9;
pub const ITEM_HASH_INDEX_ROW_CLASS: u32 = 0x8080_7BE3;
pub const ITEM_HASH_INDEX_ROW_SIZE: usize = 0x08;
pub const ITEM_EQUIPMENT_BLOCK_POINTER_OFFSET: usize = 0x10;
pub const ITEM_EQUIPMENT_BLOCK_CLASS: u32 = 0x8080_7C02;
pub const ITEM_EQUIPMENT_SLOT_OFFSET: usize = 0x18;
pub const ITEM_EQUIPMENT_SLOT_SENTINEL_OFFSET: usize = ITEM_EQUIPMENT_SLOT_OFFSET + 2;
pub const ITEM_INVESTMENT_STAT_POINTER_OFFSET: usize = 0x70;
pub const ITEM_SOCKET_ENTRY_LIST_BLOCK_POINTER_OFFSET: usize = 0x80;
pub const ITEM_SOCKET_ENTRY_LIST_INDEX_OFFSET: usize = 0x00;
pub const ITEM_SOCKET_ENTRY_LIST_BLOCK_SIZE: usize = 0x0C;
pub const ITEM_TRANSLATION_BLOCK_POINTER_OFFSET: usize = 0x88;
pub const ITEM_DEFINITION_HASH_OFFSET: usize = 0xA0;
pub const ITEM_MAX_STACK_SIZE_OFFSET: usize = 0xB4;
pub const ITEM_INVENTORY_SLOT_OFFSET: usize = 0xB8;
pub const ITEM_RARITY_OFFSET: usize = 0xBA;
pub const ITEM_INSTANCED_OFFSET: usize = 0xBB;
pub const ITEM_STRING_ICON_INDEX_OFFSET: usize = 0x80;
pub const ITEM_STRING_NAME_REFERENCE_OFFSET: usize = 0x84;
pub const ITEM_STRING_TYPE_REFERENCE_OFFSET: usize = 0x90;
/// Item-specific UI template hash, consumed by the native inspection list renderer.
pub const ITEM_STRING_UI_TEMPLATE_HASH_OFFSET: usize = 0xCC;
pub const ITEM_STRING_DESCRIPTION_REFERENCE_OFFSET: usize = 0x98;
pub const ITEM_STRING_SOURCE_REFERENCE_OFFSET: usize = 0xA0;
pub const ITEM_STRING_STAT_GROUP_POINTER_OFFSET: usize = 0x70;
pub const ITEM_STRING_STAT_GROUP_RESOURCE_CLASS: u32 = 0x8080_5CF1;
pub const ITEM_STRING_STAT_GROUP_INDEX_OFFSET: usize = 0x14;
pub const ITEM_STRING_AMMO_CLASS_OFFSET: usize = 0x13C;
pub const ITEM_STRING_AMMO_TYPE_OFFSET: usize = 0x140;
pub const ITEM_STRING_AMMO_CLASS: u32 = 0x8080_5D1A;
pub const ITEM_ICON_ROW_SIZE: usize = 0x18;
pub const ITEM_ICON_ROW_CLASS: u32 = 0x8080_2957;
pub const ITEM_ICON_CONTAINER_OFFSET: usize = 0x10;
pub const ITEM_TRAITS_DESCRIPTOR_OFFSET: usize = 0xE0;
pub const ITEM_TRAIT_ROW_CLASS: u32 = 0x8080_2C50;
pub const ITEM_TRAIT_ROW_SIZE: usize = std::mem::size_of::<u16>();
pub const ITEM_INVESTMENT_STAT_RESOURCE_CLASS: u32 = 0x8080_77B9;
pub const ITEM_INVESTMENT_STAT_ROW_CLASS: u32 = 0x8080_3033;
pub const ITEM_INVESTMENT_STAT_ROW_SIZE: usize = 0x28;
pub const ITEM_SANDBOX_PERK_DESCRIPTOR_OFFSET: usize = 0x10;
pub const ITEM_SANDBOX_PERK_ROW_CLASS: u32 = 0x8080_77BC;
pub const ITEM_SANDBOX_PERK_ROW_SIZE: usize = 0x18;
pub const LEGACY_ARC_DAMAGE_PERK_INDEX: u16 = 83;
pub const LEGACY_SOLAR_DAMAGE_PERK_INDEX: u16 = 84;
pub const LEGACY_VOID_DAMAGE_PERK_INDEX: u16 = 85;
pub const MODERN_ARC_DAMAGE_PERK_INDEX: u16 = 449;
pub const MODERN_SOLAR_DAMAGE_PERK_INDEX: u16 = 450;
pub const MODERN_VOID_DAMAGE_PERK_INDEX: u16 = 451;
pub const ELEMENTAL_DAMAGE_SOCKET_TYPE: u16 = 68;
pub const ARC_DAMAGE_PLUG_ITEM_INDEX: u16 = 1505;
pub const SOLAR_DAMAGE_PLUG_ITEM_INDEX: u16 = 1506;
pub const VOID_DAMAGE_PLUG_ITEM_INDEX: u16 = 1507;
pub const ARC_DAMAGE_PLUG_ITEM_HASH: u32 = 0xF5EF_60B6;
pub const SOLAR_DAMAGE_PLUG_ITEM_HASH: u32 = 0x8782_99D7;
pub const VOID_DAMAGE_PLUG_ITEM_HASH: u32 = 0xDE3F_F704;
pub const ITEM_PLUG_CATEGORY_FALLBACK_OFFSET: usize = 0x188;
pub const ITEM_PLUG_BLOCK_CLASS: u32 = 0x8080_77E3;
pub const ITEM_PLUG_BLOCK_SEARCH_START: usize = 0x100;
pub const ITEM_PLUG_BLOCK_SEARCH_END: usize = 0x300;
pub const ITEM_PLUG_BLOCK_CATEGORY_OFFSET: usize = 0x04;
pub const ITEM_PLUG_BLOCK_ROLL_SET_OFFSET: usize = 0x26;
pub const ITEM_LINKED_PLUG_BLOCK_CLASS: u32 = 0x8080_3036;
pub const ITEM_LINKED_PLUG_INDEX_OFFSET: usize = 0x0C;
pub const ITEM_QUALITY_BLOCK_POINTER_OFFSET: usize = 0x48;
pub const ITEM_QUALITY_VERSION_DESCRIPTOR_OFFSET: usize = 0x60;
pub const ITEM_VERSION_ROW_CLASS: u32 = 0x8080_5921;
pub const ITEM_VERSION_ROW_SIZE: usize = std::mem::size_of::<u16>();
pub const ITEM_VERSION_MAX_COUNT: usize = 16;

pub const ITEM_TRANSLATION_BLOCK_CLASS: u32 = 0x8080_77AF;
pub const ITEM_TRANSLATION_BLOCK_SIZE: usize = 0x60;
pub const ITEM_TRANSLATION_ART_DESCRIPTOR_OFFSET: usize = 0x00;
pub const ITEM_TRANSLATION_ART_ROW_CLASS: u32 = 0x8080_77B5;
pub const ITEM_TRANSLATION_ART_ROW_SIZE: usize = 0x04;
pub const ITEM_TRANSLATION_ART_VARIANT_OFFSET: usize = 0x02;
pub const ITEM_TRANSLATION_DYE_DESCRIPTOR_OFFSETS: [usize; 3] = [0x28, 0x38, 0x48];
pub const ITEM_TRANSLATION_DYE_ROW_CLASS: u32 = 0x8080_77B3;
pub const ITEM_TRANSLATION_DYE_ROW_SIZE: usize = 0x04;
pub const ITEM_TRANSLATION_DYE_VARIANT_OFFSET: usize = 0x02;
pub const ITEM_TRANSLATION_WEAPON_PATTERN_INDEX_OFFSET: usize = 0x58;

pub const ITEM_ORDINARY_SOCKET_POINTER_OFFSET: usize = 0x68;
pub const ITEM_ORDINARY_SOCKET_ROW_CLASS: u32 = 0x8080_77C4;
pub const ITEM_ORDINARY_SOCKET_ROW_SIZE: usize = 0x50;
pub const ITEM_ORDINARY_SOCKET_DEFAULT_PLUG_OFFSET: usize = 0x02;
pub const ITEM_ORDINARY_SOCKET_REUSABLE_PLUG_SET_OFFSET: usize = 0x0C;
pub const ITEM_ORDINARY_SOCKET_RANDOMIZED_SELECTION_PROGRAM_OFFSET: usize = 0x10;
pub const ITEM_ORDINARY_SOCKET_RANDOMIZED_PLUG_SET_OFFSET: usize = 0x20;
pub const ITEM_ORDINARY_SOCKET_EMBEDDED_PLUGS_OFFSET: usize = 0x40;
pub const ITEM_ORDINARY_SOCKET_PLUG_MEMBER_ROW_CLASS: u32 = 0x8080_2E03;
pub const ITEM_ORDINARY_SOCKET_PLUG_MEMBER_ROW_SIZE: usize = 0x20;
pub const ITEM_ORDINARY_SOCKET_PLUG_MEMBER_WEIGHT_OFFSET: usize = 0x18;

pub const COLLECTIBLE_DEFINITION_ROW_CLASS: u32 = 0x8080_3475;
pub const COLLECTIBLE_DEFINITION_ROW_SIZE: usize = 0xB8;
pub const COLLECTIBLE_PRESENTATION_NODE_PARENTS_OFFSET: usize = 0x18;
pub const COLLECTIBLE_HASH_OFFSET: usize = 0x28;
pub const COLLECTIBLE_INVENTORY_ITEM_INDEX_OFFSET: usize = 0x2C;
pub const COLLECTIBLE_CONDITION_OFFSETS: [usize; 5] = [0x30, 0x40, 0x50, 0x60, 0x70];
pub const COLLECTIBLE_MATERIAL_REQUIREMENT_SET_INDEX_OFFSET: usize = 0x9A;
pub const COLLECTIBLE_DISPLAY_ROW_CLASS: u32 = 0x8080_2E5F;
pub const COLLECTIBLE_DISPLAY_ROW_SIZE: usize = 0x60;
pub const COLLECTIBLE_DISPLAY_ICON_INDEX_OFFSET: usize = 0x04;
pub const COLLECTIBLE_DISPLAY_NAME_REFERENCE_OFFSET: usize = 0x08;
pub const COLLECTIBLE_DISPLAY_DESCRIPTION_REFERENCE_OFFSET: usize = 0x10;
pub const COLLECTIBLE_DISPLAY_SOURCE_REFERENCE_OFFSET: usize = 0x18;
pub const COLLECTIBLE_DISPLAY_REQUIREMENT_REFERENCE_OFFSET: usize = 0x20;

pub const MATERIAL_REQUIREMENT_SET_ROW_CLASS: u32 = 0x8080_7AD4;
pub const MATERIAL_REQUIREMENT_SET_ROW_SIZE: usize = 0x10;
pub const MATERIAL_REQUIREMENT_ROW_CLASS: u32 = 0x8080_7AD7;
pub const MATERIAL_REQUIREMENT_ROW_SIZE: usize = 0x0C;

pub const OBJECTIVE_DEFINITION_ROW_CLASS: u32 = 0x8080_775F;
pub const OBJECTIVE_DEFINITION_ROW_SIZE: usize = 0xA0;
pub const OBJECTIVE_COMPLETION_VALUE_OFFSET: usize = 0x30;
pub const OBJECTIVE_STRING_ROW_CLASS: u32 = 0x8080_59F0;
pub const OBJECTIVE_STRING_ROW_SIZE: usize = 0x40;
pub const OBJECTIVE_STRING_NAME_REFERENCE_OFFSET: usize = 0x08;
pub const OBJECTIVE_STRING_DESCRIPTION_REFERENCE_OFFSET: usize = 0x10;
pub const OBJECTIVE_STRING_PROGRESS_REFERENCE_OFFSET: usize = 0x18;

pub const PRESENTATION_NODE_DEFINITION_ROW_CLASS: u32 = 0x8080_3056;
pub const PRESENTATION_NODE_DEFINITION_ROW_SIZE: usize = 0xA8;
pub const PRESENTATION_NODE_STRING_ROW_SIZE: usize = 0x2C;
pub const PRESENTATION_NODE_HASH_OFFSET: usize = 0x28;
pub const PRESENTATION_NODE_PARENTS_OFFSET: usize = 0x18;
pub const PRESENTATION_NODE_OBJECTIVE_INDEX_OFFSET: usize = 0x50;
pub const PRESENTATION_NODE_INDEX_ROW_CLASS: u32 = 0x8080_3962;

pub const RECORD_DEFINITION_ROW_SIZE: usize = 0xD8;
pub const RECORD_STRING_ROW_SIZE: usize = 0x80;
pub const RECORD_HASH_OFFSET: usize = 0x28;
pub const RECORD_OBJECTIVE_INDEX_ROW_CLASS: u32 = 0x8080_7455;

pub const CONDITION_EXPRESSION_ROW_CLASS: u32 = 0x8080_7D31;
pub const CONDITION_EXPRESSION_ROW_SIZE: usize = 0x08;

pub const SHARED_EXPRESSION_POOL_HASHED_ROW_CLASS: u32 = 0x8080_7C4F;
pub const SHARED_EXPRESSION_POOL_HASHED_ROW_SIZE: usize = 0x18;
pub const SHARED_EXPRESSION_POOL_HASHED_EXPRESSION_OFFSET: usize = 0x08;
pub const SHARED_EXPRESSION_POOL_DIRECT_ROW_CLASS: u32 = 0x8080_7D30;
pub const SHARED_EXPRESSION_POOL_DIRECT_ROW_SIZE: usize = 0x10;
pub const SHARED_EXPRESSION_POOL_COUNT: usize = 6_541;
pub const SHARED_EXPRESSION_POOL_PARALLEL_ROW_CLASS: u32 = 0x8080_0006;

pub const UNLOCK_FLAG_DEFINITION_ROW_CLASS: u32 = 0x8080_7D4F;
pub const UNLOCK_FLAG_DEFINITION_ROW_SIZE: usize = 0x08;
pub const UNLOCK_FLAG_SORTED_INDEX_ROW_CLASS: u32 = 0x8080_0006;
pub const UNLOCK_FLAG_SORTED_INDEX_ROW_SIZE: usize = std::mem::size_of::<u16>();
pub const UNLOCK_FLAG_DISPLAY_ROW_CLASS: u32 = 0x8080_5EAF;
pub const UNLOCK_FLAG_DISPLAY_ROW_SIZE: usize = 0x10;
pub const UNLOCK_FLAG_DISPLAY_CONTENT_ROW_CLASS: u32 = 0x8080_5EB1;
pub const UNLOCK_FLAG_DISPLAY_CONTENT_ROW_SIZE: usize = 0x14;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ItemVersionArray {
    pub rows: usize,
    pub groups: Vec<u16>,
}

/// Decodes the version-group array rooted in an item's quality block.
pub fn item_version_array(data: &[u8]) -> Result<Option<ItemVersionArray>, String> {
    let quality_relative = i64_at(data, ITEM_QUALITY_BLOCK_POINTER_OFFSET)?;
    if quality_relative == 0 {
        return Ok(None);
    }
    let quality = relative_offset(ITEM_QUALITY_BLOCK_POINTER_OFFSET, 0, quality_relative)?;
    let descriptor = quality
        .checked_add(ITEM_QUALITY_VERSION_DESCRIPTOR_OFFSET)
        .ok_or("Item quality version descriptor overflowed")?;
    let count_raw = u64_at(data, descriptor)?;
    let count = usize::try_from(count_raw).map_err(|_| "Item version count is too large")?;
    if count == 0 {
        return Ok(None);
    }
    if count > ITEM_VERSION_MAX_COUNT {
        return Err("Item quality block has too many version rows".into());
    }
    let pointer = descriptor
        .checked_add(8)
        .ok_or("Item version pointer overflowed")?;
    let header = relative_offset(pointer, 0, i64_at(data, pointer)?)?;
    if u64_at(data, header)? != count_raw {
        return Err("Item version descriptor and header counts disagree".into());
    }
    let row_class = u32_at(
        data,
        header
            .checked_add(8)
            .ok_or("Item version row-class offset overflowed")?,
    )?;
    if row_class != ITEM_VERSION_ROW_CLASS {
        return Err(format!(
            "Item quality block has unexpected version row class 0x{row_class:08X}"
        ));
    }
    let rows = header
        .checked_add(16)
        .ok_or("Item version row offset overflowed")?;
    let rows_end = rows
        .checked_add(
            count
                .checked_mul(ITEM_VERSION_ROW_SIZE)
                .ok_or("Item version row extent overflowed")?,
        )
        .ok_or("Item version row extent overflowed")?;
    if rows_end > data.len() {
        return Err("Item version rows extend beyond the item definition".into());
    }
    let groups = (0..count)
        .map(|index| u16_at(data, rows + index * ITEM_VERSION_ROW_SIZE))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Some(ItemVersionArray { rows, groups }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item_with_versions(groups: &[u16]) -> Vec<u8> {
        const QUALITY: usize = 0xC0;
        const HEADER: usize = 0x140;
        const ROWS: usize = HEADER + 16;
        let descriptor = QUALITY + ITEM_QUALITY_VERSION_DESCRIPTOR_OFFSET;
        let mut data = vec![0; ROWS + groups.len() * ITEM_VERSION_ROW_SIZE];
        data[ITEM_QUALITY_BLOCK_POINTER_OFFSET..ITEM_QUALITY_BLOCK_POINTER_OFFSET + 8]
            .copy_from_slice(
                &(QUALITY as i64 - ITEM_QUALITY_BLOCK_POINTER_OFFSET as i64).to_le_bytes(),
            );
        data[descriptor..descriptor + 8].copy_from_slice(&(groups.len() as u64).to_le_bytes());
        data[descriptor + 8..descriptor + 16]
            .copy_from_slice(&(HEADER as i64 - (descriptor + 8) as i64).to_le_bytes());
        data[HEADER..HEADER + 8].copy_from_slice(&(groups.len() as u64).to_le_bytes());
        data[HEADER + 8..HEADER + 12].copy_from_slice(&ITEM_VERSION_ROW_CLASS.to_le_bytes());
        for (index, group) in groups.iter().enumerate() {
            let row = ROWS + index * ITEM_VERSION_ROW_SIZE;
            data[row..row + ITEM_VERSION_ROW_SIZE].copy_from_slice(&group.to_le_bytes());
        }
        data
    }

    #[test]
    fn item_versions_are_rooted_in_the_quality_block() {
        let data = item_with_versions(&[8, 11]);
        let decoded = item_version_array(&data).unwrap().unwrap();
        assert_eq!(decoded.groups, [8, 11]);

        let mut unrelated = vec![0; 0x180];
        unrelated[0x100..0x108].copy_from_slice(&1_u64.to_le_bytes());
        unrelated[0x108..0x110].copy_from_slice(&8_i64.to_le_bytes());
        unrelated[0x110..0x118].copy_from_slice(&1_u64.to_le_bytes());
        unrelated[0x118..0x11C].copy_from_slice(&ITEM_VERSION_ROW_CLASS.to_le_bytes());
        unrelated[0x120..0x122].copy_from_slice(&11_u16.to_le_bytes());
        assert_eq!(item_version_array(&unrelated).unwrap(), None);
    }

    #[test]
    fn malformed_rooted_version_arrays_are_rejected() {
        let mut data = item_with_versions(&[11]);
        data[0x148..0x14C].copy_from_slice(&0_u32.to_le_bytes());
        assert!(item_version_array(&data).is_err());
    }

    #[test]
    fn investment_table_tags_use_the_documented_roots_and_stride() {
        let mut globals = vec![0; INVESTMENT_GLOBALS_TABLE_TAGS_OFFSET + 3 * 16];
        globals[INVESTMENT_GLOBALS_TABLE_TAGS_OFFSET + 2 * INVESTMENT_TABLE_TAG_STRIDE
            ..INVESTMENT_GLOBALS_TABLE_TAGS_OFFSET + 2 * INVESTMENT_TABLE_TAG_STRIDE + 4]
            .copy_from_slice(&0x8132_57A2_u32.to_le_bytes());
        assert_eq!(
            investment_globals_table_tag(&globals, 2).unwrap(),
            0x8132_57A2
        );
        assert!(investment_root_table_tag(&[], usize::MAX).is_err());
    }
}

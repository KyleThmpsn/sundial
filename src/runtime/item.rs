//! Resolve an investment item through its authoritative sandbox-pattern selector.
use super::{WeaponRuntimeEntitySource, load_weapon_runtime_entity_at_pattern_index_with_manager};
use crate::{
    investment::schema::{
        ITEM_DEFINITION_HASH_OFFSET, ITEM_DEFINITION_INDEX_ROW_CLASS, ITEM_INDEX_ROW_SIZE,
        ITEM_TRANSLATION_BLOCK_CLASS, ITEM_TRANSLATION_BLOCK_POINTER_OFFSET,
        ITEM_TRANSLATION_BLOCK_SIZE, ITEM_TRANSLATION_WEAPON_PATTERN_INDEX_OFFSET,
        ROOT_ITEM_DEFINITION_TABLE_SLOT, investment_globals_table_tag, investment_root_table_tag,
    },
    package_payload::{array_at, bytes_at, i64_at, relative_offset, rows_fit, u16_at, u32_at},
    package_runtime::{reader::PackageManager, resolve_live_named_tag},
};
use tiger_pkg::TagHash;

/// Load a weapon's runtime using the selector in its investment definition. The returned source
/// keeps the selected pattern's identity, which can differ from the requested item hash.
pub fn load(manager: &PackageManager, item_hash: u32) -> Result<WeaponRuntimeEntitySource, String> {
    pattern_index(manager, item_hash)
        .and_then(|index| load_weapon_runtime_entity_at_pattern_index_with_manager(manager, index))
        .map_err(|error| format!("Weapon item 0x{item_hash:08X}: {error}"))
}

fn pattern_index(manager: &PackageManager, item_hash: u32) -> Result<u16, String> {
    let globals = manager.read_tag(resolve_live_named_tag(manager, "investment_globals", None)?)?;
    let root = manager.read_tag(TagHash(investment_globals_table_tag(&globals, 0)?))?;
    let items = manager.read_tag(TagHash(investment_root_table_tag(
        &root,
        ROOT_ITEM_DEFINITION_TABLE_SLOT,
    )?))?;
    let (count, rows, class) = array_at(&items, 8)?;
    if class != ITEM_DEFINITION_INDEX_ROW_CLASS {
        return Err(format!("Item table has unexpected row class 0x{class:08X}"));
    }
    rows_fit(&items, rows, count, ITEM_INDEX_ROW_SIZE)?;
    let mut definition_tag = None;
    for index in 0..count {
        let row = rows + index * ITEM_INDEX_ROW_SIZE;
        if u32_at(&items, row)? == item_hash {
            definition_tag = Some(TagHash(u32_at(&items, row + 16)?));
            break;
        }
    }
    let definition = manager.read_tag(definition_tag.ok_or("Item definition is missing")?)?;
    if u32_at(&definition, ITEM_DEFINITION_HASH_OFFSET)? != item_hash {
        return Err("Item definition does not match its investment identity".into());
    }
    let pointer = ITEM_TRANSLATION_BLOCK_POINTER_OFFSET;
    let relative = i64_at(&definition, pointer)?;
    if relative == 0 {
        return Err("Item has no translation block".into());
    }
    let translation = relative_offset(pointer, 0, relative)?;
    let marker = translation
        .checked_sub(4)
        .ok_or("Translation block has no class marker")?;
    if translation % 8 != 0 || u32_at(&definition, marker)? != ITEM_TRANSLATION_BLOCK_CLASS {
        return Err("Item has no recognized weapon translation block".into());
    }
    bytes_at::<ITEM_TRANSLATION_BLOCK_SIZE>(&definition, translation)?;
    let index = u16_at(
        &definition,
        translation + ITEM_TRANSLATION_WEAPON_PATTERN_INDEX_OFFSET,
    )?;
    if index == u16::MAX {
        return Err("Weapon sandbox-pattern selector is disabled".into());
    }
    Ok(index)
}

//! Reads the lore inherited from an installed item definition.
use std::{collections::HashMap, path::Path};

use tiger_pkg::{PackageManager, TagHash};

use crate::{
    investment_localization::decode_strings,
    investment_schema::{
        GLOBALS_LOCALIZED_STRING_INDEX_TABLE_SLOT, ITEM_DEFINITION_HASH_OFFSET,
        ITEM_DEFINITION_INDEX_ROW_CLASS, ITEM_INDEX_ROW_SIZE, LOCALIZED_STRING_INDEX_ROW_CLASS,
        LOCALIZED_STRING_INDEX_ROW_SIZE, ROOT_ITEM_DEFINITION_TABLE_SLOT,
        investment_globals_table_tag, investment_root_table_tag,
    },
    package_payload::{array_at, i64_at, relative_offset, u16_at, u32_at, u64_at},
    package_runtime::resolve_live_named_tag,
};

#[derive(Clone, Debug)]
pub struct LoreEntry {
    pub title: String,
    pub text: String,
}

/// Returns `None` only when the item has no lore reference.
pub fn load_item_lore(packages: &Path, item_hash: u32) -> Result<Option<LoreEntry>, String> {
    let manager = crate::package_authoring::open_shadowkeep_package_manager(packages)?;
    let globals = read(
        &manager,
        resolve_live_named_tag(&manager, "investment_globals", None)?,
    )?;
    let root = read(
        &manager,
        TagHash(investment_globals_table_tag(&globals, 0)?),
    )?;
    let items = read(
        &manager,
        TagHash(investment_root_table_tag(
            &root,
            ROOT_ITEM_DEFINITION_TABLE_SLOT,
        )?),
    )?;
    let (count, rows) = table(&items, ITEM_DEFINITION_INDEX_ROW_CLASS, ITEM_INDEX_ROW_SIZE)?;
    let mut definition_tag = None;
    for index in 0..count {
        let row = rows + index * ITEM_INDEX_ROW_SIZE;
        if u32_at(&items, row)? == item_hash {
            definition_tag = Some(TagHash(u32_at(&items, row + 16)?));
            break;
        }
    }
    let definition = read(
        &manager,
        definition_tag.ok_or_else(|| format!("Lore source 0x{item_hash:08X} is missing"))?,
    )?;
    if u32_at(&definition, ITEM_DEFINITION_HASH_OFFSET)? != item_hash {
        return Err("Lore source definition does not match the selected weapon".into());
    }
    let pointer = i64_at(&definition, 0x28)?;
    if pointer == 0 {
        return Ok(None);
    }
    let block = relative_offset(0x28, 0, pointer)?;
    if block < 4 || u32_at(&definition, block - 4)? != 0x8080_77ED {
        return Err("Unsupported item lore block".into());
    }
    let index = u16_at(&definition, block)?;
    if index == u16::MAX {
        return Ok(None);
    }
    let definitions = read(&manager, TagHash(investment_root_table_tag(&root, 52)?))?;
    let display = read(
        &manager,
        TagHash(investment_globals_table_tag(&globals, 34)?),
    )?;
    let (count, definitions_start) = table(&definitions, 0x8080_77F4, 16)?;
    let (display_count, display_start) = table(&display, 0x8080_5ABA, 40)?;
    if count != display_count || usize::from(index) >= count {
        return Err("Lore reference is outside the installed tables".into());
    }
    let definition_row = definitions_start + usize::from(index) * 16;
    let display_row = display_start + usize::from(index) * 40;
    if u64_at(&definitions, definition_row)? != 16
        || u64_at(&display, display_row)? != 40
        || u32_at(&definitions, definition_row + 8)? != u32_at(&display, display_row + 8)?
    {
        return Err("Lore definition and display rows do not match".into());
    }
    let strings = read(
        &manager,
        TagHash(investment_globals_table_tag(
            &globals,
            GLOBALS_LOCALIZED_STRING_INDEX_TABLE_SLOT,
        )?),
    )?;
    let (string_count, string_rows) = table(
        &strings,
        LOCALIZED_STRING_INDEX_ROW_CLASS,
        LOCALIZED_STRING_INDEX_ROW_SIZE,
    )?;
    let mut cache = HashMap::new();
    let mut resolve = |offset| -> Result<String, String> {
        let bank = u32_at(&display, display_row + offset)? as usize;
        let hash = u32_at(&display, display_row + offset + 4)?;
        if bank >= string_count {
            return Err("Lore text references a missing language table".into());
        }
        if let std::collections::hash_map::Entry::Vacant(entry) = cache.entry(bank) {
            let tag = TagHash(u32_at(
                &strings,
                string_rows + bank * LOCALIZED_STRING_INDEX_ROW_SIZE + 4,
            )?);
            entry.insert(
                decode_strings(&manager, tag)?
                    .into_iter()
                    .collect::<HashMap<_, _>>(),
            );
        }
        cache[&bank]
            .get(&hash)
            .cloned()
            .ok_or_else(|| format!("Lore text 0x{hash:08X} is unavailable"))
    };
    Ok(Some(LoreEntry {
        title: resolve(12)?,
        text: resolve(28)?,
    }))
}

fn read(manager: &PackageManager, tag: TagHash) -> Result<Vec<u8>, String> {
    manager
        .read_tag(tag)
        .map_err(|error| format!("Could not read lore data {tag}: {error}"))
}

fn table(data: &[u8], expected_class: u32, stride: usize) -> Result<(usize, usize), String> {
    let (count, rows, class) = array_at(data, 8)?;
    let end = count
        .checked_mul(stride)
        .and_then(|length| rows.checked_add(length));
    if class != expected_class || end.is_none_or(|end| end > data.len()) {
        return Err("Unsupported lore source table layout".into());
    }
    Ok((count, rows))
}

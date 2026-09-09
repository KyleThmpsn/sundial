//! Shared readers for Destiny's localized investment-string tables.

use std::collections::{HashMap, hash_map::Entry};

use tiger_pkg::{PackageManager, TagHash};

use crate::{
    investment_schema::{
        GLOBALS_LOCALIZED_STRING_INDEX_TABLE_SLOT, ITEM_STRING_NAME_REFERENCE_OFFSET,
        LOCALIZED_STRING_INDEX_ROW_SIZE, investment_globals_table_tag,
    },
    package_payload::{array_at, i64_at, relative_offset, u16_at, u32_at},
    package_runtime::resolve_live_named_tag,
};

pub(crate) type LocalizedStringCache = HashMap<u32, HashMap<u32, String>>;

pub(crate) fn resolve_string(
    manager: &PackageManager,
    tags: &[TagHash],
    cache: &mut LocalizedStringCache,
    data: &[u8],
    offset: usize,
) -> Option<String> {
    let index = u32_at(data, offset).ok()?;
    if index == 0xFFFF || index as usize >= tags.len() {
        return None;
    }
    let hash = u32_at(data, offset + 4).ok()?;
    if let Entry::Vacant(entry) = cache.entry(index) {
        let values = decode_strings(manager, tags[index as usize])
            .ok()?
            .into_iter()
            .collect();
        entry.insert(values);
    }
    cache.get(&index)?.get(&hash).cloned()
}

pub(crate) fn resolve_localized_hash(
    manager: &PackageManager,
    tags: &[TagHash],
    cache: &mut LocalizedStringCache,
    indices: &[u32],
    hash: u32,
) -> Option<String> {
    for &index in indices {
        if index as usize >= tags.len() {
            continue;
        }
        if let Entry::Vacant(entry) = cache.entry(index) {
            let values = decode_strings(manager, tags[index as usize])
                .ok()?
                .into_iter()
                .collect();
            entry.insert(values);
        }
        if let Some(value) = cache.get(&index).and_then(|values| values.get(&hash)) {
            return Some(value.clone());
        }
    }
    None
}

/// Resolves the display name referenced by an existing item-string tag.
pub fn resolve_item_name(manager: &PackageManager, string_tag: TagHash) -> Result<String, String> {
    let globals_tag = resolve_live_named_tag(manager, "investment_globals", None)?;
    let globals = read_tag(manager, globals_tag, "investment globals")?;
    let localized_index_tag = TagHash(investment_globals_table_tag(
        &globals,
        GLOBALS_LOCALIZED_STRING_INDEX_TABLE_SLOT,
    )?);
    let localized_index = read_tag(manager, localized_index_tag, "localized-string index")?;
    let (tag_count, tag_rows, _) = array_at(&localized_index, 8)?;

    let item_string = read_tag(manager, string_tag, "item-string")?;
    let table_index = u32_at(&item_string, ITEM_STRING_NAME_REFERENCE_OFFSET)? as usize;
    let name_hash = u32_at(&item_string, ITEM_STRING_NAME_REFERENCE_OFFSET + 4)?;
    if table_index >= tag_count {
        return Err(format!(
            "Item-string tag {string_tag} references missing localized-string table {table_index}"
        ));
    }
    let tag_offset = tag_rows
        .checked_add(
            table_index
                .checked_mul(LOCALIZED_STRING_INDEX_ROW_SIZE)
                .ok_or("Localized-string index row offset overflowed")?,
        )
        .and_then(|offset| offset.checked_add(4))
        .ok_or("Localized-string index row offset overflowed")?;
    let localized_header_tag = TagHash(u32_at(&localized_index, tag_offset)?);
    if !localized_header_tag.is_some() {
        return Err(format!(
            "Item-string tag {string_tag} references an empty localized-string table"
        ));
    }

    decode_strings(manager, localized_header_tag)?
        .into_iter()
        .find_map(|(hash, value)| (hash == name_hash).then_some(value))
        .ok_or_else(|| {
            format!("Item-string tag {string_tag} has unresolved name hash 0x{name_hash:08X}")
        })
}

fn read_tag(manager: &PackageManager, tag: TagHash, description: &str) -> Result<Vec<u8>, String> {
    manager
        .read_tag(tag)
        .map_err(|error| format!("Could not read {description} tag {tag}: {error}"))
}

fn decode_strings(manager: &PackageManager, tag: TagHash) -> Result<Vec<(u32, String)>, String> {
    let header = manager.read_tag(tag).map_err(|error| error.to_string())?;
    let (hash_count, hash_data, _) = array_at(&header, 8)?;
    let data = manager
        .read_tag(TagHash(u32_at(&header, 24)?))
        .map_err(|error| error.to_string())?;
    let (part_count, parts, _) = array_at(&data, 8)?;
    let (combo_count, combos, _) = array_at(&data, 0x48)?;
    if hash_count != combo_count {
        return Err("Localized string table mismatch".into());
    }
    let mut result = Vec::with_capacity(hash_count);
    for index in 0..combo_count {
        let combo = combos + index * 0x10;
        let first = relative_offset(combo, 0, i64_at(&data, combo)?)?;
        let count = usize::try_from(i64_at(&data, combo + 8)?)
            .map_err(|_| "Localized string part count is negative or too large")?;
        let selected_bytes = count
            .checked_mul(0x20)
            .ok_or("Localized string part range overflowed")?;
        let selected_end = first
            .checked_add(selected_bytes)
            .ok_or("Localized string part range overflowed")?;
        let parts_bytes = part_count
            .checked_mul(0x20)
            .ok_or("Localized string table range overflowed")?;
        let parts_end = parts
            .checked_add(parts_bytes)
            .ok_or("Localized string table range overflowed")?;
        if first < parts || selected_end > parts_end {
            continue;
        }
        let mut value = Vec::new();
        for part_index in 0..count {
            let part = first
                .checked_add(
                    part_index
                        .checked_mul(0x20)
                        .ok_or("Localized string part offset overflowed")?,
                )
                .ok_or("Localized string part offset overflowed")?;
            let part_pointer = part
                .checked_add(8)
                .ok_or("Localized string pointer overflowed")?;
            let start = relative_offset(part, 8, i64_at(&data, part_pointer)?)?;
            let len = u16_at(&data, part + 0x14)? as usize;
            // The high byte is significant for Destiny's private-use symbol
            // glyphs, which are rendered with the installed PC symbol font.
            let shift = u16_at(&data, part + 0x18)?;
            let Some(end) = start.checked_add(len) else {
                continue;
            };
            let Some(bytes) = data.get(start..end) else {
                continue;
            };
            for character in String::from_utf8_lossy(bytes).chars() {
                let shifted = shift_localized_character(character, shift);
                let mut encoded = [0; 4];
                value.extend_from_slice(shifted.encode_utf8(&mut encoded).as_bytes());
            }
        }
        result.push((
            u32_at(&header, hash_data + index * 4)?,
            String::from_utf8_lossy(&value).into_owned(),
        ));
    }
    Ok(result)
}

fn shift_localized_character(character: char, shift: u16) -> char {
    char::from_u32(character as u32 + u32::from(shift)).unwrap_or(character)
}

#[cfg(test)]
mod tests {
    use super::shift_localized_character;

    #[test]
    fn localized_string_shift_preserves_destiny_symbol_codepoints() {
        assert_eq!(shift_localized_character('\u{1}', 0xE142), '\u{E143}');
        assert_eq!(shift_localized_character('\u{1}', 0x001F), ' ');
    }
}

//! Installed record rows that grant character titles.
mod icons;
mod unlocks;

pub(crate) use unlocks::Unlock;

use crate::{
    investment_localization::{LocalizedStringCache, resolve_string},
    investment_schema::*,
    package_payload::{array_at, u16_at, u32_at},
    package_runtime::resolve_live_named_tag,
};
use std::path::Path;
use tiger_pkg::TagHash;

#[derive(Clone, Debug)]
pub(crate) struct Title {
    pub index: u16,
    pub name: String,
    pub description: String,
    pub icon_container: Option<u32>,
    pub unlock: Result<Unlock, String>,
}

pub(crate) fn load(packages: &Path) -> Result<Vec<Title>, String> {
    let manager = crate::package_authoring::open_shadowkeep_package_manager(packages)?;
    let read = |tag| {
        manager
            .read_tag(TagHash(tag))
            .map_err(|error| format!("Could not read title data: {error}"))
    };
    let globals = read(resolve_live_named_tag(&manager, "investment_globals", None)?.0)?;
    let root = read(investment_globals_table_tag(&globals, 0)?)?;
    let unlocks = unlocks::Mapping::load(&manager, &root)?;
    let icons = icons::load(&manager, &root, &globals)?;
    let records = read(investment_root_table_tag(
        &root,
        ROOT_RECORD_DEFINITION_TABLE_SLOT,
    )?)?;
    let display = read(investment_globals_table_tag(
        &globals,
        GLOBALS_RECORD_STRING_TABLE_SLOT,
    )?)?;
    let (count, rows, _) = array_at(&records, 8)?;
    let (display_count, display_rows, class) = array_at(&display, 8)?;
    if count > u16::MAX as usize
        || count != display_count
        || class != 0x8080_5A99
        || rows
            .checked_add(count * RECORD_DEFINITION_ROW_SIZE)
            .is_none_or(|end| end > records.len())
        || display_rows
            .checked_add(count * RECORD_STRING_ROW_SIZE)
            .is_none_or(|end| end > display.len())
    {
        return Err("The installed title tables do not match".into());
    }
    let strings = read(investment_globals_table_tag(
        &globals,
        GLOBALS_LOCALIZED_STRING_INDEX_TABLE_SLOT,
    )?)?;
    let (string_count, string_rows, string_class) = array_at(&strings, 8)?;
    if string_class != LOCALIZED_STRING_INDEX_ROW_CLASS
        || string_count
            .checked_mul(LOCALIZED_STRING_INDEX_ROW_SIZE)
            .and_then(|length| string_rows.checked_add(length))
            .is_none_or(|end| end > strings.len())
    {
        return Err("The installed title localization table is invalid".into());
    }
    let tags = (0..string_count)
        .map(|index| {
            u32_at(
                &strings,
                string_rows + index * LOCALIZED_STRING_INDEX_ROW_SIZE + 4,
            )
            .map(TagHash)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut cache = LocalizedStringCache::new();
    let mut titles = Vec::new();
    for index in 0..count {
        let row = rows + index * RECORD_DEFINITION_ROW_SIZE;
        let string = display_rows + index * RECORD_STRING_ROW_SIZE;
        if u32_at(&records, row + RECORD_HASH_OFFSET)? != u32_at(&display, string)? {
            return Err(format!("Title row {index} does not match its display text"));
        }
        if u32_at(&records, row + 0xB8)? == 0 {
            continue;
        }
        // These are the title's localized variants. Offset 0x08 names its triumph.
        let name = [0x70, 0x78]
            .into_iter()
            .filter_map(|offset| {
                resolve_string(&manager, &tags, &mut cache, &display, string + offset)
            })
            .find(|name| !name.trim().is_empty())
            .unwrap_or_else(|| format!("Title {index}"));
        titles.push(Title {
            index: index as u16,
            name,
            description: resolve_string(&manager, &tags, &mut cache, &display, string + 8)
                .unwrap_or_default(),
            icon_container: icons.get(&(index as u16)).copied(),
            unlock: unlocks.resolve(u16_at(&records, row + 100)?),
        });
    }
    titles.sort_by_cached_key(|title| (title.name.to_lowercase(), title.index));
    Ok(titles)
}

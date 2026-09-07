//! Checked native roots and aligned item/string table inputs.
use crate::investment_schema::{
    GLOBALS_ITEM_STRING_TABLE_SLOT, GLOBALS_LOCALIZED_STRING_INDEX_TABLE_SLOT,
    INVESTMENT_ROOT_CLASS, ITEM_DEFINITION_INDEX_ROW_CLASS, ITEM_INDEX_ROW_SIZE,
    ITEM_STRING_INDEX_ROW_CLASS, ROOT_ITEM_DEFINITION_TABLE_SLOT,
    ROOT_REUSABLE_PLUG_SET_TABLE_SLOT, ROOT_SOCKET_ENTRY_LIST_TABLE_SLOT,
    investment_globals_table_tag, investment_root_table_tag,
};
use crate::{
    package_payload::{array_at, u32_at},
    package_runtime,
};
use std::path::Path;
use tiger_pkg::{PackageManager, TagHash};

pub(super) struct Sources {
    pub manager: PackageManager,
    pub globals_data: Vec<u8>,
    pub root: Vec<u8>,
    pub localized_tags: Vec<TagHash>,
}

pub(super) fn read(install: &Path) -> Result<Sources, String> {
    let manager = package_runtime::open_shadowkeep_packages(install)?;
    let globals = package_runtime::resolve_live_named_tag(&manager, "investment_globals", None)?;
    let globals_data = manager
        .read_tag(globals)
        .map_err(|e| format!("Could not read investment globals: {e}"))?;
    let localized_index = manager
        .read_tag(TagHash(investment_globals_table_tag(
            &globals_data,
            GLOBALS_LOCALIZED_STRING_INDEX_TABLE_SLOT,
        )?))
        .map_err(|e| format!("Could not read localized-string index: {e}"))?;
    let (localized_count, localized_rows, _) = array_at(&localized_index, 8)?;
    let localized_tags: Vec<TagHash> = (0..localized_count)
        .map(|index| {
            u32_at(&localized_index, localized_rows + index * 8 + 4)
                .map(TagHash)
                .map_err(|error| format!("Localized-string bank row {index} is malformed: {error}"))
        })
        .collect::<Result<_, _>>()?;
    let root_tag = TagHash(investment_globals_table_tag(&globals_data, 0)?);
    let root_entry = manager
        .get_entry(root_tag)
        .ok_or_else(|| format!("Investment root {root_tag:?} is not live"))?;
    if root_entry.reference != INVESTMENT_ROOT_CLASS {
        return Err(format!(
            "Investment root {root_tag:?} has class 0x{:08X}, expected 0x{INVESTMENT_ROOT_CLASS:08X}",
            root_entry.reference,
        ));
    }
    let root = manager
        .read_tag(root_tag)
        .map_err(|e| format!("Could not read investment root: {e}"))?;
    Ok(Sources {
        manager,
        globals_data,
        root,
        localized_tags,
    })
}

pub(super) struct ItemTables {
    pub plug_set_table: Vec<u8>,
    pub reusable_plug_set_count: usize,
    pub socket_entry_list_count: usize,
    pub string_map: Vec<u8>,
    pub string_count: usize,
    pub string_rows: usize,
    pub hashes: Vec<u64>,
    pub definition_tags: Vec<u32>,
}

impl ItemTables {
    pub(super) fn read(sources: &Sources) -> Result<Self, String> {
        let Sources {
            manager,
            root,
            globals_data,
            ..
        } = sources;
        let plug_set_table = manager
            .read_tag(TagHash(investment_root_table_tag(
                root,
                ROOT_REUSABLE_PLUG_SET_TABLE_SLOT,
            )?))
            .map_err(|e| format!("Could not read reusable plug sets: {e}"))?;
        let (reusable_plug_set_count, _, _) = array_at(&plug_set_table, 8)?;
        let socket_entry_list_table = manager
            .read_tag(TagHash(investment_root_table_tag(
                root,
                ROOT_SOCKET_ENTRY_LIST_TABLE_SLOT,
            )?))
            .map_err(|e| format!("Could not read socket-entry lists: {e}"))?;
        let (socket_entry_list_count, _, _) = array_at(&socket_entry_list_table, 8)?;
        let item_table = manager
            .read_tag(TagHash(investment_root_table_tag(
                root,
                ROOT_ITEM_DEFINITION_TABLE_SLOT,
            )?))
            .map_err(|e| format!("Could not read item table: {e}"))?;
        let string_map = manager
            .read_tag(TagHash(investment_globals_table_tag(
                globals_data,
                GLOBALS_ITEM_STRING_TABLE_SLOT,
            )?))
            .map_err(|e| format!("Could not read item strings: {e}"))?;
        let PairedItemRows {
            count,
            rows,
            string_rows,
        } = validate_item_tables(&item_table, &string_map)?;
        let string_count = count;
        let hashes: Vec<u64> = (0..count)
            .map(|i| u32_at(&item_table, rows + i * 24).map(u64::from))
            .collect::<Result<_, _>>()?;
        let definition_tags: Vec<u32> = (0..count)
            .map(|i| u32_at(&item_table, rows + i * 24 + 16))
            .collect::<Result<_, _>>()?;
        Ok(Self {
            plug_set_table,
            reusable_plug_set_count,
            socket_entry_list_count,
            string_map,
            string_count,
            string_rows,
            hashes,
            definition_tags,
        })
    }
}

struct PairedItemRows {
    count: usize,
    rows: usize,
    string_rows: usize,
}

fn validate_item_tables(item_table: &[u8], string_map: &[u8]) -> Result<PairedItemRows, String> {
    let (count, rows, item_row_class) = array_at(item_table, 8)?;
    let (string_count, string_rows, string_row_class) = array_at(string_map, 8)?;
    let item_rows_end = rows
        .checked_add(
            count
                .checked_mul(ITEM_INDEX_ROW_SIZE)
                .ok_or("Installed item table extent overflowed")?,
        )
        .ok_or("Installed item table extent overflowed")?;
    let string_rows_end = string_rows
        .checked_add(
            string_count
                .checked_mul(ITEM_INDEX_ROW_SIZE)
                .ok_or("Installed item-string table extent overflowed")?,
        )
        .ok_or("Installed item-string table extent overflowed")?;
    if item_row_class != ITEM_DEFINITION_INDEX_ROW_CLASS
        || string_row_class != ITEM_STRING_INDEX_ROW_CLASS
        || item_rows_end != item_table.len()
        || string_rows_end != string_map.len()
    {
        return Err(
            "The installed item and item-string tables have unexpected native layouts".into(),
        );
    }
    if count != string_count {
        return Err("The installed item and string tables do not match".into());
    }
    for index in 0..count {
        let item_hash = u32_at(item_table, rows + index * 24)?;
        let string_hash = u32_at(string_map, string_rows + index * 24)?;
        if item_hash != string_hash {
            return Err(format!(
                "The installed item and string tables diverge at row {index}: gameplay 0x{item_hash:08X}, strings 0x{string_hash:08X}"
            ));
        }
    }

    Ok(PairedItemRows {
        count,
        rows,
        string_rows,
    })
}

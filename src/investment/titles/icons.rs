use crate::{
    catalog::scan_item_icon_containers,
    investment_schema::*,
    package_payload::{array_at, u16_at, u32_at},
};
use std::collections::BTreeMap;
use tiger_pkg::{PackageManager, TagHash};

const RECORD_INDEX_OFFSET: usize = 0x52;
const STRING_ICON_OFFSET: usize = 0x04;
const STRING_ROW_CLASS: u32 = 0x8080_2E15;

pub(super) fn load(
    manager: &PackageManager,
    root: &[u8],
    globals: &[u8],
) -> Result<BTreeMap<u16, u32>, String> {
    let read = |tag| {
        manager
            .read_tag(TagHash(tag))
            .map_err(|error| format!("Could not read title badges: {error}"))
    };
    let nodes = read(investment_root_table_tag(
        root,
        ROOT_PRESENTATION_NODE_DEFINITION_TABLE_SLOT,
    )?)?;
    let strings = read(investment_globals_table_tag(
        globals,
        GLOBALS_PRESENTATION_NODE_STRING_TABLE_SLOT,
    )?)?;
    let (count, rows, class) = array_at(&nodes, 8)?;
    let (string_count, string_rows, string_class) = array_at(&strings, 8)?;
    if class != PRESENTATION_NODE_DEFINITION_ROW_CLASS
        || string_class != STRING_ROW_CLASS
        || count != string_count
        || count
            .checked_mul(PRESENTATION_NODE_DEFINITION_ROW_SIZE)
            .and_then(|length| rows.checked_add(length))
            .is_none_or(|end| end > nodes.len())
        || count
            .checked_mul(PRESENTATION_NODE_STRING_ROW_SIZE)
            .and_then(|length| string_rows.checked_add(length))
            .is_none_or(|end| end > strings.len())
    {
        return Err("The installed title badge tables do not match".into());
    }
    let containers = scan_item_icon_containers(manager, globals)?;
    let mut icons = BTreeMap::new();
    for index in 0..count {
        let row = rows + index * PRESENTATION_NODE_DEFINITION_ROW_SIZE;
        let string = string_rows + index * PRESENTATION_NODE_STRING_ROW_SIZE;
        if u32_at(&nodes, row + PRESENTATION_NODE_HASH_OFFSET)? != u32_at(&strings, string)? {
            return Err(format!(
                "Title badge row {index} does not match its display data"
            ));
        }
        // A seal points to the record that awards its title.
        let record = u16_at(&nodes, row + RECORD_INDEX_OFFSET)?;
        let icon = u16_at(&strings, string + STRING_ICON_OFFSET)?;
        if record == u16::MAX || icon == u16::MAX {
            continue;
        }
        let container = containers
            .get(usize::from(icon))
            .ok_or_else(|| format!("Title badge row {index} references an unavailable icon"))?;
        if let Some(container) = container {
            icons.entry(record).or_insert(*container);
        }
    }
    Ok(icons)
}

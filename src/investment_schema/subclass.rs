//! Shadowkeep subclass equip conditions, separate from the class of the ability grid.
use super::*;
use crate::package_payload::native_array_at;

/// Reads the stock subclass's single class-flag requirement, or no requirement for an
/// unrestricted subclass. Unknown condition shapes are rejected rather than called neutral.
pub fn subclass_equipment_class(data: &[u8]) -> Result<Option<u8>, String> {
    let equipment = relative_offset(
        ITEM_EQUIPMENT_BLOCK_POINTER_OFFSET,
        0,
        i64_at(data, ITEM_EQUIPMENT_BLOCK_POINTER_OFFSET)?,
    )?;
    if equipment < 4 || u32_at(data, equipment - 4)? != ITEM_EQUIPMENT_BLOCK_CLASS {
        return Err("Subclass has no recognized equipment block".into());
    }
    let (count, _, groups, class) = native_array_at(data, equipment)?;
    if class != 0x8080_7D2F {
        return Err("Subclass equipment conditions have an unsupported group class".into());
    }
    if count == 0 {
        return Ok(None);
    }
    if count != 1 {
        return Err("Subclass equipment has additional requirements".into());
    }
    let (count, _, rows, class) = native_array_at(data, groups)?;
    if class != CONDITION_EXPRESSION_ROW_CLASS || count != 1 || u32_at(data, rows)? != 1 {
        return Err("Subclass equipment has an unsupported class condition".into());
    }
    // Stock unlock-flag rows: Titan Class, Hunter Class and Warlock Class. These are
    // table indexes in the Shadowkeep corpus, not item hashes or list indexes.
    match u32_at(data, rows + 4)? {
        0x10E => Ok(Some(0)),
        0x0F5 => Ok(Some(1)),
        0x115 => Ok(Some(2)),
        _ => Err("Subclass equipment condition names no recognized class flag".into()),
    }
}

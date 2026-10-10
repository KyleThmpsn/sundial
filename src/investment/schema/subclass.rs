//! Shadowkeep subclass equip conditions, separate from the class of the ability grid.
use super::*;
use crate::package_payload::native_array_at;

/// Stock unlock-flag rows: Titan Class, Hunter Class and Warlock Class. These are table indexes in
/// the Shadowkeep corpus, not item hashes or list indexes.
const CLASS_FLAGS: [u32; 3] = [0x10E, 0x0F5, 0x115];

/// Reads the stock subclass's single class-flag requirement, or no requirement for an
/// unrestricted subclass. Unknown condition shapes are rejected rather than called neutral.
pub fn subclass_equipment_class(data: &[u8]) -> Result<Option<u8>, String> {
    let Some(flag) = class_flag_offset(data)? else {
        return Ok(None);
    };
    let flag = u32_at(data, flag)?;
    CLASS_FLAGS
        .iter()
        .position(|each| *each == flag)
        .map(|class| u8::try_from(class).ok())
        .ok_or_else(|| "Subclass equipment condition names no recognized class flag".into())
}

/// Points a subclass's class-flag requirement at `class`: 0 Titan, 1 Hunter, 2 Warlock. The
/// condition keeps its shape, so only the flag it names changes. A subclass with no requirement
/// is refused, since there is no condition to point.
pub fn set_subclass_equipment_class(data: &mut [u8], class: u8) -> Result<(), String> {
    let flag = *CLASS_FLAGS
        .get(usize::from(class))
        .ok_or("Subclass class must be Titan, Hunter or Warlock")?;
    subclass_equipment_class(data)?.ok_or("Subclass has no class requirement to change")?;
    let offset = class_flag_offset(data)?.ok_or("Subclass has no class requirement to change")?;
    data.get_mut(offset..offset + 4)
        .ok_or("Subclass class flag is truncated")?
        .copy_from_slice(&flag.to_le_bytes());
    Ok(())
}

/// Where the subclass's single class-flag condition names its flag, or none for an unrestricted
/// subclass.
fn class_flag_offset(data: &[u8]) -> Result<Option<usize>, String> {
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
    Ok(Some(rows + 4))
}

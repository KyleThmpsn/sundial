//! Shadowkeep artifact class restrictions use the class unlock flags of stock helmets.
//!
//! Functional artifacts are bucket 49 with equipment slot `0112`. The stock ones carry no equip
//! condition, and armor's class-and-slot flags have no artifact member. Their equipment block
//! is the one stock armor and subclasses restrict with a single class-flag instruction. A
//! restriction on a subclass's class flag showed Requires with no class name on the item's
//! tooltip, where stock armor names its class, so an artifact takes the helmet flag of its
//! class. An artifact written with a subclass flag still reads as that class.
use super::armor::{append_array, descriptor};
use super::subclass::CLASS_FLAGS;
use super::*;
use crate::package_payload::native_array_at;
use crate::package_payload::{bytes_at, write_bytes};

const GROUP_CLASS: u32 = 0x8080_7D2F;
const BUCKET: u8 = 49;
const SLOT: u16 = 0x0112;
/// The helmet flags of Titan, Hunter and Warlock, the first of each class's five armor flags
/// in bucket order.
const HELMET_FLAGS: [u32; 3] = [0x10C, 0x0F3, 0x113];

fn equipment(data: &[u8]) -> Result<usize, String> {
    if bytes_at::<1>(data, ITEM_INVENTORY_SLOT_OFFSET)?[0] != BUCKET {
        return Err("Item is outside the artifact bucket".into());
    }
    let equipment = relative_offset(
        ITEM_EQUIPMENT_BLOCK_POINTER_OFFSET,
        0,
        i64_at(data, ITEM_EQUIPMENT_BLOCK_POINTER_OFFSET)?,
    )?;
    if equipment < 4
        || u32_at(data, equipment - 4)? != ITEM_EQUIPMENT_BLOCK_CLASS
        || u16_at(data, equipment + 0x18)? != SLOT
    {
        return Err("Artifact has no functional equipment block".into());
    }
    Ok(equipment)
}

/// Returns the class a functional artifact requires, or no restriction. Any other equip
/// condition is rejected rather than called unrestricted.
pub fn artifact_equipment_class(data: &[u8]) -> Result<Option<u8>, String> {
    let equipment = equipment(data)?;
    if u64_at(data, equipment)? == 0 {
        return Ok(None);
    }
    let (count, _, groups, class) = native_array_at(data, equipment)?;
    if class != GROUP_CLASS || count != 1 {
        return Err("Artifact has unsupported equip conditions".into());
    }
    let (count, _, rows, class) = native_array_at(data, groups)?;
    if class != CONDITION_EXPRESSION_ROW_CLASS || count != 1 || u32_at(data, rows)? != 1 {
        return Err("Artifact has an unsupported equip condition".into());
    }
    let flag = u32_at(data, rows + 4)?;
    HELMET_FLAGS
        .iter()
        .position(|each| *each == flag)
        .or_else(|| CLASS_FLAGS.iter().position(|each| *each == flag))
        .map(|class| u8::try_from(class).ok())
        .ok_or_else(|| "Artifact equip condition names no class flag".into())
}

/// Give a functional artifact one class requirement, or none. Other equipment fields keep their
/// bytes. The caller finalizes the returned payload size.
pub fn set_artifact_equipment_class(data: &[u8], selected: Option<u8>) -> Result<Vec<u8>, String> {
    let flag = selected
        .map(|class| {
            HELMET_FLAGS
                .get(usize::from(class))
                .copied()
                .ok_or("Artifact class must be Titan, Hunter or Warlock")
        })
        .transpose()?;
    // An unknown existing condition is refused here, so it is never replaced.
    artifact_equipment_class(data)?;
    let equipment = equipment(data)?;
    let mut authored = data.to_vec();
    match flag {
        None => write_bytes(&mut authored, equipment, &[0; 16])?,
        Some(flag) => {
            let header = append_array(&mut authored, GROUP_CLASS, 1, &[0; 16])?;
            descriptor(&mut authored, equipment, 1, header)?;
            let mut token = [0u8; 8];
            write_bytes(&mut token, 0, &1u32.to_le_bytes())?;
            write_bytes(&mut token, 4, &flag.to_le_bytes())?;
            let program = append_array(&mut authored, CONDITION_EXPRESSION_ROW_CLASS, 1, &token)?;
            descriptor(&mut authored, header + 16, 1, program)?;
        }
    }
    if artifact_equipment_class(&authored)? != selected {
        return Err("Authored artifact did not retain its selected class".into());
    }
    Ok(authored)
}

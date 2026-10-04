//! Shadowkeep armor class restrictions are native class-and-slot equip flags.
use super::*;
use crate::package_payload::native_array_at;
use crate::package_payload::{bytes_at, write_bytes};

const GROUP_CLASS: u32 = 0x8080_7D2F;

fn flag_class(flag: u32) -> Option<u8> {
    match flag {
        0x109..=0x10D => Some(0),
        0x0F0..=0x0F4 => Some(1),
        0x110..=0x114 => Some(2),
        _ => None,
    }
}

struct Requirements {
    equipment: usize,
    programs: Vec<Vec<[u8; 8]>>,
}

fn requirements(data: &[u8]) -> Result<Requirements, String> {
    if !matches!(bytes_at::<1>(data, ITEM_INVENTORY_SLOT_OFFSET)?[0], 3..=7) {
        return Err("Armor has no supported inventory slot".into());
    }
    let equipment = relative_offset(
        ITEM_EQUIPMENT_BLOCK_POINTER_OFFSET,
        0,
        i64_at(data, ITEM_EQUIPMENT_BLOCK_POINTER_OFFSET)?,
    )?;
    if equipment < 4 || u32_at(data, equipment - 4)? != ITEM_EQUIPMENT_BLOCK_CLASS {
        return Err("Armor has no recognized equipment block".into());
    }
    let mut programs = Vec::new();
    if u64_at(data, equipment)? != 0 {
        let (count, _, groups, class) = native_array_at(data, equipment)?;
        if class != GROUP_CLASS {
            return Err("Armor equip conditions have an unsupported group class".into());
        }
        let end = count
            .checked_mul(16)
            .and_then(|size| groups.checked_add(size))
            .ok_or("Armor condition group range overflowed")?;
        data.get(groups..end)
            .ok_or("Armor condition groups are truncated")?;
        for position in 0..count {
            let (count, _, rows, class) = native_array_at(data, groups + position * 16)?;
            if class != CONDITION_EXPRESSION_ROW_CLASS {
                return Err("Armor equip condition has an unsupported instruction class".into());
            }
            let end = count
                .checked_mul(8)
                .and_then(|size| rows.checked_add(size))
                .ok_or("Armor condition instruction range overflowed")?;
            data.get(rows..end)
                .ok_or("Armor condition instructions are truncated")?;
            let program = (0..count)
                .map(|token| bytes_at::<8>(data, rows + token * 8))
                .collect::<Result<Vec<_>, _>>()?;
            programs.push(program);
        }
    }
    Ok(Requirements {
        equipment,
        programs,
    })
}

/// Returns the supported class, or no class restriction. Additional non-class conditions,
/// such as Festival requirements, do not change the class. Combined class programs are rejected.
pub fn armor_equipment_class(data: &[u8]) -> Result<Option<u8>, String> {
    let Requirements { programs, .. } = requirements(data)?;
    let mut supported = None;
    for program in &programs {
        for token in program {
            if u32_at(token, 0)? != 1 {
                continue;
            }
            let class = flag_class(u32_at(token, 4)?);
            if let Some(class) = class {
                if program.len() != 1 || supported.is_some_and(|old| old != class) {
                    return Err("Armor has an unsupported combined class requirement".into());
                }
                supported = Some(class);
            }
        }
    }
    // The native Festival predicate can remain after removing only the class group.
    // Unknown standalone predicates remain unsupported rather than being called unrestricted.
    if supported.is_none()
        && !programs.iter().all(|program| {
            program.as_slice() == [[1, 0, 0, 0, 0xA4, 7, 0, 0], [2, 0, 0, 0, 0, 0, 0, 0]]
        })
    {
        return Err("Armor equip conditions identify no supported class".into());
    }
    Ok(supported)
}

/// Change only armor's native class-and-slot condition, preserving other equip predicates.
/// None removes the class restriction. The caller finalizes the returned payload size.
pub fn set_armor_equipment_class(data: &[u8], selected: Option<u8>) -> Result<Vec<u8>, String> {
    if selected.is_some_and(|class| class > 2) {
        return Err("Armor class must be Titan, Hunter or Warlock".into());
    }
    armor_equipment_class(data)?;
    let Requirements {
        equipment,
        programs,
    } = requirements(data)?;
    let slot = usize::from(bytes_at::<1>(data, ITEM_INVENTORY_SLOT_OFFSET)?[0] - 3);
    // Native bucket order: helmet, arms, chest, class item, legs.
    let flag: Option<u32> = selected.map(|class| {
        [
            [0x10C, 0x109, 0x10A, 0x10D, 0x10B],
            [0xF3, 0xF0, 0xF1, 0xF4, 0xF2],
            [0x113, 0x110, 0x111, 0x114, 0x112],
        ][usize::from(class)][slot]
    });
    let mut result = Vec::new();
    let mut changed = false;
    for mut program in programs {
        let class_group = program.len() == 1
            && u32_at(&program[0], 0)? == 1
            && flag_class(u32_at(&program[0], 4)?).is_some();
        if class_group {
            changed = true;
            if let Some(flag) = flag {
                write_bytes(&mut program[0], 4, &flag.to_le_bytes())?;
            } else {
                continue;
            }
        }
        result.push(program);
    }
    if !changed && let Some(flag) = flag {
        let mut token = [0u8; 8];
        write_bytes(&mut token, 0, &1u32.to_le_bytes())?;
        write_bytes(&mut token, 4, &flag.to_le_bytes())?;
        result.insert(0, vec![token]);
    }
    let mut authored = data.to_vec();
    let blank = vec![0u8; result.len() * 16];
    let header = append_array(&mut authored, GROUP_CLASS, result.len(), &blank)?;
    descriptor(&mut authored, equipment, result.len(), header)?;
    for (index, program) in result.iter().enumerate() {
        let bytes = program.iter().flatten().copied().collect::<Vec<_>>();
        let program_header = append_array(
            &mut authored,
            CONDITION_EXPRESSION_ROW_CLASS,
            program.len(),
            &bytes,
        )?;
        descriptor(
            &mut authored,
            header + 16 + index * 16,
            program.len(),
            program_header,
        )?;
    }
    if armor_equipment_class(&authored)? != selected {
        return Err("Authored armor did not retain its selected class".into());
    }
    Ok(authored)
}

fn append_array(
    data: &mut Vec<u8>,
    class: u32,
    count: usize,
    rows: &[u8],
) -> Result<usize, String> {
    let header = data
        .len()
        .checked_add(19)
        .ok_or("Armor condition alignment overflowed")?
        & !15;
    data.resize(header - 4, 0);
    data.extend_from_slice(&0x8080_9FBDu32.to_le_bytes());
    data.extend_from_slice(&(count as u64).to_le_bytes());
    data.extend_from_slice(&class.to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(rows);
    while (data.len() + NESTED_ARRAY_TRAILER.len()) % 16 != 0 {
        data.push(0);
    }
    data.extend_from_slice(&NESTED_ARRAY_TRAILER);
    Ok(header)
}

fn descriptor(data: &mut [u8], at: usize, count: usize, header: usize) -> Result<(), String> {
    write_bytes(data, at, &(count as u64).to_le_bytes())?;
    let relative = i64::try_from(header).map_err(|_| "Armor condition offset exceeds i64")?
        - i64::try_from(at + 8).map_err(|_| "Armor condition pointer exceeds i64")?;
    write_bytes(data, at + 8, &relative.to_le_bytes())
}

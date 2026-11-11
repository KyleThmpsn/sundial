//! Recover native palette representation and affine matrix-row bone indexing.
use super::*;
use program::{Operand, Program};

pub(super) fn quaternion(code: &Program) -> bool {
    for (at, row) in code
        .instructions
        .iter()
        .enumerate()
        .filter(|(_, r)| r.code == 41)
    {
        let destination = &row.operands[0];
        let amount = &row.operands[2];
        if register(destination, 0).is_none() || amount.kind != 4 || amount.modifier != 0 {
            continue;
        }
        for lane in 0..4 {
            if destination.mask & (1 << lane) == 0 || amount.literal[amount.lanes[lane]] != 1 {
                continue;
            }
            if shifted_palette(&code.instructions[at + 1..], destination, lane) {
                return true;
            }
        }
    }
    false
}

fn shifted_palette(code: &[program::Instruction], destination: &Operand, lane: usize) -> bool {
    // Follow the shifted lane only until it is overwritten. An unrelated shift
    // must not select a different representation of the object palette.
    for row in code {
        let destinations = if matches!(row.code, 38 | 77 | 78) {
            2
        } else {
            1
        };
        if row
            .operands
            .iter()
            .skip(destinations)
            .any(|value| palette_index(value, destination.indices[0].base, lane))
        {
            return true;
        }
        if row.operands.iter().take(destinations).any(|value| {
            register(value, 0) == register(destination, 0) && value.mask & (1 << lane) != 0
        }) {
            return false;
        }
    }
    false
}

fn palette_index(value: &Operand, temporary: u32, lane: usize) -> bool {
    value.kind == 8
        && value.indices[0].base == 11
        && matches!(value.indices[1].base, 8 | 9)
        && value.indices[1].relative.as_ref().is_some_and(|index| {
            register(index, 0) == Some(temporary as usize) && index.lanes[0] == lane
        })
}

pub(in crate::model_preview) fn apply(
    manager: &PackageManager,
    material: Option<u32>,
    cache: &mut std::collections::BTreeMap<u32, Option<u8>>,
    weights: &mut [Option<animation::Weights>],
) -> Result<(), String> {
    let Some(tag) = material else { return Ok(()) };
    let offset = *cache
        .entry(tag)
        .or_insert_with(|| offset(manager, tag).ok().flatten());
    let Some(offset) = offset.filter(|offset| *offset != 0) else {
        return Ok(());
    };
    for weights in weights.iter_mut().flatten() {
        for bone in &mut weights.bones {
            *bone = bone
                .checked_add(offset)
                .ok_or("The shader bone lookup exceeds native byte storage")?;
        }
    }
    Ok(())
}

fn offset(manager: &PackageManager, material: u32) -> Result<Option<u8>, String> {
    let bytes = checked(manager, material, 0x8080_71E8)?;
    let tag = u32_at(&bytes, 0x48)?;
    if matches!(tag, 0 | u32::MAX) {
        return Ok(None);
    }
    let bytes = super::super::read::shader_bytes(manager, tag, 1)?;
    Ok(Program::read_stored(&bytes)
        .ok()
        .and_then(|code| recover(&code)))
}

fn register(value: &Operand, kind: u8) -> Option<usize> {
    (value.kind == kind
        && value.modifier == 0
        && value.indices.len() == 1
        && value.indices[0].relative.is_none())
    .then(|| value.indices[0].base as usize)
}

fn recover(code: &Program) -> Option<u8> {
    let indices = code
        .inputs
        .iter()
        .find(|v| v.name == "BLENDINDICES" && v.index == 0)?;
    if !code
        .inputs
        .iter()
        .any(|v| v.name == "BLENDWEIGHT" && v.index == 0)
    {
        return None;
    }
    let first = code.instructions.first()?;
    if first.code != 35 || first.saturate || first.operands.len() != 4 {
        return None;
    }
    let [destination, input, scale, bias] = first.operands.as_slice() else {
        return None;
    };
    let temporary = register(destination, 0)?;
    if destination.mask != 15
        || register(input, 1)? != indices.register
        || input.lanes != [0, 1, 2, 3]
        || scale.kind != 4
        || bias.kind != 4
        || scale.modifier != 0
        || bias.modifier != 0
        || scale.literal != [3; 4]
        || bias.literal != [bias.literal[0]; 4]
        || !bias.literal[0].is_multiple_of(3)
    {
        return None;
    }
    let offset = u8::try_from(bias.literal[0] / 3).ok()?;
    matrix_rows(code, temporary)?;
    Some(offset)
}

fn matrix_rows(code: &Program, temporary: usize) -> Option<()> {
    let mut rows = [0u8; 4];
    for instruction in code.instructions.iter().skip(1) {
        if !matches!(instruction.code, 16 | 17 | 49 | 50 | 54 | 56 | 68) {
            return None;
        }
        for value in instruction.operands.iter().skip(1) {
            if let Some((lane, bit)) = matrix_row(value, temporary)? {
                rows[lane] |= bit;
            }
        }
        if rows == [7; 4] {
            return Some(());
        }
        if instruction.operands.first().and_then(|v| register(v, 0)) == Some(temporary) {
            return None;
        }
    }
    None
}

fn matrix_row(value: &Operand, temporary: usize) -> Option<Option<(usize, u8)>> {
    if value.kind != 8 || value.indices.first()?.base != 11 {
        return Some(None);
    }
    let row = value.indices.get(1)?;
    let Some(relative) = &row.relative else {
        return Some(None);
    };
    if register(relative, 0)? != temporary
        || !(8..=10).contains(&row.base)
        || relative.lanes != [relative.lanes[0]; 4]
    {
        return None;
    }
    Some(Some((relative.lanes[0], 1 << (row.base - 8))))
}

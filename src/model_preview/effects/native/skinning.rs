//! Recover affine bone indexing from the vertex program's native matrix-row reads.
use super::*;
use program::{Operand, Program};

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

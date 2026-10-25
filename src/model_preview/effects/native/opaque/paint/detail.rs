//! Validate every possible detail sample at a structured branch join.
use super::*;
use std::collections::BTreeSet;
type Definitions = [BTreeSet<Option<usize>>; 3];
type Branch = (Definitions, Option<Definitions>);

fn merge(a: &mut Definitions, b: Definitions) {
    for (a, b) in a.iter_mut().zip(b) {
        a.extend(b);
    }
}

pub(super) fn definitions(code: &Code, value: &Value) -> Option<Definitions> {
    let operand = &value.operand;
    if operand.kind != 0
        || operand.modifier != 0
        || operand.indices.len() != 1
        || operand.indices[0].relative.is_some()
    {
        return None;
    }
    let mut current = std::array::from_fn(|_| BTreeSet::from([None]));
    let mut branches: Vec<Branch> = Vec::new();
    for (at, instruction) in code.instructions[..value.before].iter().enumerate() {
        match instruction.code {
            31 => branches.push((current.clone(), None)),
            18 => {
                let (entry, first) = branches.last_mut()?;
                if first.is_some() {
                    return None;
                }
                *first = Some(current);
                current = entry.clone();
            }
            21 => {
                let (entry, first) = branches.pop()?;
                merge(&mut current, first.unwrap_or(entry));
            }
            13 | 62 => {}
            _ => update(&mut current, operand, at, instruction),
        }
    }
    branches.is_empty().then_some(current)
}

fn update(current: &mut Definitions, operand: &Operand, at: usize, instruction: &Instruction) {
    let destinations = if matches!(instruction.code, 38 | 77 | 78) {
        2
    } else {
        1
    };
    for destination in instruction.operands.iter().take(destinations) {
        if destination.kind != 0 || destination.indices[0].base != operand.indices[0].base {
            continue;
        }
        for (lane, definition) in current.iter_mut().enumerate() {
            if destination.mask & (1 << operand.lanes[lane]) != 0 {
                *definition = BTreeSet::from([Some(at)]);
            }
        }
    }
}

type Affine = [f32; 3];
fn product(a: Affine, b: Affine) -> Option<Affine> {
    if a[..2] == [0.0; 2] {
        Some(b.map(|v| v * a[2]))
    } else if b[..2] == [0.0; 2] {
        Some(a.map(|v| v * b[2]))
    } else {
        None
    }
}

pub(super) fn coordinate(
    code: &Code,
    constants: &[[f32; 4]],
    value: &Operand,
    lane: usize,
    before: usize,
    depth: usize,
) -> Option<Affine> {
    if depth > 16 || value.modifier != 0 || value.indices.iter().any(|i| i.relative.is_some()) {
        return None;
    }
    let selected = value.lanes[lane];
    let number = match value.kind {
        1 if code.inputs.iter().any(|s| {
            s.register == value.indices[0].base as usize && s.name == "TEXCOORD" && s.index == 3
        }) && (2..4).contains(&selected) =>
        {
            let mut result = [0.0; 3];
            result[selected - 2] = 1.0;
            return Some(result);
        }
        4 => f32::from_bits(value.literal[selected]),
        8 if value.indices[0].base == 0 => constants.get(value.indices[1].base as usize)?[selected],
        0 => return written_coordinate(code, constants, value, lane, before, depth),
        _ => return None,
    };
    number.is_finite().then_some([0.0, 0.0, number])
}

fn written_coordinate(
    code: &Code,
    constants: &[[f32; 4]],
    value: &Operand,
    lane: usize,
    before: usize,
    depth: usize,
) -> Option<Affine> {
    let at = gain::writer(code, value, lane, before)?;
    let instruction = &code.instructions[at];
    if instruction.saturate {
        return None;
    }
    let read = |source| {
        coordinate(
            code,
            constants,
            &instruction.operands[source],
            value.lanes[lane],
            at,
            depth + 1,
        )
    };
    let add = |a: Affine, b: Affine| std::array::from_fn(|i| a[i] + b[i]);
    let result = match instruction.code {
        54 => read(1)?,
        0 => add(read(1)?, read(2)?),
        56 => product(read(1)?, read(2)?)?,
        50 => add(product(read(1)?, read(2)?)?, read(3)?),
        _ => return None,
    };
    result.iter().all(|v| v.is_finite()).then_some(result)
}

fn sample(code: &Code, constants: &[[f32; 4]], value: &Value, at: usize) -> Option<()> {
    let instruction = &code.instructions[at];
    if !matches!(instruction.code, 69 | 72 | 73) || instruction.saturate {
        return None;
    }
    let resource = &instruction.operands[2];
    let sampler = &instruction.operands[3];
    let slot = resource.indices[0].base as usize;
    if resource.kind != 7
        || ![4, 6, 8].contains(&slot)
        || sampler.kind != 6
        || sampler.indices[0].base != 1
        || (0..3).any(|lane| resource.lanes[value.operand.lanes[lane]] != lane)
        || resource.lanes[3] != 3
        || instruction.operands[0].mask & 8 == 0
        || !code
            .resources
            .iter()
            .any(|r| r.slot == slot && r.dimension == 3 && !r.integer)
    {
        return None;
    }
    let x = coordinate(code, constants, &instruction.operands[1], 0, at, 0)?;
    let y = coordinate(code, constants, &instruction.operands[1], 1, at, 0)?;
    (x[1] == 0.0 && y[0] == 0.0).then_some(())
}

pub(super) fn validate(code: &Code, constants: &[[f32; 4]], value: &Value) -> Option<()> {
    let definitions = definitions(code, value)?;
    if definitions[0] != definitions[1] || definitions[0] != definitions[2] {
        return None;
    }
    for at in &definitions[0] {
        sample(code, constants, value, (*at)?)?;
    }
    Some(())
}

//! Recover the native gear-mask discard rule from its data flow, including static branches.
use super::*;
use program::{Instruction, Operand};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Value {
    Known(u32),
    Mask,
    Coverage(f32),
    Difference(f32, f32),
    Predicate(f32, f32),
}
type State = Vec<[Option<Value>; 4]>;
struct Branch {
    condition: Option<bool>,
    before: State,
    yes: Option<State>,
}

pub(in crate::model_preview) fn load(
    manager: &PackageManager,
    tag: u32,
) -> Result<Option<f32>, String> {
    let bytes = checked(manager, tag, 0x8080_71E8)?;
    let shader = u32_at(&bytes, 0x2C8)?;
    if matches!(shader, 0 | u32::MAX) {
        return Ok(None);
    }
    let raw = super::super::read::shader_bytes(manager, shader, 0)?;
    let Ok(code) = program::Program::read(&raw, 0) else {
        return Ok(None);
    };
    if !code.instructions.iter().any(|i| i.code == 13) {
        return Ok(None);
    }
    let constants = super::super::read::stage_constants(manager, &bytes, 0x2C8)?;
    let mut state = vec![[None; 4]; code.temps];
    let mut branches = Vec::<Branch>::new();
    let mut cutoff = None;
    for i in &code.instructions {
        let read = |operand: &Operand, lane| read(&state, &constants, operand, lane);
        match i.code {
            31 => {
                let condition = match read(&i.operands[0], 0) {
                    Some(Value::Known(bits)) => Some((bits != 0) == i.nonzero),
                    _ => None,
                };
                branches.push(Branch {
                    condition,
                    before: state.clone(),
                    yes: None,
                });
            }
            18 => {
                let branch = branches.last_mut().ok_or("Invalid coverage branch")?;
                branch.yes = Some(state);
                state = branch.before.clone();
            }
            21 => {
                let branch = branches.pop().ok_or("Invalid coverage branch")?;
                let (yes, no) = match branch.yes {
                    Some(yes) => (yes, state),
                    None => (state, branch.before),
                };
                state = match branch.condition {
                    Some(true) => yes,
                    Some(false) => no,
                    None => yes
                        .iter()
                        .zip(no)
                        .map(|(a, b)| {
                            std::array::from_fn(|lane| {
                                (a[lane] == b[lane]).then_some(a[lane]).flatten()
                            })
                        })
                        .collect(),
                };
            }
            13 => {
                // The supported rule is unconditionally reached. An unknown branch must
                // not apply its discard to the whole material.
                if branches
                    .iter()
                    .any(|b| b.condition != Some(true) || b.yes.is_some())
                {
                    return Ok(None);
                }
                let Some(Value::Predicate(scale, value)) = read(&i.operands[0], 0) else {
                    return Ok(None);
                };
                if !i.nonzero || scale != 7.96875 || !(0.0..=1.0).contains(&value) || value == 0.0 {
                    return Ok(None);
                }
                if cutoff.replace(value).is_some_and(|old| old != value) {
                    return Ok(None);
                }
            }
            62 => break,
            _ => {
                let mut writes = Vec::new();
                let destinations = if matches!(i.code, 38 | 77) { 2 } else { 1 };
                for d in i.operands.iter().take(destinations).filter(|d| d.kind == 0) {
                    for lane in 0..4 {
                        if d.mask & (1 << lane) == 0 {
                            continue;
                        }
                        writes.push((
                            d.indices[0].base as usize,
                            lane,
                            operation(&code, i, lane, &read),
                        ));
                    }
                }
                for (register, lane, value) in writes {
                    if let Some(dest) = state.get_mut(register) {
                        dest[lane] = value;
                    }
                }
            }
        }
    }
    Ok(cutoff)
}

fn read(state: &State, constants: &[[f32; 4]], operand: &Operand, lane: usize) -> Option<Value> {
    if operand.indices.iter().any(|i| i.relative.is_some()) {
        return None;
    }
    let lane = operand.lanes[lane];
    let value = match operand.kind {
        0 => *state.get(operand.indices[0].base as usize)?.get(lane)?,
        4 => Some(Value::Known(operand.literal[lane])),
        8 if operand.indices[0].base == 0 => Some(Value::Known(
            constants.get(operand.indices[1].base as usize)?[lane].to_bits(),
        )),
        _ => None,
    }?;
    match (value, operand.modifier) {
        (value, 0) => Some(value),
        (Value::Known(bits), 1) => Some(Value::Known(bits ^ 0x8000_0000)),
        (Value::Known(bits), 2) => Some(Value::Known(bits & 0x7FFF_FFFF)),
        (Value::Known(bits), 3) => Some(Value::Known(bits | 0x8000_0000)),
        _ => None,
    }
}

fn operation(
    code: &program::Program,
    i: &Instruction,
    lane: usize,
    read: &impl Fn(&Operand, usize) -> Option<Value>,
) -> Option<Value> {
    let arg = |n| read(&i.operands[n], lane);
    let float = |n| match arg(n)? {
        Value::Known(bits) => Some(f32::from_bits(bits)),
        _ => None,
    };
    let result = match i.code {
        54 => arg(1),
        69 if lane == 2 => gear_mask(code, i, lane).then_some(Value::Mask),
        0 | 56 => arithmetic(i, arg(1)?, arg(2)?),
        49 if float(2)? == 0.0 && !i.saturate => match arg(1)? {
            Value::Difference(scale, cutoff) => Some(Value::Predicate(scale, cutoff)),
            _ => None,
        },
        57 => Some(Value::Known(if float(1)? != float(2)? {
            u32::MAX
        } else {
            0
        })),
        55 => match arg(1)? {
            Value::Known(bits) => arg(if bits != 0 { 2 } else { 3 }),
            _ => None,
        },
        _ => None,
    }?;
    match result {
        Value::Known(bits) if i.saturate => {
            Some(Value::Known(f32::from_bits(bits).clamp(0.0, 1.0).to_bits()))
        }
        value => Some(value),
    }
}

fn arithmetic(i: &Instruction, left: Value, right: Value) -> Option<Value> {
    match (i.code, left, right) {
        (56, Value::Mask, Value::Known(scale)) | (56, Value::Known(scale), Value::Mask)
            if i.saturate =>
        {
            Some(Value::Coverage(f32::from_bits(scale)))
        }
        (0, Value::Coverage(scale), Value::Known(offset))
        | (0, Value::Known(offset), Value::Coverage(scale))
            if !i.saturate =>
        {
            Some(Value::Difference(scale, -f32::from_bits(offset)))
        }
        (56, Value::Known(a), Value::Known(b)) => Some(Value::Known(
            (f32::from_bits(a) * f32::from_bits(b)).to_bits(),
        )),
        (0, Value::Known(a), Value::Known(b)) => Some(Value::Known(
            (f32::from_bits(a) + f32::from_bits(b)).to_bits(),
        )),
        _ => None,
    }
}

fn gear_mask(code: &program::Program, i: &Instruction, lane: usize) -> bool {
    let resource = &i.operands[2];
    let uv = &i.operands[1];
    resource.kind == 7
        && resource.indices[0].base == 2
        && resource.lanes[lane] == 2
        && uv.kind == 1
        && uv.lanes[..2] == [0, 1]
        && uv.modifier == 0
        && code.inputs.iter().any(|s| {
            s.register == uv.indices[0].base as usize && s.name == "TEXCOORD" && s.index == 3
        })
        && code
            .resources
            .iter()
            .any(|r| r.slot == 2 && r.dimension == 3 && !r.integer)
}

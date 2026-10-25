//! Recover the affine plate placement of a bounded per-pixel gear dye program.
use super::*;
use program::{Operand, Program};

type Affine = [f32; 3];

pub(super) fn value(
    p: &Program,
    constants: &[[f32; 4]],
    operand: &Operand,
    lane: usize,
    before: usize,
    depth: usize,
) -> Option<Affine> {
    if depth > 64 || operand.modifier > 1 {
        return None;
    }
    let lane = operand.lanes[lane];
    let index = operand.indices.first().map_or(0, |i| i.base as usize);
    let result = match operand.kind {
        1 if lane < 2 && texcoord_input(p, index) => {
            let mut result = [0.0; 3];
            result[lane] = 1.0;
            result
        }
        4 => [0.0, 0.0, f32::from_bits(operand.literal[lane])],
        8 if index == 0 && operand.indices.get(1)?.relative.is_none() => [
            0.0,
            0.0,
            constants.get(operand.indices[1].base as usize)?[lane],
        ],
        8 if index == 11
            && operand.indices.get(1)?.relative.is_none()
            && operand.indices[1].base == 6 =>
        {
            [0.0, 0.0, if lane < 2 { 1.0 } else { 0.0 }]
        }
        0 | 2 => written(p, constants, (operand, index, lane), before, depth)?,
        _ => return None,
    };
    let result = if operand.modifier == 1 {
        result.map(|v| -v)
    } else {
        result
    };
    result.iter().all(|v| v.is_finite()).then_some(result)
}

/// Whether input register `index` is the texture coordinate the program reads: the fourth set
/// when it declares one, the first otherwise.
fn texcoord_input(p: &Program, index: usize) -> bool {
    let set = if p
        .inputs
        .iter()
        .any(|s| s.name == "TEXCOORD" && s.index == 3)
    {
        3
    } else {
        0
    };
    p.inputs
        .iter()
        .any(|s| s.register == index && s.name == "TEXCOORD" && s.index == set)
}

/// The affine value the last instruction before `before` that writes `lane` of a temporary or
/// output register left there, outside any loop.
fn written(
    p: &Program,
    constants: &[[f32; 4]],
    (operand, index, lane): (&Operand, usize, usize),
    before: usize,
    depth: usize,
) -> Option<Affine> {
    let (at, instruction) = p.instructions[..before]
        .iter()
        .enumerate()
        .rev()
        .find(|(_, i)| {
            let destinations = match i.code {
                13 | 18 | 21 | 31 | 62 => 0,
                38 | 77 | 78 => 2,
                _ => 1,
            };
            i.operands.iter().take(destinations).any(|d| {
                d.kind == operand.kind
                    && d.indices[0].base as usize == index
                    && d.mask & (1 << lane) != 0
            })
        })?;
    let nesting = p.instructions[..at]
        .iter()
        .fold(0usize, |depth, i| match i.code {
            31 => depth + 1,
            21 => depth.saturating_sub(1),
            _ => depth,
        });
    if nesting != 0 {
        return None;
    }
    let read = |n| value(p, constants, &instruction.operands[n], lane, at, depth + 1);
    let result = match instruction.code {
        54 => read(1)?,
        0 => add(read(1)?, read(2)?),
        56 => multiply(read(1)?, read(2)?)?,
        50 => add(multiply(read(1)?, read(2)?)?, read(3)?),
        _ => return None,
    };
    let saturated = result[2] + result[0].min(0.0) + result[1].min(0.0) < 0.0
        || result[2] + result[0].max(0.0) + result[1].max(0.0) > 1.00001;
    (!(instruction.saturate && saturated)).then_some(result)
}

pub(super) fn stored_vertex_uv(p: &Program) -> Option<[f32; 4]> {
    let output = p
        .outputs
        .iter()
        .find(|s| s.name == "TEXCOORD" && s.index == 3)?;
    let lane = |lane| {
        let operand = Operand {
            kind: 2,
            indices: vec![program::Index {
                base: output.register as u32,
                relative: None,
            }],
            lanes: [0, 1, 2, 3],
            mask: 15,
            modifier: 0,
            literal: [0; 4],
        };
        value(p, &[], &operand, lane, p.instructions.len(), 0)
    };
    let x = lane(0)?;
    let y = lane(1)?;
    (x[1] == 0.0 && y[0] == 0.0 && x[0] > 0.0 && y[1] > 0.0).then_some([x[0], y[1], x[2], y[2]])
}

/// Bind the model placement before recovering raw-vertex UV equations. Palette
/// dependencies stay untouched and cannot be evaluated by the affine reader.
pub(super) fn bind_model_uv(code: &mut Program, uv: [f32; 4]) {
    for row in &mut code.instructions {
        for operand in &mut row.operands {
            if operand.kind == 8
                && operand.indices.len() == 2
                && operand.indices[0].base == 11
                && operand.indices[1].base == 6
                && operand.indices.iter().all(|i| i.relative.is_none())
            {
                operand.kind = 4;
                operand.literal = uv.map(f32::to_bits);
                operand.indices.clear();
            }
        }
    }
}

fn add(a: Affine, b: Affine) -> Affine {
    std::array::from_fn(|i| a[i] + b[i])
}
fn multiply(a: Affine, b: Affine) -> Option<Affine> {
    if a[0] == 0.0 && a[1] == 0.0 {
        Some(b.map(|v| v * a[2]))
    } else if b[0] == 0.0 && b[1] == 0.0 {
        Some(a.map(|v| v * b[2]))
    } else {
        None
    }
}

pub(in crate::model_preview) struct Placement {
    pub slot: u32,
    pub transform: [f32; 4],
    pub uv: Option<[f32; 4]>,
}

pub(in crate::model_preview) fn map_transform(
    manager: &PackageManager,
    bytes: &[u8],
    model_uv: [f32; 4],
) -> Option<Placement> {
    let pixel = super::super::read::shader_bytes(manager, u32_at(bytes, 0x2C8).ok()?, 0).ok()?;
    let program = Program::read_affine(&pixel, 0).ok()?;
    // Translated dyes use either a merged constant bank or three native banks.
    // Their explicit per-pixel selection images occupy different resource slots.
    let slot = if program
        .buffers
        .iter()
        .any(|&(slot, size)| slot == 0 && size >= 63)
    {
        3
    } else if (5..=7).all(|slot| {
        program
            .buffers
            .iter()
            .any(|&(binding, count)| binding == slot && (25..=27).contains(&count))
    }) {
        9
    } else {
        return None;
    };
    if !program
        .instructions
        .iter()
        .flat_map(|i| &i.operands)
        .any(|v| v.kind == 8 && v.indices.get(1).is_some_and(|i| i.relative.is_some()))
    {
        return None;
    }
    // Translated placements are literals. Surface selection from the dye constant bank
    // is evaluated separately from these coordinate coefficients.
    let constants = super::super::read::stage_constants(manager, bytes, 0x2C8).unwrap_or_default();
    let coordinates = |slot| {
        let samples: Vec<_> = program
            .instructions
            .iter()
            .enumerate()
            .filter(|(_, i)| {
                matches!(i.code, 69 | 70 | 72 | 73 | 74)
                    && i.operands
                        .get(2)
                        .is_some_and(|r| r.kind == 7 && r.indices[0].base == slot)
            })
            .collect();
        if samples.len() != 1 {
            return None;
        }
        let (at, sample) = samples[0];
        Some([
            value(&program, &constants, &sample.operands[1], 0, at, 0)?,
            value(&program, &constants, &sample.operands[1], 1, at, 0)?,
        ])
    };
    let plate = coordinates(0)?;
    if coordinates(1)? != plate
        || coordinates(2)? != plate
        || coordinates(slot)? != [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]
    {
        return None;
    }
    let [x, y] = plate;
    if x[1] != 0.0
        || y[0] != 0.0
        || x[0] <= 0.0
        || y[1] <= 0.0
        || x[2] < 0.0
        || y[2] < 0.0
        || x[0] + x[2] > 1.00001
        || y[1] + y[2] > 1.00001
    {
        return None;
    }
    let map = [1.0 / x[0], 1.0 / y[1], -x[2] / x[0], -y[2] / y[1]];
    // The compiled vertex equations consume raw native UVs. Decoding has already
    // applied the model's atlas placement, which need not be this material's plate.
    let vertex = super::super::read::shader_bytes(manager, u32_at(bytes, 0x48).ok()?, 1)
        .ok()
        .and_then(|bytes| Program::read_stored(&bytes).ok())
        .map(|mut code| {
            bind_model_uv(&mut code, model_uv);
            code
        });
    let uv = match vertex {
        Some(vertex) => {
            let v = stored_vertex_uv(&vertex)?;
            Some([
                x[0] * v[0],
                y[1] * v[1],
                x[0] * v[2] + x[2],
                y[1] * v[3] + y[2],
            ])
        }
        None => None,
    };
    Some(Placement {
        slot,
        transform: map,
        uv,
    })
}

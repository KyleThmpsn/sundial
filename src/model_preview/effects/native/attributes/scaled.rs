//! Native placed primary UV multiplied by the two auxiliary TEXCOORD2 lanes.
use super::*;

fn writer(code: &Code, kind: u8, register: u32, lane: usize, before: usize) -> Option<usize> {
    let at = code.instructions[..before].iter().rposition(|i| {
        let count = match i.code {
            13 | 18 | 21 | 31 | 62 => 0,
            38 | 77 | 78 => 2,
            _ => 1,
        };
        i.operands.iter().take(count).any(|d| {
            d.kind == kind
                && d.indices.len() == 1
                && d.indices[0].relative.is_none()
                && d.indices[0].base == register
                && d.mask & (1 << lane) != 0
        })
    })?;
    let depth = code.instructions[..at]
        .iter()
        .fold(0usize, |depth, i| match i.code {
            31 => depth + 1,
            21 => depth.saturating_sub(1),
            _ => depth,
        });
    (depth == 0).then_some(at)
}

fn input(code: &Code, operand: &Operand, lane: usize, index: u32, axis: usize) -> bool {
    operand.kind == 1
        && operand.modifier == 0
        && operand.indices.len() == 1
        && operand.indices[0].relative.is_none()
        && operand.lanes[lane] == axis
        && code.inputs.iter().any(|s| {
            s.name == "TEXCOORD"
                && s.index == index
                && s.register == operand.indices[0].base as usize
        })
}

fn constant(operand: &Operand, lane: usize, axis: usize) -> bool {
    operand.kind == 8
        && operand.modifier == 0
        && operand.indices.len() == 2
        && operand.indices.iter().all(|i| i.relative.is_none())
        && operand.indices[0].base == 11
        && operand.indices[1].base == 6
        && operand.lanes[lane] == axis
}

fn placed(
    code: &Code,
    operand: &Operand,
    lane: usize,
    before: usize,
    axis: usize,
    depth: u8,
) -> bool {
    if depth >= 8
        || operand.modifier != 0
        || !matches!(operand.kind, 0 | 2)
        || operand.indices.len() != 1
        || operand.indices[0].relative.is_some()
    {
        return false;
    }
    let lane = operand.lanes[lane];
    let Some(at) = writer(code, operand.kind, operand.indices[0].base, lane, before) else {
        return false;
    };
    let op = &code.instructions[at];
    if op.saturate {
        return false;
    }
    if op.code == 54 {
        return placed(code, &op.operands[1], lane, at, axis, depth + 1);
    }
    op.code == 50
        && constant(&op.operands[3], lane, axis + 2)
        && [(1, 2), (2, 1)].into_iter().any(|(raw, scale)| {
            input(code, &op.operands[raw], lane, 0, axis)
                && constant(&op.operands[scale], lane, axis)
        })
}

pub(super) fn recover(code: &Code) -> bool {
    if !code.resources.is_empty()
        || !code.buffers.contains(&(11, 24))
        || !code.buffers.contains(&(12, 14))
    {
        return false;
    }
    let Some(output) = code
        .outputs
        .iter()
        .find(|s| s.name == "TEXCOORD" && s.index == 3)
    else {
        return false;
    };
    (0..2).all(|axis| {
        let Some(at) = writer(
            code,
            2,
            output.register as u32,
            axis + 2,
            code.instructions.len(),
        ) else {
            return false;
        };
        let op = &code.instructions[at];
        if op.code != 56 || op.saturate {
            return false;
        }
        let output = Operand {
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
        placed(code, &output, axis, code.instructions.len(), axis, 0)
            && [(1, 2), (2, 1)].into_iter().any(|(primary, auxiliary)| {
                input(code, &op.operands[auxiliary], axis + 2, 2, axis)
                    && placed(code, &op.operands[primary], axis + 2, at, axis, 0)
            })
    })
}

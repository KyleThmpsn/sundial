//! Recognize native normal-radius packing, including fused normalization.
use super::{Code, Instruction, Operand, literal, producer};

fn straight(code: &Code, start: usize, end: usize) -> bool {
    !code.instructions[start..end]
        .iter()
        .any(|row| matches!(row.code, 18 | 21 | 31))
}

fn radius(code: &Code, value: &Operand, lane: usize, at: usize) -> bool {
    if value.kind == 4 && value.modifier == 0 {
        return (0.375..=0.5).contains(&f32::from_bits(value.literal[value.lanes[lane]]));
    }
    let Some((before, row, lane)) = producer(code, value, lane, at) else {
        return false;
    };
    straight(code, before, at)
        && row.code == 50
        && !row.saturate
        && (literal(&row.operands[1], lane, 0.125) || literal(&row.operands[2], lane, 0.125))
        && literal(&row.operands[3], lane, 0.375)
}

fn same_vector(a: &Operand, b: &Operand) -> bool {
    a.kind == b.kind
        && a.modifier == 0
        && b.modifier == 0
        && a.lanes[..3] == b.lanes[..3]
        && a.indices.len() == b.indices.len()
        && a.indices
            .iter()
            .zip(&b.indices)
            .all(|(a, b)| a.base == b.base && a.relative.is_none() && b.relative.is_none())
}

fn inverse_length(code: &Code, value: &Operand, lane: usize, at: usize, vector: &Operand) -> bool {
    let Some((rsq_at, rsq, lane)) = producer(code, value, lane, at) else {
        return false;
    };
    if rsq.code != 68 || rsq.saturate || !straight(code, rsq_at, at) {
        return false;
    }
    let Some((dot_at, dot, _)) = producer(code, &rsq.operands[1], lane, rsq_at) else {
        return false;
    };
    dot.code == 16
        && !dot.saturate
        && straight(code, dot_at, at)
        && same_vector(&dot.operands[1], vector)
        && same_vector(&dot.operands[2], vector)
        // The length must refer to the vector subsequently packed, without a rewrite.
        && (0..3).all(|lane| {
            producer(code, vector, lane, dot_at).map(|(at, _, lane)| (at, lane))
                == producer(code, vector, lane, at).map(|(at, _, lane)| (at, lane))
        })
}

fn factor(code: &Code, value: &Operand, at: usize, vector: &Operand) -> bool {
    if value.modifier != 0 || !(1..3).all(|lane| value.lanes[lane] == value.lanes[0]) {
        // Immediate vectors can repeat the radius in separate literal lanes.
        return value.kind == 4
            && (0..3).all(|lane| radius(code, value, lane, at))
            && (1..3)
                .all(|lane| literal(value, lane, f32::from_bits(value.literal[value.lanes[0]])));
    }
    if radius(code, value, 0, at) {
        return true;
    }
    let Some((mul_at, mul, lane)) = producer(code, value, 0, at) else {
        return false;
    };
    mul.code == 56
        && !mul.saturate
        && straight(code, mul_at, at)
        && [(1, 2), (2, 1)].into_iter().any(|(a, b)| {
            radius(code, &mul.operands[a], lane, mul_at)
                && inverse_length(code, &mul.operands[b], lane, mul_at, vector)
        })
}

pub(super) fn packed(code: &Code, at: usize, row: &Instruction) -> bool {
    packed_through_copy(code, at, row, 0)
}

fn packed_through_copy(code: &Code, at: usize, row: &Instruction, copies: usize) -> bool {
    // Output adapters may retain the original packed normal in a temporary. Follow only
    // whole, unmodified RGB copies whose producer is shared by every lane and unconditional.
    if row.code == 54 && !row.saturate && copies < 4 {
        let source = &row.operands[1];
        if source.lanes[..3] != [0, 1, 2] || row.operands[0].mask != 7 {
            return false;
        }
        let Some((before, produced, _)) = producer(code, source, 0, at) else {
            return false;
        };
        return straight(code, before, at)
            && (1..3).all(|lane| {
                producer(code, source, lane, at).is_some_and(|(other, _, _)| other == before)
            })
            && packed_through_copy(code, before, produced, copies + 1);
    }
    // The final packing is unconditional even when material equations branch earlier.
    let depth: i32 = code.instructions[..at]
        .iter()
        .map(|row| match row.code {
            31 => 1,
            21 => -1,
            _ => 0,
        })
        .sum();
    depth == 0
        && row.code == 50
        && row.saturate
        && row.operands[0].mask == 7
        && (0..3).all(|lane| literal(&row.operands[3], lane, 0.5))
        && [(1, 2), (2, 1)]
            .into_iter()
            .any(|(a, b)| factor(code, &row.operands[a], at, &row.operands[b]))
}
